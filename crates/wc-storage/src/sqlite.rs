//! SQLite storage — schema, migration, verify, resync, backup, restore.

use std::path::Path;

use rusqlite::{params, Connection};
use wc_core::config::ConfigDir;
use wc_core::error::WcError;

use crate::flat;

/// Create the wallpaper-console SQLite schema.
pub fn create_schema(conn: &Connection) -> Result<(), WcError> {
    conn.execute_batch(
        "PRAGMA user_version = 1;

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
            source_id  INTEGER REFERENCES sources(id),
            last_seen  TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_wallpapers_path ON wallpapers(path);
        CREATE INDEX IF NOT EXISTS idx_wallpapers_type ON wallpapers(type);

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

    // History — reverse order so newest gets highest id
    let mut history = flat::history_list(cd)?;
    history.reverse();
    for path in &history {
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

    Ok(())
}

/// Compare flat files vs SQLite. Returns Ok(()) if consistent,
/// Err with mismatch details if not.
pub fn verify(cd: &ConfigDir) -> Result<(), WcError> {
    let db_path = cd.db_path();
    if !db_path.exists() {
        return Err(WcError::Other(
            "wallpapers.db not found. Run migrate-to-sqlite first.".into(),
        ));
    }
    let conn = Connection::open(&db_path).map_err(|e| WcError::Sqlite(e.to_string()))?;

    let mut errors: Vec<String> = Vec::new();

    // Config
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
            errors.push("config".into());
        }
    }

    // Sources
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
            errors.push("sources".into());
        }
    }

    // Favorites
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

    // History (sorted set comparison)
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

    // State: current — missing row is OK (empty), SQL error is NOT.
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

    // State: last_backend — missing row is OK, SQL error is NOT.
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

    if errors.is_empty() {
        Ok(())
    } else {
        Err(WcError::Other(format!(
            "VERIFY FAILED: {} mismatch(es) found: {}",
            errors.len(),
            errors.join(", ")
        )))
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
    // Sources
    for path in flat::sources_list(cd)? {
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
    // History (reverse for id ordering)
    let mut history = flat::history_list(cd)?;
    history.reverse();
    for path in &history {
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
    conn.execute(
        "INSERT OR IGNORE INTO wallpapers (path, type, ext, backend, size, mtime, resolution)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
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

// ── Direct SQLite source writes (sqlite mode — no mirror-active gate) ─────

/// Add a source directly to the SQLite sources table.
/// Fails if wallpapers.db does not exist or the write fails.
pub fn sqlite_source_add(cd: &ConfigDir, path: &str) -> Result<bool, WcError> {
    let db = cd.db_path();
    if !db.exists() {
        return Err(WcError::Other(
            "wallpapers.db not found. Run migrate-to-sqlite first.".into(),
        ));
    }
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
/// Fails if wallpapers.db does not exist or the write fails.
pub fn sqlite_source_remove(cd: &ConfigDir, path: &str) -> Result<bool, WcError> {
    let db = cd.db_path();
    if !db.exists() {
        return Err(WcError::Other(
            "wallpapers.db not found. Run migrate-to-sqlite first.".into(),
        ));
    }
    let conn = Connection::open(&db).map_err(|e| WcError::Sqlite(e.to_string()))?;
    let n = conn
        .execute("DELETE FROM sources WHERE path = (?1)", params![path])
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    Ok(n > 0)
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
