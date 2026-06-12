use std::path::{Path, PathBuf};

use super::common::*;

const PKG_VERSION: &str = env!("CARGO_PKG_VERSION");
const PKG_NAME: &str = "wallpaper-console-gui-rust";

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
pub async fn config_set(key: String, value: String) -> CommandResult {
    let result = tauri::async_runtime::spawn_blocking(move || {
        match storage().and_then(|s| s.config_set(&key, &value).map_err(|e| e.to_string())) {
            Ok(()) => ok(format!("{} = {}", key, value)),
            Err(err) => fail(err),
        }
    })
    .await;
    match result {
        Ok(r) => r,
        Err(e) => fail(e.to_string()),
    }
}

// ── SQLite maintenance ─────────────────────────────────────────────────────

#[tauri::command]
pub async fn migrate_to_sqlite() -> CommandResult {
    let result = tauri::async_runtime::spawn_blocking(|| {
        match storage()
            .and_then(|s| wc_storage::sqlite::migrate_to_sqlite(&s.cd).map_err(|e| e.to_string()))
        {
            Ok(()) => ok("Migrated to SQLite."),
            Err(err) => fail(err),
        }
    })
    .await;
    match result {
        Ok(r) => r,
        Err(e) => fail(e.to_string()),
    }
}

#[tauri::command]
pub async fn sqlite_verify() -> CommandResult {
    let result = tauri::async_runtime::spawn_blocking(|| {
        match storage().and_then(|s| wc_storage::sqlite::verify(&s.cd).map_err(|e| e.to_string())) {
            Ok(wc_storage::sqlite::VerifyResult::Ok) => ok("VERIFY OK"),
            Ok(wc_storage::sqlite::VerifyResult::OkWithWarnings(warnings)) => {
                ok(format!("VERIFY OK WITH WARNINGS\n{}", warnings.join("\n")))
            }
            Ok(wc_storage::sqlite::VerifyResult::Failed(errors)) => fail(format!(
                "VERIFY FAILED: {} mismatch(es) found: {}",
                errors.len(),
                errors.join(", ")
            )),
            Err(err) => fail(err),
        }
    })
    .await;
    match result {
        Ok(r) => r,
        Err(e) => fail(e.to_string()),
    }
}

#[tauri::command]
pub async fn sqlite_resync() -> CommandResult {
    let result = tauri::async_runtime::spawn_blocking(|| {
        match storage().and_then(|s| wc_storage::sqlite::resync(&s.cd).map_err(|e| e.to_string())) {
            Ok(()) => ok("Resync complete."),
            Err(err) => fail(err),
        }
    })
    .await;
    match result {
        Ok(r) => r,
        Err(e) => fail(e.to_string()),
    }
}

#[tauri::command]
pub async fn sqlite_backup() -> CommandResult {
    let result = tauri::async_runtime::spawn_blocking(|| {
        match storage().and_then(|s| wc_storage::sqlite::backup(&s.cd).map_err(|e| e.to_string())) {
            Ok(path) => ok(path),
            Err(err) => fail(err),
        }
    })
    .await;
    match result {
        Ok(r) => r,
        Err(e) => fail(e.to_string()),
    }
}

#[tauri::command]
pub async fn sqlite_restore(path: String) -> CommandResult {
    let result = tauri::async_runtime::spawn_blocking(move || {
        match storage().and_then(|s| {
            wc_storage::sqlite::restore(&s.cd, &PathBuf::from(path)).map_err(|e| e.to_string())
        }) {
            Ok(()) => ok("Restore complete."),
            Err(err) => fail(err),
        }
    })
    .await;
    match result {
        Ok(r) => r,
        Err(e) => fail(e.to_string()),
    }
}

#[tauri::command]
pub async fn sqlite_export_flat() -> CommandResult {
    let result = tauri::async_runtime::spawn_blocking(|| {
        match storage()
            .and_then(|s| wc_storage::sqlite::export_flat(&s.cd).map_err(|e| e.to_string()))
        {
            Ok(()) => ok("Export complete."),
            Err(err) => fail(err),
        }
    })
    .await;
    match result {
        Ok(r) => r,
        Err(e) => fail(e.to_string()),
    }
}

// ── Export Diagnostics ────────────────────────────────────────────────────

#[tauri::command]
pub async fn export_diagnostics() -> CommandResult {
    let result = tauri::async_runtime::spawn_blocking(|| match storage() {
        Ok(s) => export_diagnostics_impl(&s.cd),
        Err(e) => fail(e),
    })
    .await;
    match result {
        Ok(r) => r,
        Err(e) => fail(e.to_string()),
    }
}

fn path_basename(p: &Path) -> String {
    p.file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| p.to_string_lossy().to_string())
}

fn export_diagnostics_impl(cd: &wc_core::config::ConfigDir) -> CommandResult {
    let diag_dir = cd.path.join("diagnostics");
    if let Err(e) = std::fs::create_dir_all(&diag_dir) {
        return fail(format!("Failed to create diagnostics directory: {}", e));
    }

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let filename = format!("wallpaper-console-diagnostics-{}.txt", ts);
    let file_path = diag_dir.join(&filename);

    let mut sections: Vec<String> = Vec::new();

    sections.push(format!(
        "wallpaper-console diagnostics\n==========================\nExported: {}",
        ts
    ));

    // [Application]
    sections.push(format!(
        "[Application]\nversion: {}\npackage: {}",
        PKG_VERSION, PKG_NAME
    ));

    // [System]
    sections.push(format!(
        "[System]\nos: {}\narch: {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    ));

    // [Config]
    let s = wc_storage::StorageApi::new(wc_core::config::ConfigDir {
        path: cd.path.clone(),
    });
    let storage_backend = s.config_get("storage_backend", "sqlite");
    sections.push(format!(
        "[Config]\nconfig_dir: {}\nstorage_backend: {}",
        path_basename(&cd.path),
        storage_backend,
    ));

    // [Library]
    let sqlite_ready = cd.db_path().exists();
    let sqlite_rows = if sqlite_ready {
        wc_storage::sqlite::library_count(cd).unwrap_or(0)
    } else {
        0
    };
    let tsv_rows = std::fs::read_to_string(cd.library_tsv_path())
        .map(|c| c.lines().filter(|l| !l.trim().is_empty()).count())
        .unwrap_or(0);

    let lib_status = format!("sqlite ({} entries)", sqlite_rows);

    sections.push(format!(
        "[Library]\nlibrary_source_status: {}\nsqlite_ready: {}\nsqlite_rows: {}\ntsv_rows: {}",
        lib_status, sqlite_ready, sqlite_rows, tsv_rows
    ));

    // [Sources]
    let sources = s.sources_list().unwrap_or_default();
    let source_count = sources.len();
    let source_basenames: Vec<String> = sources
        .iter()
        .map(|p| path_basename(Path::new(p)))
        .collect();
    sections.push(format!(
        "[Sources]\nsource_count: {}\nsources: {}",
        source_count,
        source_basenames.join(", ")
    ));

    // [History]
    let history_count = s.history_list().unwrap_or_default().len();
    sections.push(format!("[History]\nhistory_count: {}", history_count));

    // [Favorites]
    let favorites_count = s.favorites_list().unwrap_or_default().len();
    sections.push(format!("[Favorites]\nfavorites_count: {}", favorites_count));

    // [Thumbnails]
    let thumb_dir = cd.gui_thumbnail_cache_dir();
    let thumb_info = wc_preview::thumbnail_cache_info(&thumb_dir);
    sections.push(format!(
        "[Thumbnails]\ncache_dir: {}\ncache_entries: {}\ncache_size: {}\ncache_failure_entries: {}",
        path_basename(&thumb_dir),
        thumb_info.entries,
        format_bytes(thumb_info.total_bytes),
        thumb_info.failure_entries
    ));

    let content = sections.join("\n\n");
    match std::fs::write(&file_path, &content) {
        Ok(()) => ok(file_path.to_string_lossy().to_string()),
        Err(e) => fail(format!("Failed to write diagnostics file: {}", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_diagnostics_creates_file() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = wc_core::config::ConfigDir {
            path: tmp.path().to_path_buf(),
        };
        cd.init().unwrap();

        let result = export_diagnostics_impl(&cd);
        assert!(
            result.success,
            "export_diagnostics should succeed: {:?}",
            result.stderr
        );
        assert!(
            result.stdout.contains("diagnostics"),
            "should mention diagnostics in output: {}",
            result.stdout
        );

        let diag_dir = tmp.path().join("diagnostics");
        assert!(diag_dir.exists());
        let files: Vec<_> = std::fs::read_dir(&diag_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(files.len(), 1);
        let content = std::fs::read_to_string(&files[0].path()).unwrap();
        assert!(content.contains("[Application]"));
        assert!(content.contains("[System]"));
        assert!(content.contains("[Config]"));
        assert!(content.contains("[Library]"));
        assert!(content.contains("[Sources]"));
        assert!(content.contains("[History]"));
        assert!(content.contains("[Favorites]"));
        assert!(content.contains("[Thumbnails]"));
        assert!(content.contains(&format!("version: {}", PKG_VERSION)));
    }

    #[test]
    fn path_basename_strips_directory() {
        assert_eq!(
            path_basename(Path::new("/home/user/.config/wallpaper-console")),
            "wallpaper-console"
        );
        assert_eq!(
            path_basename(Path::new("wallpaper-console")),
            "wallpaper-console"
        );
    }

    #[test]
    fn diagnostics_no_full_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = wc_core::config::ConfigDir {
            path: tmp.path().to_path_buf(),
        };
        cd.init().unwrap();

        let result = export_diagnostics_impl(&cd);
        assert!(result.success);
        let diag_dir = tmp.path().join("diagnostics");
        let file = std::fs::read_dir(&diag_dir)
            .unwrap()
            .next()
            .unwrap()
            .unwrap();
        let content = std::fs::read_to_string(file.path()).unwrap();
        let full_path = tmp.path().to_string_lossy().to_string();
        assert!(
            !content.contains(&full_path),
            "diagnostics must not contain full config path: {}",
            full_path
        );
    }
}
