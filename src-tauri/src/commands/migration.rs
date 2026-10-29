// Importing data from a previous CFMS installation
// ---------------------------------------------------------------------------
//
// The modded build is a standalone application, so a fresh install starts with
// an empty profile. These commands let the user pull the previous client's
// data directory across from the settings page, before or after signing in.

/// Settings store adapter for the migration importer.
#[cfg(not(target_os = "android"))]
struct MigrationSettingsBridge {
    settings: cfms_service::db::settings::SettingsStore,
}

#[cfg(not(target_os = "android"))]
impl crate::migration::SettingsBridge for MigrationSettingsBridge {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        self.settings
            .get(key)
            .map_err(|e| format!("Failed to read setting {key}: {e}"))
    }

    fn set(&mut self, key: &str, value: &str) -> Result<(), String> {
        self.settings
            .set(key, value)
            .map_err(|e| format!("Failed to write setting {key}: {e}"))
    }
}

#[tauri::command]
pub async fn scan_migration_source<R: Runtime>(
    app: tauri::AppHandle<R>,
    source: Option<String>,
) -> Result<crate::migration::MigrationScan, String> {
    #[cfg(target_os = "android")]
    {
        let _ = (app, source);
        return Ok(crate::migration::unsupported_scan());
    }
    #[cfg(not(target_os = "android"))]
    {
        let target = app
            .path()
            .app_data_dir()
            .map_err(|e| format!("Failed to resolve the application data directory: {e}"))?;
        tokio::task::spawn_blocking(move || {
            crate::migration::scan_for_target(&target, source.as_deref())
        })
        .await
        .map_err(|e| format!("Migration scan task failed: {e}"))?
    }
}

#[tauri::command]
pub async fn import_migration<R: Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, AppHandleState>,
    request: crate::migration::MigrationRequest,
) -> Result<crate::migration::MigrationReport, String> {
    #[cfg(target_os = "android")]
    {
        let _ = (app, state, request);
        return Err(
            "Android does not allow one application to read another application's private data."
                .into(),
        );
    }
    #[cfg(not(target_os = "android"))]
    {
        let target = app
            .path()
            .app_data_dir()
            .map_err(|e| format!("Failed to resolve the application data directory: {e}"))?;
        let mut bridge = MigrationSettingsBridge {
            settings: state.settings.clone(),
        };
        let crate::migration::MigrationRequest {
            source,
            entries,
            overwrite,
        } = request;

        let report = tokio::task::spawn_blocking(move || {
            let source = crate::migration::resolve_source(&target, source.as_deref())?;
            crate::migration::import(&source, &target, &entries, overwrite, &mut bridge)
        })
        .await
        .map_err(|e| format!("Migration task failed: {e}"))??;

        // Apply the imported language right away so the next screen is already
        // localized; appearance and download settings still need a restart.
        if report.requires_restart
            && let Ok(Some(language)) = state.settings.get("language")
        {
            let _ = state.localizer.set_locale(&language);
        }

        Ok(report)
    }
}
