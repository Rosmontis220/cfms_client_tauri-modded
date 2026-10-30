import { getName, getVersion } from '@tauri-apps/api/app';
import { writable } from 'svelte/store';

export const FALLBACK_APP_VERSION = '0.15.0';
/** Shown outside the Tauri runtime (SSR and unit tests). */
export const FALLBACK_APP_NAME = 'CFMS Client';

/**
 * Display name of the running build, read from `productName` in
 * `tauri.conf.json`. The standalone modded build therefore shows "CFMS Modded"
 * while the official and Custom builds keep their own names, instead of every
 * build claiming to be "CFMS Client".
 */
export const appName = writable(FALLBACK_APP_NAME);

export async function loadAppVersion(): Promise<string> {
  try {
    return await getVersion();
  } catch {
    return FALLBACK_APP_VERSION;
  }
}

export async function loadAppName(): Promise<string> {
  try {
    const name = (await getName()).trim();
    if (name) {
      appName.set(name);
      return name;
    }
  } catch {
    /* Not running inside the Tauri runtime. */
  }
  return FALLBACK_APP_NAME;
}
