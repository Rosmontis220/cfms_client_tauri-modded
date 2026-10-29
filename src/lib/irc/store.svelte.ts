// CFMS Client — Libera Chat session state.
//
// The socket lives in Rust; this store is the view model: it keeps the channel
// topic, the member list and a bounded transcript, and folds the backend event
// stream into them. One store per app so navigating away from the page does not
// throw the conversation away.

import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import {
  IRC_EVENT_CHANNEL,
  ircConnect,
  ircDisconnect,
  ircRaw,
  ircSendMessage,
  ircStatus,
  type IrcConnectRequest,
  type IrcEvent,
  type IrcMember,
  type IrcSnapshot,
  type IrcStatus,
} from '$lib/api/irc';
import { parseIrcInput } from './format';

/** Transcript cap — plenty for a support room, cheap to keep in memory. */
const MAX_LINES = 400;

export type IrcLineKind = 'message' | 'action' | 'notice' | 'system' | 'error';

/** A channel event rendered through `irc.events.*` instead of free text. */
export interface IrcSystemEvent {
  key: string;
  values?: Record<string, string>;
}

export interface IrcLine {
  id: number;
  kind: IrcLineKind;
  from: string;
  text: string;
  event?: IrcSystemEvent;
  at: number;
}

let sequence = 0;
let unlisten: UnlistenFn | null = null;
let listening = false;

class IrcStoreImpl {
  status = $state<IrcStatus>('disconnected');
  nick = $state('');
  channel = $state('');
  topic = $state('');
  members = $state<IrcMember[]>([]);
  lines = $state<IrcLine[]>([]);
  /** Last error: a bare i18n code (`invalidNick`) or `code|detail`. */
  error = $state<string | null>(null);
  /** `true` while `irc_connect` is in flight. */
  pending = $state(false);

  get connected(): boolean {
    return this.status === 'connected' || this.status === 'registered';
  }

  get registered(): boolean {
    return this.status === 'registered';
  }

  reset(): void {
    this.status = 'disconnected';
    this.nick = '';
    this.channel = '';
    this.topic = '';
    this.members = [];
    this.lines = [];
    this.error = null;
    this.pending = false;
  }

  applySnapshot(snapshot: IrcSnapshot): void {
    this.status = snapshot.status;
    this.nick = snapshot.nick;
    this.channel = snapshot.channel;
    this.topic = snapshot.topic;
    this.members = snapshot.members;
  }

  /** Fold one backend event into the view model. */
  ingest(event: IrcEvent): void {
    switch (event.type) {
      case 'status':
        this.status = event.status;
        if (event.status === 'connecting') {
          this.error = null;
        }
        return;
      case 'ready':
        this.nick = event.nick;
        this.applySystem('registered', { nick: event.nick });
        return;
      case 'message':
        this.pushLine(event.action ? 'action' : event.notice ? 'notice' : 'message', event.from, event.text, event.at);
        return;
      case 'join':
        this.addMember(event.nick);
        this.applySystem('join', { nick: event.nick });
        return;
      case 'part':
        this.removeMember(event.nick);
        this.applySystem(event.reason ? 'partReason' : 'part', { nick: event.nick, reason: event.reason });
        return;
      case 'quit':
        this.removeMember(event.nick);
        this.applySystem(event.reason ? 'quitReason' : 'quit', { nick: event.nick, reason: event.reason });
        return;
      case 'rename':
        this.renameMember(event.from, event.to);
        if (this.nick === event.from) {
          this.nick = event.to;
        }
        this.applySystem('rename', { from: event.from, to: event.to });
        return;
      case 'topic':
        this.topic = event.topic;
        this.applySystem(event.topic ? 'topic' : 'topicCleared', { topic: event.topic });
        return;
      case 'names':
        this.members = event.members;
        return;
      case 'error':
        this.error = event.detail ? `${event.code}|${event.detail}` : event.code;
        this.applyError(event.code, event.detail);
        return;
      default:
        return;
    }
  }

  async connect(request: IrcConnectRequest): Promise<void> {
    this.error = null;
    this.pending = true;
    try {
      await ircConnect(request);
      this.status = 'connecting';
      this.nick = request.nick;
      this.channel = request.channel ?? '';
      this.lines = [];
      this.members = [];
      this.topic = '';
    } finally {
      this.pending = false;
    }
  }

  async disconnect(): Promise<void> {
    try {
      await ircDisconnect();
    } finally {
      this.status = 'disconnected';
      this.members = [];
      this.topic = '';
      this.applySystem('disconnected');
    }
  }

  /**
   * Send composer input. Returns `false` when there was nothing to send, so the
   * view can leave the draft in the composer.
   */
  async send(input: string): Promise<boolean> {
    const action = parseIrcInput(input, this.channel);
    if (action.kind === 'empty') {
      return false;
    }
    if (action.kind === 'raw') {
      await ircRaw(action.line);
      return true;
    }
    if (action.kind === 'action') {
      await ircSendMessage(this.channel, `\u0001ACTION ${action.text}\u0001`);
      this.pushLine('action', this.nick, action.text);
      return true;
    }
    await ircSendMessage(this.channel, action.text);
    this.pushLine('message', this.nick, action.text);
    return true;
  }

  /** Restore from the backend when the page mounts mid-session. */
  async hydrate(): Promise<void> {
    try {
      this.applySnapshot(await ircStatus());
    } catch {
      // No backend (browser preview) — keep the local, empty state.
    }
  }

  private pushLine(kind: IrcLineKind, from: string, text: string, at = Date.now()): void {
    const line: IrcLine = { id: (sequence += 1), kind, from, text, at };
    const next = [...this.lines, line];
    this.lines = next.length > MAX_LINES ? next.slice(next.length - MAX_LINES) : next;
  }

  private applySystem(key: string, values?: Record<string, string>): void {
    this.pushSystem({ key, values });
  }

  private applyError(code: string, detail: string | null): void {
    this.pushSystem({ key: code, values: detail ? { detail } : undefined }, 'error');
  }

  private pushSystem(event: IrcSystemEvent, kind: IrcLineKind = 'system'): void {
    const line: IrcLine = {
      id: (sequence += 1),
      kind,
      from: '',
      text: '',
      event,
      at: Date.now(),
    };
    const next = [...this.lines, line];
    this.lines = next.length > MAX_LINES ? next.slice(next.length - MAX_LINES) : next;
  }

  private addMember(nick: string): void {
    if (!nick || this.members.some((member) => member.nick === nick)) {
      return;
    }
    this.members = [...this.members, { nick, mode: '' }];
  }

  private removeMember(nick: string): void {
    this.members = this.members.filter((member) => member.nick !== nick);
  }

  private renameMember(from: string, to: string): void {
    this.members = this.members.map((member) => (member.nick === from ? { ...member, nick: to } : member));
  }
}

export const ircStore = new IrcStoreImpl();

/** Subscribe to the backend event stream once per app run. */
export async function startIrcEvents(): Promise<void> {
  if (listening) {
    return;
  }
  listening = true;
  try {
    unlisten = await listen<IrcEvent>(IRC_EVENT_CHANNEL, (payload) => {
      ircStore.ingest(payload.payload);
    });
  } catch {
    // Hosts without the Tauri event bridge simply never receive events.
    listening = false;
  }
}

export function stopIrcEvents(): void {
  unlisten?.();
  unlisten = null;
  listening = false;
}
