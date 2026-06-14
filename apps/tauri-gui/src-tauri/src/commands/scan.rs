use std::sync::{Mutex, OnceLock};

use wc_core::types::WallpaperEntry;

use super::common::{fail, ok, storage, CommandResult, ScanProgressDto};

static SCAN_STATE: OnceLock<Mutex<ScanProgressDto>> = OnceLock::new();

fn scan_state() -> &'static Mutex<ScanProgressDto> {
    SCAN_STATE.get_or_init(|| {
        Mutex::new(ScanProgressDto {
            running: false,
            stage: "idle".into(),
            scanned: 0,
            total_hint: None,
            reused_metadata: 0,
            probed_metadata: 0,
            inserted_sqlite: 0,
            current_path: None,
            cancel_requested: false,
            error: None,
        })
    })
}

pub(crate) fn mark_scan_started(stage: &str) -> Result<(), String> {
    let mut state = scan_state().lock().map_err(|e| e.to_string())?;
    if state.running {
        return Err(format!("Scan already running: {}", state.stage));
    }
    *state = ScanProgressDto {
        running: true,
        stage: stage.into(),
        scanned: 0,
        total_hint: None,
        reused_metadata: 0,
        probed_metadata: 0,
        inserted_sqlite: 0,
        current_path: None,
        cancel_requested: false,
        error: None,
    };
    Ok(())
}

pub(crate) fn update_scan_stage(stage: &str) {
    if let Ok(mut state) = scan_state().lock() {
        state.stage = stage.into();
    }
}

fn scan_cancelled() -> Result<bool, String> {
    scan_state()
        .lock()
        .map(|state| state.cancel_requested)
        .map_err(|e| e.to_string())
}

pub(crate) fn finish_scan_success() {
    if let Ok(mut state) = scan_state().lock() {
        state.running = false;
        state.stage = "idle".into();
        state.current_path = None;
        state.cancel_requested = false;
    }
}

pub(crate) fn finish_scan_error(err: &str) {
    if let Ok(mut state) = scan_state().lock() {
        state.running = false;
        state.error = Some(err.to_string());
        state.current_path = None;
    }
}

pub(crate) fn index_current_sources(s: &wc_storage::StorageApi) -> Result<usize, String> {
    update_scan_stage("loading sources");
    let sources = s.sources_list().map_err(|e| e.to_string())?;

    if scan_cancelled()? {
        return Err("scan cancelled".to_string());
    }

    update_scan_stage("walking files");
    let paths = wc_scan::scan_wallpapers(&sources);
    if let Ok(mut state) = scan_state().lock() {
        state.total_hint = Some(paths.len());
    }

    update_scan_stage("reading metadata");
    let mut entries: Vec<WallpaperEntry> = Vec::with_capacity(paths.len());
    for (idx, path) in paths.iter().enumerate() {
        if let Ok(mut state) = scan_state().lock() {
            if state.cancel_requested {
                return Err("scan cancelled".to_string());
            }
            state.scanned = idx + 1;
            state.current_path = Some(path.clone());
        }
        if let Some(entry) = wc_scan::make_entry(path) {
            entries.push(entry);
        }
    }

    if scan_cancelled()? {
        return Err("scan cancelled".to_string());
    }

    update_scan_stage("writing SQLite");
    let inserted = wc_storage::sqlite::library_replace_entries_batch_atomic(&s.cd, &entries)
        .map_err(|e| e.to_string())?;
    if let Ok(mut state) = scan_state().lock() {
        state.inserted_sqlite = inserted;
    }
    Ok(inserted)
}

#[tauri::command]
pub async fn rescan() -> CommandResult {
    tauri::async_runtime::spawn_blocking(|| {
        if let Err(err) = mark_scan_started("starting scan") {
            return fail(err);
        }

        let result: Result<String, String> = (|| {
            let s = storage()?;
            let inserted = index_current_sources(&s)?;
            Ok(format!("Scan complete. {} wallpaper(s) indexed.", inserted))
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

#[tauri::command]
pub async fn scan_progress() -> Result<ScanProgressDto, String> {
    Ok(scan_state().lock().map_err(|e| e.to_string())?.clone())
}

#[tauri::command]
pub async fn scan_cancel() -> CommandResult {
    match scan_state().lock() {
        Ok(mut state) => {
            if state.running {
                state.cancel_requested = true;
                ok("Cancel requested.")
            } else {
                ok("No scan is running.")
            }
        }
        Err(e) => fail(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static TEST_SCAN_LOCK: Mutex<()> = Mutex::new(());

    fn reset_scan_state_for_test() {
        if let Ok(mut state) = scan_state().lock() {
            *state = ScanProgressDto {
                running: false,
                stage: "idle".into(),
                scanned: 0,
                total_hint: None,
                reused_metadata: 0,
                probed_metadata: 0,
                inserted_sqlite: 0,
                current_path: None,
                cancel_requested: false,
                error: None,
            };
        }
    }

    #[test]
    fn scan_start_rejects_concurrent_scan() {
        let _guard = TEST_SCAN_LOCK.lock().unwrap();
        reset_scan_state_for_test();
        mark_scan_started("first").unwrap();
        let err = mark_scan_started("second").unwrap_err();
        assert!(err.contains("already running"));
        finish_scan_success();
    }

    #[test]
    fn scan_finish_error_records_error() {
        let _guard = TEST_SCAN_LOCK.lock().unwrap();
        reset_scan_state_for_test();
        mark_scan_started("first").unwrap();
        finish_scan_error("boom");
        let state = scan_state().lock().unwrap().clone();
        assert!(!state.running);
        assert_eq!(state.error.as_deref(), Some("boom"));
    }
}
