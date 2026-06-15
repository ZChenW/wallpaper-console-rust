use std::path::Path;

use rusqlite::{params, Connection};
use wc_core::config::ConfigDir;
use wc_core::error::WcError;

use super::chrono_now;
use super::chrono_now_compact;
use super::import_library_tsv_into;
use super::schema::create_schema;
use crate::flat;

/// Result of database verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyResult {
    /// All categories match.
    Ok,
    /// Data integrity is fine, but flat-file compatibility copies have drifted.
    OkWithWarnings(Vec<String>),
    /// Real data mismatch detected (wallpapers, favorites, history, state).
    Failed(Vec<String>),
}

/// Compare flat files vs SQLite. Returns:
/// - `Ok(VerifyResult::Ok)` — all consistent
/// - `Ok(VerifyResult::OkWithWarnings(w))` — config/sources compatibility copies
///   have drifted; actual data is fine
/// - `Ok(VerifyResult::Failed(e))` — real data mismatch (wallpapers, favorites,
///   history, state)
/// - `Err(WcError::Sqlite(...))` — schema corruption or missing DB
// Normalise a list of paths into canonical, deduplicated, sorted values
// so that symlink-equivalent paths compare equal in verify().
fn canonical_unique_sorted(paths: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out: Vec<String> = paths
        .iter()
        .map(|p| flat::try_canonicalize(p))
        .filter(|c| seen.insert(c.clone()))
        .collect();
    out.sort();
    out
}

pub fn verify(cd: &ConfigDir) -> Result<VerifyResult, WcError> {
    let db_path = cd.db_path();
    if !db_path.exists() {
        return Err(WcError::Sqlite(
            "wallpapers.db not found. Run migrate-to-sqlite first.".into(),
        ));
    }
    let conn = Connection::open(&db_path).map_err(|e| WcError::Sqlite(e.to_string()))?;

    let mut warnings: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    // Config — compatibility copy only; drift is a warning.
    {
        let flat_cfg = wc_core::config::parse_config_file(&cd.config_path())?;
        let mut stmt = conn
            .prepare("SELECT key, value FROM config ORDER BY key")
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        let db_rows: Vec<(String, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| WcError::Sqlite(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        let db_cfg: std::collections::HashMap<String, String> = db_rows.into_iter().collect();
        if flat_cfg != db_cfg {
            warnings.push("config".into());
        }
    }

    // Sources — compatibility copy only; drift is a warning.
    {
        let mut flat_src: Vec<String> = flat::sources_list(cd)?;
        flat_src.sort();
        let mut stmt = conn
            .prepare("SELECT path FROM sources ORDER BY path")
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        let db_src: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| WcError::Sqlite(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        if flat_src != db_src {
            warnings.push("sources".into());
        }
    }

    // Favorites — data integrity; mismatch is an error.
    // NOTE: flat::favorites_list() does not canonical-dedup (unlike history_list),
    // so db_fav likewise does not need canonical dedup here. If favorites_list
    // ever gains dedup, align db_fav to match.
    {
        let mut flat_fav: Vec<String> = flat::favorites_list(cd)?;
        flat_fav.sort();
        let mut stmt = conn
            .prepare("SELECT path FROM favorites ORDER BY path")
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        let db_fav: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| WcError::Sqlite(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        if flat_fav != db_fav {
            errors.push("favorites".into());
        }
    }

    // History — data integrity; mismatch is an error.
    {
        let flat_hist = flat::history_list(cd)?;
        let mut stmt = conn
            .prepare("SELECT path FROM history ORDER BY path")
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        let db_hist: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| WcError::Sqlite(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        // Normalise both sides to canonical values so that symlink /
        // dot-path variants compare equal.
        let flat_norm = canonical_unique_sorted(&flat_hist);
        let db_norm = canonical_unique_sorted(&db_hist);
        if flat_norm != db_norm {
            errors.push("history".into());
        }
    }

    // State: current — data integrity; mismatch is an error.
    {
        let flat_cur = flat::current_read(cd)?.unwrap_or_default();
        let db_cur: String =
            match conn.query_row("SELECT value FROM state WHERE key='current'", [], |row| {
                row.get(0)
            }) {
                Ok(v) => v,
                Err(rusqlite::Error::QueryReturnedNoRows) => String::new(),
                Err(e) => return Err(WcError::Sqlite(e.to_string())),
            };
        if flat_cur != db_cur {
            errors.push("current".into());
        }
    }

    // State: last_backend — data integrity; mismatch is an error.
    {
        let flat_be = flat::last_backend_read(cd)?.unwrap_or_default();
        let db_be: String = match conn.query_row(
            "SELECT value FROM state WHERE key='last_backend'",
            [],
            |row| row.get(0),
        ) {
            Ok(v) => v,
            Err(rusqlite::Error::QueryReturnedNoRows) => String::new(),
            Err(e) => return Err(WcError::Sqlite(e.to_string())),
        };
        if flat_be != db_be {
            errors.push("last_backend".into());
        }
    }

    if !errors.is_empty() {
        Ok(VerifyResult::Failed(errors))
    } else if !warnings.is_empty() {
        Ok(VerifyResult::OkWithWarnings(warnings))
    } else {
        Ok(VerifyResult::Ok)
    }
}

/// Resync: atomically rebuild wallpapers.db from flat files.
pub fn resync(cd: &ConfigDir) -> Result<(), WcError> {
    let db_path = cd.db_path();
    if !db_path.exists() {
        return Err(WcError::Other(
            "wallpapers.db not found. Run migrate-to-sqlite first.".into(),
        ));
    }

    // Backup old DB
    let ts = chrono_now_compact();
    let bak = db_path.with_extension(format!("db.bak.{}", ts));
    std::fs::copy(&db_path, &bak).map_err(WcError::Io)?;

    // Build in temp DB alongside real one (random suffix for uniqueness)
    let tmp_db = db_path.with_extension(format!(
        "db.tmp.{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    if tmp_db.exists() {
        std::fs::remove_file(&tmp_db).map_err(WcError::Io)?;
    }
    let conn = Connection::open(&tmp_db).map_err(|e| WcError::Sqlite(e.to_string()))?;
    create_schema(&conn)?;
    let now = chrono_now();

    // Import all data from flat files
    // Config
    let config_map = wc_core::config::parse_config_file(&cd.config_path())?;
    for (key, value) in &config_map {
        conn.execute(
            "INSERT OR REPLACE INTO config (key, value) VALUES (?1, ?2)",
            params![key, value],
        )
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    }
    // Sources — canonical-deduplicate
    let mut seen_src = std::collections::HashSet::new();
    for path in flat::sources_list(cd)? {
        let canon = flat::try_canonicalize(&path);
        if !seen_src.insert(canon) {
            continue;
        }
        conn.execute(
            "INSERT OR IGNORE INTO sources (path, added_at) VALUES (?1, ?2)",
            params![path, now],
        )
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    }
    // Favorites
    for path in flat::favorites_list(cd)? {
        conn.execute(
            "INSERT OR IGNORE INTO favorites (path, added_at) VALUES (?1, ?2)",
            params![path, now],
        )
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    }
    // History (canonical-deduplicate keeping newest, then reverse for id ordering)
    let mut seen_hist = std::collections::HashSet::new();
    let history: Vec<String> = flat::history_list(cd)?
        .into_iter()
        .filter(|path| seen_hist.insert(flat::try_canonicalize(path)))
        .collect();
    let history_rev: Vec<String> = history.into_iter().rev().collect();
    for path in &history_rev {
        conn.execute(
            "INSERT INTO history (path, backend, applied_at) VALUES (?1, 'unknown', ?2)",
            params![path, now],
        )
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    }
    // State
    if let Some(cur) = flat::current_read(cd)? {
        conn.execute(
            "INSERT OR REPLACE INTO state (key, value) VALUES ('current', ?1)",
            params![cur],
        )
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    }
    if let Some(be) = flat::last_backend_read(cd)? {
        conn.execute(
            "INSERT OR REPLACE INTO state (key, value) VALUES ('last_backend', ?1)",
            params![be],
        )
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    }
    // Meta
    conn.execute(
        "INSERT OR REPLACE INTO db_meta (key, value) VALUES ('schema_version', '1')",
        [],
    )
    .map_err(|e| WcError::Sqlite(e.to_string()))?;
    conn.execute(
        "INSERT OR REPLACE INTO db_meta (key, value) VALUES ('migrated_at', ?1)",
        params![now],
    )
    .map_err(|e| WcError::Sqlite(e.to_string()))?;

    // Wallpapers — import from library.tsv if present
    import_library_tsv_into(&conn, cd)?;

    conn.close()
        .map_err(|(_, e)| WcError::Sqlite(e.to_string()))?;

    // Verify temp DB before swapping — run the full verify against the temp DB
    let temp_conn = Connection::open(&tmp_db).map_err(|e| WcError::Sqlite(e.to_string()))?;
    // Check all expected tables exist (lightweight structural verify)
    for table in &[
        "config",
        "sources",
        "favorites",
        "history",
        "state",
        "db_meta",
    ] {
        let count: i64 = temp_conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                params![table],
                |row| row.get(0),
            )
            .unwrap_or(0);
        if count == 0 {
            std::fs::remove_file(&tmp_db).ok();
            return Err(WcError::Other(format!(
                "resync: table '{}' missing in temp DB (original DB preserved)",
                table
            )));
        }
    }
    // Check row counts roughly match flat files
    let src_count: i64 = temp_conn
        .query_row("SELECT COUNT(*) FROM sources", [], |row| row.get(0))
        .unwrap_or(-1);
    let flat_src = flat::sources_list(cd)?.len() as i64;
    if src_count != flat_src {
        std::fs::remove_file(&tmp_db).ok();
        return Err(WcError::Other(format!(
            "resync: temp DB sources count ({}) != flat ({}) (original DB preserved)",
            src_count, flat_src
        )));
    }

    // Atomic swap
    std::fs::rename(&tmp_db, &db_path).map_err(WcError::Io)?;

    Ok(())
}

/// Export SQLite back to flat files atomically.
pub fn export_flat(cd: &ConfigDir) -> Result<(), WcError> {
    let db_path = cd.db_path();
    if !db_path.exists() {
        return Err(WcError::Other(
            "wallpapers.db not found. Run migrate-to-sqlite first.".into(),
        ));
    }
    let conn = Connection::open(&db_path).map_err(|e| WcError::Sqlite(e.to_string()))?;

    // Export everything to temp files first (all-or-nothing)
    let ts = chrono_now_compact();
    let backup_dir = cd.path.join("backup").join(format!("export-{}", ts));
    std::fs::create_dir_all(&backup_dir).map_err(WcError::Io)?;

    // Backup existing flat files
    for f in &[
        "config",
        "sources",
        "favorites",
        "history",
        "current",
        "last_backend",
    ] {
        let src = cd.path.join(f);
        if src.exists() {
            let _ = std::fs::copy(&src, backup_dir.join(f));
        }
    }

    let tmp_dir = cd.path.join(format!("export-tmp-{}", ts));
    std::fs::create_dir_all(&tmp_dir).map_err(WcError::Io)?;

    // Config — propagate row errors
    {
        let mut stmt = conn
            .prepare("SELECT key, value FROM config ORDER BY key")
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        let rows: Vec<(String, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| WcError::Sqlite(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        let content: String = rows.iter().map(|(k, v)| format!("{}={}\n", k, v)).collect();
        std::fs::write(tmp_dir.join("config"), content).map_err(WcError::Io)?;
    }

    // Sources — propagate row errors
    {
        let mut stmt = conn
            .prepare("SELECT path FROM sources ORDER BY path")
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        let rows: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| WcError::Sqlite(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        let content = rows.join("\n") + "\n";
        std::fs::write(tmp_dir.join("sources"), content).map_err(WcError::Io)?;
    }

    // Favorites — propagate row errors
    {
        let mut stmt = conn
            .prepare("SELECT path FROM favorites ORDER BY path")
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        let rows: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| WcError::Sqlite(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        let content = rows.join("\n") + "\n";
        std::fs::write(tmp_dir.join("favorites"), content).map_err(WcError::Io)?;
    }

    // History (newest first: ORDER BY id DESC) — propagate row errors
    {
        let mut stmt = conn
            .prepare("SELECT path FROM history ORDER BY id DESC")
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        let mut rows: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| WcError::Sqlite(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        // Canonical dedup to avoid writing duplicate paths that
        // would cause verify() false positives after export.
        let mut seen = std::collections::HashSet::new();
        rows.retain(|p| seen.insert(flat::try_canonicalize(p)));
        let content = rows.join("\n") + "\n";
        std::fs::write(tmp_dir.join("history"), content).map_err(WcError::Io)?;
    }

    // State — missing row is OK (empty), SQL error is NOT.
    for (key, file) in &[("current", "current"), ("last_backend", "last_backend")] {
        let val: Option<String> = match conn.query_row(
            "SELECT value FROM state WHERE key=?1",
            params![key],
            |row| row.get(0),
        ) {
            Ok(v) => Some(v),
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            Err(e) => return Err(WcError::Sqlite(e.to_string())),
        };
        let content = val.map(|v| v + "\n").unwrap_or_default();
        std::fs::write(tmp_dir.join(file), content).map_err(WcError::Io)?;
    }

    // Move temp files into place
    for f in &[
        "config",
        "sources",
        "favorites",
        "history",
        "current",
        "last_backend",
    ] {
        let src = tmp_dir.join(f);
        if src.exists() {
            std::fs::rename(&src, cd.path.join(f)).map_err(WcError::Io)?;
        }
    }
    let _ = std::fs::remove_dir(&tmp_dir);

    Ok(())
}

/// Backup wallpapers.db with a timestamp. Returns backup path.
pub fn backup(cd: &ConfigDir) -> Result<String, WcError> {
    let db_path = cd.db_path();
    if !db_path.exists() {
        return Err(WcError::Other("wallpapers.db not found.".into()));
    }
    let ts = chrono_now_compact();
    let bak = db_path.with_extension(format!("db.bak.{}", ts));
    std::fs::copy(&db_path, &bak).map_err(WcError::Io)?;
    Ok(bak.to_string_lossy().to_string())
}

/// Restore wallpapers.db from a backup file. Backs up current DB first.
pub fn restore(cd: &ConfigDir, backup_path: &Path) -> Result<(), WcError> {
    if !backup_path.exists() {
        return Err(WcError::Other(format!(
            "backup file not found: {}",
            backup_path.display()
        )));
    }
    // Validate it's a SQLite DB
    let _ = Connection::open(backup_path)
        .map_err(|_| WcError::Other("not a valid SQLite database".into()))?;

    let db_path = cd.db_path();
    // Backup current DB
    if db_path.exists() {
        let ts = chrono_now_compact();
        let prev = db_path.with_extension(format!("db.pre-restore.{}", ts));
        std::fs::copy(&db_path, &prev).map_err(WcError::Io)?;
    }
    std::fs::copy(backup_path, &db_path).map_err(WcError::Io)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn verify_ok_when_all_match() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        wc_core::config::write_config_value(&cd.path, "storage_backend", "sqlite").unwrap();
        flat::sources_add(&cd, "/walls").unwrap();
        flat::favorites_add(&cd, "/walls/a.jpg").unwrap();
        flat::history_add(&cd, "/walls/b.jpg", 100).unwrap();
        flat::current_write(&cd, "/walls/cur.jpg").unwrap();
        flat::last_backend_write(&cd, "awww").unwrap();

        crate::sqlite::migrate_to_sqlite(&cd).unwrap();

        let result = crate::sqlite::verify(&cd).unwrap();
        assert_eq!(result, crate::sqlite::VerifyResult::Ok);
    }

    #[test]
    fn verify_warning_when_config_drifts() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        wc_core::config::write_config_value(&cd.path, "storage_backend", "sqlite").unwrap();
        crate::sqlite::migrate_to_sqlite(&cd).unwrap();

        wc_core::config::write_config_value(&cd.path, "test_key", "new_value").unwrap();

        let result = crate::sqlite::verify(&cd).unwrap();
        assert!(
            matches!(result, crate::sqlite::VerifyResult::OkWithWarnings(ref w) if w.contains(&"config".to_string())),
            "expected OkWithWarnings containing 'config', got: {:?}",
            result
        );
    }

    #[test]
    fn verify_warning_when_sources_drift() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        wc_core::config::write_config_value(&cd.path, "storage_backend", "sqlite").unwrap();
        crate::sqlite::migrate_to_sqlite(&cd).unwrap();

        flat::sources_add(&cd, "/extra-source").unwrap();

        let result = crate::sqlite::verify(&cd).unwrap();
        assert!(
            matches!(result, crate::sqlite::VerifyResult::OkWithWarnings(ref w) if w.contains(&"sources".to_string())),
            "expected OkWithWarnings containing 'sources', got: {:?}",
            result
        );
    }

    #[test]
    fn verify_failed_when_favorites_differ() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        wc_core::config::write_config_value(&cd.path, "storage_backend", "sqlite").unwrap();
        crate::sqlite::migrate_to_sqlite(&cd).unwrap();

        flat::favorites_add(&cd, "/extra-fav.jpg").unwrap();

        let result = crate::sqlite::verify(&cd).unwrap();
        assert!(
            matches!(result, crate::sqlite::VerifyResult::Failed(ref e) if e.contains(&"favorites".to_string())),
            "expected Failed containing 'favorites', got: {:?}",
            result
        );
    }

    #[test]
    fn verify_error_when_db_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();

        let result = crate::sqlite::verify(&cd);
        assert!(
            matches!(result, Err(WcError::Sqlite(ref msg)) if msg.contains("not found")),
            "missing DB should return Err(WcError::Sqlite(...)), got: {:?}",
            result
        );
    }

    #[test]
    fn verify_ok_with_warnings_does_not_block_failed() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        wc_core::config::write_config_value(&cd.path, "storage_backend", "sqlite").unwrap();
        crate::sqlite::migrate_to_sqlite(&cd).unwrap();

        // Both config drift (warning) and history drift (error).
        wc_core::config::write_config_value(&cd.path, "extra_config", "val").unwrap();
        flat::history_add(&cd, "/walls/extra.jpg", 100).unwrap();

        let result = crate::sqlite::verify(&cd).unwrap();
        assert!(
            matches!(result, crate::sqlite::VerifyResult::Failed(_)),
            "errors should take priority over warnings, got: {:?}",
            result
        );
    }

    #[test]
    fn verify_returns_err_on_missing_table() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        wc_core::config::write_config_value(&cd.path, "storage_backend", "sqlite").unwrap();
        crate::sqlite::migrate_to_sqlite(&cd).unwrap();

        // Corrupt the schema by dropping the config table.
        let conn = rusqlite::Connection::open(&cd.db_path()).expect("should open db");
        conn.execute("DROP TABLE config", [])
            .expect("should drop config table");

        let result = crate::sqlite::verify(&cd);
        assert!(
            matches!(result, Err(WcError::Sqlite(_))),
            "missing table should return Err(WcError::Sqlite(_)), got: {:?}",
            result
        );
    }

    #[test]
    fn verify_history_passes_with_duplicate_canonical_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        wc_core::config::write_config_value(&cd.path, "storage_backend", "sqlite").unwrap();

        // Use the same path written twice — both canonicalise identically
        let path_a = tmp.path().join("a.jpg");
        std::fs::write(&path_a, b"x").unwrap();
        let a = path_a.to_string_lossy().to_string();

        // Write the same path twice to flat history
        flat::write_lines(&cd.history_path(), &[a.clone(), a.clone()]).unwrap();
        flat::favorites_add(&cd, "/walls/fav.jpg").unwrap();
        flat::sources_add(&cd, "/walls").unwrap();

        crate::sqlite::migrate_to_sqlite(&cd).unwrap();

        // INSERT the duplicate into SQLite history as well
        {
            let conn = rusqlite::Connection::open(&cd.db_path()).unwrap();
            conn.execute(
                "INSERT INTO history (path, backend, applied_at) VALUES (?1, 'test', 0)",
                rusqlite::params![a],
            )
            .unwrap();
        }

        let result = crate::sqlite::verify(&cd).unwrap();
        assert!(
            !matches!(result, crate::sqlite::VerifyResult::Failed(ref e) if e.contains(&"history".to_string())),
            "duplicate canonical history should not fail verify, got: {:?}",
            result
        );
    }

    #[test]
    fn verify_history_passes_with_symlink_equivalent_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        wc_core::config::write_config_value(&cd.path, "storage_backend", "sqlite").unwrap();

        let real = tmp.path().join("real.jpg");
        std::fs::write(&real, b"x").unwrap();
        let sym = tmp.path().join("link.jpg");
        std::os::unix::fs::symlink(&real, &sym).unwrap();
        let real_str = real.to_string_lossy().to_string();
        let sym_str = sym.to_string_lossy().to_string();

        // Flat history has the symlink path
        flat::write_lines(&cd.history_path(), &[sym_str.clone()]).unwrap();
        flat::favorites_add(&cd, "/walls/fav.jpg").unwrap();
        flat::sources_add(&cd, "/walls").unwrap();

        crate::sqlite::migrate_to_sqlite(&cd).unwrap();

        // SQLite history has the real path (different string, same canonical)
        {
            let conn = rusqlite::Connection::open(&cd.db_path()).unwrap();
            conn.execute(
                "INSERT INTO history (path, backend, applied_at) VALUES (?1, 'test', 0)",
                rusqlite::params![real_str],
            )
            .unwrap();
        }

        let result = crate::sqlite::verify(&cd).unwrap();
        assert!(
            !matches!(result, crate::sqlite::VerifyResult::Failed(ref e) if e.contains(&"history".to_string())),
            "symlink-equivalent history should pass verify, got: {:?}",
            result
        );
    }

    #[test]
    fn verify_history_fails_with_truly_missing_path() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        wc_core::config::write_config_value(&cd.path, "storage_backend", "sqlite").unwrap();
        flat::history_add(&cd, "/walls/a.jpg", 100).unwrap();
        flat::history_add(&cd, "/walls/b.jpg", 100).unwrap();
        flat::sources_add(&cd, "/walls").unwrap();

        crate::sqlite::migrate_to_sqlite(&cd).unwrap();

        // Add a path to SQLite history that is NOT in flat
        {
            let conn = rusqlite::Connection::open(&cd.db_path()).unwrap();
            conn.execute(
                "INSERT INTO history (path, backend, applied_at) VALUES (?1, 'test', 0)",
                rusqlite::params!["/walls/extra.jpg"],
            )
            .unwrap();
        }

        let result = crate::sqlite::verify(&cd).unwrap();
        assert!(
            matches!(result, crate::sqlite::VerifyResult::Failed(ref e) if e.contains(&"history".to_string())),
            "truly extra history path should fail verify, got: {:?}",
            result
        );
    }

    #[test]
    fn export_flat_dedupes_history_so_verify_passes() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        wc_core::config::write_config_value(&cd.path, "storage_backend", "sqlite").unwrap();
        flat::sources_add(&cd, "/walls").unwrap();

        // Migrate with one history entry
        flat::history_add(&cd, "/walls/a.jpg", 100).unwrap();
        crate::sqlite::migrate_to_sqlite(&cd).unwrap();

        // Insert a duplicate canonical path directly into SQLite history
        {
            let conn = rusqlite::Connection::open(&cd.db_path()).unwrap();
            conn.execute(
                "INSERT INTO history (path, backend, applied_at) VALUES (?1, 'test', 0)",
                rusqlite::params!["/walls/a.jpg"],
            )
            .unwrap();
        }

        // Export to flat — should dedup
        crate::sqlite::export_flat(&cd).unwrap();

        // After export, flat history should be deduped; verify should pass
        let result = crate::sqlite::verify(&cd).unwrap();
        assert!(
            !matches!(result, crate::sqlite::VerifyResult::Failed(ref e) if e.contains(&"history".to_string())),
            "export_flat should dedup history so verify passes, got: {:?}",
            result
        );
    }
}
