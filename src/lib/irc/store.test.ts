import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  connect: vi.fn(async () => undefined),
  disconnect: vi.fn(async () => undefined),
  sendMessage: vi.fn(async () => undefined),
  raw: vi.fn(async () => undefined),
  status: vi.fn(async () => ({ status: 'disconnected', nick: '', channel: '', topic: '', members: [] })),
  listen: vi.fn(async () => () => undefined),
}));

vi.mock('@tauri-apps/api/event', () => ({ listen: mocks.listen }));
vi.mock('$lib/api/irc', () => ({
  IRC_EVENT_CHANNEL: 'cfms:irc',
  ircConnect: mocks.connect,
  ircDisconnect: mocks.disconnect,
  ircSendMessage: mocks.sendMessage,
  ircRaw: mocks.raw,
  ircStatus: mocks.status,
}));

import { ircStore } from './store.svelte';

beforeEach(() => {
  ircStore.reset();
  mocks.sendMessage.mockClear();
  mocks.raw.mockClear();
  mocks.connect.mockClear();
});

describe('irc store', () => {
  it('renders channel messages and keeps the sender', () => {
    ircStore.ingest({ type: 'message', channel: '#lorelibre_s2', from: 'alice', text: 'hi', action: false, notice: false, at: 1 });

    expect(ircStore.lines).toHaveLength(1);
    expect(ircStore.lines[0]).toMatchObject({ kind: 'message', from: 'alice', text: 'hi' });
  });

  it('marks CTCP actions separately from plain messages', () => {
    ircStore.ingest({ type: 'message', channel: '#lorelibre_s2', from: 'alice', text: 'waves', action: true, notice: false, at: 1 });

    expect(ircStore.lines[0].kind).toBe('action');
  });

  it('tracks members from names, joins, parts and renames', () => {
    ircStore.ingest({
      type: 'names',
      channel: '#lorelibre_s2',
      members: [{ nick: 'alice', mode: '@' }, { nick: 'bob', mode: '' }],
    });
    ircStore.ingest({ type: 'join', channel: '#lorelibre_s2', nick: 'carol' });
    ircStore.ingest({ type: 'part', channel: '#lorelibre_s2', nick: 'bob', reason: 'bye' });
    ircStore.ingest({ type: 'rename', from: 'carol', to: 'carol_' });

    expect(ircStore.members.map((member) => member.nick)).toEqual(['alice', 'carol_']);
    expect(ircStore.lines.at(-1)?.kind).toBe('system');
  });

  it('keeps the transcript bounded', () => {
    for (let index = 0; index < 450; index += 1) {
      ircStore.ingest({ type: 'message', channel: '#chan', from: 'alice', text: `line ${index}`, action: false, notice: false, at: index });
    }

    expect(ircStore.lines).toHaveLength(400);
    expect(ircStore.lines.at(-1)?.text).toBe('line 449');
  });

  it('records errors both as state and as a transcript line', () => {
    ircStore.ingest({ type: 'error', code: 'connectFailed', detail: 'timed out' });

    expect(ircStore.error).toBe('connectFailed|timed out');
    expect(ircStore.lines[0]).toMatchObject({ kind: 'error', event: { key: 'connectFailed', values: { detail: 'timed out' } } });
  });

  it('follows status transitions', () => {
    ircStore.ingest({ type: 'status', status: 'registered', detail: null });

    expect(ircStore.status).toBe('registered');
    expect(ircStore.connected).toBe(true);
    expect(ircStore.registered).toBe(true);
  });

  it('sends plain text through the channel and echoes it locally', async () => {
    ircStore.ingest({ type: 'status', status: 'registered', detail: null });
    ircStore.channel = '#lorelibre_s2';
    ircStore.nick = 'doctor';

    await expect(ircStore.send('  hello  ')).resolves.toBe(true);

    expect(mocks.sendMessage).toHaveBeenCalledWith('#lorelibre_s2', 'hello');
    expect(ircStore.lines.at(-1)).toMatchObject({ kind: 'message', from: 'doctor', text: 'hello' });
  });

  it('wraps /me into a CTCP action', async () => {
    ircStore.channel = '#lorelibre_s2';
    ircStore.nick = 'doctor';

    await ircStore.send('/me waves');

    expect(mocks.sendMessage).toHaveBeenCalledWith('#lorelibre_s2', '\u0001ACTION waves\u0001');
    expect(ircStore.lines.at(-1)?.kind).toBe('action');
  });

  it('forwards slash commands as raw lines without echoing them', async () => {
    ircStore.channel = '#lorelibre_s2';

    await expect(ircStore.send('/join #other')).resolves.toBe(true);

    expect(mocks.raw).toHaveBeenCalledWith('JOIN #other');
    expect(mocks.sendMessage).not.toHaveBeenCalled();
    expect(ircStore.lines).toHaveLength(0);
  });

  it('ignores empty drafts', async () => {
    await expect(ircStore.send('   ')).resolves.toBe(false);
    expect(mocks.sendMessage).not.toHaveBeenCalled();
  });
});
