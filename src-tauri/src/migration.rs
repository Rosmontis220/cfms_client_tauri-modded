//! Importing local data from a previously installed CFMS client.
//!
//! This build ships as its own application, so its data directory (and its OS
//! keyring services) differ from the official and Custom clients. A user who
//! switches to this build starts with an empty profile unless the previous
//! data is copied over, which is what this module does.
//!
//! Two rules shape the implementation:
//!
//! * The source is only ever read. Nothing here deletes or rewrites data in
//!   the source directory.
//! * Only the paths listed in [`COPY_ENTRIES`] (plus the settings table) are
//!   touched, so a caller can never point the importer at arbitrary files.
//!
//! Cache-like directories (`preview_cache`, the WebView profile, logs) are
//! intentionally skipped: they are regenerated on demand and copying them
//! would either be huge or corrupt the running WebView.

// Android only exposes the "unsupported" answer, so the importer below has no
// callers there.
#![cfg_attr(target_os = "android", allow(dead_code))]

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Component, Path, PathBuf};

/// Identifiers used by the clients this build can import from. The official
/// client, the Custom build and the first modded build all shipped under the
/// same identifier, so one directory covers all of them.
pub const LEGACY_IDENTIFIERS: &[&str] = &["org.crpteam.cfms-client-tauri"];

/// Identifier of this build. Used to refuse importing a directory into itself.
pub const CURRENT_IDENTIFIER: &str = "com.rosmontis220.cfmsmodded";

const LEGACY_DATABASE_FILE: &str = "cfms_client.db";

/// Guard against pathological sources (a user picking their home folder).
const MAX_MEASURED_ENTRIES: u64 = 200_000;
const MAX_ERRORS_PER_ITEM: usize = 4;

struct CopyEntry {
    id: &'static str,
    paths: &'static [&'static str],
}

/// Every file/directory the importer is allowed to copy, relative to the
/// source data directory.
const COPY_ENTRIES: &[CopyEntry] = &[
    CopyEntry {
        id: "accountPreferences",
        paths: &["user_preferences"],
    },
    CopyEntry {
        id: "transferQueue",
        paths: &["download_tasks", "upload_tasks"],
    },
    CopyEntry {
        id: "customCa",
        paths: &["ca"],
    },
    CopyEntry {
        id: "extensions",
        paths: &["com_ext"],
    },
    CopyEntry {
        id: "avatars",
        paths: &["avatars"],
    },
    CopyEntry {
        id: "downloadRoot",
        paths: &[".cfms-download-root"],
    },
];

/// Settings live in SQLite rather than in files, so they get their own id.
pub const SETTINGS_ENTRY_ID: &str = "deviceSettings";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationEntrySummary {
    pub id: String,
    pub item_count: u64,
    pub byte_size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationScan {
    /// False on platforms where another app's private data cannot be read.
    pub supported: bool,
    /// The directory that would be imported, when one was found.
    pub source: Option<String>,
    /// Every legacy directory that exists, so the UI can offer a choice.
    pub detected_sources: Vec<String>,
    pub entries: Vec<MigrationEntrySummary>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationRequest {
    /// `None` selects the first detected source.
    pub source: Option<String>,
    /// Entry ids to import. Empty means "everything that is available".
    pub entries: Vec<String>,
    pub overwrite: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationItemReport {
    pub id: String,
    pub imported: u32,
    pub skipped: u32,
    pub failed: u32,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationReport {
    pub source: String,
    pub items: Vec<MigrationItemReport>,
    /// True when device settings were imported: the language and appearance
    /// already loaded in this process cannot pick them up without a restart.
    pub requires_restart: bool,
}

/// Access to the current application's settings store. Abstracted so the
/// importer can be unit-tested without a running Tauri application.
pub trait SettingsBridge {
    fn get(&self, key: &str) -> Result<Option<String>, String>;
    fn set(&mut self, key: &str, value: &str) -> Result<(), String>;
}

#[cfg(target_os = "android")]
pub fn unsupported_scan() -> MigrationScan {
    MigrationScan {
        supported: false,
        source: None,
        detected_sources: Vec::new(),
        entries: Vec::new(),
    }
}

/// Legacy data directories that sit next to `current_data_dir`.
///
/// Every desktop platform resolves the application data directory to
/// `<base>/<identifier>`, so the parent directory is the right place to look
/// regardless of the platform conventions below it.
pub fn candidate_sources(current_data_dir: &Path) -> Vec<PathBuf> {
    let Some(parent) = current_data_dir.parent() else {
        return Vec::new();
    };
    LEGACY_IDENTIFIERS
        .iter()
        .map(|identifier| parent.join(identifier))
        .filter(|path| path != current_data_dir)
        .collect()
}

pub fn resolve_source(current_data_dir: &Path, requested: Option<&str>) -> Result<PathBuf, String> {
    if let Some(requested) = requested {
        let path = PathBuf::from(requested.trim());
        validate_source(&path, current_data_dir)?;
        return Ok(path);
    }
    candidate_sources(current_data_dir)
        .into_iter()
        .find(|path| path.is_dir())
        .ok_or_else(|| "No previous client data was found.".to_string())
}

/// Build the scan payload for the settings page: which previous installations
/// exist and what each of them holds.
pub fn scan_for_target(
    current_data_dir: &Path,
    requested: Option<&str>,
) -> Result<MigrationScan, String> {
    let detected = candidate_sources(current_data_dir)
        .into_iter()
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();

    let source = match requested.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => {
            let path = PathBuf::from(value);
            validate_source(&path, current_data_dir)?;
            Some(path)
        }
        None => detected.first().cloned(),
    };

    let entries = match source.as_deref() {
        Some(source) => scan_source(source)?,
        None => Vec::new(),
    };

    Ok(MigrationScan {
        supported: true,
        source: source.map(|path| path.display().to_string()),
        detected_sources: detected
            .iter()
            .map(|path| path.display().to_string())
            .collect(),
        entries,
    })
}

pub fn scan_source(source: &Path) -> Result<Vec<MigrationEntrySummary>, String> {
    let mut entries = Vec::new();
    for entry in COPY_ENTRIES {
        let mut item_count = 0;
        let mut byte_size = 0;
        for relative in entry.paths {
            let path = source.join(relative);
            let (count, bytes) = measure(&path, 0)?;
            item_count += count;
            byte_size += bytes;
        }
        if item_count > 0 {
            entries.push(MigrationEntrySummary {
                id: entry.id.to_string(),
                item_count,
                byte_size,
            });
        }
    }

    let database = source.join(LEGACY_DATABASE_FILE);
    if database.is_file() {
        let settings = read_legacy_settings(&database).unwrap_or_default();
        if !settings.is_empty() {
            entries.push(MigrationEntrySummary {
                id: SETTINGS_ENTRY_ID.to_string(),
                item_count: settings.len() as u64,
                byte_size: fs::metadata(&database).map(|meta| meta.len()).unwrap_or(0),
            });
        }
    }

    Ok(entries)
}

/// Copy the selected entries from `source` into `target`.
///
/// Existing files are kept unless `overwrite` is set, so importing twice (or
/// importing into a profile that already has newer data) never loses anything.
pub fn import(
    source: &Path,
    target: &Path,
    entries: &[String],
    overwrite: bool,
    settings: &mut dyn SettingsBridge,
) -> Result<MigrationReport, String> {
    validate_source(source, target)?;
    if !target.is_dir() {
        return Err("The destination data directory is unavailable.".to_string());
    }

    let selected = |id: &str| entries.is_empty() || entries.iter().any(|entry| entry == id);
    let mut report = MigrationReport {
        source: source.display().to_string(),
        items: Vec::new(),
        requires_restart: false,
    };

    for entry in COPY_ENTRIES {
        if !selected(entry.id) {
            continue;
        }
        let mut item = MigrationItemReport {
            id: entry.id.to_string(),
            ..Default::default()
        };
        for relative in entry.paths {
            copy_path(&source.join(relative), &target.join(relative), overwrite, &mut item);
        }
        if item.imported + item.skipped + item.failed > 0 {
            report.items.push(item);
        }
    }

    if selected(SETTINGS_ENTRY_ID) {
        if let Some(item) = import_legacy_settings(source, settings, overwrite)? {
            if item.imported > 0 {
                report.requires_restart = true;
            }
            report.items.push(item);
        }
    }

    Ok(report)
}

fn validate_source(source: &Path, target: &Path) -> Result<(), String> {
    if !source.is_absolute() {
        return Err("The selected folder must be an absolute path.".to_string());
    }
    if !source.is_dir() {
        return Err("The selected folder does not exist.".to_string());
    }
    let source = absolute_lexical(source)?;
    let target = absolute_lexical(target)?;
    if source == target || target.starts_with(&source) || source.starts_with(&target) {
        return Err("The selected folder overlaps this application's data.".to_string());
    }
    let components = source
        .components()
        .filter(|component| matches!(component, Component::Normal(_)))
        .count();
    if components < 2 {
        return Err("Refusing to import from a broad filesystem path.".to_string());
    }
    let name = source
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if name == CURRENT_IDENTIFIER {
        return Err("This is the data folder of the current installation.".to_string());
    }
    let known = LEGACY_IDENTIFIERS.contains(&name) || name.contains("cfms");
    if !known {
        return Err(
            "The selected folder does not look like a CFMS client data directory.".to_string(),
        );
    }
    Ok(())
}

fn absolute_lexical(path: &Path) -> Result<PathBuf, String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| format!("Failed to resolve the working directory: {e}"))?
            .join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    Ok(normalized)
}

/// Recursively count files and bytes. Symlinks are measured as single entries
/// and never followed, so a link cannot pull an unrelated tree into the copy.
fn measure(path: &Path, depth: usize) -> Result<(u64, u64), String> {
    if depth > 32 {
        return Ok((0, 0));
    }
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok((0, 0)),
        Err(error) => return Err(format!("Failed to inspect {}: {error}", path.display())),
    };
    if metadata.is_symlink() {
        return Ok((1, 0));
    }
    if metadata.is_file() {
        return Ok((1, metadata.len()));
    }
    if !metadata.is_dir() {
        return Ok((0, 0));
    }

    let mut count = 0;
    let mut bytes = 0;
    let entries =
        fs::read_dir(path).map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
        let (child_count, child_bytes) = measure(&entry.path(), depth + 1)?;
        count += child_count;
        bytes += child_bytes;
        if count > MAX_MEASURED_ENTRIES {
            break;
        }
    }
    Ok((count, bytes))
}

fn copy_path(source: &Path, target: &Path, overwrite: bool, item: &mut MigrationItemReport) {
    let metadata = match fs::symlink_metadata(source) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => {
            item.failed += 1;
            push_error(item, format!("Failed to inspect {}: {error}", source.display()));
            return;
        }
    };
    if metadata.is_dir() {
        if let Err(error) = fs::create_dir_all(target) {
            item.failed += 1;
            push_error(item, format!("Failed to create {}: {error}", target.display()));
            return;
        }
        let entries = match fs::read_dir(source) {
            Ok(entries) => entries,
            Err(error) => {
                item.failed += 1;
                push_error(item, format!("Failed to read {}: {error}", source.display()));
                return;
            }
        };
        for entry in entries.flatten() {
            copy_path(&entry.path(), &target.join(entry.file_name()), overwrite, item);
        }
        return;
    }
    if metadata.is_file() {
        if target.exists() && !overwrite {
            item.skipped += 1;
            return;
        }
        if let Some(parent) = target.parent()
            && let Err(error) = fs::create_dir_all(parent)
        {
            item.failed += 1;
            push_error(item, format!("Failed to create {}: {error}", parent.display()));
            return;
        }
        match fs::copy(source, target) {
            Ok(_) => item.imported += 1,
            Err(error) => {
                item.failed += 1;
                push_error(
                    item,
                    format!(
                        "Failed to copy {} to {}: {error}",
                        source.display(),
                        target.display()
                    ),
                );
            }
        }
        return;
    }
    // Symlinks and special files are not part of a CFMS profile.
    item.skipped += 1;
}

fn push_error(item: &mut MigrationItemReport, message: String) {
    if item.errors.len() < MAX_ERRORS_PER_ITEM {
        item.errors.push(message);
    }
}

/// Import the legacy `user_settings` table.
///
/// The database is opened from a temporary copy (including any `-wal`/`-shm`
/// sidecars) so a previous installation that is still running keeps its
/// journal intact.
fn import_legacy_settings(
    source: &Path,
    settings: &mut dyn SettingsBridge,
    overwrite: bool,
) -> Result<Option<MigrationItemReport>, String> {
    let database = source.join(LEGACY_DATABASE_FILE);
    if !database.is_file() {
        return Ok(None);
    }
    let rows = match read_legacy_settings(&database) {
        Ok(rows) => rows,
        Err(error) => {
            return Ok(Some(MigrationItemReport {
                id: SETTINGS_ENTRY_ID.to_string(),
                failed: 1,
                errors: vec![error],
                ..Default::default()
            }));
        }
    };

    let mut item = MigrationItemReport {
        id: SETTINGS_ENTRY_ID.to_string(),
        ..Default::default()
    };
    for (key, value) in rows {
        match settings.get(&key) {
            Ok(Some(_)) if !overwrite => {
                item.skipped += 1;
            }
            Ok(_) => match settings.set(&key, &value) {
                Ok(()) => item.imported += 1,
                Err(error) => {
                    item.failed += 1;
                    push_error(&mut item, format!("Failed to write setting {key}: {error}"));
                }
            },
            Err(error) => {
                item.failed += 1;
                push_error(&mut item, format!("Failed to read setting {key}: {error}"));
            }
        }
    }
    Ok(Some(item))
}

fn read_legacy_settings(database: &Path) -> Result<Vec<(String, String)>, String> {
    let directory = tempfile::Builder::new()
        .prefix("cfms-migration-")
        .tempdir()
        .map_err(|e| format!("Failed to prepare a temporary folder: {e}"))?;
    let copy = directory.path().join(LEGACY_DATABASE_FILE);
    fs::copy(database, &copy).map_err(|e| format!("Failed to read the previous database: {e}"))?;
    for suffix in ["-wal", "-shm"] {
        let sidecar = PathBuf::from(format!("{}{suffix}", database.display()));
        if sidecar.is_file() {
            let _ = fs::copy(&sidecar, directory.path().join(format!("{LEGACY_DATABASE_FILE}{suffix}")));
        }
    }

    let connection = rusqlite::Connection::open(&copy)
        .map_err(|e| format!("The previous database could not be opened: {e}"))?;
    let mut statement = connection
        .prepare("SELECT key, value FROM user_settings")
        .map_err(|e| format!("The previous database has no settings table: {e}"))?;
    let rows = statement
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
        .map_err(|e| format!("The previous settings could not be read: {e}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("The previous settings could not be read: {e}"))?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct MapBridge {
        values: HashMap<String, String>,
    }

    impl SettingsBridge for MapBridge {
        fn get(&self, key: &str) -> Result<Option<String>, String> {
            Ok(self.values.get(key).cloned())
        }

        fn set(&mut self, key: &str, value: &str) -> Result<(), String> {
            self.values.insert(key.to_string(), value.to_string());
            Ok(())
        }
    }

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
    }

    fn legacy_source(root: &Path) -> PathBuf {
        let source = root.join("org.crpteam.cfms-client-tauri");
        write(&source.join("user_preferences/abc_alice.json"), "{}");
        write(&source.join("download_tasks/abc_1.json"), "{}");
        write(&source.join("upload_tasks/abc_2.json"), "{}");
        write(&source.join("ca/.manifest.json"), "{}");
        write(&source.join("com_ext/packages/pkg.zip"), "zip");
        write(&source.join("avatars/abcdef"), "png");
        write(&source.join(".cfms-download-root"), "D:/downloads");
        write(&source.join("preview_cache/huge.png"), "skip me");
        source
    }

    fn create_settings_db(path: &Path, rows: &[(&str, &str)]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let connection = rusqlite::Connection::open(path).unwrap();
        connection
            .execute_batch("CREATE TABLE user_settings (key TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL);")
            .unwrap();
        for (key, value) in rows {
            connection
                .execute(
                    "INSERT INTO user_settings (key, value) VALUES (?1, ?2)",
                    rusqlite::params![key, value],
                )
                .unwrap();
        }
    }

    #[test]
    fn candidates_sit_next_to_the_current_data_directory() {
        let root = Path::new("/tmp/app/com.rosmontis220.cfmsmodded");
        let candidates = candidate_sources(root);
        assert_eq!(
            candidates,
            vec![PathBuf::from("/tmp/app/org.crpteam.cfms-client-tauri")]
        );
    }

    #[test]
    fn scan_reports_known_entries_only() {
        let dir = tempfile::tempdir().unwrap();
        let source = legacy_source(dir.path());

        let entries = scan_source(&source).unwrap();
        let ids = entries.iter().map(|entry| entry.id.as_str()).collect::<Vec<_>>();

        assert_eq!(
            ids,
            vec![
                "accountPreferences",
                "transferQueue",
                "customCa",
                "extensions",
                "avatars",
                "downloadRoot"
            ]
        );
        let queue = entries.iter().find(|entry| entry.id == "transferQueue").unwrap();
        assert_eq!(queue.item_count, 2);
        // Cache directories are never offered.
        assert!(!ids.contains(&"previewCache"));
    }

    #[test]
    fn import_copies_files_and_keeps_existing_targets() {
        let dir = tempfile::tempdir().unwrap();
        let source = legacy_source(dir.path());
        let target = dir.path().join(CURRENT_IDENTIFIER);
        write(&target.join("user_preferences/abc_alice.json"), "newer");
        let mut bridge = MapBridge::default();

        let report = import(
            &source,
            &target,
            &["accountPreferences".into(), "transferQueue".into()],
            false,
            &mut bridge,
        )
        .unwrap();

        assert!(report.items.iter().any(|item| item.id == "accountPreferences"
            && item.skipped == 1
            && item.failed == 0));
        assert!(report.items.iter().any(|item| item.id == "transferQueue"
            && item.imported == 2));
        // The existing preference file is not replaced.
        assert_eq!(
            fs::read_to_string(target.join("user_preferences/abc_alice.json")).unwrap(),
            "newer"
        );
        // Nothing outside the selection was copied.
        assert!(!target.join("com_ext").exists());
        // The source is untouched.
        assert!(source.join("com_ext/packages/pkg.zip").exists());
    }

    #[test]
    fn import_overwrites_when_requested() {
        let dir = tempfile::tempdir().unwrap();
        let source = legacy_source(dir.path());
        let target = dir.path().join(CURRENT_IDENTIFIER);
        write(&target.join(".cfms-download-root"), "old");
        let mut bridge = MapBridge::default();

        let report = import(
            &source,
            &target,
            &["downloadRoot".into()],
            true,
            &mut bridge,
        )
        .unwrap();

        assert_eq!(report.items[0].imported, 1);
        assert_eq!(
            fs::read_to_string(target.join(".cfms-download-root")).unwrap(),
            "D:/downloads"
        );
    }

    #[test]
    fn empty_selection_imports_everything_available() {
        let dir = tempfile::tempdir().unwrap();
        let source = legacy_source(dir.path());
        let target = dir.path().join(CURRENT_IDENTIFIER);
        fs::create_dir_all(&target).unwrap();
        let mut bridge = MapBridge::default();

        let report = import(&source, &target, &[], false, &mut bridge).unwrap();

        let imported: u32 = report.items.iter().map(|item| item.imported).sum();
        assert_eq!(imported, 7);
        assert!(target.join("com_ext/packages/pkg.zip").exists());
    }

    #[test]
    fn settings_are_imported_without_clobbering_local_values() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("org.crpteam.cfms-client-tauri");
        create_settings_db(
            &source.join(LEGACY_DATABASE_FILE),
            &[("language", "\"en\""), ("appearance", "{\"theme\":\"dark\"}")],
        );
        let target = dir.path().join(CURRENT_IDENTIFIER);
        fs::create_dir_all(&target).unwrap();
        let mut bridge = MapBridge::default();
        bridge.values.insert("language".into(), "\"zh_CN\"".into());

        let report = import(
            &source,
            &target,
            &[SETTINGS_ENTRY_ID.to_string()],
            false,
            &mut bridge,
        )
        .unwrap();

        assert!(report.requires_restart);
        assert_eq!(bridge.values.get("language").unwrap(), "\"zh_CN\"");
        assert_eq!(
            bridge.values.get("appearance").unwrap(),
            "{\"theme\":\"dark\"}"
        );
        let item = &report.items[0];
        assert_eq!((item.imported, item.skipped, item.failed), (1, 1, 0));
    }

    #[test]
    fn rejects_a_source_that_overlaps_the_target() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join(CURRENT_IDENTIFIER);
        fs::create_dir_all(&target).unwrap();
        let mut bridge = MapBridge::default();

        let error = import(&target, &target, &[], false, &mut bridge).unwrap_err();
        assert!(error.contains("overlaps"));
    }

    #[test]
    fn rejects_relative_and_unrelated_sources() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join(CURRENT_IDENTIFIER);
        fs::create_dir_all(&target).unwrap();
        let mut bridge = MapBridge::default();

        assert!(import(Path::new("relative/dir"), &target, &[], false, &mut bridge).is_err());

        let unrelated = dir.path().join("some-random-folder");
        fs::create_dir_all(&unrelated).unwrap();
        let error = import(&unrelated, &target, &[], false, &mut bridge).unwrap_err();
        assert!(error.contains("CFMS"));
    }

    #[test]
    fn resolve_source_requires_an_existing_directory() {
        let dir = tempfile::tempdir().unwrap();
        let current = dir.path().join(CURRENT_IDENTIFIER);
        fs::create_dir_all(&current).unwrap();

        assert!(resolve_source(&current, None).is_err());

        let legacy = dir.path().join(LEGACY_IDENTIFIERS[0]);
        fs::create_dir_all(&legacy).unwrap();
        assert_eq!(resolve_source(&current, None).unwrap(), legacy);
        assert_eq!(
            resolve_source(&current, Some(legacy.to_str().unwrap())).unwrap(),
            legacy
        );
    }
}
