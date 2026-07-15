use super::common::{fail, ok, storage, CommandResult, SourceDto};
use super::scan::{
    finish_scan_error, finish_scan_success, index_source_by_id, mark_scan_started,
    scan_steam_workshop_and_index, update_scan_stage, with_scan_idle_operation, IndexSourcesResult,
};
use std::path::Path;
use wc_storage::{SourceKind, SourceRecord, StorageApi};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FirstRunSourceSuggestionDto {
    Directory { label: String, path: String },
    WallpaperEngine { roots: Vec<String> },
}

fn detect_first_run_source_suggestions(home: &Path) -> Vec<FirstRunSourceSuggestionDto> {
    let mut suggestions = Vec::new();
    let downloads = home.join("Downloads");
    if downloads.is_dir() {
        suggestions.push(FirstRunSourceSuggestionDto::Directory {
            label: "Downloads".to_string(),
            path: downloads.to_string_lossy().into_owned(),
        });
    }

    let roots = wc_scan::discover_steam_workshop_roots(home);
    if !roots.is_empty() {
        suggestions.push(FirstRunSourceSuggestionDto::WallpaperEngine {
            roots: roots
                .into_iter()
                .map(|root| root.to_string_lossy().into_owned())
                .collect(),
        });
    }
    suggestions
}

fn source_dtos_with_storage(storage: &StorageApi) -> Result<Vec<SourceDto>, String> {
    let records = storage
        .source_records()
        .map_err(|error| error.to_string())?;
    Ok(records
        .into_iter()
        .map(|source| {
            let is_we = source.kind == SourceKind::WallpaperEngineWorkshop;
            SourceDto {
                id: source.id,
                path: source.path.clone(),
                display_name: source.display_name.clone(),
                kind: source.kind.as_str().to_string(),
                recursive: source.recursive,
                availability: source.availability.as_str().to_string(),
                added_at: source.added_at,
                exists: Path::new(&source.path).exists(),
                is_we,
                label: source.display_name,
            }
        })
        .collect())
}

fn source_record_by_id(storage: &StorageApi, id: i64) -> Result<SourceRecord, String> {
    storage
        .source_records()
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|source| source.id == id)
        .ok_or_else(|| format!("source id {id} not found"))
}

fn format_targeted_source_message(source: &SourceRecord, result: &IndexSourcesResult) -> String {
    if result.offline_sources > 0 {
        return format!(
            "Source '{}' is offline; preserved its previous snapshot. Library contains {} wallpaper(s).",
            source.display_name, result.library_total
        );
    }
    if result.incomplete_sources > 0 {
        return format!(
            "Source '{}' scan was incomplete; preserved its previous snapshot. Library contains {} wallpaper(s).",
            source.display_name, result.library_total
        );
    }
    format!(
        "Source '{}' indexed {} wallpaper(s). Library contains {} wallpaper(s).",
        source.display_name, result.indexed, result.library_total
    )
}

fn run_targeted_source_operation<F>(
    storage: &StorageApi,
    stage: &str,
    saved_state: Option<&str>,
    prepare: F,
) -> CommandResult
where
    F: FnOnce() -> Result<SourceRecord, String>,
{
    if let Err(error) = mark_scan_started(stage) {
        return fail(error);
    }

    let source = match prepare() {
        Ok(source) => source,
        Err(error) => {
            finish_scan_error(&error);
            return fail(error);
        }
    };
    match index_source_by_id(storage, source.id) {
        Ok(indexed) => {
            finish_scan_success();
            ok(format_targeted_source_message(&source, &indexed))
        }
        Err(error) => {
            finish_scan_error(&error);
            match saved_state {
                Some(saved_state) if error == "scan cancelled" => fail(format!(
                    "{saved_state}, but refresh was cancelled; the previous snapshot was preserved."
                )),
                Some(saved_state) => fail(format!(
                    "{saved_state}, but refresh failed: {error}; the previous snapshot was preserved."
                )),
                None => fail(error),
            }
        }
    }
}

fn source_add_with_storage(storage: &StorageApi, path: &str) -> CommandResult {
    run_targeted_source_operation(storage, "adding source", Some("Source is saved"), || {
        storage
            .source_create(path)
            .map_err(|error| error.to_string())
    })
}

fn source_rename_with_storage(storage: &StorageApi, id: i64, display_name: &str) -> CommandResult {
    match storage.source_rename(id, display_name) {
        Ok(source) => ok(format!("Source renamed to '{}'.", source.display_name)),
        Err(error) => fail(error.to_string()),
    }
}

fn source_set_recursive_with_storage(
    storage: &StorageApi,
    id: i64,
    recursive: bool,
) -> CommandResult {
    run_targeted_source_operation(
        storage,
        "updating source",
        Some("Recursion setting is saved"),
        || {
            storage
                .source_set_recursive(id, recursive)
                .map_err(|error| error.to_string())
        },
    )
}

fn source_refresh_with_storage(storage: &StorageApi, id: i64) -> CommandResult {
    run_targeted_source_operation(storage, "refreshing source", None, || {
        source_record_by_id(storage, id)
    })
}

fn source_remove_by_id_with_storage(storage: &StorageApi, id: i64) -> CommandResult {
    match with_scan_idle_operation(|| storage.source_remove_by_id(id)) {
        Ok(Ok(source)) => ok(format!("Source '{}' removed.", source.display_name)),
        Ok(Err(error)) => fail(error.to_string()),
        Err(error) => fail(error),
    }
}

fn source_remove_with_storage(storage: &StorageApi, path: &str) -> CommandResult {
    match with_scan_idle_operation(|| storage.sources_remove(path)) {
        Ok(Ok(true)) => ok("Source removed."),
        Ok(Ok(false)) => ok("Source was not configured."),
        Ok(Err(error)) => fail(error.to_string()),
        Err(error) => fail(error),
    }
}

#[tauri::command]
pub async fn sources_list() -> Result<Vec<SourceDto>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let s = storage()?;
        source_dtos_with_storage(s)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn first_run_source_suggestions() -> Result<Vec<FirstRunSourceSuggestionDto>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let home = std::env::var_os("HOME").ok_or_else(|| "HOME is not set.".to_string())?;
        Ok(detect_first_run_source_suggestions(Path::new(&home)))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn source_add(path: String) -> CommandResult {
    tauri::async_runtime::spawn_blocking(move || match storage() {
        Ok(s) => source_add_with_storage(s, &path),
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn source_rename(id: i64, display_name: String) -> CommandResult {
    tauri::async_runtime::spawn_blocking(move || match storage() {
        Ok(storage) => source_rename_with_storage(storage, id, &display_name),
        Err(error) => fail(error),
    })
    .await
    .unwrap_or_else(|error| fail(error.to_string()))
}

#[tauri::command]
pub async fn source_set_recursive(id: i64, recursive: bool) -> CommandResult {
    tauri::async_runtime::spawn_blocking(move || match storage() {
        Ok(storage) => source_set_recursive_with_storage(storage, id, recursive),
        Err(error) => fail(error),
    })
    .await
    .unwrap_or_else(|error| fail(error.to_string()))
}

#[tauri::command]
pub async fn source_refresh(id: i64) -> CommandResult {
    tauri::async_runtime::spawn_blocking(move || match storage() {
        Ok(storage) => source_refresh_with_storage(storage, id),
        Err(error) => fail(error),
    })
    .await
    .unwrap_or_else(|error| fail(error.to_string()))
}

#[tauri::command]
pub async fn source_remove_by_id(id: i64) -> CommandResult {
    tauri::async_runtime::spawn_blocking(move || match storage() {
        Ok(storage) => source_remove_by_id_with_storage(storage, id),
        Err(error) => fail(error),
    })
    .await
    .unwrap_or_else(|error| fail(error.to_string()))
}

#[tauri::command]
pub async fn source_remove(path: String) -> CommandResult {
    tauri::async_runtime::spawn_blocking(move || match storage() {
        Ok(storage) => source_remove_with_storage(storage, &path),
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn validate_sources() -> CommandResult {
    tauri::async_runtime::spawn_blocking(|| match storage() {
        Ok(s) => match wc_app::sources_maintenance::validate_sources(s) {
            Ok(report) => {
                if report.missing.is_empty() {
                    ok("All sources are valid.")
                } else {
                    fail(format!("{} source(s) are missing.", report.missing.len()))
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
        Ok(s) => match wc_app::sources_maintenance::remove_missing_sources(s) {
            Ok(report) => ok(format!(
                "Removed {} missing source(s).",
                report.removed.len()
            )),
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
            update_scan_stage("adding Wallpaper Engine sources");
            scan_steam_workshop_and_index(s, Path::new(&home))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn wait_until_scan_is_running() {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !crate::commands::scan::current_scan_progress_snapshot().running {
            assert!(
                std::time::Instant::now() < deadline,
                "targeted source operation did not enter the scan state"
            );
            std::thread::yield_now();
        }
    }

    fn storage_for_same_config(storage: &StorageApi) -> StorageApi {
        StorageApi::new(wc_core::ConfigDir {
            path: storage.cd.path.clone(),
        })
    }

    fn storage() -> (tempfile::TempDir, wc_storage::StorageApi) {
        let tmp = tempfile::tempdir().unwrap();
        let cd = wc_core::ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        (tmp, wc_storage::StorageApi::new(cd))
    }

    fn source_backed_count(storage: &wc_storage::StorageApi) -> usize {
        wc_storage::sqlite::source_backed_library_count(&storage.cd).unwrap()
    }

    #[test]
    fn source_dtos_use_persisted_metadata_and_keep_legacy_is_we_spelling() {
        let _guard = crate::commands::scan::TEST_SCAN_LOCK.lock().unwrap();
        crate::commands::scan::reset_scan_state_for_test();
        let (tmp, storage) = storage();
        let workshop = tmp.path().join("steamapps/workshop/content/431960");
        std::fs::create_dir_all(&workshop).unwrap();
        let source = storage.source_create(&workshop.to_string_lossy()).unwrap();
        storage.source_rename(source.id, "Curated scenes").unwrap();

        let dto = source_dtos_with_storage(&storage).unwrap().remove(0);

        assert_eq!(dto.id, source.id);
        assert_eq!(dto.display_name, "Curated scenes");
        assert_eq!(dto.label, "Curated scenes");
        assert_eq!(dto.kind, "wallpaper_engine_workshop");
        assert!(!dto.recursive);
        assert_eq!(dto.availability, "unknown");
        assert!(!dto.added_at.is_empty());
        assert!(dto.exists);
        assert!(dto.is_we);
        let json = serde_json::to_value(dto).unwrap();
        assert_eq!(json["isWE"], true);
        assert!(json.get("isWe").is_none());
    }

    #[test]
    fn rename_updates_only_the_display_name_without_scanning() {
        let _guard = crate::commands::scan::TEST_SCAN_LOCK.lock().unwrap();
        crate::commands::scan::reset_scan_state_for_test();
        let (tmp, storage) = storage();
        let root = tmp.path().join("walls");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("not-indexed.jpg"), b"wallpaper").unwrap();
        let source = storage.source_create(&root.to_string_lossy()).unwrap();

        let result = source_rename_with_storage(&storage, source.id, "My walls");

        assert!(result.success, "{}", result.stderr);
        assert_eq!(source_backed_count(&storage), 0);
        assert_eq!(
            storage.source_records().unwrap()[0].display_name,
            "My walls"
        );
    }

    #[test]
    fn recursive_change_refreshes_only_that_source() {
        let _guard = crate::commands::scan::TEST_SCAN_LOCK.lock().unwrap();
        crate::commands::scan::reset_scan_state_for_test();
        let (tmp, storage) = storage();
        let first = tmp.path().join("first");
        let second = tmp.path().join("second");
        std::fs::create_dir_all(first.join("nested")).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        std::fs::write(first.join("nested/hidden.jpg"), b"hidden").unwrap();
        std::fs::write(second.join("keep.jpg"), b"keep").unwrap();
        let first_source = storage.source_create(&first.to_string_lossy()).unwrap();
        storage.source_create(&second.to_string_lossy()).unwrap();
        wc_app::library_refresh::refresh_library_sources(&storage, |_, _| {
            wc_scan::ScanControl::Continue
        })
        .unwrap();

        let result = source_set_recursive_with_storage(&storage, first_source.id, false);

        assert!(result.success, "{}", result.stderr);
        assert!(result.stdout.contains("indexed"));
        assert_eq!(source_backed_count(&storage), 1);
        let records = storage.source_records().unwrap();
        assert!(
            !records
                .iter()
                .find(|s| s.id == first_source.id)
                .unwrap()
                .recursive
        );
    }

    #[test]
    fn refresh_and_add_index_only_the_target_and_explain_partial_outcomes() {
        let _guard = crate::commands::scan::TEST_SCAN_LOCK.lock().unwrap();
        crate::commands::scan::reset_scan_state_for_test();
        let (tmp, storage) = storage();
        let first = tmp.path().join("first");
        let second = tmp.path().join("second");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        std::fs::write(first.join("one.jpg"), b"one").unwrap();
        std::fs::write(second.join("two.jpg"), b"two").unwrap();

        let added = source_add_with_storage(&storage, &first.to_string_lossy());
        assert!(added.success, "{}", added.stderr);
        assert!(added.stdout.contains("indexed 1"), "{}", added.stdout);
        assert_eq!(source_backed_count(&storage), 1);

        std::fs::write(first.join("new.jpg"), b"new").unwrap();
        let existing = source_add_with_storage(&storage, &first.to_string_lossy());
        assert!(existing.success, "{}", existing.stderr);
        assert!(existing.stdout.contains("indexed 2"), "{}", existing.stdout);
        assert_eq!(source_backed_count(&storage), 2);

        let second_source = storage.source_create(&second.to_string_lossy()).unwrap();
        let refreshed = source_refresh_with_storage(&storage, second_source.id);
        assert!(refreshed.success, "{}", refreshed.stderr);
        assert_eq!(source_backed_count(&storage), 3);

        let missing = tmp.path().join("missing");
        let offline = source_add_with_storage(&storage, &missing.to_string_lossy());
        assert!(offline.success, "{}", offline.stderr);
        assert!(offline.stdout.contains("offline"), "{}", offline.stdout);

        std::fs::write(&missing, b"not a directory").unwrap();
        let missing_source = storage
            .source_records()
            .unwrap()
            .into_iter()
            .find(|source| source.path == missing.to_string_lossy())
            .unwrap();
        let incomplete = source_refresh_with_storage(&storage, missing_source.id);
        assert!(incomplete.success, "{}", incomplete.stderr);
        assert!(
            incomplete.stdout.contains("incomplete"),
            "{}",
            incomplete.stdout
        );

        let unknown = source_refresh_with_storage(&storage, 999_999);
        assert!(!unknown.success);
        assert!(unknown.stderr.contains("not found"));
    }

    #[test]
    fn add_reports_that_the_source_was_saved_when_refresh_is_cancelled() {
        let _guard = crate::commands::scan::TEST_SCAN_LOCK.lock().unwrap();
        crate::commands::scan::reset_scan_state_for_test();
        let (tmp, storage) = storage();
        storage.source_records().unwrap();
        let root = tmp.path().join("added-before-cancel");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("keep.jpg"), b"keep").unwrap();
        let path = root.to_string_lossy().into_owned();

        let worker_storage = storage_for_same_config(&storage);
        let write_lock = wc_storage::sqlite::open_runtime_connection(&storage.cd).unwrap();
        write_lock.execute_batch("BEGIN IMMEDIATE").unwrap();
        let worker = std::thread::spawn(move || source_add_with_storage(&worker_storage, &path));
        wait_until_scan_is_running();
        let cancellation = crate::commands::scan::request_scan_cancel();
        write_lock.execute_batch("COMMIT").unwrap();
        drop(write_lock);
        let result = worker.join().unwrap();

        assert!(cancellation.success, "{}", cancellation.stderr);
        assert!(!result.success);
        assert!(
            result.stderr.contains("Source is saved"),
            "{}",
            result.stderr
        );
        assert!(
            result.stderr.contains("refresh was cancelled"),
            "{}",
            result.stderr
        );
        assert!(
            result.stderr.contains("previous snapshot was preserved"),
            "{}",
            result.stderr
        );
        assert_eq!(storage.source_records().unwrap().len(), 1);
        assert_eq!(source_backed_count(&storage), 0);
    }

    #[test]
    fn recursive_change_reports_saved_setting_when_refresh_is_cancelled() {
        let _guard = crate::commands::scan::TEST_SCAN_LOCK.lock().unwrap();
        crate::commands::scan::reset_scan_state_for_test();
        let (tmp, storage) = storage();
        let root = tmp.path().join("recursive-before-cancel");
        std::fs::create_dir_all(root.join("nested")).unwrap();
        std::fs::write(root.join("nested/keep.jpg"), b"keep").unwrap();
        let source = storage.source_create(&root.to_string_lossy()).unwrap();
        wc_app::library_refresh::refresh_library_source(&storage, source.id, |_, _| {
            wc_scan::ScanControl::Continue
        })
        .unwrap();

        let worker_storage = storage_for_same_config(&storage);
        let write_lock = wc_storage::sqlite::open_runtime_connection(&storage.cd).unwrap();
        write_lock.execute_batch("BEGIN IMMEDIATE").unwrap();
        let worker = std::thread::spawn(move || {
            source_set_recursive_with_storage(&worker_storage, source.id, false)
        });
        wait_until_scan_is_running();
        let cancellation = crate::commands::scan::request_scan_cancel();
        write_lock.execute_batch("COMMIT").unwrap();
        drop(write_lock);
        let result = worker.join().unwrap();

        assert!(cancellation.success, "{}", cancellation.stderr);
        assert!(!result.success);
        assert!(
            result.stderr.contains("Recursion setting is saved"),
            "{}",
            result.stderr
        );
        assert!(
            result.stderr.contains("refresh was cancelled"),
            "{}",
            result.stderr
        );
        assert!(
            result.stderr.contains("previous snapshot was preserved"),
            "{}",
            result.stderr
        );
        assert!(!storage.source_records().unwrap()[0].recursive);
        assert_eq!(source_backed_count(&storage), 1);
    }

    #[test]
    fn remove_commands_reject_a_running_scan_without_mutating_sources() {
        let _guard = crate::commands::scan::TEST_SCAN_LOCK.lock().unwrap();
        crate::commands::scan::reset_scan_state_for_test();
        let (tmp, storage) = storage();
        let by_id_root = tmp.path().join("by-id");
        let legacy_root = tmp.path().join("legacy");
        std::fs::create_dir_all(&by_id_root).unwrap();
        std::fs::create_dir_all(&legacy_root).unwrap();
        let by_id = storage
            .source_create(&by_id_root.to_string_lossy())
            .unwrap();
        storage
            .source_create(&legacy_root.to_string_lossy())
            .unwrap();

        crate::commands::scan::mark_scan_started("test scan").unwrap();
        let by_id_result = source_remove_by_id_with_storage(&storage, by_id.id);
        let legacy_result = source_remove_with_storage(&storage, &legacy_root.to_string_lossy());
        crate::commands::scan::finish_scan_success();

        assert!(!by_id_result.success);
        assert!(
            by_id_result.stderr.contains("scan is running"),
            "{}",
            by_id_result.stderr
        );
        assert!(!legacy_result.success);
        assert!(
            legacy_result.stderr.contains("scan is running"),
            "{}",
            legacy_result.stderr
        );
        assert_eq!(storage.source_records().unwrap().len(), 2);
    }

    #[test]
    fn remove_by_id_drops_only_database_membership_and_never_deletes_files() {
        let _guard = crate::commands::scan::TEST_SCAN_LOCK.lock().unwrap();
        crate::commands::scan::reset_scan_state_for_test();
        let (tmp, storage) = storage();
        let root = tmp.path().join("walls");
        std::fs::create_dir_all(&root).unwrap();
        let wallpaper = root.join("keep.jpg");
        std::fs::write(&wallpaper, b"keep").unwrap();
        let source = storage.source_create(&root.to_string_lossy()).unwrap();
        wc_app::library_refresh::refresh_library_source(&storage, source.id, |_, _| {
            wc_scan::ScanControl::Continue
        })
        .unwrap();

        let result = source_remove_by_id_with_storage(&storage, source.id);

        assert!(result.success, "{}", result.stderr);
        assert!(wallpaper.exists());
        assert!(storage.source_records().unwrap().is_empty());
        assert_eq!(source_backed_count(&storage), 0);
    }

    #[test]
    fn first_run_suggestions_offer_only_an_existing_downloads_directory() {
        let home = tempfile::tempdir().unwrap();
        let downloads = home.path().join("Downloads");
        std::fs::create_dir_all(&downloads).unwrap();

        let suggestions = detect_first_run_source_suggestions(home.path());

        assert_eq!(
            suggestions,
            vec![FirstRunSourceSuggestionDto::Directory {
                label: "Downloads".to_string(),
                path: downloads.to_string_lossy().into_owned(),
            }]
        );

        std::fs::remove_dir(&downloads).unwrap();
        assert!(detect_first_run_source_suggestions(home.path()).is_empty());
    }

    #[test]
    fn first_run_suggestions_include_discovered_wallpaper_engine_roots() {
        let home = tempfile::tempdir().unwrap();
        let workshop = home
            .path()
            .join(".local/share/Steam/steamapps/workshop/content/431960");
        std::fs::create_dir_all(&workshop).unwrap();
        let canonical_workshop = std::fs::canonicalize(&workshop).unwrap();

        let suggestions = detect_first_run_source_suggestions(home.path());

        assert_eq!(
            suggestions,
            vec![FirstRunSourceSuggestionDto::WallpaperEngine {
                roots: vec![canonical_workshop.to_string_lossy().into_owned()],
            }]
        );
        let json = serde_json::to_value(&suggestions).unwrap();
        assert_eq!(json[0]["kind"], "wallpaperEngine");
        assert_eq!(
            json[0]["roots"][0],
            canonical_workshop.to_string_lossy().as_ref()
        );
    }

    #[test]
    fn first_run_suggestion_detection_never_mutates_sources_or_scan_state() {
        let _guard = crate::commands::scan::TEST_SCAN_LOCK.lock().unwrap();
        crate::commands::scan::reset_scan_state_for_test();
        let (config_root, storage) = storage();
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join("Downloads")).unwrap();
        std::fs::create_dir_all(
            home.path()
                .join(".steam/steam/steamapps/workshop/content/431960"),
        )
        .unwrap();
        assert!(storage.source_records().unwrap().is_empty());

        let suggestions = detect_first_run_source_suggestions(home.path());

        assert_eq!(suggestions.len(), 2);
        assert!(storage.source_records().unwrap().is_empty());
        assert_eq!(source_backed_count(&storage), 0);
        assert!(!crate::commands::scan::current_scan_progress_snapshot().running);
        drop(config_root);
    }
}
