use std::collections::HashMap;

use super::common::{fail, format_bytes, ok, storage, CommandResult, ScanProgressDto};

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
        Ok(s) => match s.config_set(&key, &value) {
            Ok(()) => ok(format!("{} = {}", key, value)),
            Err(e) => fail(e.to_string()),
        },
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn export_diagnostics() -> CommandResult {
    tauri::async_runtime::spawn_blocking(|| match storage() {
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
            let content = build_diagnostics_content(s, &scan_snapshot);
            match std::fs::write(&path, content) {
                Ok(()) => ok(path.to_string_lossy().to_string()),
                Err(e) => fail(e.to_string()),
            }
        }
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

/// Build the privacy-safe diagnostics text written to the diagnostics file.
///
/// Only basenames, counts, and status summaries are included. Full filesystem
/// paths (config dir, DB, wallpaper/source paths, LWE executable, thumbnail
/// cache dir, scan `current_path`) are deliberately redacted. LWE stderr is
/// reported as length + short hash only, never the raw content.
pub(crate) fn build_diagnostics_content(
    s: &wc_storage::StorageApi,
    scan_snapshot: &ScanProgressDto,
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
        match rusqlite::Connection::open(&db_path) {
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
            out.push_str("library_counts=ok\n");
            out.push_str(&format!("library_total={}\n", c.total));
            out.push_str(&format!("library_images={}\n", c.images));
            out.push_str(&format!("library_gifs={}\n", c.gifs));
            out.push_str(&format!("library_videos={}\n", c.videos));
        }
        Err(_) => {
            out.push_str("library_counts=error\n");
            out.push_str("library_total=0\n");
            out.push_str("library_images=0\n");
            out.push_str("library_gifs=0\n");
            out.push_str("library_videos=0\n");
        }
    }

    out.push_str(&format!(
        "sources={}\n",
        s.sources_list().unwrap_or_default().len()
    ));

    let current = s.current_read().unwrap_or_default().unwrap_or_default();
    let current_basename = std::path::Path::new(&current)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    out.push_str(&format!("current={}\n", current_basename));

    let last_backend = s.last_backend_read().unwrap_or(None).unwrap_or_default();
    out.push_str(&format!("last_backend={}\n", last_backend));

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
        assert!(content.contains("sources="), "missing sources: {content}");
        assert!(
            content.contains("current=wallpaper.jpg"),
            "missing current basename: {content}"
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
}
