use super::common::{fail, ok, source_label, storage, CommandResult, SourceDto};

#[tauri::command]
pub async fn sources_list() -> Result<Vec<SourceDto>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let s = storage()?;
        let sources = s.sources_list().map_err(|e| e.to_string())?;
        Ok(sources
            .into_iter()
            .map(|path| SourceDto {
                exists: std::path::Path::new(&path).exists(),
                is_we: wc_scan::is_wallpaper_engine_source(&path),
                label: source_label(&path),
                path,
            })
            .collect())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn source_add(path: String) -> CommandResult {
    tauri::async_runtime::spawn_blocking(move || match storage() {
        Ok(s) => match s.sources_add(&path) {
            Ok(true) => ok("Source added."),
            Ok(false) => ok("Source already exists."),
            Err(e) => fail(e.to_string()),
        },
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn source_remove(path: String) -> CommandResult {
    tauri::async_runtime::spawn_blocking(move || match storage() {
        Ok(s) => match s.sources_remove(&path) {
            Ok(true) => ok("Source removed."),
            Ok(false) => ok("Source was not configured."),
            Err(e) => fail(e.to_string()),
        },
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn validate_sources() -> CommandResult {
    tauri::async_runtime::spawn_blocking(|| match storage() {
        Ok(s) => match s.sources_list() {
            Ok(src) => {
                let missing = src
                    .iter()
                    .filter(|p| !std::path::Path::new(p).exists())
                    .count();
                if missing == 0 {
                    ok("All sources are valid.")
                } else {
                    fail(format!("{} source(s) are missing.", missing))
                }
            }
            Err(e) => fail(e.to_string()),
        },
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn remove_missing_sources() -> CommandResult {
    tauri::async_runtime::spawn_blocking(|| match storage() {
        Ok(s) => match s.sources_list() {
            Ok(src) => {
                let mut removed = 0;
                for path in src {
                    if !std::path::Path::new(&path).exists()
                        && s.sources_remove(&path).unwrap_or(false)
                    {
                        removed += 1;
                    }
                }
                ok(format!("Removed {} missing source(s).", removed))
            }
            Err(e) => fail(e.to_string()),
        },
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn scan_steam_workshop() -> CommandResult {
    super::rescan().await
}
