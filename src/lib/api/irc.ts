// CFMS Client — Libera Chat (IRC) IPC bridge.
//
// The connection itself lives in Rust (`src-tauri/src/irc.rs`) so the socket
// survives navigation and never touches the webview CSP. This module only
// shapes the request and the typed event stream.

import { invoke } from '@tauri-apps/api/core';

/** Tauri event channel the backend emits IRC traffic on. */
export const IRC_EVENT_CHANNEL = 'cfms:irc';

export type IrcStatus = 'disconnected' | 'connecting' | 'connected' | 'registered';

export interface IrcMember {
  nick: string;
  /** Highest channel prefix the member holds: "@", "+", "" … */
  mode: string;
}

export interface IrcSnapshot {
  status: IrcStatus;
  nick: string;
  channel: string;
  topic: string;
  members: IrcMember[];
}

export interface IrcConnectRequest {
  server?: string;
  port?: number | null;
  tls?: boolean | null;
  nick: string;
  account?: string | null;
  password?: string | null;
  channel?: string;
}

export type IrcEvent =
  | { type: 'status'; status: IrcStatus; detail: string | null }
  | { type: 'ready'; nick: string }
  | { type: 'message'; channel: string; from: string; text: string; action: boolean; notice: boolean; at: number }
  | { type: 'join'; channel: string; nick: string }
  | { type: 'part'; channel: string; nick: string; reason: string }
  | { type: 'quit'; nick: string; reason: string }
  | { type: 'rename'; from: string; to: string }
  | { type: 'topic'; channel: string; topic: string }
  | { type: 'names'; channel: string; members: IrcMember[] }
  | { type: 'error'; code: string; detail: string | null };

/** Open a session. Replaces any session that is already running. */
export function ircConnect(request: IrcConnectRequest): Promise<void> {
  return invoke('irc_connect', { config: request });
}

export function ircDisconnect(): Promise<void> {
  return invoke('irc_disconnect');
}

export function ircSendMessage(target: string, text: string): Promise<void> {
  return invoke('irc_send_message', { target, text });
}

/** Send a raw IRC line, used by the `/join`, `/nick`, … shortcuts. */
export function ircRaw(line: string): Promise<void> {
  return invoke('irc_raw', { line });
}

/** Current backend session, so a remounting page can restore its view. */
export function ircStatus(): Promise<IrcSnapshot> {
  return invoke('irc_status');
}
