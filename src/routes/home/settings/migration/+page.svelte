<script lang="ts">
  // Import local data from a previously installed CFMS client.
  //
  // This build is a standalone application with its own data directory, so a
  // fresh install has an empty profile. The page is reachable without signing
  // in: device settings and account preferences are worth importing before the
  // first login, and the transfer queue only matters afterwards.

  import { _ as t } from 'svelte-i18n';
  import { onMount } from 'svelte';
  import { relaunch } from '@tauri-apps/plugin-process';
  import FluentButton from '$lib/components/FluentButton.svelte';
  import Icon from '$lib/components/Icon.svelte';
  import MdSwitch from '$lib/components/MdSwitch.svelte';
  import ProgressRing from '$lib/components/ProgressRing.svelte';
  import SettingsPageHeader from '$lib/components/SettingsPageHeader.svelte';
  import {
    importMigration,
    scanMigrationSource,
    type MigrationEntrySummary,
    type MigrationReport,
    type MigrationScan,
  } from '$lib/api';
  import { pickDirectory } from '$lib/directory-picker';
  import { formatBytes } from '$lib/files/formatting';
  import { notificationStore } from '$lib/stores.svelte';

  // Full literal keys keep the translation catalog check meaningful.
  const ENTRY_COPY: Record<string, { label: string; hint: string }> = {
    accountPreferences: {
      label: 'settings.migration.entries.accountPreferences.label',
      hint: 'settings.migration.entries.accountPreferences.hint',
    },
    transferQueue: {
      label: 'settings.migration.entries.transferQueue.label',
      hint: 'settings.migration.entries.transferQueue.hint',
    },
    customCa: {
      label: 'settings.migration.entries.customCa.label',
      hint: 'settings.migration.entries.customCa.hint',
    },
    extensions: {
      label: 'settings.migration.entries.extensions.label',
      hint: 'settings.migration.entries.extensions.hint',
    },
    avatars: {
      label: 'settings.migration.entries.avatars.label',
      hint: 'settings.migration.entries.avatars.hint',
    },
    downloadRoot: {
      label: 'settings.migration.entries.downloadRoot.label',
      hint: 'settings.migration.entries.downloadRoot.hint',
    },
    deviceSettings: {
      label: 'settings.migration.entries.deviceSettings.label',
      hint: 'settings.migration.entries.deviceSettings.hint',
    },
  };

  let scan = $state<MigrationScan | null>(null);
  let scanning = $state(true);
  let importing = $state(false);
  let scanError = $state<string | null>(null);
  let report = $state<MigrationReport | null>(null);
  let selected = $state<string[]>([]);
  let overwrite = $state(false);

  const entries = $derived<MigrationEntrySummary[]>(scan?.entries ?? []);
  const source = $derived(scan?.source ?? null);
  const canImport = $derived(!importing && !!source && selected.length > 0);

  async function loadScan(path?: string | null) {
    scanning = true;
    scanError = null;
    try {
      const result = await scanMigrationSource(path ?? null);
      scan = result;
      selected = result.entries.map((entry) => entry.id);
    } catch (error) {
      scanError = error instanceof Error ? error.message : String(error);
    } finally {
      scanning = false;
    }
  }

  function toggle(id: string, checked: boolean) {
    selected = checked
      ? [...new Set([...selected, id])]
      : selected.filter((entry) => entry !== id);
  }

  async function chooseFolder() {
    const picked = await pickDirectory({ title: $t('settings.migration.pickFolderTitle') });
    if (!picked) return;
    report = null;
    await loadScan(picked.path);
  }

  async function runImport() {
    if (!canImport) return;
    importing = true;
    report = null;
    try {
      report = await importMigration({ source, entries: selected, overwrite });
      const failed = report.items.reduce((total, item) => total + item.failed, 0);
      const imported = report.items.reduce((total, item) => total + item.imported, 0);
      if (failed > 0) {
        notificationStore.error($t('settings.migration.importFailed'));
      } else if (imported > 0) {
        notificationStore.success($t('settings.migration.importDone'));
      }
      // Refresh counters so already-imported items show up as existing data.
      await loadScan(source);
      selected = report.items.map((item) => item.id);
    } catch (error) {
      notificationStore.error(error instanceof Error ? error.message : String(error));
    } finally {
      importing = false;
    }
  }

  async function restartApp() {
    try {
      await relaunch();
    } catch (error) {
      notificationStore.error(error instanceof Error ? error.message : String(error));
    }
  }

  onMount(() => {
    void loadScan();
  });
</script>

<div class="workspace-page settings-page-shell">
  <SettingsPageHeader
    title={$t('settings.migration.title')}
    description={$t('settings.migration.description')}
    icon="restoreDialog"
    onRefresh={() => loadScan(source)}
    refreshDisabled={scanning || importing}
  />

  <div class="settings-section-list">
    <section class="settings-section space-y-4" aria-labelledby="migration-source-heading">
      <div class="settings-section-heading">
        <h2
          id="migration-source-heading"
          class="text-sm font-semibold text-md3-on-surface"
          style="font-family: var(--font-md3-sans);"
        >
          {$t('settings.migration.sourceTitle')}
        </h2>
      </div>

      {#if scanning}
        <div class="migration-status">
          <ProgressRing size={20} strokeWidth={2.4} label={$t('settings.migration.scanning')} />
          <span>{$t('settings.migration.scanning')}</span>
        </div>
      {:else if scan && !scan.supported}
        <div class="migration-notice">
          <Icon name="info" size="18px" />
          <div>
            <p class="migration-notice-title">{$t('settings.migration.unsupportedTitle')}</p>
            <p class="migration-notice-body">{$t('settings.migration.unsupportedDescription')}</p>
          </div>
        </div>
      {:else}
        {#if scanError}
          <div class="migration-notice is-error">
            <Icon name="warning" size="18px" />
            <div>
              <p class="migration-notice-title">{$t('settings.migration.scanFailed')}</p>
              <p class="migration-notice-body">{scanError}</p>
            </div>
          </div>
        {/if}

        {#if source}
          <div class="migration-source">
            <Icon name="folderOpen" size="20px" />
            <code class="migration-path">{source}</code>
          </div>
        {:else}
          <div class="migration-notice">
            <Icon name="info" size="18px" />
            <div>
              <p class="migration-notice-title">{$t('settings.migration.noSourceTitle')}</p>
              <p class="migration-notice-body">{$t('settings.migration.noSourceDescription')}</p>
            </div>
          </div>
        {/if}

        <div class="migration-actions">
          <FluentButton data-fluent-owner="routes-home-settings-migration-page"
            type="button"
            onclick={chooseFolder}
            disabled={importing}
            appearance="subtle"
          >
            <Icon name="folderOpen" size="18px" />
            {source ? $t('settings.migration.changeFolder') : $t('settings.migration.chooseFolder')}
          </FluentButton>
        </div>
      {/if}
    </section>

    {#if scan?.supported && entries.length > 0}
      <section class="settings-section space-y-4" aria-labelledby="migration-entries-heading">
        <div class="settings-section-heading">
          <h2
            id="migration-entries-heading"
            class="text-sm font-semibold text-md3-on-surface"
            style="font-family: var(--font-md3-sans);"
          >
            {$t('settings.migration.entriesTitle')}
          </h2>
          <p class="mt-1 text-xs leading-5 text-md3-on-surface-variant">
            {$t('settings.migration.entriesHint')}
          </p>
        </div>

        <ul class="migration-entry-list">
          {#each entries as entry (entry.id)}
            <li class="migration-entry">
              <div class="migration-entry-copy">
                <span class="migration-entry-label">
                  {ENTRY_COPY[entry.id] ? $t(ENTRY_COPY[entry.id].label) : entry.id}
                </span>
                <span class="migration-entry-hint">
                  {ENTRY_COPY[entry.id] ? $t(ENTRY_COPY[entry.id].hint) : ''}
                </span>
                <span class="migration-entry-meta">
                  {$t('settings.migration.entryMeta', {
                    values: {
                      count: entry.itemCount,
                      size: formatBytes(entry.byteSize),
                    },
                  })}
                </span>
              </div>
              <MdSwitch
                checked={selected.includes(entry.id)}
                disabled={importing}
                ariaLabel={ENTRY_COPY[entry.id] ? $t(ENTRY_COPY[entry.id].label) : entry.id}
                onChange={(checked) => toggle(entry.id, checked)}
              />
            </li>
          {/each}
        </ul>

        <div class="migration-overwrite">
          <MdSwitch
            bind:checked={overwrite}
            disabled={importing}
            ariaLabel={$t('settings.migration.overwriteLabel')}
          />
          <div class="migration-entry-copy">
            <span class="migration-entry-label">{$t('settings.migration.overwriteLabel')}</span>
            <span class="migration-entry-hint">{$t('settings.migration.overwriteHint')}</span>
          </div>
        </div>

        <FluentButton data-fluent-owner="routes-home-settings-migration-page"
          type="button"
          onclick={runImport}
          disabled={!canImport}
          appearance="primary"
        >
          {#if importing}
            <ProgressRing size={18} strokeWidth={2.4} label={$t('settings.migration.importing')} />
          {:else}
            <Icon name="restoreDialog" size="18px" />
          {/if}
          {importing ? $t('settings.migration.importing') : $t('settings.migration.importAction')}
        </FluentButton>
      </section>
    {:else if scan?.supported && !scanning && source}
      <section class="settings-section space-y-4">
        <div class="migration-notice">
          <Icon name="info" size="18px" />
          <div>
            <p class="migration-notice-title">{$t('settings.migration.emptyTitle')}</p>
            <p class="migration-notice-body">{$t('settings.migration.emptyDescription')}</p>
          </div>
        </div>
      </section>
    {/if}

    {#if report}
      <section class="settings-section space-y-4" aria-labelledby="migration-report-heading">
        <div class="settings-section-heading">
          <h2
            id="migration-report-heading"
            class="text-sm font-semibold text-md3-on-surface"
            style="font-family: var(--font-md3-sans);"
          >
            {$t('settings.migration.reportTitle')}
          </h2>
        </div>

        <ul class="migration-entry-list">
          {#each report.items as item (item.id)}
            <li class="migration-entry">
              <div class="migration-entry-copy">
                <span class="migration-entry-label">
                  {ENTRY_COPY[item.id] ? $t(ENTRY_COPY[item.id].label) : item.id}
                </span>
                <span class="migration-entry-meta" data-tone={item.failed > 0 ? 'error' : 'neutral'}>
                  {$t('settings.migration.reportItem', {
                    values: {
                      imported: item.imported,
                      skipped: item.skipped,
                      failed: item.failed,
                    },
                  })}
                </span>
                {#each item.errors as message (message)}
                  <span class="migration-entry-error">{message}</span>
                {/each}
              </div>
            </li>
          {/each}
        </ul>

        {#if report.requiresRestart}
          <div class="migration-notice is-warning">
            <Icon name="warning" size="18px" />
            <div>
              <p class="migration-notice-title">{$t('settings.migration.restartTitle')}</p>
              <p class="migration-notice-body">{$t('settings.migration.restartDescription')}</p>
            </div>
          </div>
          <FluentButton data-fluent-owner="routes-home-settings-migration-page"
            type="button"
            onclick={restartApp}
            appearance="subtle"
          >
            <Icon name="restartAlt" size="18px" />
            {$t('settings.migration.restartAction')}
          </FluentButton>
        {/if}
      </section>
    {/if}
  </div>
</div>

<style>
  .migration-status {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    font-size: 0.85rem;
    color: var(--color-md3-on-surface-variant);
  }

  .migration-notice {
    display: flex;
    gap: 0.6rem;
    padding: 0.75rem 0.9rem;
    border-radius: var(--radius-md);
    background: color-mix(in srgb, var(--color-md3-surface-container-high) 72%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-md3-outline) 40%, transparent);
  }

  .migration-notice.is-error {
    border-color: color-mix(in srgb, var(--color-md3-error) 55%, transparent);
  }

  .migration-notice.is-warning {
    border-color: color-mix(in srgb, var(--color-md3-primary-emphasis) 45%, transparent);
  }

  .migration-notice-title {
    margin: 0;
    font-size: 0.85rem;
    font-weight: 600;
    color: var(--color-md3-on-surface);
  }

  .migration-notice-body {
    margin: 0.2rem 0 0;
    font-size: 0.78rem;
    line-height: 1.6;
    color: var(--color-md3-on-surface-variant);
  }

  .migration-source {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.8rem;
    color: var(--color-md3-on-surface-variant);
  }

  .migration-path {
    overflow-wrap: anywhere;
    font-family: var(--font-mono);
    font-size: 0.78rem;
  }

  .migration-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
  }

  .migration-entry-list {
    display: grid;
    gap: 0.25rem;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .migration-entry {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
    padding: 0.6rem 0.15rem;
    border-bottom: 1px solid color-mix(in srgb, var(--color-md3-outline-variant) 55%, transparent);
  }

  .migration-entry:last-child {
    border-bottom: none;
  }

  .migration-entry-copy {
    display: grid;
    gap: 0.15rem;
    min-width: 0;
  }

  .migration-entry-label {
    font-size: 0.85rem;
    color: var(--color-md3-on-surface);
  }

  .migration-entry-hint {
    font-size: 0.76rem;
    color: var(--color-md3-on-surface-variant);
  }

  .migration-entry-meta {
    font-size: 0.74rem;
    color: var(--color-md3-on-surface-variant);
    font-variant-numeric: tabular-nums;
  }

  .migration-entry-meta[data-tone='error'] {
    color: var(--color-md3-error);
  }

  .migration-entry-error {
    font-size: 0.72rem;
    color: var(--color-md3-error);
    overflow-wrap: anywhere;
  }

  .migration-overwrite {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding-top: 0.25rem;
  }
</style>
