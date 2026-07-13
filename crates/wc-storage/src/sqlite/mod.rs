//! SQLite storage — schema, migration, verify, resync, backup, restore.

mod backup;
mod connection;
mod display_state;
mod library_page;
pub mod library_session;
mod metadata_cache;
mod row_map;
mod schema;
mod source_config_state;
mod source_reconcile;
mod sources;

pub use backup::*;
#[cfg(test)]
pub(crate) use connection::{reset_runtime_connection_open_count, runtime_connection_open_count};
pub use display_state::*;
pub use library_page::*;
pub use library_session::*;
pub use metadata_cache::*;
pub use row_map::wallpaper_entry_from_row;
pub use schema::*;
pub use source_config_state::*;
pub use source_reconcile::*;
pub use sources::*;

use rusqlite::{params, Connection};
use std::collections::{HashMap, HashSet};
use wc_core::config::ConfigDir;
use wc_core::error::WcError;
use wc_core::types::WallpaperEntry;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LibraryIndexSnapshot {
    pub paths: HashSet<String>,
    pub workshop_ids_by_path: HashMap<String, String>,
}

/// Snapshot path and workshop_id columns from the live wallpapers table.
pub fn library_index_snapshot(cd: &ConfigDir) -> Result<LibraryIndexSnapshot, WcError> {
    try_ensure_sqlite_db(cd)?;
    let db_path = cd.db_path();
    if !db_path.exists() {
        return Ok(LibraryIndexSnapshot::default());
    }
    let conn = open_runtime_connection(cd)?;
    let mut stmt = conn
        .prepare("SELECT path, workshop_id FROM wallpapers")
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    let mut snap = LibraryIndexSnapshot::default();
    for row in rows {
        let (path, workshop_id) = row.map_err(|e| WcError::Sqlite(e.to_string()))?;
        snap.paths.insert(path.clone());
        if !workshop_id.is_empty() {
            snap.workshop_ids_by_path.insert(path, workshop_id);
        }
    }
    Ok(snap)
}

/// Compare two snapshots and return removed row count plus removed workshop IDs.
pub fn library_index_diff_removed(
    before: &LibraryIndexSnapshot,
    after: &LibraryIndexSnapshot,
) -> (usize, Vec<String>) {
    let removed_paths: Vec<String> = before.paths.difference(&after.paths).cloned().collect();
    let removed = removed_paths.len();
    let mut workshop_ids: Vec<String> = removed_paths
        .iter()
        .filter_map(|path| before.workshop_ids_by_path.get(path))
        .cloned()
        .collect();
    workshop_ids.sort();
    workshop_ids.dedup();
    (removed, workshop_ids)
}

/// Clear the wallpapers table (before rescan rebuilds it).
pub fn library_clear(cd: &ConfigDir) -> Result<(), WcError> {
    let db_path = cd.db_path();
    if !db_path.exists() {
        return Ok(());
    }
    let conn = open_runtime_connection(cd)?;
    conn.execute("DELETE FROM wallpapers", [])
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    Ok(())
}

/// Insert one wallpaper entry into the wallpapers table.
#[allow(clippy::too_many_arguments)]
pub fn library_insert(
    cd: &ConfigDir,
    path: &str,
    ftype: &str,
    ext: &str,
    backend: &str,
    size: u64,
    mtime: u64,
    resolution: &str,
) -> Result<(), WcError> {
    let db_path = cd.db_path();
    if !db_path.exists() {
        return Ok(());
    }
    let conn = open_runtime_connection(cd)?;
    ensure_wallpaper_query_indexes(&conn)?;
    conn.execute(
        "INSERT OR IGNORE INTO wallpapers
         (path, type, ext, backend, size, mtime, resolution,
          project_type, preview_path, workshop_id, title, we_file, unsupported_reason)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, '', '', '', '', '', '')",
        params![
            path,
            ftype,
            ext,
            backend,
            size as i64,
            mtime as i64,
            resolution
        ],
    )
    .map_err(|e| WcError::Sqlite(e.to_string()))?;
    Ok(())
}

/// Insert many wallpaper entries in a single transaction.
pub fn library_insert_batch(
    cd: &ConfigDir,
    entries: &[(&str, &str, &str, &str, u64, u64, &str)],
) -> Result<usize, WcError> {
    let db_path = cd.db_path();
    if !db_path.exists() || entries.is_empty() {
        return Ok(0);
    }
    let conn = open_runtime_connection(cd)?;
    ensure_wallpaper_query_indexes(&conn)?;
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    {
        let mut stmt = tx
            .prepare(
                "INSERT OR IGNORE INTO wallpapers
                 (path, type, ext, backend, size, mtime, resolution,
                  project_type, preview_path, workshop_id, title, we_file, unsupported_reason)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, '', '', '', '', '', '')",
            )
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        for (path, ftype, ext, backend, size, mtime, resolution) in entries {
            stmt.execute(params![
                path,
                ftype,
                ext,
                backend,
                *size as i64,
                *mtime as i64,
                resolution
            ])
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        }
    }
    tx.commit().map_err(|e| WcError::Sqlite(e.to_string()))?;
    Ok(entries.len())
}

/// Atomically replace the wallpapers table with a freshly scanned batch.
///
/// The old table remains visible if staging, delete, insert, or commit fails.
pub fn library_replace_batch_atomic(
    cd: &ConfigDir,
    entries: &[(&str, &str, &str, &str, u64, u64, &str)],
) -> Result<usize, WcError> {
    try_ensure_sqlite_db(cd)?;

    let conn = open_runtime_connection(cd)?;
    ensure_wallpaper_query_indexes(&conn)?;
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| WcError::Sqlite(e.to_string()))?;

    tx.execute_batch(
        "CREATE TEMP TABLE wc_wallpapers_replace (
            path       TEXT NOT NULL UNIQUE,
            type       TEXT NOT NULL,
            ext        TEXT NOT NULL,
            backend    TEXT NOT NULL,
            size       INTEGER NOT NULL DEFAULT 0,
            mtime      INTEGER NOT NULL DEFAULT 0,
            resolution TEXT NOT NULL DEFAULT '?x?',
            project_type TEXT NOT NULL DEFAULT '',
            preview_path TEXT NOT NULL DEFAULT '',
            workshop_id  TEXT NOT NULL DEFAULT '',
            title        TEXT NOT NULL DEFAULT '',
            we_file      TEXT NOT NULL DEFAULT '',
            unsupported_reason TEXT NOT NULL DEFAULT ''
        );",
    )
    .map_err(|e| WcError::Sqlite(e.to_string()))?;

    {
        let mut stmt = tx
            .prepare(
                "INSERT OR IGNORE INTO wc_wallpapers_replace
                 (path, type, ext, backend, size, mtime, resolution,
                  project_type, preview_path, workshop_id, title, we_file, unsupported_reason)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, '', '', '', '', '', '')",
            )
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        for (path, ftype, ext, backend, size, mtime, resolution) in entries {
            stmt.execute(params![
                path,
                ftype,
                ext,
                backend,
                *size as i64,
                *mtime as i64,
                resolution
            ])
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        }
    }

    tx.execute("DELETE FROM wallpapers", [])
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    let inserted = tx
        .execute(
            "INSERT INTO wallpapers
             (path, type, ext, backend, size, mtime, resolution,
              project_type, preview_path, workshop_id, title, we_file, unsupported_reason)
             SELECT path, type, ext, backend, size, mtime, resolution,
                    project_type, preview_path, workshop_id, title, we_file, unsupported_reason
             FROM wc_wallpapers_replace",
            [],
        )
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    tx.execute("DROP TABLE wc_wallpapers_replace", [])
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    tx.commit().map_err(|e| WcError::Sqlite(e.to_string()))?;

    Ok(inserted)
}

/// Atomically replace the wallpapers table using full WallpaperEntry metadata.
pub fn library_replace_entries_batch_atomic(
    cd: &ConfigDir,
    entries: &[WallpaperEntry],
) -> Result<usize, WcError> {
    try_ensure_sqlite_db(cd)?;

    let conn = open_runtime_connection(cd)?;
    ensure_wallpaper_query_indexes(&conn)?;
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| WcError::Sqlite(e.to_string()))?;

    tx.execute_batch(
        "CREATE TEMP TABLE wc_wallpapers_replace (
            path       TEXT NOT NULL UNIQUE,
            type       TEXT NOT NULL,
            ext        TEXT NOT NULL,
            backend    TEXT NOT NULL,
            size       INTEGER NOT NULL DEFAULT 0,
            mtime      INTEGER NOT NULL DEFAULT 0,
            resolution TEXT NOT NULL DEFAULT '?x?',
            project_type TEXT NOT NULL DEFAULT '',
            preview_path TEXT NOT NULL DEFAULT '',
            workshop_id  TEXT NOT NULL DEFAULT '',
            title        TEXT NOT NULL DEFAULT '',
            we_file      TEXT NOT NULL DEFAULT '',
            unsupported_reason TEXT NOT NULL DEFAULT ''
        );",
    )
    .map_err(|e| WcError::Sqlite(e.to_string()))?;

    {
        let mut stmt = tx
            .prepare(
                "INSERT OR IGNORE INTO wc_wallpapers_replace
                 (path, type, ext, backend, size, mtime, resolution,
                  project_type, preview_path, workshop_id, title, we_file, unsupported_reason)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            )
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        for entry in entries {
            let project = entry.project.as_ref();
            stmt.execute(params![
                entry.path.as_str(),
                entry.file_type.as_str(),
                entry.ext.as_str(),
                entry.backend.as_str(),
                entry.size as i64,
                entry.mtime as i64,
                entry.resolution.as_str(),
                project.map(|p| p.project_type.as_str()).unwrap_or(""),
                project
                    .and_then(|p| p.preview_path.as_deref())
                    .unwrap_or(""),
                project.and_then(|p| p.workshop_id.as_deref()).unwrap_or(""),
                project.and_then(|p| p.title.as_deref()).unwrap_or(""),
                project.and_then(|p| p.we_file.as_deref()).unwrap_or(""),
                project
                    .and_then(|p| p.unsupported_reason.as_deref())
                    .unwrap_or(""),
            ])
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        }
    }

    tx.execute("DELETE FROM wallpapers", [])
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    let inserted = tx
        .execute(
            "INSERT INTO wallpapers
             (path, type, ext, backend, size, mtime, resolution,
              project_type, preview_path, workshop_id, title, we_file, unsupported_reason)
             SELECT path, type, ext, backend, size, mtime, resolution,
                    project_type, preview_path, workshop_id, title, we_file, unsupported_reason
             FROM wc_wallpapers_replace",
            [],
        )
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    tx.execute("DROP TABLE wc_wallpapers_replace", [])
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    tx.commit().map_err(|e| WcError::Sqlite(e.to_string()))?;

    Ok(inserted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;
    use wc_core::types::{Backend, FileType, WallpaperEntry, WallpaperProject};

    fn temp_config_dir() -> (tempfile::TempDir, ConfigDir) {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().to_path_buf(),
        };
        cd.init().unwrap();
        let conn = Connection::open(cd.db_path()).unwrap();
        create_schema(&conn).unwrap();
        (tmp, cd)
    }

    fn wallpaper_paths(cd: &ConfigDir) -> Vec<String> {
        let conn = Connection::open(cd.db_path()).unwrap();
        let mut stmt = conn
            .prepare("SELECT path FROM wallpapers ORDER BY path")
            .unwrap();
        stmt.query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    }

    #[test]
    fn future_schema_rejects_library_ensure_paths_without_replacing_rows() {
        let (_tmp, cd) = temp_config_dir();
        let future_version = CURRENT_SCHEMA_VERSION + 1;
        let conn = Connection::open(cd.db_path()).unwrap();
        conn.execute(
            "INSERT INTO wallpapers (path, type, ext, backend, size, mtime, resolution)
             VALUES ('/walls/sentinel.jpg', 'image', 'jpg', 'awww', 1, 1, '1x1')",
            [],
        )
        .unwrap();
        conn.pragma_update(None, "user_version", future_version)
            .unwrap();
        drop(conn);

        let results = [
            library_index_snapshot(&cd).map(|_| ()),
            library_replace_batch_atomic(
                &cd,
                &[("/walls/new.jpg", "image", "jpg", "awww", 2, 2, "2x2")],
            )
            .map(|_| ()),
        ];

        let conn = Connection::open(cd.db_path()).unwrap();
        let version = conn
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap();
        drop(conn);
        let paths = wallpaper_paths(&cd);

        for result in results {
            let error = result.expect_err("future-schema library operation must be rejected");
            assert!(
                error.to_string().contains("newer") || error.to_string().contains("version"),
                "{error}"
            );
        }
        assert_eq!(version, future_version);
        assert_eq!(paths, vec!["/walls/sentinel.jpg"]);
    }

    #[test]
    fn library_replace_batch_atomic_replaces_existing_rows() {
        let (_tmp, cd) = temp_config_dir();
        library_insert_batch(
            &cd,
            &[
                ("old-a.jpg", "image", "jpg", "awww", 10, 100, "100x100"),
                ("old-b.jpg", "image", "jpg", "awww", 20, 200, "100x100"),
            ],
        )
        .unwrap();

        let inserted = library_replace_batch_atomic(
            &cd,
            &[("new-a.jpg", "image", "jpg", "awww", 30, 300, "200x200")],
        )
        .unwrap();

        assert_eq!(inserted, 1);
        assert_eq!(wallpaper_paths(&cd), vec!["new-a.jpg"]);
    }

    #[test]
    fn library_replace_batch_atomic_empty_batch_commits_empty_library() {
        let (_tmp, cd) = temp_config_dir();
        library_insert_batch(
            &cd,
            &[("old-a.jpg", "image", "jpg", "awww", 10, 100, "100x100")],
        )
        .unwrap();

        let inserted = library_replace_batch_atomic(&cd, &[]).unwrap();

        assert_eq!(inserted, 0);
        assert!(wallpaper_paths(&cd).is_empty());
    }

    fn insert_source_wallpaper_membership(cd: &ConfigDir, path: &str) -> (i64, i64) {
        let conn = Connection::open(cd.db_path()).unwrap();
        conn.execute(
            "INSERT INTO sources (path, display_name) VALUES ('/walls', 'Walls')",
            [],
        )
        .unwrap();
        let source_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO wallpapers (path, type, ext, backend)
             VALUES (?1, 'image', 'jpg', 'awww')",
            params![path],
        )
        .unwrap();
        let wallpaper_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO wallpaper_sources (wallpaper_id, source_id) VALUES (?1, ?2)",
            params![wallpaper_id, source_id],
        )
        .unwrap();
        (wallpaper_id, source_id)
    }

    fn assert_no_memberships_or_foreign_key_violations(cd: &ConfigDir) {
        let conn = Connection::open(cd.db_path()).unwrap();
        let memberships: i64 = conn
            .query_row("SELECT COUNT(*) FROM wallpaper_sources", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(memberships, 0);
        let violations: i64 = conn
            .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(violations, 0);
    }

    #[test]
    fn library_clear_uses_foreign_keys_and_cascades_memberships() {
        let (_tmp, cd) = temp_config_dir();
        insert_source_wallpaper_membership(&cd, "/walls/old.jpg");
        reset_runtime_connection_open_count();

        library_clear(&cd).unwrap();

        assert_eq!(runtime_connection_open_count(), 1);
        assert_no_memberships_or_foreign_key_violations(&cd);
    }

    #[test]
    fn library_replace_uses_foreign_keys_and_cascades_old_memberships() {
        let (_tmp, cd) = temp_config_dir();
        insert_source_wallpaper_membership(&cd, "/walls/old.jpg");
        reset_runtime_connection_open_count();

        library_replace_batch_atomic(
            &cd,
            &[("/walls/new.jpg", "image", "jpg", "awww", 1, 1, "1x1")],
        )
        .unwrap();

        assert_eq!(runtime_connection_open_count(), 1);
        assert_eq!(wallpaper_paths(&cd), vec!["/walls/new.jpg"]);
        assert_no_memberships_or_foreign_key_violations(&cd);
    }

    #[test]
    fn library_replace_batch_atomic_preserves_old_rows_when_insert_fails() {
        let (_tmp, cd) = temp_config_dir();
        library_insert_batch(
            &cd,
            &[("old-a.jpg", "image", "jpg", "awww", 10, 100, "100x100")],
        )
        .unwrap();
        let conn = Connection::open(cd.db_path()).unwrap();
        conn.execute_batch(
            "CREATE TRIGGER fail_bad_wallpaper
             BEFORE INSERT ON wallpapers
             WHEN NEW.path = 'bad.jpg'
             BEGIN
               SELECT RAISE(ABORT, 'injected insert failure');
             END;",
        )
        .unwrap();

        let err = library_replace_batch_atomic(
            &cd,
            &[
                ("new-a.jpg", "image", "jpg", "awww", 30, 300, "200x200"),
                ("bad.jpg", "image", "jpg", "awww", 40, 400, "200x200"),
            ],
        )
        .unwrap_err();

        assert!(err.to_string().contains("injected insert failure"));
        assert_eq!(wallpaper_paths(&cd), vec!["old-a.jpg"]);
    }

    #[test]
    fn library_replace_batch_atomic_auto_creates_db_and_inserts_rows() {
        let (_tmp, cd) = temp_config_dir();
        // Delete the DB that temp_config_dir might have created.
        let _ = std::fs::remove_file(cd.db_path());

        let inserted = library_replace_batch_atomic(
            &cd,
            &[("a.jpg", "image", "jpg", "awww", 100, 1000, "800x600")],
        )
        .unwrap();
        assert_eq!(inserted, 1);
        assert!(cd.db_path().exists());
        assert_eq!(wallpaper_paths(&cd), vec!["a.jpg"]);
    }

    #[test]
    fn sqlite_source_add_auto_creates_db() {
        let (_tmp, cd) = temp_config_dir();
        let _ = std::fs::remove_file(cd.db_path());

        let added = sqlite_source_add(&cd, "/test/dir").unwrap();
        assert!(added);
        assert!(cd.db_path().exists());
    }

    #[test]
    fn library_replace_entries_batch_atomic_preserves_we_metadata() {
        let (_tmp, cd) = temp_config_dir();
        let entry = WallpaperEntry {
            path: Utf8PathBuf::from("/steamapps/workshop/content/431960/3558034522"),
            file_type: FileType::WeScene,
            ext: "scene".into(),
            backend: Backend::LinuxWallpaperEngine,
            size: 42,
            mtime: 1234,
            resolution: "WE".into(),
            project: Some(WallpaperProject {
                project_type: "we_scene".into(),
                preview_path: Some(
                    "/steamapps/workshop/content/431960/3558034522/preview.gif".into(),
                ),
                workshop_id: Some("3558034522".into()),
                title: Some("Scene title".into()),
                we_file: Some("scene.json".into()),
                backend: Some("linux-wallpaperengine".into()),
                unsupported_reason: None,
            }),
        };

        let inserted = library_replace_entries_batch_atomic(&cd, &[entry]).unwrap();
        assert_eq!(inserted, 1);

        let conn = Connection::open(cd.db_path()).unwrap();
        let row: (String, String, String, String, String) = conn
            .query_row(
                "SELECT type, project_type, preview_path, workshop_id, title FROM wallpapers",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(row.0, "we_scene");
        assert_eq!(row.1, "we_scene");
        assert!(row.2.ends_with("preview.gif"));
        assert_eq!(row.3, "3558034522");
        assert_eq!(row.4, "Scene title");
    }

    #[test]
    fn library_index_snapshot_and_diff_track_removed_workshop_ids() {
        let (_tmp, cd) = temp_config_dir();
        let conn = Connection::open(cd.db_path()).unwrap();
        conn.execute(
            "INSERT INTO wallpapers (path, type, ext, backend, size, mtime, resolution, workshop_id)
             VALUES ('/steamapps/workshop/content/431960/3589454154', 'we_scene', 'scene', 'linux-wallpaperengine', 1, 1, 'WE', '3589454154')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO wallpapers (path, type, ext, backend, size, mtime, resolution)
             VALUES ('/walls/keep.jpg', 'image', 'jpg', 'awww', 1, 1, '1x1')",
            [],
        )
        .unwrap();

        let before = library_index_snapshot(&cd).unwrap();
        let after = LibraryIndexSnapshot {
            paths: HashSet::from(["/walls/keep.jpg".into()]),
            workshop_ids_by_path: HashMap::new(),
        };
        let (removed, workshop_ids) = library_index_diff_removed(&before, &after);
        assert_eq!(removed, 1);
        assert_eq!(workshop_ids, vec!["3589454154".to_string()]);
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────

fn chrono_now() -> String {
    // ISO 8601 UTC timestamp
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let days_since_epoch = secs / 86400;
    let (y, m, d) = civil_from_days(days_since_epoch as i64);
    let time_secs = secs % 86400;
    let h = time_secs / 3600;
    let min = (time_secs % 3600) / 60;
    let s = time_secs % 60;
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}", y, m, d, h, min, s)
}

fn chrono_now_compact() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let days_since_epoch = secs / 86400;
    let (y, m, d) = civil_from_days(days_since_epoch as i64);
    let time_secs = secs % 86400;
    let h = time_secs / 3600;
    let min = (time_secs % 3600) / 60;
    let s = time_secs % 60;
    format!("{:04}{:02}{:02}-{:02}{:02}{:02}", y, m, d, h, min, s)
}

/// Convert days since Unix epoch to (year, month, day) — simplified Gregorian.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// Import library.tsv into the wallpapers table of an existing transaction.
/// Parses each line as: type\text\tbackend\tsize\tmtime\tresolution\tpath
/// Silently skips if the file doesn't exist or contains no valid rows.
/// The caller owns transaction boundaries.
fn import_library_tsv_into(conn: &Connection, cd: &ConfigDir) -> Result<(), WcError> {
    let tsv_path = cd.library_tsv_path();
    if !tsv_path.exists() {
        return Ok(());
    }
    let content = std::fs::read_to_string(&tsv_path).map_err(WcError::Io)?;
    let mut batch: Vec<(&str, &str, &str, &str, u64, u64, &str)> = Vec::new();
    for line in content.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 7 {
            continue;
        }
        let path = parts[6];
        let ftype = parts[0];
        let ext = parts[1];
        let backend = parts[2];
        let size: u64 = parts[3].parse().unwrap_or(0);
        let mtime: u64 = parts[4].parse().unwrap_or(0);
        let resolution = parts[5];
        batch.push((path, ftype, ext, backend, size, mtime, resolution));
    }
    if batch.is_empty() {
        return Ok(());
    }

    let mut stmt = conn
        .prepare(
            "INSERT OR IGNORE INTO wallpapers (path, type, ext, backend, size, mtime, resolution)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    for (path, ftype, ext, backend, size, mtime, resolution) in &batch {
        stmt.execute(params![
            path,
            ftype,
            ext,
            backend,
            *size as i64,
            *mtime as i64,
            resolution
        ])
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    }
    Ok(())
}
