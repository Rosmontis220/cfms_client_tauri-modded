// CFMS Client — pure helpers for the Libera Chat view.
//
// Everything here is deterministic and side-effect free so the tricky parts of
// IRC text handling (control codes, nick colours, slash commands) can be tested
// without a socket.

/** Number of distinct nick colours the view renders. */
export const NICK_COLOR_COUNT = 8;

/**
 * Remove mIRC formatting codes (\x02 bold, \x03 colour, \x0f reset, …).
 *
 * Libera traffic regularly carries bold/colour codes; rendering them raw shows
 * stray glyphs, so the view strips them and keeps only the readable text.
 */
export function stripIrcFormatting(text: string): string {
  let output = '';
  for (let index = 0; index < text.length; index += 1) {
    const code = text.charCodeAt(index);
    if (code === 0x03) {
      // Colour codes may consume up to four digits; resume right after them.
      index = skipColorArguments(text, index + 1) - 1;
      continue;
    }
    if (code <= 0x08 || code === 0x0b || code === 0x0c || (code >= 0x0e && code <= 0x1f)) {
      continue;
    }
    output += text[index];
  }
  return output;
}

/** Returns the index of the last character consumed by a `\x03` colour code. */
function skipColorArguments(text: string, start: number): number {
  const foregroundEnd = skipDigits(text, start, 2);
  if (foregroundEnd === start) {
    return start;
  }
  if (text[foregroundEnd] !== ',') {
    return foregroundEnd;
  }
  const backgroundEnd = skipDigits(text, foregroundEnd + 1, 2);
  return backgroundEnd === foregroundEnd + 1 ? foregroundEnd : backgroundEnd;
}

function skipDigits(text: string, start: number, limit: number): number {
  let cursor = start;
  while (cursor < text.length && cursor - start < limit && text[cursor] >= '0' && text[cursor] <= '9') {
    cursor += 1;
  }
  return cursor;
}

/** Stable, case-insensitive colour bucket for a nick. */
export function nickColorIndex(nick: string, buckets = NICK_COLOR_COUNT): number {
  if (buckets <= 0) {
    return 0;
  }
  let hash = 0;
  const normalized = nick.toLowerCase();
  for (let index = 0; index < normalized.length; index += 1) {
    hash = (hash * 31 + normalized.charCodeAt(index)) % 1_000_003;
  }
  return hash % buckets;
}

export type IrcInputAction =
  | { kind: 'empty' }
  | { kind: 'message'; text: string }
  | { kind: 'action'; text: string }
  | { kind: 'raw'; line: string };

/**
 * Turn composer input into what should be sent.
 *
 * Known slash commands map to raw IRC lines; an unknown `/word` stays a normal
 * message so a message that merely starts with a slash is never swallowed.
 */
export function parseIrcInput(input: string, channel: string): IrcInputAction {
  const trimmed = input.trim();
  if (!trimmed) {
    return { kind: 'empty' };
  }
  if (!trimmed.startsWith('/')) {
    return { kind: 'message', text: trimmed };
  }

  const withoutSlash = trimmed.slice(1);
  const separator = withoutSlash.search(/\s/);
  const command = (separator === -1 ? withoutSlash : withoutSlash.slice(0, separator)).toLowerCase();
  const argument = separator === -1 ? '' : withoutSlash.slice(separator + 1).trim();

  switch (command) {
    case 'me':
      return argument ? { kind: 'action', text: argument } : { kind: 'empty' };
    case 'join':
      return argument ? { kind: 'raw', line: `JOIN ${argument}` } : { kind: 'empty' };
    case 'part':
      return { kind: 'raw', line: argument ? `PART ${channel} :${argument}` : `PART ${channel}` };
    case 'nick':
      return argument ? { kind: 'raw', line: `NICK ${argument}` } : { kind: 'empty' };
    case 'topic':
      return { kind: 'raw', line: argument ? `TOPIC ${channel} :${argument}` : `TOPIC ${channel}` };
    case 'quit':
      return { kind: 'raw', line: `QUIT :${argument || 'Leaving'}` };
    case 'msg': {
      const [target, ...rest] = argument.split(/\s+/);
      const text = rest.join(' ').trim();
      return target && text ? { kind: 'raw', line: `PRIVMSG ${target} :${text}` } : { kind: 'empty' };
    }
    default:
      return { kind: 'message', text: trimmed };
  }
}

/** Member ordering: channel operators first, then voiced, then alphabetical. */
export function memberRank(mode: string): number {
  if (mode.includes('~')) return 0;
  if (mode.includes('&')) return 1;
  if (mode.includes('@')) return 2;
  if (mode.includes('%')) return 3;
  if (mode.includes('+')) return 4;
  return 5;
}

/** `14:07` in the user's locale, used for the message gutter. */
export function formatIrcClock(at: number): string {
  if (!Number.isFinite(at) || at <= 0) {
    return '';
  }
  return new Date(at).toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' });
}
