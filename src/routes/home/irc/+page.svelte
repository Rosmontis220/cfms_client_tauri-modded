<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { _ as t } from 'svelte-i18n';
  import FluentButton from '$lib/components/FluentButton.svelte';
  import Icon from '$lib/components/Icon.svelte';
  import MdOutlinedField from '$lib/components/MdOutlinedField.svelte';
  import PageHeading from '$lib/components/PageHeading.svelte';
  import { ircStore, startIrcEvents, type IrcLine } from '$lib/irc/store.svelte';
  import { formatIrcClock, memberRank, nickColorIndex, stripIrcFormatting } from '$lib/irc/format';
  import { isScrolledToBottom } from '$lib/scroll-edges';
  import { isTauriRuntime } from '$lib/tauri-runtime';
  import type { IrcStatus } from '$lib/api/irc';

  const FORM_KEY = 'cfms.irc.form.v1';
  const DEFAULT_CHANNEL = '#lorelibre_s2';
  const DEFAULT_SERVER = 'irc.libera.chat';
  const DEFAULT_PORT = 6697;

  const STATUS_KEYS: Record<IrcStatus, string> = {
    disconnected: 'irc.status.disconnected',
    connecting: 'irc.status.connecting',
    connected: 'irc.status.connected',
    registered: 'irc.status.registered',
  };

  const ERROR_KEYS: Record<string, string> = {
    invalidNick: 'irc.errors.invalidNick',
    invalidChannel: 'irc.errors.invalidChannel',
    invalidServer: 'irc.errors.invalidServer',
    invalidMessage: 'irc.errors.invalidMessage',
    invalidTarget: 'irc.errors.invalidTarget',
    missingAccount: 'irc.errors.missingAccount',
    notConnected: 'irc.errors.notConnected',
    connectFailed: 'irc.errors.connectFailed',
    saslFailed: 'irc.errors.saslFailed',
    saslUnavailable: 'irc.errors.saslUnavailable',
    nickInUse: 'irc.errors.nickInUse',
    kicked: 'irc.errors.kicked',
    serverClosed: 'irc.errors.serverClosed',
    internal: 'irc.errors.internal',
  };

  const EVENT_KEYS: Record<string, string> = {
    registered: 'irc.events.registered',
    join: 'irc.events.join',
    part: 'irc.events.part',
    partReason: 'irc.events.partReason',
    quit: 'irc.events.quit',
    quitReason: 'irc.events.quitReason',
    rename: 'irc.events.rename',
    topic: 'irc.events.topic',
    topicCleared: 'irc.events.topicCleared',
    disconnected: 'irc.events.disconnected',
  };

  let nick = $state('');
  let account = $state('');
  let password = $state('');
  let channel = $state(DEFAULT_CHANNEL);
  // Only the channel is pre-filled: everything else starts empty and falls back
  // to the Libera defaults inside `connect()` when the field is left untouched.
  let server = $state('');
  let port = $state('');
  let advancedOpen = $state(false);
  let draft = $state('');
  let connectError = $state<string | null>(null);
  let sendError = $state<string | null>(null);
  let messagesEl = $state<HTMLDivElement | null>(null);
  let atBottom = $state(true);
  let runtimeAvailable = $state(true);

  const connected = $derived(ircStore.connected);
  const statusKey = $derived(STATUS_KEYS[ircStore.status]);
  const canConnect = $derived(
    nick.trim().length > 0 && !ircStore.pending && ircStore.status !== 'connecting',
  );
  const sortedMembers = $derived(
    [...ircStore.members].sort(
      (left, right) => memberRank(left.mode) - memberRank(right.mode) || left.nick.localeCompare(right.nick),
    ),
  );

  function loadForm(): void {
    try {
      const raw = window.localStorage.getItem(FORM_KEY);
      if (!raw) {
        return;
      }
      const stored = JSON.parse(raw) as Partial<Record<'nick' | 'account' | 'channel' | 'server' | 'port', unknown>>;
      if (typeof stored.nick === 'string') nick = stored.nick;
      if (typeof stored.account === 'string') account = stored.account;
      if (typeof stored.channel === 'string' && stored.channel.trim()) channel = stored.channel;
      if (typeof stored.server === 'string' && stored.server.trim()) server = stored.server;
      if (typeof stored.port === 'number' && Number.isFinite(stored.port)) port = String(stored.port);
      else if (typeof stored.port === 'string' && stored.port.trim()) port = stored.port;
    } catch {
      // A corrupted entry is not worth blocking the form over.
    }
  }

  function saveForm(): void {
    try {
      window.localStorage.setItem(FORM_KEY, JSON.stringify({ nick, account, channel, server, port }));
    } catch {
      // Persistence is a convenience; ignore quota or privacy-mode failures.
    }
  }

  onMount(() => {
    loadForm();
    runtimeAvailable = isTauriRuntime();
    if (!runtimeAvailable) {
      return;
    }
    void startIrcEvents();
    void ircStore.hydrate();
  });

  // Keep the transcript pinned to the newest line unless the reader scrolled up.
  $effect(() => {
    const count = ircStore.lines.length;
    if (count === 0 || !atBottom) {
      return;
    }
    void tick().then(() => {
      if (messagesEl) {
        messagesEl.scrollTop = messagesEl.scrollHeight;
      }
    });
  });

  function describeError(raw: string | null): string | null {
    if (!raw) {
      return null;
    }
    const [code, detail] = raw.split('|');
    const key = ERROR_KEYS[code];
    if (!key) {
      return raw;
    }
    return detail ? $t(key, { values: { detail } }) : $t(key);
  }

  function describeLine(line: IrcLine): string {
    if (!line.event) {
      return line.text;
    }
    const key = (line.kind === 'error' ? ERROR_KEYS : EVENT_KEYS)[line.event.key];
    if (!key) {
      return line.event.key;
    }
    return $t(key, { values: line.event.values ?? {} });
  }

  function updateScrollState(): void {
    if (!messagesEl) {
      return;
    }
    atBottom = isScrolledToBottom(messagesEl);
  }

  function scrollToLatest(): void {
    atBottom = true;
    if (!messagesEl) {
      return;
    }
    messagesEl.scrollTo({ top: messagesEl.scrollHeight, behavior: 'smooth' });
  }

  function nickStyle(nickname: string): string {
    return `var(--irc-nick-${nickColorIndex(nickname)})`;
  }

  function mentionsMe(text: string): boolean {
    const me = ircStore.nick.toLowerCase();
    return me.length > 0 && text.toLowerCase().includes(me);
  }

  async function connect(): Promise<void> {
    connectError = null;
    sendError = null;
    try {
      saveForm();
      await ircStore.connect({
        nick: nick.trim(),
        account: account.trim() || null,
        password: password || null,
        channel: channel.trim() || DEFAULT_CHANNEL,
        server: server.trim() || DEFAULT_SERVER,
        port: Number(port) || DEFAULT_PORT,
      });
    } catch (error) {
      connectError = typeof error === 'string' ? error : 'internal';
    }
  }

  async function disconnect(): Promise<void> {
    await ircStore.disconnect();
    atBottom = true;
  }

  async function submitDraft(event?: SubmitEvent): Promise<void> {
    event?.preventDefault();
    const value = draft;
    if (!value.trim()) {
      return;
    }
    sendError = null;
    try {
      if (await ircStore.send(value)) {
        draft = '';
        atBottom = true;
      }
    } catch (error) {
      sendError = typeof error === 'string' ? error : 'internal';
    }
  }

  function handleComposerKeydown(event: KeyboardEvent): void {
    if (event.key === 'Enter' && !event.shiftKey && !event.isComposing) {
      event.preventDefault();
      void submitDraft();
    }
  }
</script>

<div class="irc-page workspace-page">
  <header class="irc-header">
    <PageHeading title={$t('irc.title')} description={$t('irc.description')} />
    <div class="irc-header-actions">
      <span class="irc-status" data-status={ircStore.status}>
        <span class="irc-status-dot" aria-hidden="true"></span>
        {$t(statusKey)}
      </span>
      {#if connected}
        <FluentButton data-fluent-owner="routes-home-irc-page" appearance="subtle" onclick={disconnect}>
          <Icon name="linkOff" size="16px" />
          <span>{$t('irc.disconnect')}</span>
        </FluentButton>
      {/if}
    </div>
  </header>

  {#if !runtimeAvailable}
    <p class="irc-notice" role="status">{$t('irc.backendUnavailable')}</p>
  {/if}

  {#if !connected}
    <div class="irc-connect-wrap">
      <section class="irc-connect" aria-labelledby="irc-connect-title">
        <div class="irc-connect-copy">
          <h2 id="irc-connect-title">{$t('irc.connectTitle')}</h2>
          <p>{$t('irc.connectHint')}</p>
        </div>

        <div class="irc-fields">
          <MdOutlinedField inputId="irc-nick" label={$t('irc.nick')} disabled={ircStore.pending}>
            <input
              id="irc-nick"
              class="cfms-native-field"
              data-focus-ring="delegated"
              bind:value={nick}
              autocomplete="off"
              spellcheck="false"
            />
          </MdOutlinedField>

          <MdOutlinedField inputId="irc-account" label={$t('irc.account')} disabled={ircStore.pending}>
            <input
              id="irc-account"
              class="cfms-native-field"
              data-focus-ring="delegated"
              bind:value={account}
              autocomplete="username"
              spellcheck="false"
              placeholder={$t('irc.accountHint')}
            />
          </MdOutlinedField>

          <MdOutlinedField inputId="irc-password" label={$t('irc.password')} disabled={ircStore.pending}>
            <input
              id="irc-password"
              class="cfms-native-field"
              data-focus-ring="delegated"
              type="password"
              bind:value={password}
              autocomplete="current-password"
            />
          </MdOutlinedField>

          <MdOutlinedField inputId="irc-channel" label={$t('irc.channel')} disabled={ircStore.pending}>
            <input
              id="irc-channel"
              class="cfms-native-field"
              data-focus-ring="delegated"
              bind:value={channel}
              autocomplete="off"
              spellcheck="false"
            />
          </MdOutlinedField>
        </div>

        <p class="irc-hint">{$t('irc.passwordNote')}</p>

        <details class="irc-advanced" bind:open={advancedOpen}>
          <summary>{$t('irc.advanced')}</summary>
          <div class="irc-fields irc-fields--advanced">
            <MdOutlinedField inputId="irc-server" label={$t('irc.server')} disabled={ircStore.pending}>
              <input
                id="irc-server"
                class="cfms-native-field"
                data-focus-ring="delegated"
                bind:value={server}
                autocomplete="off"
                spellcheck="false"
              />
            </MdOutlinedField>
            <MdOutlinedField inputId="irc-port" label={$t('irc.port')} disabled={ircStore.pending}>
              <input
                id="irc-port"
                class="cfms-native-field"
                data-focus-ring="delegated"
                type="text"
                inputmode="numeric"
                bind:value={port}
              />
            </MdOutlinedField>
          </div>
          <p class="irc-hint">{$t('irc.serverNote', { values: { server: DEFAULT_SERVER, port: String(DEFAULT_PORT) } })}</p>
        </details>

        {#if connectError || ircStore.error}
          <p class="irc-error" role="alert">
            <Icon name="errorFilled" size="16px" />
            <span>{describeError(connectError ?? ircStore.error)}</span>
          </p>
        {/if}

        <div class="irc-connect-actions">
          <FluentButton
            data-fluent-owner="routes-home-irc-page"
            appearance="primary"
            disabled={!canConnect || !runtimeAvailable}
            onclick={connect}
          >
            {$t(ircStore.pending || ircStore.status === 'connecting' ? 'irc.connecting' : 'irc.connect')}
          </FluentButton>
        </div>
      </section>
    </div>
  {:else}
    <div class="irc-body">
      <section class="irc-room" aria-label={$t('irc.title')}>
        <header class="irc-room-header">
          <div class="irc-room-copy">
            <h2>{ircStore.channel}</h2>
            <p class:irc-room-topic--empty={!ircStore.topic}>
              {ircStore.topic || $t('irc.noTopic')}
            </p>
          </div>
          <div class="irc-room-tools">
            {#if !atBottom}
              <button type="button" class="irc-jump" onclick={scrollToLatest}>
                <Icon name="arrowDownward" size="14px" />
                <span>{$t('irc.jumpToLatest')}</span>
              </button>
            {/if}
            <span class="irc-room-members">{$t('irc.memberCount', { values: { count: ircStore.members.length } })}</span>
          </div>
        </header>

        <div class="irc-messages" bind:this={messagesEl} onscroll={updateScrollState} role="log" aria-live="polite">
          {#if ircStore.lines.length === 0}
            <p class="irc-empty">{$t('irc.logEmpty')}</p>
          {/if}
          {#each ircStore.lines as line (line.id)}
            {#if line.kind === 'system'}
              <p class="irc-line irc-line--system">{describeLine(line)}</p>
            {:else if line.kind === 'error'}
              <p class="irc-line irc-line--error">
                <Icon name="errorFilled" size="14px" />
                <span>{describeLine(line)}</span>
              </p>
            {:else}
              <article class="irc-line" class:irc-line--mention={line.kind === 'message' && mentionsMe(line.text)}>
                <time class="irc-line-time" datetime={new Date(line.at).toISOString()}>{formatIrcClock(line.at)}</time>
                <span class="irc-line-nick" style:color={nickStyle(line.from)}>{line.from}</span>
                {#if line.kind === 'action'}
                  <span class="irc-line-action">{$t('irc.action', { values: { nick: line.from, text: stripIrcFormatting(line.text) } })}</span>
                {:else if line.kind === 'notice'}
                  <span class="irc-line-notice">{stripIrcFormatting(line.text)}</span>
                {:else}
                  <span class="irc-line-text">{stripIrcFormatting(line.text)}</span>
                {/if}
              </article>
            {/if}
          {/each}
        </div>

        <form class="irc-composer" onsubmit={submitDraft}>
          <div class="irc-composer-field cfms-field-surface">
            <textarea
              class="cfms-native-field"
              data-focus-ring="delegated"
              bind:value={draft}
              onkeydown={handleComposerKeydown}
              rows="1"
              spellcheck="false"
              placeholder={$t('irc.composerPlaceholder', { values: { channel: ircStore.channel } })}
              aria-label={$t('irc.composerPlaceholder', { values: { channel: ircStore.channel } })}
            ></textarea>
          </div>
          <FluentButton
            data-fluent-owner="routes-home-irc-page"
            appearance="primary"
            type="submit"
            disabled={!draft.trim()}
          >
            <Icon name="arrowForward" size="16px" />
            <span>{$t('irc.send')}</span>
          </FluentButton>
        </form>

        {#if sendError}
          <p class="irc-error" role="alert">
            <Icon name="errorFilled" size="16px" />
            <span>{describeError(sendError)}</span>
          </p>
        {/if}
      </section>

      <aside class="irc-members" aria-label={$t('irc.members')}>
        <h3>{$t('irc.members')}</h3>
        <ul>
          {#each sortedMembers as member (member.nick)}
            <li class:irc-member--self={member.nick === ircStore.nick}>
              {#if member.mode}
                <span class="irc-member-mode" aria-hidden="true">{member.mode}</span>
              {/if}
              <span class="irc-member-nick" style:color={nickStyle(member.nick)}>{member.nick}</span>
            </li>
          {/each}
        </ul>
      </aside>
    </div>
  {/if}
</div>

<style>
  .irc-page {
    --irc-nick-0: var(--colorPaletteBlueForeground2);
    --irc-nick-1: var(--colorPaletteCranberryForeground2);
    --irc-nick-2: var(--colorPaletteGreenForeground2);
    --irc-nick-3: var(--colorPaletteDarkOrangeForeground2);
    --irc-nick-4: var(--colorPaletteGrapeForeground2);
    --irc-nick-5: var(--colorPaletteTealForeground2, var(--colorPaletteCornflowerForeground2));
    --irc-nick-6: var(--colorPaletteBerryForeground2);
    --irc-nick-7: var(--colorPaletteGoldForeground2);
    display: flex;
    height: 100%;
    min-width: 0;
    min-height: 0;
    flex-direction: column;
    gap: 16px;
    overflow: hidden;
    padding: 24px clamp(1rem, 2vw, 1.5rem);
  }

  .irc-header {
    display: flex;
    flex: none;
    align-items: flex-start;
    justify-content: space-between;
    gap: 16px;
  }

  .irc-header-actions {
    display: flex;
    flex: none;
    align-items: center;
    gap: 10px;
  }

  .irc-status {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    border: 1px solid var(--explorer-border);
    border-radius: var(--borderRadiusCircular);
    padding: 4px 10px;
    color: var(--explorer-text-muted);
    background: var(--explorer-surface-raised);
    font: 400 12px/16px var(--fontFamilyBase);
  }

  .irc-status-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--explorer-text-muted);
  }

  .irc-status[data-status='connecting'] .irc-status-dot { background: var(--colorPaletteDarkOrangeForeground2); }
  .irc-status[data-status='connected'] .irc-status-dot { background: var(--colorPaletteGoldForeground2); }
  .irc-status[data-status='registered'] .irc-status-dot { background: var(--colorPaletteGreenForeground2); }
  .irc-status[data-status='registered'] { color: var(--explorer-text); }

  .irc-notice {
    margin: 0;
    border: 1px solid var(--explorer-border);
    border-radius: var(--borderRadiusMedium);
    padding: 10px 12px;
    color: var(--explorer-text-muted);
    background: var(--explorer-surface-raised);
    font: 400 12px/18px var(--fontFamilyBase);
  }

  /* `margin: auto` centres the card and, unlike `align-items: center`, never
     pushes it out of reach when the window is shorter than the form. */
  .irc-connect-wrap {
    display: flex;
    min-height: 0;
    flex: 1;
    overflow-y: auto;
  }

  .irc-connect {
    display: flex;
    width: min(460px, 100%);
    flex-direction: column;
    gap: 14px;
    margin: auto;
    border: 1px solid var(--explorer-border);
    border-radius: var(--borderRadiusLarge, 8px);
    padding: 20px;
    background: var(--explorer-surface);
    box-shadow: 0 8px 24px -18px rgb(0 0 0 / 45%);
  }

  .irc-connect-copy h2 { margin: 0; color: var(--explorer-text); font: 600 16px/22px var(--fontFamilyBase); }
  .irc-connect-copy p { margin: 4px 0 0; color: var(--explorer-text-muted); font: 400 12px/18px var(--fontFamilyBase); }

  .irc-fields { display: grid; gap: 10px; }
  .irc-fields--advanced { margin-top: 10px; grid-template-columns: minmax(0, 1fr) 120px; }

  .irc-hint { margin: 0; color: var(--explorer-text-muted); font: 400 11px/16px var(--fontFamilyBase); }

  .irc-advanced { border-top: 1px dashed var(--explorer-border); padding-top: 10px; }
  .irc-advanced summary {
    cursor: pointer;
    color: var(--explorer-text-muted);
    font: 400 12px/18px var(--fontFamilyBase);
  }
  .irc-advanced summary:hover { color: var(--explorer-text); }
  .irc-advanced .irc-hint { margin-top: 8px; }

  .irc-error {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0;
    color: var(--colorPaletteRedForeground2, #b42318);
    font: 400 12px/18px var(--fontFamilyBase);
  }

  .irc-connect-actions { display: flex; justify-content: flex-end; }

  .irc-body {
    display: grid;
    min-height: 0;
    flex: 1;
    grid-template-columns: minmax(0, 1fr) 200px;
    gap: 14px;
  }

  .irc-room {
    display: flex;
    min-width: 0;
    min-height: 0;
    flex-direction: column;
    border: 1px solid var(--explorer-border);
    border-radius: var(--borderRadiusLarge, 8px);
    background: var(--explorer-surface);
    overflow: hidden;
  }

  .irc-room-header {
    display: flex;
    flex: none;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
    border-bottom: 1px solid var(--explorer-border);
    padding: 10px 14px;
  }

  .irc-room-copy { min-width: 0; }
  .irc-room-copy h2 { margin: 0; color: var(--explorer-text); font: 600 14px/20px var(--fontFamilyBase); }
  .irc-room-copy p {
    overflow: hidden;
    margin: 2px 0 0;
    color: var(--explorer-text-muted);
    font: 400 12px/18px var(--fontFamilyBase);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .irc-room-copy p.irc-room-topic--empty { opacity: 0.7; font-style: italic; }

  .irc-room-tools { display: flex; flex: none; align-items: center; gap: 10px; }

  .irc-room-members { flex: none; color: var(--explorer-text-muted); font: 400 12px/18px var(--fontFamilyBase); }

  .irc-jump {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    border: 1px solid var(--explorer-border);
    border-radius: var(--borderRadiusCircular);
    padding: 2px 8px;
    color: var(--explorer-text-muted);
    background: var(--explorer-surface-raised);
    font: 400 11px/16px var(--fontFamilyBase);
    cursor: pointer;
    transition: color 120ms var(--motion-easing-standard), border-color 120ms var(--motion-easing-standard);
  }
  .irc-jump:hover { border-color: var(--explorer-accent); color: var(--explorer-accent); }

  .irc-messages {
    display: flex;
    min-height: 0;
    flex: 1;
    flex-direction: column;
    gap: 2px;
    overflow-y: auto;
    padding: 10px 14px;
  }

  .irc-empty { margin: auto; color: var(--explorer-text-muted); font: 400 12px/18px var(--fontFamilyBase); }

  .irc-line {
    display: flex;
    align-items: baseline;
    gap: 8px;
    border-radius: var(--borderRadiusSmall, 4px);
    padding: 2px 6px;
    color: var(--explorer-text);
    font: 400 13px/20px var(--fontFamilyBase);
    overflow-wrap: anywhere;
  }

  .irc-line--mention { background: color-mix(in srgb, var(--explorer-accent) 12%, transparent); }

  .irc-line-time {
    flex: none;
    color: var(--explorer-text-muted);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
    opacity: 0.75;
  }

  .irc-line-nick { flex: none; max-width: 40%; font-weight: 600; }
  .irc-line-text { min-width: 0; }
  .irc-line-notice { min-width: 0; color: var(--explorer-text-muted); font-style: italic; }
  .irc-line-action { min-width: 0; color: var(--explorer-text-muted); font-style: italic; }
  .irc-line--system {
    justify-content: center;
    color: var(--explorer-text-muted);
    font-size: 11px;
    font-style: italic;
    opacity: 0.85;
  }
  .irc-line--error { color: var(--colorPaletteRedForeground2, #b42318); font-size: 12px; }

  .irc-composer {
    display: flex;
    flex: none;
    align-items: flex-end;
    gap: 8px;
    border-top: 1px solid var(--explorer-border);
    padding: 10px 14px;
  }

  .irc-composer-field {
    min-width: 0;
    flex: 1;
    border: 1px solid var(--explorer-border);
    border-radius: var(--borderRadiusMedium);
    background: var(--explorer-surface-raised);
    transition: border-color 140ms var(--motion-easing-standard), box-shadow 140ms var(--motion-easing-standard);
  }
  .irc-composer-field:focus-within {
    border-color: var(--explorer-accent);
    box-shadow: inset 0 0 0 1px var(--explorer-accent);
  }
  .irc-composer-field textarea {
    box-sizing: border-box;
    width: 100%;
    max-height: 120px;
    min-height: 34px;
    resize: none;
    border: 0 !important;
    padding: 7px 10px;
    color: var(--explorer-text);
    background: transparent;
    box-shadow: none !important;
    font: 400 13px/20px var(--fontFamilyBase);
    outline: 0;
  }
  .irc-composer-field textarea::placeholder { color: var(--explorer-text-muted); opacity: 1; }

  .irc-members {
    display: flex;
    min-height: 0;
    flex-direction: column;
    border: 1px solid var(--explorer-border);
    border-radius: var(--borderRadiusLarge, 8px);
    background: var(--explorer-surface);
    overflow: hidden;
  }

  .irc-members h3 {
    margin: 0;
    border-bottom: 1px solid var(--explorer-border);
    padding: 10px 12px;
    color: var(--explorer-text-muted);
    font: 600 12px/16px var(--fontFamilyBase);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .irc-members ul { min-height: 0; flex: 1; overflow-y: auto; margin: 0; padding: 6px; list-style: none; }

  .irc-members li {
    display: flex;
    align-items: center;
    gap: 4px;
    border-radius: var(--borderRadiusSmall, 4px);
    padding: 3px 6px;
    font: 400 12px/18px var(--fontFamilyBase);
  }
  .irc-members li:hover { background: var(--explorer-surface-hover); }
  .irc-member--self { background: var(--explorer-surface-selected); }

  .irc-member-mode { flex: none; color: var(--explorer-text-muted); }
  .irc-member-nick { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

  @media (max-width: 900px) {
    .irc-body { grid-template-columns: minmax(0, 1fr); }
    .irc-members { display: none; }
  }
</style>
