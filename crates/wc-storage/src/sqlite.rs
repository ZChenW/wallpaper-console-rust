//! SQLite storage — schema, migration, verify, resync, backup, restore.

use std::collections::HashMap;
use std::path::Path;

use camino::Utf8PathBuf;
use rusqlite::{params, Connection};
use wc_core::config::ConfigDir;
use wc_core::error::WcError;
use wc_core::types::{Backend, FileType, WallpaperEntry, WallpaperProject};

use crate::flat;

const WALLPAPER_QUERY_INDEXES_SQL: &str = "
    CREATE UNIQUE INDEX IF NOT EXISTS idx_wallpapers_path ON wallpapers(path);
    CREATE INDEX IF NOT EXISTS idx_wallpapers_type ON wallpapers(type);
    CREATE INDEX IF NOT EXISTS idx_wallpapers_mtime ON wallpapers(mtime DESC, path ASC);
    CREATE INDEX IF NOT EXISTS idx_wallpapers_size ON wallpapers(size DESC, path ASC);
    CREATE INDEX IF NOT EXISTS idx_wallpapers_type_mtime ON wallpapers(type, mtime DESC, path ASC);
    CREATE INDEX IF NOT EXISTS idx_wallpapers_type_size ON wallpapers(type, size DESC, path ASC);
";

/// Create the wallpaper-console SQLite schema.
pub fn create_schema(conn: &Connection) -> Result<(), WcError> {
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
        PRAGMA synchronous = NORMAL;
        PRAGMA temp_store = MEMORY;
        PRAGMA user_version = 1;

        CREATE TABLE IF NOT EXISTS db_meta (
            key        TEXT PRIMARY KEY,
            value      TEXT NOT NULL,
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS config (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS sources (
            id       INTEGER PRIMARY KEY AUTOINCREMENT,
            path     TEXT NOT NULL UNIQUE,
            added_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS wallpapers (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            path       TEXT NOT NULL,
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
            unsupported_reason TEXT NOT NULL DEFAULT '',
            source_id  INTEGER REFERENCES sources(id),
            last_seen  TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE TABLE IF NOT EXISTS favorites (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            path        TEXT NOT NULL UNIQUE,
            added_at    TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS history (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            path        TEXT NOT NULL,
            backend     TEXT NOT NULL,
            applied_at  TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS state (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );",
    )
    .map_err(|e| WcError::Sqlite(e.to_string()))?;
    ensure_wallpaper_metadata_columns(conn)?;
    ensure_wallpaper_query_indexes(conn)?;
    Ok(())
}

/// Add project metadata columns to older wallpapers tables.
pub fn ensure_wallpaper_metadata_columns(conn: &Connection) -> Result<(), WcError> {
    let mut stmt = conn
        .prepare("PRAGMA table_info(wallpapers)")
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    let columns: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| WcError::Sqlite(e.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    let existing: std::collections::HashSet<String> = columns.into_iter().collect();
    for (name, sql_type) in [
        ("project_type", "TEXT NOT NULL DEFAULT ''"),
        ("preview_path", "TEXT NOT NULL DEFAULT ''"),
        ("workshop_id", "TEXT NOT NULL DEFAULT ''"),
        ("title", "TEXT NOT NULL DEFAULT ''"),
        ("we_file", "TEXT NOT NULL DEFAULT ''"),
        ("unsupported_reason", "TEXT NOT NULL DEFAULT ''"),
    ] {
        if !existing.contains(name) {
            conn.execute_batch(&format!(
                "ALTER TABLE wallpapers ADD COLUMN {name} {sql_type};"
            ))
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        }
    }
    Ok(())
}

/// Ensure existing databases have the indexes needed by paged GUI queries.
pub fn ensure_wallpaper_query_indexes(conn: &Connection) -> Result<(), WcError> {
    ensure_wallpaper_metadata_columns(conn)?;
    conn.execute_batch(WALLPAPER_QUERY_INDEXES_SQL)
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    Ok(())
}

/// Escape a string for safe use in a SQLite single-quoted literal.
pub fn sqlite_escape(s: &str) -> String {
    s.replace('\'', "''")
}

/// Migrate flat files into wallpapers.db (one-shot operation).
pub fn migrate_to_sqlite(cd: &ConfigDir) -> Result<(), WcError> {
    let db_path = cd.db_path();
    if db_path.exists() {
        return Err(WcError::Other(
            "database already exists. Remove it manually to re-migrate.".into(),
        ));
    }

    let conn = Connection::open(&db_path).map_err(|e| WcError::Sqlite(e.to_string()))?;
    create_schema(&conn)?;
    let now = chrono_now();

    // Config
    let config_map = wc_core::config::parse_config_file(&cd.config_path())?;
    for (key, value) in &config_map {
        let ek = sqlite_escape(key);
        let ev = sqlite_escape(value);
        conn.execute(
            "INSERT OR REPLACE INTO config (key, value) VALUES (?1, ?2)",
            params![ek, ev],
        )
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    }

    // Sources
    for path in flat::sources_list(cd)? {
        let ep = sqlite_escape(&path);
        conn.execute(
            "INSERT OR IGNORE INTO sources (path, added_at) VALUES (?1, ?2)",
            params![ep, now],
        )
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    }

    // Favorites
    for path in flat::favorites_list(cd)? {
        let ep = sqlite_escape(&path);
        conn.execute(
            "INSERT OR IGNORE INTO favorites (path, added_at) VALUES (?1, ?2)",
            params![ep, now],
        )
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    }

    // History — canonical-deduplicate keeping newest (first occurrence in flat file),
    // then reverse so newest gets highest id
    let mut seen = std::collections::HashSet::new();
    let history: Vec<String> = flat::history_list(cd)?
        .into_iter()
        .filter(|path| seen.insert(flat::try_canonicalize(path)))
        .collect();
    let history_rev: Vec<String> = history.into_iter().rev().collect();
    for path in &history_rev {
        let ep = sqlite_escape(path);
        conn.execute(
            "INSERT INTO history (path, backend, applied_at) VALUES (?1, 'unknown', ?2)",
            params![ep, now],
        )
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    }

    // State
    if let Some(cur) = flat::current_read(cd)? {
        conn.execute(
            "INSERT OR REPLACE INTO state (key, value) VALUES ('current', ?1)",
            params![sqlite_escape(&cur)],
        )
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    }
    if let Some(be) = flat::last_backend_read(cd)? {
        conn.execute(
            "INSERT OR REPLACE INTO state (key, value) VALUES ('last_backend', ?1)",
            params![sqlite_escape(&be)],
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
    conn.execute(
        "INSERT OR REPLACE INTO db_meta (key, value) VALUES ('source_runtime_dir', ?1)",
        params![sqlite_escape(cd.path.to_string_lossy().as_ref())],
    )
    .map_err(|e| WcError::Sqlite(e.to_string()))?;

    // Wallpapers — import from library.tsv if present
    import_library_tsv_into(&conn, cd)?;

    Ok(())
}

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
        let mut flat_hist: Vec<String> = flat::history_list(cd)?;
        flat_hist.sort();
        let mut stmt = conn
            .prepare("SELECT path FROM history ORDER BY path")
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        let mut db_hist: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| WcError::Sqlite(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        db_hist.sort();
        if flat_hist != db_hist {
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
        let rows: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| WcError::Sqlite(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
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

// ── Library wallpapers table ──────────────────────────────────────────────

/// Clear the wallpapers table (before rescan rebuilds it).
pub fn library_clear(cd: &ConfigDir) -> Result<(), WcError> {
    let db_path = cd.db_path();
    if !db_path.exists() {
        return Ok(());
    }
    let conn = Connection::open(&db_path).map_err(|e| WcError::Sqlite(e.to_string()))?;
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
    let conn = Connection::open(&db_path).map_err(|e| WcError::Sqlite(e.to_string()))?;
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
    let conn = Connection::open(&db_path).map_err(|e| WcError::Sqlite(e.to_string()))?;
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
    let db_path = cd.db_path();
    ensure_sqlite_db(cd);

    let conn = Connection::open(&db_path).map_err(|e| WcError::Sqlite(e.to_string()))?;
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
    let db_path = cd.db_path();
    ensure_sqlite_db(cd);

    let conn = Connection::open(&db_path).map_err(|e| WcError::Sqlite(e.to_string()))?;
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
}

// ── Direct SQLite source writes (sqlite mode — no mirror-active gate) ─────

/// Ensure wallpapers.db exists with the full schema.
/// No-op if the file already exists. Failures are logged and silently ignored
/// so that callers never get blocked by bootstrap failures.
pub fn ensure_sqlite_db(cd: &ConfigDir) {
    let db = cd.db_path();
    if let Ok(conn) = Connection::open(&db) {
        create_schema(&conn).ok();
    }
}

/// Add a source directly to the SQLite sources table.
/// Auto-creates the database if it does not exist.
pub fn sqlite_source_add(cd: &ConfigDir, path: &str) -> Result<bool, WcError> {
    ensure_sqlite_db(cd);
    let db = cd.db_path();
    let conn = Connection::open(&db).map_err(|e| WcError::Sqlite(e.to_string()))?;
    let n = conn
        .execute(
            "INSERT OR IGNORE INTO sources (path) VALUES (?1)",
            params![path],
        )
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    Ok(n > 0)
}

/// Remove a source directly from the SQLite sources table.
/// Auto-creates the database if it does not exist.
pub fn sqlite_source_remove(cd: &ConfigDir, path: &str) -> Result<bool, WcError> {
    ensure_sqlite_db(cd);
    let db = cd.db_path();
    let conn = Connection::open(&db).map_err(|e| WcError::Sqlite(e.to_string()))?;
    let n = conn
        .execute("DELETE FROM sources WHERE path = (?1)", params![path])
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    Ok(n > 0)
}

pub fn sqlite_config_set(cd: &ConfigDir, key: &str, value: &str) -> Result<(), WcError> {
    ensure_sqlite_db(cd);
    let conn = Connection::open(cd.db_path()).map_err(|e| WcError::Sqlite(e.to_string()))?;
    conn.execute(
        "INSERT OR REPLACE INTO config (key, value) VALUES (?1, ?2)",
        params![key, value],
    )
    .map_err(|e| WcError::Sqlite(e.to_string()))?;
    Ok(())
}

pub fn sqlite_favorite_add(cd: &ConfigDir, path: &str) -> Result<bool, WcError> {
    ensure_sqlite_db(cd);
    let conn = Connection::open(cd.db_path()).map_err(|e| WcError::Sqlite(e.to_string()))?;
    let n = conn
        .execute(
            "INSERT OR IGNORE INTO favorites (path) VALUES (?1)",
            params![path],
        )
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    Ok(n > 0)
}

pub fn sqlite_favorite_remove(cd: &ConfigDir, path: &str) -> Result<(), WcError> {
    ensure_sqlite_db(cd);
    let conn = Connection::open(cd.db_path()).map_err(|e| WcError::Sqlite(e.to_string()))?;
    conn.execute("DELETE FROM favorites WHERE path = ?1", params![path])
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    Ok(())
}

pub fn sqlite_history_add(
    cd: &ConfigDir,
    path: &str,
    backend: &str,
    max_entries: usize,
) -> Result<(), WcError> {
    ensure_sqlite_db(cd);
    let conn = Connection::open(cd.db_path()).map_err(|e| WcError::Sqlite(e.to_string()))?;
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    tx.execute("DELETE FROM history WHERE path = ?1", params![path])
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    tx.execute(
        "INSERT INTO history (path, backend) VALUES (?1, ?2)",
        params![path, backend],
    )
    .map_err(|e| WcError::Sqlite(e.to_string()))?;
    tx.execute(
        "DELETE FROM history WHERE id NOT IN (SELECT id FROM history ORDER BY id DESC LIMIT ?1)",
        params![max_entries as i64],
    )
    .map_err(|e| WcError::Sqlite(e.to_string()))?;
    tx.commit().map_err(|e| WcError::Sqlite(e.to_string()))?;
    Ok(())
}

pub fn sqlite_history_clear(cd: &ConfigDir) -> Result<(), WcError> {
    ensure_sqlite_db(cd);
    let conn = Connection::open(cd.db_path()).map_err(|e| WcError::Sqlite(e.to_string()))?;
    conn.execute("DELETE FROM history", [])
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    Ok(())
}

pub fn sqlite_state_write(cd: &ConfigDir, key: &str, value: &str) -> Result<(), WcError> {
    ensure_sqlite_db(cd);
    let conn = Connection::open(cd.db_path()).map_err(|e| WcError::Sqlite(e.to_string()))?;
    conn.execute(
        "INSERT OR REPLACE INTO state (key, value) VALUES (?1, ?2)",
        params![key, value],
    )
    .map_err(|e| WcError::Sqlite(e.to_string()))?;
    Ok(())
}

pub fn library_count(cd: &ConfigDir) -> Result<usize, WcError> {
    let db_path = cd.db_path();
    if !db_path.exists() {
        return Ok(0);
    }
    let conn = Connection::open(&db_path).map_err(|e| WcError::Sqlite(e.to_string()))?;
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM wallpapers", [], |row| row.get(0))
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    Ok(count as usize)
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

/// Import library.tsv into the wallpapers table of an existing connection.
/// Parses each line as: type\text\tbackend\tsize\tmtime\tresolution\tpath
/// Silently skips if the file doesn't exist or contains no valid rows.
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

    let tx = conn
        .unchecked_transaction()
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    {
        let mut stmt = tx
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
    }
    tx.commit().map_err(|e| WcError::Sqlite(e.to_string()))?;
    Ok(())
}

/// Load prior metadata from the SQLite wallpapers table into a HashMap keyed by
/// canonical path. Returns an empty cache if the database does not exist.
pub fn prior_metadata_cache_from_sqlite(cd: &ConfigDir) -> HashMap<String, WallpaperEntry> {
    let mut cache = HashMap::new();
    let db_path = cd.db_path();
    if !db_path.exists() {
        return cache;
    }
    let conn = match Connection::open(&db_path) {
        Ok(c) => c,
        Err(_) => return cache,
    };
    ensure_wallpaper_metadata_columns(&conn).ok();
    let mut stmt = match conn.prepare(
        "SELECT path, type, ext, backend, size, mtime, resolution,
                project_type, preview_path, workshop_id, title, we_file, unsupported_reason
         FROM wallpapers",
    ) {
        Ok(s) => s,
        Err(_) => return cache,
    };
    let rows = match stmt.query_map([], |row| {
        let path: String = row.get(0)?;
        let ftype_s: String = row.get(1)?;
        let ext: String = row.get(2)?;
        let backend_s: String = row.get(3)?;
        let size: i64 = row.get(4)?;
        let mtime: i64 = row.get(5)?;
        let resolution: String = row.get(6)?;
        let project_type: String = row.get(7)?;
        let preview_path: String = row.get(8)?;
        let workshop_id: String = row.get(9)?;
        let title: String = row.get(10)?;
        let we_file: String = row.get(11)?;
        let unsupported_reason: String = row.get(12)?;
        Ok((
            path,
            ftype_s,
            ext,
            backend_s,
            size,
            mtime,
            resolution,
            project_type,
            preview_path,
            workshop_id,
            title,
            we_file,
            unsupported_reason,
        ))
    }) {
        Ok(r) => r,
        Err(_) => return cache,
    };
    for row in rows {
        let (
            path,
            ftype_s,
            ext,
            backend_s,
            size,
            mtime,
            resolution,
            project_type,
            preview_path,
            workshop_id,
            title,
            we_file,
            unsupported_reason,
        ) = match row {
            Ok(r) => r,
            Err(_) => continue,
        };
        let canon = std::fs::canonicalize(&path)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| path.clone());
        let file_type = match ftype_s.as_str() {
            "gif" => FileType::Gif,
            "video" => FileType::Video,
            "we_scene" => FileType::WeScene,
            "we_web" => FileType::WeWeb,
            "unsupported" => FileType::WeApplication,
            _ => FileType::Image,
        };
        let backend = match backend_s.as_str() {
            "mpvpaper" => Backend::Mpvpaper,
            "linux-wallpaperengine" => Backend::LinuxWallpaperEngine,
            "webkit-layer-shell" => Backend::WebKitLayerShell,
            "unsupported" => Backend::Unsupported,
            _ => Backend::Awww,
        };
        let project = if project_type.is_empty()
            && preview_path.is_empty()
            && workshop_id.is_empty()
            && title.is_empty()
            && we_file.is_empty()
            && unsupported_reason.is_empty()
        {
            None
        } else {
            Some(WallpaperProject {
                project_type,
                preview_path: non_empty(preview_path),
                workshop_id: non_empty(workshop_id),
                title: non_empty(title),
                we_file: non_empty(we_file),
                backend: Some(backend.as_str().to_string()),
                unsupported_reason: non_empty(unsupported_reason),
            })
        };
        cache.insert(
            canon,
            WallpaperEntry {
                path: Utf8PathBuf::from(&path),
                file_type,
                ext,
                backend,
                size: size as u64,
                mtime: mtime as u64,
                resolution,
                project,
            },
        );
    }
    cache
}

fn non_empty(value: String) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}
