import { invoke } from '@tauri-apps/api/core';

/**
 * Importing data from a previously installed CFMS client.
 *
 * The modded build is a standalone application, so a fresh install starts with
 * an empty profile. These calls read the previous client's data directory
 * (never modifying it) and copy the selected parts into this one.
 */

export type MigrationEntryId =
  | 'accountPreferences'
  | 'transferQueue'
  | 'customCa'
  | 'extensions'
  | 'avatars'
  | 'downloadRoot'
  | 'deviceSettings';

export interface MigrationEntrySummary {
  id: MigrationEntryId;
  itemCount: number;
  byteSize: number;
}

export interface MigrationScan {
  supported: boolean;
  source: string | null;
  detectedSources: string[];
  entries: MigrationEntrySummary[];
}

export interface MigrationItemReport {
  id: MigrationEntryId;
  imported: number;
  skipped: number;
  failed: number;
  errors: string[];
}

export interface MigrationReport {
  source: string;
  items: MigrationItemReport[];
  requiresRestart: boolean;
}

export interface MigrationRequest {
  source?: string | null;
  entries: string[];
  overwrite: boolean;
}

export async function scanMigrationSource(source?: string | null): Promise<MigrationScan> {
  return invoke('scan_migration_source', { source: source ?? null });
}

export async function importMigration(request: MigrationRequest): Promise<MigrationReport> {
  return invoke('import_migration', { request });
}
