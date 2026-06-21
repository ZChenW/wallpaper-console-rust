use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use wc_core::types::WallpaperEntry;

use super::common::{fail, ok, storage, CommandResult, ScanProgressDto};
use wc_storage::sqlite::library_session;

static SCAN_STATE: OnceLock<Mutex<ScanProgressDto>> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IndexSourcesResult {
    pub inserted: usize,
    pub removed: usize,
    pub removed_we_workshop_ids: Vec<String>,
}

pub(crate) fn format_index_sources_message(result: &IndexSourcesResult) -> String {
    let mut msg = format!("Scan complete. {} wallpaper(s) indexed.", result.inserted);
    if result.removed == 0 {
        return msg;
    }
    if result.removed == 1 && result.removed_we_workshop_ids.len() == 1 {
        msg.push_str(&format!(
            " Removed {} missing workshop item ({}).",
            result.removed, result.removed_we_workshop_ids[0]
        ));
    } else if !result.removed_we_workshop_ids.is_empty() {
        msg.push_str(&format!(
            " Removed {} missing item(s), including workshop ID(s): {}.",
            result.removed,
            result.removed_we_workshop_ids.join(", ")
        ));
    } else {
        msg.push_str(&format!(" Removed {} missing item(s).", result.removed));
    }
    msg
}

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

pub(crate) fn index_current_sources(
    s: &wc_storage::StorageApi,
) -> Result<IndexSourcesResult, String> {
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
    let mut seen_new_paths: HashSet<String> = HashSet::new();

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
                let canon = std::fs::canonicalize(&path)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| path.clone());
                seen_new_paths.insert(canon);
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
    let (removed, removed_we_workshop_ids) =
        wc_storage::sqlite::removed_from_prior_cache(&prior_cache, &seen_new_paths);
    Ok(IndexSourcesResult {
        inserted,
        removed,
        removed_we_workshop_ids,
    })
}

#[tauri::command]
pub async fn rescan() -> CommandResult {
    tauri::async_runtime::spawn_blocking(|| {
        if let Err(err) = mark_scan_started("starting scan") {
            return fail(err);
        }

        let result: Result<String, String> = (|| {
            let s = storage()?;
            let index_result = index_current_sources(&s)?;
            Ok(format_index_sources_message(&index_result))
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

        let result = index_current_sources(&s).unwrap();
        assert_eq!(result.inserted, 1);
        assert_eq!(result.removed, 0);

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

    #[test]
    fn format_index_sources_message_reports_removed_workshop_item() {
        let msg = format_index_sources_message(&IndexSourcesResult {
            inserted: 12,
            removed: 1,
            removed_we_workshop_ids: vec!["3589454154".into()],
        });
        assert!(msg.contains("12 wallpaper(s) indexed"));
        assert!(msg.contains("Removed 1 missing workshop item (3589454154)"));
    }

    #[test]
    fn format_index_sources_message_uses_plural_wording_for_mixed_removals() {
        let msg = format_index_sources_message(&IndexSourcesResult {
            inserted: 10,
            removed: 3,
            removed_we_workshop_ids: vec!["3589454154".into()],
        });
        assert!(msg.contains("Removed 3 missing item(s), including workshop ID(s): 3589454154"));
        assert!(
            !msg.contains("missing workshop item"),
            "mixed removals must not use singular workshop wording: {msg}"
        );
    }

    #[test]
    fn rescan_removes_deleted_we_workshop_project_from_sqlite() {
        let _guard = TEST_SCAN_LOCK.lock().unwrap();
        reset_scan_state_for_test();

        let tmp = tempfile::tempdir().unwrap();
        let cd = wc_core::ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        let s = wc_storage::StorageApi::new(cd);

        let workshop_root = tmp.path().join("steamapps/workshop/content/431960");
        let project_dir = workshop_root.join("3589454154");
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::write(
            project_dir.join("project.json"),
            r#"{"type":"scene","title":"Deleted Scene Test"}"#,
        )
        .unwrap();
        s.sources_add(&workshop_root.to_string_lossy()).unwrap();

        let first = index_current_sources(&s).unwrap();
        assert_eq!(first.inserted, 1, "scene project should be indexed");
        assert_eq!(first.removed, 0);

        let conn = rusqlite::Connection::open(s.cd.db_path()).unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM wallpapers WHERE workshop_id = '3589454154'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "workshop_id should exist after first rescan");

        std::fs::remove_dir_all(&project_dir).unwrap();

        let second = index_current_sources(&s).unwrap();
        assert_eq!(
            second.inserted, 0,
            "deleted project should not be reindexed"
        );
        assert_eq!(
            second.removed, 1,
            "deleted project should be removed from sqlite"
        );
        assert_eq!(
            second.removed_we_workshop_ids,
            vec!["3589454154".to_string()]
        );

        let count_after: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM wallpapers WHERE workshop_id = '3589454154'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            count_after, 0,
            "workshop_id must be gone from sqlite after rescan"
        );
    }
}
