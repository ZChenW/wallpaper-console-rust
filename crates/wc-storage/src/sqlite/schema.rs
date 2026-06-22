use rusqlite::{params, Connection};
use std::time::Duration;
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
pub const FTS_SCHEMA_VERSION: &str = "2";

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
        );

        CREATE VIRTUAL TABLE IF NOT EXISTS wallpapers_fts USING fts5(
            path,
            title,
            workshop_id,
            project_type,
            content='wallpapers',
            content_rowid='id'
        );

        CREATE TRIGGER IF NOT EXISTS wallpapers_ai AFTER INSERT ON wallpapers BEGIN
            INSERT INTO wallpapers_fts(rowid, path, title, workshop_id, project_type)
            VALUES (new.id, new.path, new.title, new.workshop_id, new.project_type);
        END;

        CREATE TRIGGER IF NOT EXISTS wallpapers_ad AFTER DELETE ON wallpapers BEGIN
            INSERT INTO wallpapers_fts(wallpapers_fts, rowid, path, title, workshop_id, project_type)
            VALUES ('delete', old.id, old.path, old.title, old.workshop_id, old.project_type);
        END;

        CREATE TRIGGER IF NOT EXISTS wallpapers_au AFTER UPDATE ON wallpapers BEGIN
            INSERT INTO wallpapers_fts(wallpapers_fts, rowid, path, title, workshop_id, project_type)
            VALUES ('delete', old.id, old.path, old.title, old.workshop_id, old.project_type);
            INSERT INTO wallpapers_fts(rowid, path, title, workshop_id, project_type)
            VALUES (new.id, new.path, new.title, new.workshop_id, new.project_type);
        END;",
    )
    .map_err(|e| WcError::Sqlite(e.to_string()))?;
    ensure_wallpaper_metadata_columns(conn)?;
    ensure_wallpaper_query_indexes(conn)?;
    ensure_wallpapers_fts_rebuilt(conn)?;
    Ok(())
}

fn ensure_wallpapers_fts_rebuilt(conn: &Connection) -> Result<(), WcError> {
    let current_version: Option<String> = conn
        .query_row(
            "SELECT value FROM db_meta WHERE key = 'fts_schema_version'",
            [],
            |row| row.get(0),
        )
        .ok();
    if current_version.as_deref() != Some(FTS_SCHEMA_VERSION) {
        rebuild_wallpapers_fts(conn)?;
        conn.execute(
            "INSERT OR REPLACE INTO db_meta (key, value) VALUES ('fts_schema_version', ?1)",
            params![FTS_SCHEMA_VERSION],
        )
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
        conn.execute(
            "INSERT OR REPLACE INTO db_meta (key, value) VALUES ('fts_rebuilt_at', datetime('now'))",
            [],
        )
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    }
    Ok(())
}

pub fn wallpapers_count(conn: &Connection) -> Result<i64, WcError> {
    conn.query_row("SELECT COUNT(*) FROM wallpapers", [], |row| row.get(0))
        .map_err(|e| WcError::Sqlite(e.to_string()))
}

pub fn wallpapers_fts_count(conn: &Connection) -> Result<i64, WcError> {
    conn.query_row("SELECT COUNT(*) FROM wallpapers_fts", [], |row| row.get(0))
        .map_err(|e| WcError::Sqlite(e.to_string()))
}

pub fn check_wallpapers_fts_integrity(conn: &Connection) -> Result<(), WcError> {
    conn.execute(
        "INSERT INTO wallpapers_fts(wallpapers_fts, rank) VALUES ('integrity-check', 1)",
        [],
    )
    .map(|_| ())
    .map_err(|e| WcError::Sqlite(e.to_string()))
}

pub fn rebuild_wallpapers_fts(conn: &Connection) -> Result<(), WcError> {
    conn.execute(
        "INSERT INTO wallpapers_fts(wallpapers_fts) VALUES ('rebuild')",
        [],
    )
    .map_err(|e| WcError::Sqlite(e.to_string()))?;
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

    // History — canonical-deduplicate keeping newest (first occurrence in flat file),
    // then reverse so newest gets highest id
    let mut seen = std::collections::HashSet::new();
    let history: Vec<String> = flat::history_list(cd)?
        .into_iter()
        .filter(|path| seen.insert(flat::try_canonicalize(path)))
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
    conn.execute(
        "INSERT OR REPLACE INTO db_meta (key, value) VALUES ('source_runtime_dir', ?1)",
        params![cd.path.to_string_lossy().as_ref()],
    )
    .map_err(|e| WcError::Sqlite(e.to_string()))?;

    // Wallpapers — import from library.tsv if present
    super::import_library_tsv_into(&conn, cd)?;

    Ok(())
}

/// Default busy wait for runtime SQLite connections (milliseconds).
pub const RUNTIME_BUSY_TIMEOUT_MS: u64 = 5000;

/// Apply runtime PRAGMAs to a connection used for library operations.
///
/// Sets `busy_timeout` first so subsequent PRAGMAs (including `journal_mode`)
/// wait on brief lock contention instead of failing immediately with
/// `SQLITE_BUSY`.
pub fn apply_runtime_pragmas(conn: &Connection) -> Result<(), WcError> {
    conn.busy_timeout(Duration::from_millis(RUNTIME_BUSY_TIMEOUT_MS))
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    conn.execute_batch("PRAGMA journal_mode = WAL;")
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    Ok(())
}

/// Open a SQLite connection for hot runtime reads/writes.
///
/// Does not run schema bootstrap or repair. Callers that need a guaranteed
/// schema (startup, migration, repair, explicit init) must call
/// [`ensure_sqlite_db`] separately.
pub fn open_runtime_connection(cd: &ConfigDir) -> Result<Connection, WcError> {
    let db_path = cd.db_path();
    if !db_path.exists() {
        return Err(WcError::Sqlite(format!(
            "database not found: {}",
            db_path.display()
        )));
    }
    let conn = Connection::open(&db_path).map_err(|e| WcError::Sqlite(e.to_string()))?;
    apply_runtime_pragmas(&conn)?;
    Ok(conn)
}

/// Ensure wallpapers.db exists with the full schema.
/// No-op if the file already exists. Failures are logged and silently ignored
/// so that callers never get blocked by bootstrap failures.
pub fn ensure_sqlite_db(cd: &ConfigDir) {
    let db = cd.db_path();
    if let Ok(conn) = Connection::open(&db) {
        create_schema(&conn).ok();
        apply_runtime_pragmas(&conn).ok();
    }
}

/// Ensure SQLite exists, importing legacy flat files only when the DB is absent.
/// Returns `true` if legacy flat files were imported into a newly created DB,
/// `false` if the DB already existed and was only ensured/repaired.
pub fn ensure_or_import_legacy_flat(cd: &ConfigDir) -> Result<bool, WcError> {
    if cd.db_path().exists() {
        ensure_sqlite_db(cd);
        return Ok(false);
    }
    migrate_to_sqlite(cd)?;
    ensure_sqlite_db(cd);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrate_to_sqlite_preserves_apostrophes_in_bound_values() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        wc_core::config::write_config_value(&cd.path, "custom_name", "artist's value").unwrap();
        flat::sources_add(&cd, "/wall's/source").unwrap();
        flat::favorites_add(&cd, "/wall's/favorite.jpg").unwrap();
        flat::history_add(&cd, "/wall's/history.jpg", 100).unwrap();
        flat::current_write(&cd, "/wall's/current.jpg").unwrap();
        flat::last_backend_write(&cd, "awww's").unwrap();

        migrate_to_sqlite(&cd).unwrap();

        let conn = Connection::open(cd.db_path()).unwrap();
        let config_value: String = conn
            .query_row(
                "SELECT value FROM config WHERE key='custom_name'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let source: String = conn
            .query_row("SELECT path FROM sources", [], |row| row.get(0))
            .unwrap();
        let favorite: String = conn
            .query_row("SELECT path FROM favorites", [], |row| row.get(0))
            .unwrap();
        let history: String = conn
            .query_row("SELECT path FROM history", [], |row| row.get(0))
            .unwrap();
        let current: String = conn
            .query_row("SELECT value FROM state WHERE key='current'", [], |row| {
                row.get(0)
            })
            .unwrap();
        let last_backend: String = conn
            .query_row(
                "SELECT value FROM state WHERE key='last_backend'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(config_value, "artist's value");
        assert_eq!(source, "/wall's/source");
        assert_eq!(favorite, "/wall's/favorite.jpg");
        assert_eq!(history, "/wall's/history.jpg");
        assert_eq!(current, "/wall's/current.jpg");
        assert_eq!(last_backend, "awww's");
    }

    #[test]
    fn create_schema_records_current_fts_schema_version() {
        let conn = Connection::open_in_memory().unwrap();
        create_schema(&conn).unwrap();
        conn.execute(
            "INSERT INTO wallpapers (path, type, ext, backend, size, mtime, resolution, title, workshop_id)
             VALUES ('/walls/old.jpg', 'image', 'jpg', 'awww', 10, 10, '1x1', 'old title', '111')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO db_meta (key, value) VALUES ('fts_schema_version', 'old')",
            [],
        )
        .unwrap();

        create_schema(&conn).unwrap();

        let version: String = conn
            .query_row(
                "SELECT value FROM db_meta WHERE key='fts_schema_version'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, "2");
        let fts_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM wallpapers_fts", [], |row| row.get(0))
            .unwrap();
        assert_eq!(fts_count, 1);
    }

    #[test]
    fn create_schema_propagates_fts_rebuild_errors() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute("CREATE TABLE wallpapers_fts(dummy TEXT)", [])
            .unwrap();

        let err = create_schema(&conn).unwrap_err();

        assert!(
            err.to_string().contains("wallpapers_fts") || err.to_string().contains("table"),
            "{err}"
        );
    }

    #[test]
    fn open_runtime_connection_waits_through_exclusive_transaction_lock() {
        use std::sync::{Arc, Barrier};
        use std::thread;
        use std::time::{Duration, Instant};

        use rusqlite::ErrorCode;

        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        ensure_sqlite_db(&cd);
        let conn = Connection::open(cd.db_path()).unwrap();
        conn.execute_batch("PRAGMA journal_mode = DELETE;")
            .expect("switch test db to rollback journal for lock contention");
        conn.execute(
            "INSERT INTO wallpapers (path, type, ext, backend, size, mtime, resolution)
             VALUES ('/walls/a.png', 'image', 'png', 'awww', 100, 1000, '1x1')",
            [],
        )
        .unwrap();

        let db_path = cd.db_path().to_path_buf();
        let barrier = Arc::new(Barrier::new(2));
        let writer_barrier = barrier.clone();
        let writer_db_path = db_path.clone();
        let writer = thread::spawn(move || {
            let conn = Connection::open(&writer_db_path).unwrap();
            conn.execute("BEGIN EXCLUSIVE", [])
                .expect("writer should acquire exclusive lock");
            writer_barrier.wait();
            thread::sleep(Duration::from_millis(200));
            conn.execute("COMMIT", []).unwrap();
        });

        barrier.wait();

        let no_wait = Connection::open(&db_path).unwrap();
        no_wait
            .busy_timeout(Duration::from_millis(0))
            .expect("set zero busy timeout");
        let started = Instant::now();
        let no_wait_err = no_wait
            .query_row("SELECT COUNT(*) FROM wallpapers", [], |row| row.get::<_, i64>(0))
            .expect_err("read without busy_timeout should fail while exclusive lock is held");
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_millis(100),
            "expected immediate SQLITE_BUSY, waited {:?}",
            elapsed
        );
        match no_wait_err {
            rusqlite::Error::SqliteFailure(err, _) => {
                assert_eq!(err.code, ErrorCode::DatabaseBusy, "{no_wait_err}");
            }
            other => panic!("expected SQLITE_BUSY, got {other}"),
        }

        let page = crate::sqlite::library_page_sqlite(
            &cd,
            &crate::sqlite::LibraryPageQuery {
                filter: crate::sqlite::LibraryFilter::All,
                sort: crate::sqlite::LibrarySort::Newest,
                search: String::new(),
                offset: 0,
                limit: 10,
            },
        )
        .expect("paged read should wait for exclusive lock instead of SQLITE_BUSY");

        writer.join().unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.items.len(), 1);
    }
}
