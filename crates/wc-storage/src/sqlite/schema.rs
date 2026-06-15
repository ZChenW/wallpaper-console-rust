use rusqlite::{params, Connection};
use wc_core::config::ConfigDir;
use wc_core::error::WcError;

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
    let now = super::chrono_now();

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
    super::import_library_tsv_into(&conn, cd)?;

    Ok(())
}

/// Ensure wallpapers.db exists with the full schema.
/// No-op if the file already exists. Failures are logged and silently ignored
/// so that callers never get blocked by bootstrap failures.
pub fn ensure_sqlite_db(cd: &ConfigDir) {
    let db = cd.db_path();
    if let Ok(conn) = Connection::open(&db) {
        create_schema(&conn).ok();
    }
}
