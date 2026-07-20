use std::collections::HashMap;
#[cfg(test)]
use wc_config::ConfigDirExt;

use super::common::{fail, format_bytes, ok, storage, CommandResult, ScanProgressDto};
use super::files;

/// Keys the GUI may write via `config_set` IPC.
///
/// Mirrors `apps/tauri-gui/frontend/src/settings/configSchema.ts` plus keys
/// written directly from shell preference hooks.
const WRITABLE_CONFIG_KEYS: &[&str] = &[
    "gui_theme",
    "image_backend",
    "gif_backend",
    "video_backend",
    "awww_resize",
    "awww_transition_type",
    "awww_transition_duration",
    "wallpaper_transition_fps",
    "mpvpaper_options",
    "mpvpaper_output",
    "linux_wallpaperengine_enabled",
    "linux_wallpaperengine_path",
    "linux_wallpaperengine_target_mode",
    "linux_wallpaperengine_target",
    "linux_wallpaperengine_scaling",
    "linux_wallpaperengine_fps",
    "linux_wallpaperengine_muted",
    "linux_wallpaperengine_volume",
    "gui_thumbnail_mode",
    "gui_thumbnail_cleanup_days",
    "gui_thumbnail_failure_ttl_secs",
    "preview_metadata",
    "gui_debug_logs",
    "open_project_location_mode",
    "gui_file_manager",
    "gui_file_manager_custom",
    "gui_terminal_file_manager",
    "gui_terminal_file_manager_custom",
    "gui_shell_preferences",
    "restore_on_login",
];

fn config_key_writable_from_gui(key: &str) -> bool {
    WRITABLE_CONFIG_KEYS.contains(&key)
}

fn validate_writable_config_set(key: &str, value: &str) -> Result<(), String> {
    if !config_key_writable_from_gui(key) {
        return Err(format!("Config key is not writable from the GUI: {key}"));
    }

    if value.contains('\n') || value.contains('\r') {
        return Err(format!(
            "Config value for {key} must not contain line breaks."
        ));
    }

    match key {
        "linux_wallpaperengine_path" => validate_linux_wallpaperengine_path(value),
        "gui_file_manager_custom" | "gui_terminal_file_manager_custom" => {
            validate_custom_command_config(value)
        }
        _ => Ok(()),
    }
}

fn validate_linux_wallpaperengine_path(value: &str) -> Result<(), String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("auto") {
        return Ok(());
    }
    let path = std::path::Path::new(trimmed);
    if !path.is_absolute() {
        return Err("linux_wallpaperengine_path must be an absolute path or \"auto\".".into());
    }
    if !path.is_file() {
        return Err(format!("linux_wallpaperengine_path not found: {trimmed}"));
    }
    Ok(())
}

fn validate_custom_command_config(value: &str) -> Result<(), String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    let first_token = trimmed
        .split_whitespace()
        .next()
        .ok_or_else(|| "Custom command is empty.".to_string())?;
    files::validate_custom_executable(first_token)
}

#[tauri::command]
pub async fn config_get(key: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let s = storage()?;
        Ok(s.config_get(&key, ""))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn config_get_many(keys: Vec<String>) -> Result<HashMap<String, String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let s = storage()?;
        let mut out = HashMap::new();
        for key in keys {
            out.insert(key.clone(), s.config_get(&key, ""));
        }
        Ok(out)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn config_set(key: String, value: String) -> CommandResult {
    tauri::async_runtime::spawn_blocking(move || match storage() {
        Ok(s) => {
            if let Err(error) = validate_writable_config_set(&key, &value) {
                return fail(error);
            }
            match s.config_set(&key, &value) {
                Ok(()) => ok(format!("{} = {}", key, value)),
                Err(e) => fail(e.to_string()),
            }
        }
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn export_diagnostics(
    state: tauri::State<'_, crate::library_service::LibraryService>,
) -> Result<CommandResult, String> {
    let service = state.inner().clone();
    Ok(
        tauri::async_runtime::spawn_blocking(move || match storage() {
            Ok(s) => {
                let dir = s.cd.path.join("diagnostics");
                if let Err(e) = std::fs::create_dir_all(&dir) {
                    return fail(e.to_string());
                }
                let path = dir.join(format!(
                    "wallpaper-console-diagnostics-{}.txt",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs()
                ));
                let scan_snapshot = super::scan::current_scan_progress_snapshot();
                let library = service.diagnostics_snapshot(&s.cd);
                let content =
                    build_diagnostics_content_with_library(s, &scan_snapshot, Some(&library));
                match std::fs::write(&path, content) {
                    Ok(()) => ok(path.to_string_lossy().to_string()),
                    Err(e) => fail(e.to_string()),
                }
            }
            Err(e) => fail(e),
        })
        .await
        .unwrap_or_else(|e| fail(e.to_string())),
    )
}

/// Build the privacy-safe diagnostics text written to the diagnostics file.
///
/// Only basenames, counts, and status summaries are included. Full filesystem
/// paths (config dir, DB, wallpaper/source paths, LWE executable, thumbnail
/// cache dir, scan `current_path`) are deliberately redacted. LWE stderr is
/// reported as length + short hash only, never the raw content.
#[cfg(test)]
pub(crate) fn build_diagnostics_content(
    s: &wc_storage::StorageApi,
    scan_snapshot: &ScanProgressDto,
) -> String {
    build_diagnostics_content_with_library(s, scan_snapshot, None)
}

fn build_diagnostics_content_with_library(
    s: &wc_storage::StorageApi,
    scan_snapshot: &ScanProgressDto,
    library: Option<&crate::library_service::LibraryServiceDiagnostics>,
) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hasher;

    let mut out = String::new();
    out.push_str("wallpaper-console diagnostics\n");

    let config_dir_basename =
        s.cd.path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
    out.push_str(&format!("config_dir={}\n", config_dir_basename));

    let db_path = s.cd.db_path();
    let db_exists = db_path.exists();
    out.push_str(&format!("db_exists={}\n", db_exists));

    let integrity = if !db_exists {
        "missing".to_string()
    } else {
        match wc_storage::sqlite::open_runtime_connection(&s.cd) {
            Ok(conn) => {
                match conn.query_row("PRAGMA integrity_check;", [], |row| row.get::<_, String>(0)) {
                    Ok(v) => {
                        let first = v.lines().next().unwrap_or(&v);
                        if first == "ok" {
                            "ok".to_string()
                        } else {
                            format!("error: {}", first)
                        }
                    }
                    Err(_) => "error: query_failed".to_string(),
                }
            }
            Err(_) => "error: open_failed".to_string(),
        }
    };
    out.push_str(&format!("sqlite_integrity={}\n", integrity));

    match wc_storage::sqlite::library_counts_sqlite(&s.cd) {
        Ok(c) => {
            out.push_str("library_counts_status=ok\n");
            out.push_str(&format!("library_total={}\n", c.total));
            out.push_str(&format!("library_images={}\n", c.images));
            out.push_str(&format!("library_gifs={}\n", c.gifs));
            out.push_str(&format!("library_videos={}\n", c.videos));
        }
        Err(_) => {
            out.push_str("library_counts_status=error\n");
        }
    }

    match s.sources_list() {
        Ok(sources) => {
            out.push_str("sources_status=ok\n");
            out.push_str(&format!("sources={}\n", sources.len()));
        }
        Err(_) => {
            out.push_str("sources_status=error\n");
        }
    }

    match s.current_read() {
        Ok(Some(current)) => {
            out.push_str("current_status=ok\n");
            let current_basename = std::path::Path::new(&current)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            out.push_str(&format!("current={}\n", current_basename));
        }
        Ok(None) => {
            out.push_str("current_status=ok\n");
            out.push_str("current=\n");
        }
        Err(_) => {
            out.push_str("current_status=error\n");
        }
    }

    match s.last_backend_read() {
        Ok(Some(last_backend)) => {
            out.push_str("last_backend_status=ok\n");
            out.push_str(&format!("last_backend={}\n", last_backend));
        }
        Ok(None) => {
            out.push_str("last_backend_status=ok\n");
            out.push_str("last_backend=\n");
        }
        Err(_) => {
            out.push_str("last_backend_status=error\n");
        }
    }

    let config = wc_backend::linux_wallpaperengine::LinuxWallpaperEngineConfig::from_storage(s);
    let lwe = wc_backend::linux_wallpaperengine::status(&config);
    out.push_str(&format!("lwe_available={}\n", lwe.available));
    out.push_str(&format!("lwe_message={}\n", lwe.message));
    out.push_str(&format!(
        "lwe_path={}\n",
        if lwe.path.is_some() {
            "present"
        } else {
            "absent"
        }
    ));

    let thumb = wc_preview::thumbnail_cache_info(&s.cd.gui_thumbnail_cache_dir());
    out.push_str(&format!("thumbnail_entries={}\n", thumb.entries));
    out.push_str(&format!(
        "thumbnail_failure_entries={}\n",
        thumb.failure_entries
    ));
    out.push_str(&format!(
        "thumbnail_size={}\n",
        format_bytes(thumb.total_bytes)
    ));

    out.push_str(&format!("scan_running={}\n", scan_snapshot.running));
    out.push_str(&format!("scan_stage={}\n", scan_snapshot.stage));
    out.push_str(&format!("scan_scanned={}\n", scan_snapshot.scanned));
    out.push_str(&format!("scan_staged={}\n", scan_snapshot.staged));
    out.push_str(&format!("scan_skipped={}\n", scan_snapshot.skipped));

    if let Some(library) = library {
        out.push_str(&format!("library_cache_hits={}\n", library.page_hits));
        out.push_str(&format!("library_cache_misses={}\n", library.page_misses));
        out.push_str(&format!("library_cache_waiters={}\n", library.page_waiters));
        out.push_str(&format!(
            "library_query_timeouts={}\n",
            library.query_timeouts
        ));
        out.push_str(&format!("library_cached_pages={}\n", library.cached_pages));
        out.push_str(&format!("library_cached_bytes={}\n", library.cached_bytes));
        out.push_str(&format!(
            "library_cached_totals={}\n",
            library.cached_totals
        ));
        out.push_str(&format!(
            "library_observer_started={}\n",
            library.observer_started
        ));
        out.push_str(&format!(
            "library_watcher_started={}\n",
            library.scheduler_started
        ));
        out.push_str(&format!("library_fts_status={}\n", library.fts_status));
        out.push_str(&format!("library_fts_revision={}\n", library.fts_revision));
        out.push_str(&format!(
            "library_fts_next_wallpaper_id={}\n",
            library.fts_next_wallpaper_id
        ));
    }
    out.push_str("ordinary_lock_deadline_ms=2000\n");
    out.push_str("maintenance_lock_deadline_ms=5000\n");
    out.push_str("scan_worker_heartbeat_timeout_ms=30000\n");
    out.push_str("display_probe_overall_budget_ms=3000\n");
    out.push_str(&format!(
        "legacy_snapshot_dirty={}\n",
        wc_app::library_rescan::library_dirty_marker_path(s).exists()
    ));

    let stderr = s.config_get("lwe_last_stderr", "");
    let exit_status = s.config_get("lwe_last_exit_status", "");
    out.push_str(&format!("lwe_last_exit_status={}\n", exit_status));
    out.push_str(&format!("lwe_last_stderr_len={}\n", stderr.len()));
    let mut hasher = DefaultHasher::new();
    hasher.write(stderr.as_bytes());
    let hash = format!("{:016x}", hasher.finish());
    out.push_str(&format!("lwe_last_stderr_hash={}\n", &hash[..8]));

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idle_scan_snapshot() -> ScanProgressDto {
        ScanProgressDto {
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
        }
    }

    fn diagnostics_storage() -> (tempfile::TempDir, wc_storage::StorageApi) {
        let tmp = tempfile::tempdir().unwrap();
        let cd = wc_core::ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        let s = wc_storage::StorageApi::new(cd);
        (tmp, s)
    }

    #[test]
    fn config_set_allows_known_gui_keys() {
        assert!(validate_writable_config_set("gui_theme", "light").is_ok());
        assert!(validate_writable_config_set("restore_on_login", "on").is_ok());
        assert!(validate_writable_config_set("linux_wallpaperengine_path", "auto").is_ok());
    }

    #[test]
    fn config_set_rejects_unknown_keys() {
        let error = validate_writable_config_set("lwe_last_stderr", "evil").unwrap_err();
        assert!(
            error.contains("Config key is not writable from the GUI: lwe_last_stderr"),
            "{error}"
        );

        let error = validate_writable_config_set("linux_wallpaperengine_pid", "1234").unwrap_err();
        assert!(
            error.contains("Config key is not writable from the GUI"),
            "{error}"
        );
    }

    #[test]
    fn config_set_rejects_multiline_values() {
        let error = validate_writable_config_set("gui_theme", "light\nevil").unwrap_err();
        assert!(error.contains("line breaks"), "{error}");
    }

    #[test]
    fn config_set_validates_linux_wallpaperengine_path() {
        let error =
            validate_writable_config_set("linux_wallpaperengine_path", "./evil").unwrap_err();
        assert!(error.contains("absolute path"), "{error}");

        let error =
            validate_writable_config_set("linux_wallpaperengine_path", "/definitely/missing/lwe")
                .unwrap_err();
        assert!(error.contains("not found"), "{error}");

        let tmp = tempfile::tempdir().unwrap();
        let exe = tmp.path().join("linux-wallpaperengine");
        std::fs::write(&exe, b"#!/bin/sh\n").unwrap();
        assert!(
            validate_writable_config_set("linux_wallpaperengine_path", &exe.to_string_lossy())
                .is_ok()
        );
    }

    #[test]
    fn config_set_validates_custom_executable_commands() {
        let error = validate_writable_config_set("gui_file_manager_custom", "./evil-fm {path}")
            .unwrap_err();
        assert!(error.contains("absolute path"), "{error}");

        assert!(
            validate_writable_config_set("gui_terminal_file_manager_custom", "yazi {path}").is_ok()
        );
    }

    #[test]
    fn diagnostics_includes_all_required_fields() {
        let (_tmp, s) = diagnostics_storage();
        s.current_write("/some/secret/path/wallpaper.jpg").unwrap();
        s.last_backend_write("awww").unwrap();
        s.config_set("lwe_last_exit_status", "0").unwrap();
        s.config_set(
            "lwe_last_stderr",
            "/home/user/secret/lwe --foo /secret/path",
        )
        .unwrap();

        let snap = idle_scan_snapshot();
        let content = build_diagnostics_content(&s, &snap);

        assert!(
            content.contains("wallpaper-console diagnostics"),
            "missing app label: {content}"
        );
        assert!(
            content.contains("config_dir=wallpaper-console"),
            "missing config_dir basename: {content}"
        );
        assert!(
            content.contains("db_exists="),
            "missing db_exists: {content}"
        );
        assert!(
            content.contains("sqlite_integrity="),
            "missing sqlite_integrity: {content}"
        );
        assert!(
            content.contains("library_counts_status=ok"),
            "missing library_counts_status: {content}"
        );
        assert!(
            content.contains("library_total="),
            "missing library_total: {content}"
        );
        assert!(
            content.contains("library_images="),
            "missing library_images: {content}"
        );
        assert!(
            content.contains("library_gifs="),
            "missing library_gifs: {content}"
        );
        assert!(
            content.contains("library_videos="),
            "missing library_videos: {content}"
        );
        assert!(
            content.contains("sources_status=ok"),
            "missing sources_status: {content}"
        );
        assert!(content.contains("sources="), "missing sources: {content}");
        assert!(
            content.contains("current_status=ok"),
            "missing current_status: {content}"
        );
        assert!(
            content.contains("current=wallpaper.jpg"),
            "missing current basename: {content}"
        );
        assert!(
            content.contains("last_backend_status=ok"),
            "missing last_backend_status: {content}"
        );
        assert!(
            content.contains("last_backend=awww"),
            "missing last_backend: {content}"
        );
        assert!(
            content.contains("lwe_available="),
            "missing lwe_available: {content}"
        );
        assert!(
            content.contains("lwe_message="),
            "missing lwe_message: {content}"
        );
        assert!(content.contains("lwe_path="), "missing lwe_path: {content}");
        assert!(
            content.contains("thumbnail_entries="),
            "missing thumbnail_entries: {content}"
        );
        assert!(
            content.contains("thumbnail_failure_entries="),
            "missing thumbnail_failure_entries: {content}"
        );
        assert!(
            content.contains("thumbnail_size="),
            "missing thumbnail_size: {content}"
        );
        assert!(
            content.contains("scan_running="),
            "missing scan_running: {content}"
        );
        assert!(
            content.contains("scan_stage="),
            "missing scan_stage: {content}"
        );
        assert!(
            content.contains("scan_scanned="),
            "missing scan_scanned: {content}"
        );
        assert!(
            content.contains("scan_staged="),
            "missing scan_staged: {content}"
        );
        assert!(
            content.contains("scan_skipped="),
            "missing scan_skipped: {content}"
        );
        assert!(
            content.contains("lwe_last_exit_status="),
            "missing lwe_last_exit_status: {content}"
        );
        assert!(
            content.contains("lwe_last_stderr_len="),
            "missing lwe_last_stderr_len: {content}"
        );
        assert!(
            content.contains("lwe_last_stderr_hash="),
            "missing lwe_last_stderr_hash: {content}"
        );
    }

    #[test]
    fn diagnostics_redacts_full_paths_and_sensitive_lwe_fields() {
        let (tmp, s) = diagnostics_storage();
        let tmp_path = tmp.path().to_string_lossy().to_string();
        let secret_current = "/some/secret/path/wallpaper.jpg";
        let secret_stderr = "/home/user/secret/lwe --foo /secret/path";
        s.current_write(secret_current).unwrap();
        s.last_backend_write("awww").unwrap();
        s.config_set("lwe_last_stderr", secret_stderr).unwrap();
        s.config_set("lwe_last_command_line", "/secret/cmd /secret/target")
            .unwrap();
        s.config_set(
            "lwe_last_target_config",
            "target_mode=screen-root target=eDP-1",
        )
        .unwrap();

        let snap = idle_scan_snapshot();
        let content = build_diagnostics_content(&s, &snap);

        assert!(
            !content.contains(&tmp_path),
            "full tempdir path leaked: {content}"
        );
        assert!(
            !content.contains(secret_current),
            "full current path leaked: {content}"
        );
        assert!(
            !content.contains(secret_stderr),
            "raw lwe stderr leaked: {content}"
        );
        assert!(
            !content.contains("lwe_last_command_line"),
            "lwe_last_command_line field leaked: {content}"
        );
        assert!(
            !content.contains("lwe_last_target_config"),
            "lwe_last_target_config field leaked: {content}"
        );
        assert!(
            content.contains("wallpaper.jpg"),
            "current basename missing: {content}"
        );
    }

    #[test]
    fn diagnostics_reports_missing_db_integrity_when_db_absent() {
        let (_tmp, s) = diagnostics_storage();
        let _ = std::fs::remove_file(s.cd.db_path());

        let snap = idle_scan_snapshot();
        let content = build_diagnostics_content(&s, &snap);

        assert!(
            content.contains("db_exists=false"),
            "expected db_exists=false: {content}"
        );
        assert!(
            content.contains("sqlite_integrity=missing"),
            "expected sqlite_integrity=missing: {content}"
        );
    }

    #[test]
    fn diagnostics_redacts_configured_linux_wallpaperengine_path() {
        let (tmp, s) = diagnostics_storage();
        let secret_lwe_path = tmp
            .path()
            .join("secret-lwe-binary")
            .to_string_lossy()
            .to_string();
        s.config_set("linux_wallpaperengine_path", &secret_lwe_path)
            .unwrap();

        let snap = idle_scan_snapshot();
        let content = build_diagnostics_content(&s, &snap);

        assert!(
            !content.contains(&secret_lwe_path),
            "configured LWE path must not leak into diagnostics: {content}"
        );
    }

    #[test]
    fn diagnostics_reports_library_count_errors_without_zero_placeholders() {
        let (_tmp, s) = diagnostics_storage();
        let conn = rusqlite::Connection::open(s.cd.db_path()).unwrap();
        conn.execute("ALTER TABLE wallpapers RENAME TO wallpapers_backup", [])
            .unwrap();
        drop(conn);

        let content = build_diagnostics_content(&s, &idle_scan_snapshot());
        assert!(
            content.contains("library_counts_status=error"),
            "expected library_counts_status=error: {content}"
        );
        assert!(
            !content.contains("library_total=0"),
            "must not fabricate zero counts on read failure: {content}"
        );
    }
}
