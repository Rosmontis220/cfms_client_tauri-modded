// Scratch files that survive the platform's temporary-directory differences.

use std::path::PathBuf;

use tauri::Manager;

/// Root directory for short-lived files.
///
/// `tempfile` resolves `std::env::temp_dir()`, which is `/tmp` on Android. The
/// app sandbox has no such directory, so every command that staged a download
/// or an upload in the default temporary directory failed there: chat records
/// reported "cannot be read from the server" and sending a message failed the
/// same way. The app cache directory exists and is writable on every platform.
pub fn root<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_cache_dir()
        .map_err(|error| format!("Failed to resolve the cache directory: {error}"))?
        .join("scratch");
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("Failed to create the scratch directory: {error}"))?;
    Ok(dir)
}

/// Scratch directory that is removed when the value is dropped.
pub fn dir<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    prefix: &str,
) -> Result<tempfile::TempDir, String> {
    tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in(root(app)?)
        .map_err(|error| format!("Failed to create a temporary directory: {error}"))
}

/// Scratch file that is removed when the value is dropped.
pub fn file<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    prefix: &str,
    suffix: &str,
) -> Result<tempfile::NamedTempFile, String> {
    tempfile::Builder::new()
        .prefix(prefix)
        .suffix(suffix)
        .tempfile_in(root(app)?)
        .map_err(|error| format!("Failed to create a temporary file: {error}"))
}
