// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { locale } from 'svelte-i18n';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import '$lib/i18n';
import type { MigrationReport, MigrationScan } from '$lib/api';
import MigrationSettingsPage from './+page.svelte';

const mocks = vi.hoisted(() => ({
  scanMigrationSource: vi.fn(),
  importMigration: vi.fn(),
  pickDirectory: vi.fn(),
  notificationError: vi.fn(),
  notificationSuccess: vi.fn(),
  relaunch: vi.fn(),
}));

vi.mock('$lib/api', () => ({
  scanMigrationSource: mocks.scanMigrationSource,
  importMigration: mocks.importMigration,
}));
vi.mock('$lib/directory-picker', () => ({ pickDirectory: mocks.pickDirectory }));
vi.mock('$lib/stores.svelte', () => ({
  notificationStore: { error: mocks.notificationError, success: mocks.notificationSuccess },
}));
vi.mock('@tauri-apps/plugin-process', () => ({ relaunch: mocks.relaunch }));
vi.mock('$app/state', () => ({
  page: { url: new URL('https://example.test/home/settings/migration') },
}));
vi.mock('$lib/navigation', () => ({ navigateUp: vi.fn() }));

const detectedScan: MigrationScan = {
  supported: true,
  source: '/home/user/.config/org.crpteam.cfms-client-tauri',
  detectedSources: ['/home/user/.config/org.crpteam.cfms-client-tauri'],
  entries: [
    { id: 'accountPreferences', itemCount: 2, byteSize: 4096 },
    { id: 'transferQueue', itemCount: 5, byteSize: 8192 },
  ],
};

const emptyScan: MigrationScan = {
  supported: true,
  source: '/home/user/.config/org.crpteam.cfms-client-tauri',
  detectedSources: ['/home/user/.config/org.crpteam.cfms-client-tauri'],
  entries: [],
};

function report(overrides: Partial<MigrationReport> = {}): MigrationReport {
  return {
    source: detectedScan.source!,
    items: [
      { id: 'accountPreferences', imported: 2, skipped: 0, failed: 0, errors: [] },
      { id: 'transferQueue', imported: 4, skipped: 1, failed: 0, errors: [] },
    ],
    requiresRestart: false,
    ...overrides,
  };
}

beforeEach(() => {
  locale.set('en');
  mocks.scanMigrationSource.mockResolvedValue(detectedScan);
  mocks.importMigration.mockResolvedValue(report());
  mocks.pickDirectory.mockResolvedValue(null);
  mocks.relaunch.mockResolvedValue(undefined);
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('migration settings page', () => {
  it('lists the detected source and imports every entry by default', async () => {
    render(MigrationSettingsPage);

    await waitFor(() => expect(screen.getByText('Account preferences')).toBeTruthy());
    expect(screen.getByText('Transfer queue')).toBeTruthy();
    expect(screen.getByText(detectedScan.source!)).toBeTruthy();

    await fireEvent.click(screen.getByRole('button', { name: /Start import/ }));

    await waitFor(() => expect(mocks.importMigration).toHaveBeenCalledOnce());
    expect(mocks.importMigration).toHaveBeenCalledWith({
      source: detectedScan.source,
      entries: ['accountPreferences', 'transferQueue'],
      overwrite: false,
    });
    await waitFor(() => expect(screen.getByText('Import results')).toBeTruthy());
    expect(mocks.notificationSuccess).toHaveBeenCalled();
  });

  it('offers a folder picker when nothing was detected', async () => {
    mocks.scanMigrationSource.mockResolvedValue({
      supported: true,
      source: null,
      detectedSources: [],
      entries: [],
    });
    mocks.pickDirectory.mockResolvedValue({
      path: '/mnt/backup/org.crpteam.cfms-client-tauri',
      name: 'org.crpteam.cfms-client-tauri',
    });

    render(MigrationSettingsPage);

    await waitFor(() => expect(screen.getByText('No previous installation found')).toBeTruthy());
    await fireEvent.click(screen.getByRole('button', { name: /Choose data folder/ }));

    await waitFor(() =>
      expect(mocks.scanMigrationSource).toHaveBeenLastCalledWith(
        '/mnt/backup/org.crpteam.cfms-client-tauri',
      ),
    );
    expect(screen.queryByRole('button', { name: /Start import/ })).toBeNull();
  });

  it('asks for a restart only when application settings were imported', async () => {
    mocks.importMigration.mockResolvedValue(report({ requiresRestart: true }));

    render(MigrationSettingsPage);

    await waitFor(() => expect(screen.getByText('Account preferences')).toBeTruthy());
    await fireEvent.click(screen.getByRole('button', { name: /Start import/ }));

    await waitFor(() => expect(screen.getByText('Restart to apply the settings')).toBeTruthy());
    await fireEvent.click(screen.getByRole('button', { name: /Restart client/ }));
    expect(mocks.relaunch).toHaveBeenCalledOnce();
  });

  it('explains that the folder holds nothing to import', async () => {
    mocks.scanMigrationSource.mockResolvedValue(emptyScan);

    render(MigrationSettingsPage);

    await waitFor(() => expect(screen.getByText('This folder holds nothing to import')).toBeTruthy());
    expect(screen.queryByRole('button', { name: /Start import/ })).toBeNull();
  });

  it('explains why automatic import is unavailable on Android', async () => {
    mocks.scanMigrationSource.mockResolvedValue({
      supported: false,
      source: null,
      detectedSources: [],
      entries: [],
    });

    render(MigrationSettingsPage);

    await waitFor(() =>
      expect(screen.getByText('Automatic import is unavailable here')).toBeTruthy(),
    );
    expect(mocks.scanMigrationSource).toHaveBeenCalledOnce();
    expect(screen.queryByRole('button', { name: /Start import/ })).toBeNull();
  });
});
