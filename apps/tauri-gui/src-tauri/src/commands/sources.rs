use super::common::{fail, ok, source_label, storage, CommandResult, SourceDto};
use super::scan::{
    finish_scan_error, finish_scan_success, format_index_sources_message, index_current_sources,
    mark_scan_started, update_scan_stage,
};
use std::path::Path;

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
    tauri::async_runtime::spawn_blocking(|| {
        if let Err(err) = mark_scan_started("discovering Wallpaper Engine") {
            return fail(err);
        }

        let result: Result<String, String> = (|| {
            let s = storage()?;
            let home = std::env::var("HOME").map_err(|_| "HOME is not set.".to_string())?;
            let roots = wc_scan::discover_steam_workshop_roots(Path::new(&home));
            if roots.is_empty() {
                return Ok(
                    "No Wallpaper Engine workshop directory found. Install or download Wallpaper Engine workshop content in Steam, then scan again."
                        .to_string(),
                );
            }

            let mut added = 0usize;
            update_scan_stage("adding Wallpaper Engine sources");
            for root in &roots {
                if s.sources_add(&root.to_string_lossy())
                    .map_err(|e| e.to_string())?
                {
                    added += 1;
                }
            }

            let index_result = index_current_sources(&s)?;
            Ok(format!(
                "Wallpaper Engine scan complete. {} source root(s) found, {} new source(s). {}",
                roots.len(),
                added,
                format_index_sources_message(&index_result)
            ))
        })();

        match result {
            Ok(msg) => {
                finish_scan_success();
                ok(msg)
            }
            Err(err) => {
                finish_scan_error(&err);
                fail(err)
            }
        }
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}
