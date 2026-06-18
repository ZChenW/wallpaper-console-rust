use std::sync::{Mutex, OnceLock};

use wc_core::types::WallpaperEntry;

use super::common::{fail, ok, storage, CommandResult, ScanProgressDto};
use wc_storage::sqlite::library_session;

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
            staged: 0,
            skipped: 0,
            metadata_errors: 0,
            current_path: None,
            cancel_requested: false,
            error: None,
        })
    })
}

pub(crate) fn current_scan_progress_snapshot() -> ScanProgressDto {
    match scan_state().lock() {
        Ok(state) => state.clone(),
        Err(_) => ScanProgressDto {
            running: false,
            stage: "idle".into(),
            scanned: 0,
            total_hint: None,
            reused_metadata: 0,
            probed_metadata: 0,
            inserted_sqlite: 0,
            staged: 0,
            skipped: 0,
            metadata_errors: 0,
            current_path: None,
            cancel_requested: false,
            error: None,
        },
    }
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
        staged: 0,
        skipped: 0,
        metadata_errors: 0,
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

    update_scan_stage("loading prior metadata");
    let prior_cache = wc_storage::sqlite::prior_metadata_cache_from_sqlite(&s.cd);

    update_scan_stage("walking files");
    let mut session = library_session::library_replace_session_start(&s.cd)
        .map_err(|e: wc_core::error::WcError| e.to_string())?;

    let mut batch: Vec<WallpaperEntry> = Vec::with_capacity(250);
    let mut scanned: usize = 0;
    let mut cancelled = false;

    wc_scan::visit_wallpapers_with_callback(
        &sources,
        |event| match scan_state().lock() {
            Ok(mut state) => {
                if state.cancel_requested {
                    return wc_scan::ScanControl::Cancel;
                }
                match event {
                    wc_scan::ScanEvent::SourceStarted { source } => {
                        state.stage = "walking files".into();
                        state.current_path = Some(source);
                    }
                    wc_scan::ScanEvent::CandidateFound { path, count } => {
                        state.stage = "walking files".into();
                        state.total_hint = Some(count);
                        state.current_path = Some(path);
                    }
                    wc_scan::ScanEvent::WalkProgress { .. } => {}
                }
                wc_scan::ScanControl::Continue
            }
            Err(_) => wc_scan::ScanControl::Cancel,
        },
        |path| {
            if scan_cancelled().unwrap_or(true) {
                cancelled = true;
                return wc_scan::ScanVisitControl::Cancel;
            }
            scanned += 1;
            if let Ok(mut state) = scan_state().lock() {
                state.scanned = scanned;
                state.stage = "reading metadata".into();
                state.current_path = Some(path.clone());
            }
            let (entry, was_reused) = wc_scan::make_entry_cached(&path, &prior_cache);
            if let Ok(mut state) = scan_state().lock() {
                if was_reused {
                    state.reused_metadata += 1;
                } else {
                    state.probed_metadata += 1;
                }
            }
            if let Some(entry) = entry {
                batch.push(entry);
                if let Ok(mut state) = scan_state().lock() {
                    state.staged += 1;
                }
                if batch.len() >= 250 {
                    update_scan_stage("writing SQLite");
                    if library_session::library_replace_session_push(&mut session, &batch).is_err()
                    {
                        cancelled = true;
                        return wc_scan::ScanVisitControl::Cancel;
                    }
                    if let Ok(mut state) = scan_state().lock() {
                        state.inserted_sqlite = scanned;
                    }
                    batch.clear();
                }
            } else if let Ok(mut state) = scan_state().lock() {
                state.skipped += 1;
            }
            wc_scan::ScanVisitControl::Continue
        },
    );

    if cancelled || scan_cancelled().unwrap_or(true) {
        library_session::library_replace_session_abort(session).ok();
        return Err("scan cancelled".to_string());
    }

    update_scan_stage("writing SQLite");
    if !batch.is_empty() {
        library_session::library_replace_session_push(&mut session, &batch)
            .map_err(|e: wc_core::error::WcError| e.to_string())?;
    }
    let inserted = library_session::library_replace_session_commit(session)
        .map_err(|e: wc_core::error::WcError| e.to_string())?;
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
                staged: 0,
                skipped: 0,
                metadata_errors: 0,
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

    #[test]
    fn scan_cancel_sets_cancel_requested_when_running() {
        let _guard = TEST_SCAN_LOCK.lock().unwrap();
        reset_scan_state_for_test();
        mark_scan_started("walking files").unwrap();
        {
            let mut state = scan_state().lock().unwrap();
            state.cancel_requested = true;
        }
        assert!(scan_cancelled().unwrap());
        finish_scan_error("scan cancelled");
    }

    #[test]
    fn scan_state_reset_zeroes_new_counters() {
        let _guard = TEST_SCAN_LOCK.lock().unwrap();
        reset_scan_state_for_test();
        {
            let mut state = scan_state().lock().unwrap();
            state.staged = 5;
            state.skipped = 7;
            state.metadata_errors = 9;
        }
        reset_scan_state_for_test();
        let state = scan_state().lock().unwrap().clone();
        assert_eq!(state.staged, 0);
        assert_eq!(state.skipped, 0);
        assert_eq!(state.metadata_errors, 0);
    }

    #[test]
    fn current_scan_progress_snapshot_returns_idle_state() {
        let _guard = TEST_SCAN_LOCK.lock().unwrap();
        reset_scan_state_for_test();
        let snap = current_scan_progress_snapshot();
        assert!(!snap.running, "idle snapshot should not be running");
        assert_eq!(snap.stage, "idle");
        assert_eq!(snap.scanned, 0);
        assert_eq!(snap.staged, 0);
        assert_eq!(snap.skipped, 0);
    }

    #[test]
    fn scan_progress_dto_serializes_new_fields_camel_case() {
        let dto = ScanProgressDto {
            running: true,
            stage: "reading metadata".into(),
            scanned: 10,
            total_hint: Some(20),
            reused_metadata: 3,
            probed_metadata: 5,
            inserted_sqlite: 8,
            staged: 8,
            skipped: 2,
            metadata_errors: 0,
            current_path: Some("/x/y.jpg".into()),
            cancel_requested: false,
            error: None,
        };
        let json = serde_json::to_string(&dto).unwrap();
        assert!(
            json.contains("\"staged\":8"),
            "staged should serialize as camelCase: {json}"
        );
        assert!(
            json.contains("\"skipped\":2"),
            "skipped should serialize as camelCase: {json}"
        );
        assert!(
            json.contains("\"metadataErrors\":0"),
            "metadataErrors should serialize as camelCase: {json}"
        );
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["staged"], 8);
        assert_eq!(v["skipped"], 2);
        assert_eq!(v["metadataErrors"], 0);
    }

    #[test]
    fn index_current_sources_counts_staged_and_skipped() {
        let _guard = TEST_SCAN_LOCK.lock().unwrap();
        reset_scan_state_for_test();

        let tmp = tempfile::tempdir().unwrap();
        let cd = wc_core::ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        let s = wc_storage::StorageApi::new(cd);

        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("wall.jpg"), b"not a real image").unwrap();
        s.sources_add(&src.to_string_lossy()).unwrap();

        let inserted = index_current_sources(&s).unwrap();
        assert_eq!(inserted, 1);

        let state = scan_state().lock().unwrap().clone();
        assert_eq!(
            state.scanned, 1,
            "scanned should count the one visited file"
        );
        assert_eq!(state.staged, 1, "staged should count the one pushed entry");
        assert_eq!(
            state.skipped, 0,
            "skipped should stay 0 for a supported file"
        );
        assert_eq!(
            state.inserted_sqlite, 1,
            "inserted_sqlite should match committed count"
        );
        assert_eq!(
            state.metadata_errors, 0,
            "metadata_errors is always 0 for now"
        );
    }
}
