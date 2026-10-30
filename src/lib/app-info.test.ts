import { describe, expect, it, vi, beforeEach } from 'vitest';
import { get } from 'svelte/store';

const mocks = vi.hoisted(() => ({
  getName: vi.fn(),
  getVersion: vi.fn(),
}));

vi.mock('@tauri-apps/api/app', () => ({
  getName: mocks.getName,
  getVersion: mocks.getVersion,
}));

import { FALLBACK_APP_NAME, appName, loadAppName } from './app-info';

describe('app-info', () => {
  beforeEach(() => {
    appName.set(FALLBACK_APP_NAME);
    mocks.getName.mockReset();
    mocks.getVersion.mockReset();
  });

  it('publishes the product name so every build shows its own identity', async () => {
    mocks.getName.mockResolvedValue('CFMS Modded');

    await expect(loadAppName()).resolves.toBe('CFMS Modded');
    expect(get(appName)).toBe('CFMS Modded');
  });

  it('falls back to the shared name outside the Tauri runtime', async () => {
    mocks.getName.mockRejectedValue(new Error('not in tauri'));

    await expect(loadAppName()).resolves.toBe(FALLBACK_APP_NAME);
    expect(get(appName)).toBe(FALLBACK_APP_NAME);
  });

  it('ignores a blank product name instead of rendering an empty heading', async () => {
    mocks.getName.mockResolvedValue('   ');

    await expect(loadAppName()).resolves.toBe(FALLBACK_APP_NAME);
    expect(get(appName)).toBe(FALLBACK_APP_NAME);
  });
});
