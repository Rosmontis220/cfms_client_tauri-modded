//! Libera Chat (IRC) bridge.
//!
//! The client speaks raw IRC over a TLS socket owned by the Rust side, and the
//! webview only sees typed events on [`IRC_EVENT`]. Keeping the connection here
//! means the webview never needs a WebSocket gateway or a relaxed CSP, and the
//! connection survives page navigation.
//!
//! Scope: one Libera.Chat session, one channel. That matches the product need
//! (a single community room) and keeps the state machine small enough to reason
//! about. Everything the UI needs is derived from the events emitted below.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64ct::{Base64, Encoding as _};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

/// Event channel the webview subscribes to.
pub const IRC_EVENT: &str = "cfms:irc";

const DEFAULT_SERVER: &str = "irc.libera.chat";
const DEFAULT_PORT: u16 = 6697;
const DEFAULT_CHANNEL: &str = "#lorelibre_s2";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

/// Marker trait so the TLS and plaintext transports can share one code path.
trait Transport: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> Transport for T {}
type BoxTransport = Box<dyn Transport>;

// ---------------------------------------------------------------------------
// Wire types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum IrcStatus {
    #[default]
    Disconnected,
    Connecting,
    Connected,
    Registered,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IrcMember {
    pub nick: String,
    /// Highest channel prefix the nick holds ("@", "+", "", …).
    pub mode: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum IrcEvent {
    Status { status: IrcStatus, detail: Option<String> },
    Ready { nick: String },
    Message { channel: String, from: String, text: String, action: bool, notice: bool, at: u64 },
    Join { channel: String, nick: String },
    Part { channel: String, nick: String, reason: String },
    Quit { nick: String, reason: String },
    Rename { from: String, to: String },
    Topic { channel: String, topic: String },
    Names { channel: String, members: Vec<IrcMember> },
    Error { code: String, detail: Option<String> },
}

/// Snapshot of the live session, so a page that mounts mid-session can render
/// immediately instead of waiting for the next event.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IrcSnapshot {
    pub status: IrcStatus,
    pub nick: String,
    pub channel: String,
    pub topic: String,
    pub members: Vec<IrcMember>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IrcConnectRequest {
    #[serde(default)]
    pub server: String,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub tls: Option<bool>,
    pub nick: String,
    #[serde(default)]
    pub account: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub channel: String,
}

/// Validated connection settings.
#[derive(Debug, Clone)]
struct Plan {
    server: String,
    port: u16,
    tls: bool,
    nick: String,
    account: Option<String>,
    password: Option<String>,
    channel: String,
}

// ---------------------------------------------------------------------------
// Managed state
// ---------------------------------------------------------------------------

struct Session {
    outbound: mpsc::UnboundedSender<String>,
    task: tauri::async_runtime::JoinHandle<()>,
    snapshot: Arc<Mutex<IrcSnapshot>>,
}

/// Owns the live session (at most one). Dropping the session aborts the task,
/// which closes the socket.
#[derive(Default)]
pub struct IrcState(Mutex<Option<Session>>);

impl IrcState {
    fn snapshot_handle(&self) -> Option<Arc<Mutex<IrcSnapshot>>> {
        self.0.lock().ok().and_then(|guard| guard.as_ref().map(|session| session.snapshot.clone()))
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn irc_connect(
    app: AppHandle,
    state: State<'_, IrcState>,
    config: IrcConnectRequest,
) -> Result<(), String> {
    let plan = build_plan(config)?;
    let snapshot = Arc::new(Mutex::new(IrcSnapshot {
        status: IrcStatus::Connecting,
        nick: plan.nick.clone(),
        channel: plan.channel.clone(),
        ..Default::default()
    }));

    let (outbound, receiver) = mpsc::unbounded_channel::<String>();
    let task_app = app.clone();
    let task_snapshot = snapshot.clone();
    let task = tauri::async_runtime::spawn(async move {
        run_session(task_app, plan, receiver, task_snapshot).await;
    });

    let mut guard = state.0.lock().map_err(|_| "internal".to_string())?;
    if let Some(previous) = guard.take() {
        previous.task.abort();
    }
    *guard = Some(Session { outbound, task, snapshot });
    Ok(())
}

#[tauri::command]
pub async fn irc_disconnect(state: State<'_, IrcState>) -> Result<(), String> {
    let session = {
        let mut guard = state.0.lock().map_err(|_| "internal".to_string())?;
        guard.take()
    };
    let Some(session) = session else {
        return Ok(());
    };
    let _ = session.outbound.send("QUIT :Leaving".to_string());
    // Give the QUIT a moment to leave, then drop the socket.
    tokio::time::sleep(Duration::from_millis(120)).await;
    session.task.abort();
    if let Ok(mut snapshot) = session.snapshot.lock() {
        snapshot.status = IrcStatus::Disconnected;
        snapshot.members.clear();
    }
    Ok(())
}

#[tauri::command]
pub async fn irc_send_message(
    state: State<'_, IrcState>,
    target: String,
    text: String,
) -> Result<(), String> {
    let text = text.trim_end_matches(['\r', '\n']).to_string();
    if text.trim().is_empty() {
        return Err("invalidMessage".to_string());
    }
    if text.contains(['\r', '\n']) {
        return Err("invalidMessage".to_string());
    }
    if target.trim().is_empty() || target.contains(['\r', '\n', ' ']) {
        return Err("invalidTarget".to_string());
    }
    enqueue(&state, format!("PRIVMSG {target} :{text}"))
}

#[tauri::command]
pub async fn irc_raw(state: State<'_, IrcState>, line: String) -> Result<(), String> {
    let line = line.trim_end_matches(['\r', '\n']).to_string();
    if line.is_empty() {
        return Err("invalidMessage".to_string());
    }
    if line.contains(['\r', '\n']) {
        return Err("invalidMessage".to_string());
    }
    enqueue(&state, line)
}

#[tauri::command]
pub async fn irc_status(state: State<'_, IrcState>) -> Result<IrcSnapshot, String> {
    let Some(snapshot) = state.snapshot_handle() else {
        return Ok(IrcSnapshot::default());
    };
    let guard = snapshot.lock().map_err(|_| "internal".to_string())?;
    Ok(guard.clone())
}

fn enqueue(state: &State<'_, IrcState>, line: String) -> Result<(), String> {
    let guard = state.0.lock().map_err(|_| "internal".to_string())?;
    let Some(session) = guard.as_ref() else {
        return Err("notConnected".to_string());
    };
    session.outbound.send(line).map_err(|_| "notConnected".to_string())
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

fn build_plan(request: IrcConnectRequest) -> Result<Plan, String> {
    let server = request.server.trim();
    let server = if server.is_empty() { DEFAULT_SERVER } else { server };
    if server.is_empty()
        || server.contains(char::is_whitespace)
        || server.contains('/')
        || server.contains(':')
    {
        return Err("invalidServer".to_string());
    }

    let nick = request.nick.trim();
    if !is_valid_nick(nick) {
        return Err("invalidNick".to_string());
    }

    let channel = request.channel.trim();
    let channel = if channel.is_empty() { DEFAULT_CHANNEL } else { channel };
    if !is_valid_channel(channel) {
        return Err("invalidChannel".to_string());
    }

    let account = request
        .account
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let password = request.password.filter(|value| !value.is_empty());
    if password.is_some() && account.is_none() {
        return Err("missingAccount".to_string());
    }

    Ok(Plan {
        server: server.to_string(),
        port: request.port.unwrap_or(DEFAULT_PORT),
        tls: request.tls.unwrap_or(true),
        nick: nick.to_string(),
        account,
        password,
        channel: channel.to_string(),
    })
}

fn is_valid_nick(nick: &str) -> bool {
    // Libera.Chat advertises NICKLEN=16; longer nicks are rejected at registration.
    !nick.is_empty()
        && nick.len() <= 16
        && nick.chars().next().is_some_and(|first| first.is_alphabetic() || "_[]\\`^{}|".contains(first))
        && nick.chars().all(|ch| ch.is_alphanumeric() || "-_[]\\`^{}|".contains(ch))
}

fn is_valid_channel(channel: &str) -> bool {
    channel.len() > 1
        && channel.starts_with('#')
        && !channel.contains([' ', ',', '\u{7}', '\r', '\n'])
}

// ---------------------------------------------------------------------------
// Session task
// ---------------------------------------------------------------------------

async fn run_session(
    app: AppHandle,
    plan: Plan,
    mut outbound: mpsc::UnboundedReceiver<String>,
    snapshot: Arc<Mutex<IrcSnapshot>>,
) {
    emit(&app, IrcEvent::Status { status: IrcStatus::Connecting, detail: None });
    set_snapshot(&snapshot, |state| state.status = IrcStatus::Connecting);

    let transport = match open_transport(&plan).await {
        Ok(transport) => transport,
        Err(detail) => {
            fail(&app, &snapshot, "connectFailed", Some(detail));
            return;
        }
    };

    let (reader, mut writer) = tokio::io::split(transport);
    let mut lines = BufReader::new(reader).lines();
    let mut session = SessionState::new(plan.clone());

    emit(&app, IrcEvent::Status { status: IrcStatus::Connected, detail: None });
    set_snapshot(&snapshot, |state| state.status = IrcStatus::Connected);

    // Registration preamble. CAP negotiation has to start before NICK/USER so
    // SASL can be requested before the server considers the client registered.
    let preamble = [
        "CAP LS 302".to_string(),
        format!("NICK {}", plan.nick),
        format!("USER {} 0 * :{}", plan.nick, plan.nick),
    ];
    for line in preamble {
        if write_line(&mut writer, &line).await.is_err() {
            fail(&app, &snapshot, "connectFailed", Some("handshake failed".to_string()));
            return;
        }
    }

    loop {
        tokio::select! {
            incoming = lines.next_line() => {
                match incoming {
                    Ok(Some(line)) => {
                        if handle_line(&app, &snapshot, &mut session, &line, &mut writer).await.is_err() {
                            break;
                        }
                    }
                    _ => break,
                }
            }
            queued = outbound.recv() => {
                match queued {
                    Some(line) => {
                        if write_line(&mut writer, &line).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                }
            }
        }
    }

    emit(&app, IrcEvent::Status { status: IrcStatus::Disconnected, detail: None });
    set_snapshot(&snapshot, |state| {
        state.status = IrcStatus::Disconnected;
        state.members.clear();
        state.topic.clear();
    });
}

async fn open_transport(plan: &Plan) -> Result<BoxTransport, String> {
    let address = format!("{}:{}", plan.server, plan.port);
    let tcp = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(&address))
        .await
        .map_err(|_| format!("connection to {address} timed out"))?
        .map_err(|error| format!("{address}: {error}"))?;
    let _ = tcp.set_nodelay(true);
    if !plan.tls {
        return Ok(Box::new(tcp));
    }

    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
    let server_name = rustls::pki_types::ServerName::try_from(plan.server.clone())
        .map_err(|_| format!("invalid server name {}", plan.server))?;
    let tls = connector
        .connect(server_name, tcp)
        .await
        .map_err(|error| format!("TLS handshake failed: {error}"))?;
    Ok(Box::new(tls))
}

struct SessionState {
    plan: Plan,
    nick: String,
    registered: bool,
    members: BTreeMap<String, String>,
}

impl SessionState {
    fn new(plan: Plan) -> Self {
        let nick = plan.nick.clone();
        Self { plan, nick, registered: false, members: BTreeMap::new() }
    }

    fn members(&self) -> Vec<IrcMember> {
        self.members
            .iter()
            .map(|(nick, mode)| IrcMember { nick: nick.clone(), mode: mode.clone() })
            .collect()
    }
}

async fn handle_line(
    app: &AppHandle,
    snapshot: &Arc<Mutex<IrcSnapshot>>,
    session: &mut SessionState,
    raw: &str,
    writer: &mut (impl tokio::io::AsyncWrite + Unpin),
) -> Result<(), ()> {
    let Some(message) = parse_line(raw) else {
        return Ok(());
    };
    let sender = message.prefix.as_deref().map(nick_of).unwrap_or_default();

    match message.command.as_str() {
        "PING" => {
            let token = message.params.first().cloned().unwrap_or_default();
            let _ = write_line(writer, &format!("PONG :{token}")).await;
        }
        "CAP" => {
            let action = message.params.get(1).map(|value| value.as_str()).unwrap_or_default();
            match action {
                "LS" => {
                    if session.plan.account.is_some() && session.plan.password.is_some() {
                        let _ = write_line(writer, "CAP REQ :sasl").await;
                    } else {
                        let _ = write_line(writer, "CAP END").await;
                    }
                }
                "ACK" => {
                    let acked = message.params.get(2).map(|value| value.as_str()).unwrap_or_default();
                    if acked.split(',').any(|item| item == "sasl") {
                        let _ = write_line(writer, "AUTHENTICATE PLAIN").await;
                    } else {
                        let _ = write_line(writer, "CAP END").await;
                    }
                }
                "NAK" => {
                    emit(app, IrcEvent::Error {
                        code: "saslUnavailable".to_string(),
                        detail: None,
                    });
                    let _ = write_line(writer, "CAP END").await;
                }
                _ => {}
            }
        }
        "AUTHENTICATE" => {
            if message.params.first().map(|value| value.as_str()) == Some("+") {
                if let (Some(account), Some(password)) = (&session.plan.account, &session.plan.password) {
                    let payload = format!("{account}\0{account}\0{password}");
                    let encoded = Base64::encode_string(payload.as_bytes());
                    let mut rest = encoded.as_str();
                    while !rest.is_empty() {
                        let take = rest.len().min(400);
                        let (chunk, remainder) = rest.split_at(take);
                        let _ = write_line(writer, &format!("AUTHENTICATE {chunk}")).await;
                        rest = remainder;
                    }
                    if encoded.len() % 400 == 0 || encoded.is_empty() {
                        let _ = write_line(writer, "AUTHENTICATE +").await;
                    }
                } else {
                    let _ = write_line(writer, "AUTHENTICATE *").await;
                }
            }
        }
        "903" => {
            let _ = write_line(writer, "CAP END").await;
        }
        "904" | "905" | "906" | "907" => {
            emit(app, IrcEvent::Error {
                code: "saslFailed".to_string(),
                detail: message.params.last().cloned().filter(|value| !value.is_empty()),
            });
            let _ = write_line(writer, "CAP END").await;
        }
        "001" => {
            session.registered = true;
            emit(app, IrcEvent::Ready { nick: session.nick.clone() });
            emit(app, IrcEvent::Status { status: IrcStatus::Registered, detail: None });
            set_snapshot(snapshot, |state| {
                state.status = IrcStatus::Registered;
                state.nick = session.nick.clone();
            });
            let channel = session.plan.channel.clone();
            let _ = write_line(writer, &format!("JOIN {channel}")).await;
        }
        "432" | "433" | "436" => {
            if !session.registered {
                session.nick.push('_');
                let nick = session.nick.clone();
                emit(app, IrcEvent::Error {
                    code: "nickInUse".to_string(),
                    detail: Some(nick.clone()),
                });
                let _ = write_line(writer, &format!("NICK {nick}")).await;
            } else {
                emit(app, IrcEvent::Error {
                    code: "nickInUse".to_string(),
                    detail: message.params.get(1).cloned(),
                });
            }
        }
        "332" => {
            let channel = message.params.get(1).cloned().unwrap_or_default();
            let topic = message.params.get(2).cloned().unwrap_or_default();
            set_snapshot(snapshot, |state| state.topic = topic.clone());
            emit(app, IrcEvent::Topic { channel, topic });
        }
        "331" => {
            let channel = message.params.get(1).cloned().unwrap_or_default();
            set_snapshot(snapshot, |state| state.topic.clear());
            emit(app, IrcEvent::Topic { channel, topic: String::new() });
        }
        "353" => {
            if let Some(names) = message.params.get(3) {
                for entry in names.split_whitespace() {
                    let (mode, nick) = split_member_prefix(entry);
                    if nick.is_empty() {
                        continue;
                    }
                    session
                        .members
                        .entry(nick.to_string())
                        .and_modify(|existing| {
                            if mode.len() > existing.len() {
                                *existing = mode.clone();
                            }
                        })
                        .or_insert(mode);
                }
            }
        }
        "366" => {
            let channel = message.params.get(1).cloned().unwrap_or_default();
            let members = session.members();
            set_snapshot(snapshot, |state| state.members = members.clone());
            emit(app, IrcEvent::Names { channel, members });
        }
        "JOIN" => {
            let channel = message.params.first().cloned().unwrap_or_default();
            if sender.is_empty() {
                return Ok(());
            }
            session.members.entry(sender.clone()).or_default();
            let members = session.members();
            set_snapshot(snapshot, |state| state.members = members);
            emit(app, IrcEvent::Join { channel, nick: sender });
        }
        "PART" => {
            let channel = message.params.first().cloned().unwrap_or_default();
            let reason = message.params.get(1).cloned().unwrap_or_default();
            session.members.remove(&sender);
            let members = session.members();
            set_snapshot(snapshot, |state| state.members = members);
            emit(app, IrcEvent::Part { channel, nick: sender, reason });
        }
        "QUIT" => {
            let reason = message.params.first().cloned().unwrap_or_default();
            session.members.remove(&sender);
            let members = session.members();
            set_snapshot(snapshot, |state| state.members = members);
            emit(app, IrcEvent::Quit { nick: sender, reason });
        }
        "NICK" => {
            let to = message.params.first().cloned().unwrap_or_default();
            if session.members.remove(&sender).is_some() {
                session.members.insert(to.clone(), String::new());
            }
            if sender == session.nick {
                session.nick = to.clone();
                set_snapshot(snapshot, |state| state.nick = to.clone());
            }
            let members = session.members();
            set_snapshot(snapshot, |state| state.members = members);
            emit(app, IrcEvent::Rename { from: sender, to });
        }
        "PRIVMSG" | "NOTICE" => {
            let target = message.params.first().cloned().unwrap_or_default();
            let mut text = message.params.get(1).cloned().unwrap_or_default();
            let notice = message.command == "NOTICE";
            let mut action = false;
            if let Some(inner) = text.strip_prefix('\u{1}').and_then(|value| value.strip_suffix('\u{1}')) {
                if let Some(rest) = inner.strip_prefix("ACTION ") {
                    action = true;
                    text = rest.to_string();
                } else {
                    // CTCP (VERSION, PING, …) is noise for a channel-only view.
                    return Ok(());
                }
            }
            let channel = if target.starts_with('#') { target } else { sender.clone() };
            emit(app, IrcEvent::Message {
                channel,
                from: sender,
                text,
                action,
                notice,
                at: now_millis(),
            });
        }
        "KICK" => {
            let channel = message.params.first().cloned().unwrap_or_default();
            let kicked = message.params.get(1).cloned().unwrap_or_default();
            let reason = message.params.get(2).cloned().unwrap_or_default();
            if kicked == session.nick {
                emit(app, IrcEvent::Error {
                    code: "kicked".to_string(),
                    detail: Some(format!("{channel}: {reason}")),
                });
            }
        }
        "ERROR" => {
            emit(app, IrcEvent::Error {
                code: "serverClosed".to_string(),
                detail: message.params.first().cloned(),
            });
            return Err(());
        }
        _ => {}
    }

    Ok(())
}

async fn write_line(
    writer: &mut (impl tokio::io::AsyncWrite + Unpin),
    line: &str,
) -> Result<(), ()> {
    let framed = format!("{line}\r\n");
    writer.write_all(framed.as_bytes()).await.map_err(|_| ())?;
    writer.flush().await.map_err(|_| ())
}

fn emit(app: &AppHandle, event: IrcEvent) {
    let _ = app.emit(IRC_EVENT, event);
}

fn set_snapshot(snapshot: &Arc<Mutex<IrcSnapshot>>, update: impl FnOnce(&mut IrcSnapshot)) {
    if let Ok(mut guard) = snapshot.lock() {
        update(&mut guard);
    }
}

fn fail(app: &AppHandle, snapshot: &Arc<Mutex<IrcSnapshot>>, code: &str, detail: Option<String>) {
    emit(app, IrcEvent::Error { code: code.to_string(), detail });
    emit(app, IrcEvent::Status { status: IrcStatus::Disconnected, detail: None });
    set_snapshot(snapshot, |state| {
        state.status = IrcStatus::Disconnected;
        state.members.clear();
    });
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn nick_of(prefix: &str) -> String {
    prefix
        .split(['!', '@'])
        .next()
        .unwrap_or(prefix)
        .to_string()
}

fn split_member_prefix(entry: &str) -> (String, &str) {
    let first = entry.chars().next().unwrap_or(' ');
    if matches!(first, '~' | '&' | '@' | '%' | '+') {
        (first.to_string(), &entry[first.len_utf8()..])
    } else {
        (String::new(), entry)
    }
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

#[derive(Debug, Default, PartialEq, Eq)]
struct ParsedLine {
    prefix: Option<String>,
    command: String,
    params: Vec<String>,
}

/// Minimal RFC 1459/2812 line parser: optional tags, optional prefix, command,
/// and middle/trailing parameters. Tags are dropped — nothing in the UI needs
/// them yet (server-time is only sent by bouncers).
fn parse_line(raw: &str) -> Option<ParsedLine> {
    let mut rest = raw.trim_end_matches(['\r', '\n']);
    if rest.is_empty() {
        return None;
    }
    if let Some(tags) = rest.strip_prefix('@') {
        let (_, remainder) = tags.split_once(' ')?;
        rest = remainder;
    }
    let prefix = if let Some(after) = rest.strip_prefix(':') {
        let (prefix, remainder) = after.split_once(' ')?;
        rest = remainder;
        Some(prefix.to_string())
    } else {
        None
    };
    let (command, mut remainder) = match rest.split_once(' ') {
        Some((command, remainder)) => (command, remainder),
        None => (rest, ""),
    };
    let mut params = Vec::new();
    while !remainder.is_empty() {
        if let Some(trailing) = remainder.strip_prefix(':') {
            params.push(trailing.to_string());
            break;
        }
        match remainder.split_once(' ') {
            Some((param, rest)) => {
                params.push(param.to_string());
                remainder = rest;
            }
            None => {
                params.push(remainder.to_string());
                break;
            }
        }
    }
    Some(ParsedLine { prefix, command: command.to_uppercase(), params })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_prefix_command_and_trailing() {
        let parsed = parse_line(":nick!user@host PRIVMSG #chan :hello :world\r\n").unwrap();
        assert_eq!(parsed.prefix.as_deref(), Some("nick!user@host"));
        assert_eq!(parsed.command, "PRIVMSG");
        assert_eq!(parsed.params, vec!["#chan".to_string(), "hello :world".to_string()]);
    }

    #[test]
    fn parses_server_numeric_without_prefix() {
        let parsed = parse_line("353 me = #chan :@op +voice plain").unwrap();
        assert_eq!(parsed.prefix, None);
        assert_eq!(parsed.command, "353");
        assert_eq!(parsed.params.len(), 4);
        assert_eq!(parsed.params[3], "@op +voice plain");
    }

    #[test]
    fn ignores_empty_lines() {
        assert!(parse_line("\r\n").is_none());
    }

    #[test]
    fn splits_channel_member_prefixes() {
        assert_eq!(split_member_prefix("@nick"), ("@".to_string(), "nick"));
        assert_eq!(split_member_prefix("+voice"), ("+".to_string(), "voice"));
        assert_eq!(split_member_prefix("nick"), (String::new(), "nick"));
    }

    #[test]
    fn validates_nicks_and_channels() {
        assert!(is_valid_nick("Rosmontis"));
        assert!(is_valid_nick("_ro"));
        assert!(!is_valid_nick("1abc"));
        assert!(!is_valid_nick(""));
        assert!(is_valid_channel("#lorelibre_s2"));
        assert!(!is_valid_channel("lorelibre"));
    }

    #[test]
    fn defaults_missing_connection_fields() {
        let plan = build_plan(IrcConnectRequest {
            server: String::new(),
            port: None,
            tls: None,
            nick: " doctor ".to_string(),
            account: Some("  ".to_string()),
            password: None,
            channel: String::new(),
        })
        .unwrap();
        assert_eq!(plan.server, DEFAULT_SERVER);
        assert_eq!(plan.port, DEFAULT_PORT);
        assert!(plan.tls);
        assert_eq!(plan.nick, "doctor");
        assert_eq!(plan.channel, DEFAULT_CHANNEL);
        assert_eq!(plan.account, None);
    }

    #[test]
    fn rejects_password_without_account() {
        let error = build_plan(IrcConnectRequest {
            server: String::new(),
            port: None,
            tls: None,
            nick: "doctor".to_string(),
            account: None,
            password: Some("secret".to_string()),
            channel: String::new(),
        })
        .unwrap_err();
        assert_eq!(error, "missingAccount");
    }
}
