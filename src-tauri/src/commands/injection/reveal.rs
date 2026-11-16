// Fork-owned "show the finished download in its folder" adapter.
//
// Upstream can only open a completed transfer. After a download people usually
// want the containing directory — to rename the file, attach it elsewhere or
// move it into a project folder — so this resolves the same path the opener
// command uses and asks the desktop shell to select the entry instead of
// launching it.

/// Reveal a finished download in the system file manager.
#[tauri::command]
pub async fn reveal_downloaded_file<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, AppHandleState>,
    task_id: String,
) -> Result<(), String> {
    let path = prepare_downloaded_file_for_open(&state, &task_id)?;
    reveal_local_file(&app, std::path::Path::new(&path))
        .map_err(|error| format!("Failed to show the downloaded file: {error}"))
}

/// Android has no "select this file in a file manager" intent, and the transfer
/// UI hides the action there; the command still answers so a stale call fails
/// with a readable message instead of an unknown-command error.
#[cfg(target_os = "android")]
fn reveal_local_file<R: tauri::Runtime>(
    _app: &tauri::AppHandle<R>,
    _path: &std::path::Path,
) -> Result<(), String> {
    Err("This platform cannot show a file in its folder.".to_string())
}

#[cfg(not(target_os = "android"))]
fn reveal_local_file<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    path: &std::path::Path,
) -> Result<(), String> {
    tauri_plugin_opener::OpenerExt::opener(app)
        .reveal_item_in_dir(path)
        .map_err(|error| error.to_string())
}
