import '$lib/i18n';
import { cleanup, render, screen } from '@testing-library/svelte';
import { locale } from 'svelte-i18n';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ircStore } from '$lib/irc/store.svelte';
import IrcPage from './+page.svelte';

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
vi.mock('$lib/motion/transitions', () => ({ flyScale: () => ({ duration: 0 }) }));

beforeEach(() => {
  locale.set('en');
  ircStore.reset();
});

afterEach(() => {
  cleanup();
});

describe('Libera Chat page', () => {
  it('starts on the sign-in form with every field blank except the channel', () => {
    render(IrcPage);

    expect(screen.getByText('Sign in to Libera Chat')).toBeTruthy();
    expect((screen.getByLabelText('Nickname') as HTMLInputElement).value).toBe('');
    expect((screen.getByLabelText('Account') as HTMLInputElement).value).toBe('');
    expect((screen.getByLabelText('Password') as HTMLInputElement).value).toBe('');
    expect((screen.getByLabelText('Channel') as HTMLInputElement).value).toBe('#lorelibre_s2');
    expect((screen.getByLabelText('Server') as HTMLInputElement).value).toBe('');
    expect((screen.getByLabelText('Port') as HTMLInputElement).value).toBe('');
  });

  it('switches to the channel view once the session registers', async () => {
    render(IrcPage);

    ircStore.ingest({ type: 'status', status: 'registered', detail: null });
    ircStore.ingest({ type: 'ready', nick: 'doctor' });
    ircStore.ingest({ type: 'topic', channel: '#lorelibre_s2', topic: 'Welcome' });
    ircStore.ingest({ type: 'names', channel: '#lorelibre_s2', members: [{ nick: 'doctor', mode: '@' }] });
    ircStore.ingest({ type: 'message', channel: '#lorelibre_s2', from: 'alice', text: 'hello there', action: false, notice: false, at: Date.now() });

    expect(await screen.findByText('hello there')).toBeTruthy();
    expect(screen.getByText('Welcome')).toBeTruthy();
    expect(screen.getByText('alice')).toBeTruthy();
    expect(screen.queryByText('Sign in to Libera Chat')).toBeNull();
  });

  it('renders channel events through the localised strings', async () => {
    render(IrcPage);

    ircStore.ingest({ type: 'status', status: 'registered', detail: null });
    ircStore.ingest({ type: 'join', channel: '#lorelibre_s2', nick: 'bob' });

    expect(await screen.findByText('bob joined the channel')).toBeTruthy();
  });

  it('shows a connect failure from the backend as a readable error', async () => {
    render(IrcPage);

    ircStore.ingest({ type: 'error', code: 'connectFailed', detail: 'timed out' });

    expect(await screen.findByText('Connection failed: timed out')).toBeTruthy();
  });
});
