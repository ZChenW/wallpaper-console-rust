use rusqlite::{params, Connection};
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
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
pub const CURRENT_SCHEMA_VERSION: i64 = 2;

/// Create the wallpaper-console SQLite schema.
pub fn create_schema(conn: &Connection) -> Result<(), WcError> {
    // Foreign-key enforcement is connection-local and cannot be enabled from
    // inside a transaction.
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    let version = conn
        .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    if version > CURRENT_SCHEMA_VERSION {
        return Err(WcError::Sqlite(format!(
            "database schema version {version} is newer than supported version {CURRENT_SCHEMA_VERSION}"
        )));
    }
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA temp_store = MEMORY;",
    )
    .map_err(|e| WcError::Sqlite(e.to_string()))?;

    let tx = conn
        .unchecked_transaction()
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    let migration = (|| {
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS db_meta (
            key        TEXT PRIMARY KEY,
            value      TEXT NOT NULL,
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS config (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS sources (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            path         TEXT NOT NULL UNIQUE,
            added_at     TEXT NOT NULL DEFAULT (datetime('now')),
            display_name TEXT NOT NULL DEFAULT '',
            kind         TEXT NOT NULL DEFAULT 'directory'
                         CHECK (kind IN ('directory', 'wallpaper_engine_workshop')),
            recursive    INTEGER NOT NULL DEFAULT 1 CHECK (recursive IN (0, 1)),
            availability TEXT NOT NULL DEFAULT 'unknown'
                         CHECK (availability IN ('unknown', 'available', 'offline'))
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
            last_seen  TEXT NOT NULL DEFAULT (datetime('now')),
            added_at   TEXT NOT NULL DEFAULT (datetime('now'))
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

        CREATE TRIGGER IF NOT EXISTS wallpapers_au
        AFTER UPDATE OF path, title, workshop_id, project_type ON wallpapers BEGIN
            INSERT INTO wallpapers_fts(wallpapers_fts, rowid, path, title, workshop_id, project_type)
            VALUES ('delete', old.id, old.path, old.title, old.workshop_id, old.project_type);
            INSERT INTO wallpapers_fts(rowid, path, title, workshop_id, project_type)
            VALUES (new.id, new.path, new.title, new.workshop_id, new.project_type);
        END;",
        )
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
        ensure_wallpaper_metadata_columns(&tx)?;
        drop_wallpapers_fts_triggers(&tx)?;
        ensure_v2_columns(&tx)?;
        ensure_wallpaper_sources_schema(&tx)?;
        if version < CURRENT_SCHEMA_VERSION {
            migrate_sources_and_memberships(&tx)?;
            // Alias merging runs with FTS triggers intentionally suspended.
            // Force one rebuild after commit even if the old database already
            // carried the current FTS marker.
            tx.execute("DELETE FROM db_meta WHERE key = 'fts_schema_version'", [])
                .map_err(|e| WcError::Sqlite(e.to_string()))?;
            tx.pragma_update(None, "user_version", CURRENT_SCHEMA_VERSION)
                .map_err(|e| WcError::Sqlite(e.to_string()))?;
        }
        tx.execute(
            "INSERT INTO db_meta (key, value) VALUES ('schema_version', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value
             WHERE db_meta.value != excluded.value",
            params![CURRENT_SCHEMA_VERSION.to_string()],
        )
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
        // The v1 schema allowed duplicate paths. Build the unique index only
        // after canonical/exact aliases have merged.
        ensure_wallpaper_query_indexes(&tx)?;
        ensure_wallpapers_fts_triggers(&tx)?;
        ensure_wallpapers_fts_rebuilt(&tx)?;
        Ok(())
    })();
    if let Err(err) = migration {
        let _ = tx.rollback();
        return Err(err);
    }
    tx.commit().map_err(|e| WcError::Sqlite(e.to_string()))?;

    super::display_state::ensure_display_state(conn)?;
    Ok(())
}

fn drop_wallpapers_fts_triggers(conn: &Connection) -> Result<(), WcError> {
    conn.execute_batch(
        "DROP TRIGGER IF EXISTS wallpapers_ai;
         DROP TRIGGER IF EXISTS wallpapers_ad;
         DROP TRIGGER IF EXISTS wallpapers_au;",
    )
    .map_err(|e| WcError::Sqlite(e.to_string()))?;
    Ok(())
}

fn ensure_wallpapers_fts_triggers(conn: &Connection) -> Result<(), WcError> {
    conn.execute_batch(
        "CREATE TRIGGER wallpapers_ai AFTER INSERT ON wallpapers BEGIN
             INSERT INTO wallpapers_fts(rowid, path, title, workshop_id, project_type)
             VALUES (new.id, new.path, new.title, new.workshop_id, new.project_type);
         END;
         CREATE TRIGGER wallpapers_ad AFTER DELETE ON wallpapers BEGIN
             INSERT INTO wallpapers_fts(wallpapers_fts, rowid, path, title, workshop_id, project_type)
             VALUES ('delete', old.id, old.path, old.title, old.workshop_id, old.project_type);
         END;
         CREATE TRIGGER wallpapers_au
         AFTER UPDATE OF path, title, workshop_id, project_type ON wallpapers BEGIN
             INSERT INTO wallpapers_fts(wallpapers_fts, rowid, path, title, workshop_id, project_type)
             VALUES ('delete', old.id, old.path, old.title, old.workshop_id, old.project_type);
             INSERT INTO wallpapers_fts(rowid, path, title, workshop_id, project_type)
             VALUES (new.id, new.path, new.title, new.workshop_id, new.project_type);
         END;",
    )
    .map_err(|e| WcError::Sqlite(e.to_string()))?;
    Ok(())
}

fn table_columns(
    conn: &Connection,
    table: &str,
) -> Result<std::collections::HashSet<String>, WcError> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| WcError::Sqlite(e.to_string()))?
        .collect::<Result<std::collections::HashSet<_>, _>>()
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    Ok(columns)
}

fn ensure_v2_columns(conn: &Connection) -> Result<(), WcError> {
    let source_columns = table_columns(conn, "sources")?;
    for (name, definition) in [
        ("display_name", "TEXT NOT NULL DEFAULT ''"),
        (
            "kind",
            "TEXT NOT NULL DEFAULT 'directory' CHECK (kind IN ('directory', 'wallpaper_engine_workshop'))",
        ),
        (
            "recursive",
            "INTEGER NOT NULL DEFAULT 1 CHECK (recursive IN (0, 1))",
        ),
        (
            "availability",
            "TEXT NOT NULL DEFAULT 'unknown' CHECK (availability IN ('unknown', 'available', 'offline'))",
        ),
    ] {
        if !source_columns.contains(name) {
            conn.execute_batch(&format!(
                "ALTER TABLE sources ADD COLUMN {name} {definition};"
            ))
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        }
    }

    let wallpaper_columns = table_columns(conn, "wallpapers")?;
    if !wallpaper_columns.contains("added_at") {
        // SQLite cannot add a column with a non-constant datetime default.
        conn.execute_batch("ALTER TABLE wallpapers ADD COLUMN added_at TEXT NOT NULL DEFAULT ''; ")
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
    }
    conn.execute_batch(
        "CREATE TRIGGER IF NOT EXISTS wallpapers_added_at_ai
         AFTER INSERT ON wallpapers
         WHEN new.added_at = ''
         BEGIN
             UPDATE wallpapers SET added_at = datetime('now') WHERE id = new.id;
         END;
         UPDATE wallpapers SET added_at = datetime('now') WHERE added_at = '';",
    )
    .map_err(|e| WcError::Sqlite(e.to_string()))?;
    Ok(())
}

fn ensure_wallpaper_sources_schema(conn: &Connection) -> Result<(), WcError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS wallpaper_sources (
             wallpaper_id INTEGER NOT NULL REFERENCES wallpapers(id) ON DELETE CASCADE,
             source_id    INTEGER NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
             last_seen_at TEXT NOT NULL DEFAULT (datetime('now')),
             PRIMARY KEY (wallpaper_id, source_id)
         );
         CREATE INDEX IF NOT EXISTS idx_wallpaper_sources_source
             ON wallpaper_sources(source_id, wallpaper_id);",
    )
    .map_err(|e| WcError::Sqlite(e.to_string()))?;
    Ok(())
}

fn canonical_source_path(path: &str) -> String {
    if std::fs::canonicalize(path).is_ok() {
        wc_scan::normalize_source_path(path)
    } else {
        path.to_string()
    }
}

fn canonical_entry_path(path: &str) -> String {
    std::fs::canonicalize(path)
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|_| {
            lexical_normalize(Path::new(path))
                .to_string_lossy()
                .to_string()
        })
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if normalized.file_name().is_some() {
                    normalized.pop();
                } else if !normalized.has_root() {
                    normalized.push(component.as_os_str());
                }
            }
        }
    }
    normalized
}

fn default_source_name(path: &str, kind: &str) -> String {
    if kind == "wallpaper_engine_workshop" {
        return "Wallpaper Engine".to_string();
    }
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(path)
        .to_string()
}

fn migrate_sources_and_memberships(conn: &Connection) -> Result<(), WcError> {
    let source_rows = {
        let mut stmt = conn
            .prepare("SELECT id, path FROM sources ORDER BY id")
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| WcError::Sqlite(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        rows
    };

    let mut aliases: BTreeMap<String, Vec<(i64, String)>> = BTreeMap::new();
    for (id, path) in source_rows {
        aliases
            .entry(canonical_source_path(&path))
            .or_default()
            .push((id, path));
    }

    for (canonical_path, mut rows) in aliases {
        rows.sort_by_key(|(id, _)| *id);
        let survivor_id = rows[0].0;
        for (alias_id, _) in rows.iter().skip(1) {
            conn.execute(
                "INSERT OR IGNORE INTO wallpaper_sources (wallpaper_id, source_id, last_seen_at)
                 SELECT wallpaper_id, ?1, last_seen_at
                 FROM wallpaper_sources WHERE source_id = ?2",
                params![survivor_id, alias_id],
            )
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
            conn.execute(
                "UPDATE wallpapers SET source_id = ?1 WHERE source_id = ?2",
                params![survivor_id, alias_id],
            )
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
            conn.execute(
                "DELETE FROM wallpaper_sources WHERE source_id = ?1",
                params![alias_id],
            )
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
            conn.execute("DELETE FROM sources WHERE id = ?1", params![alias_id])
                .map_err(|e| WcError::Sqlite(e.to_string()))?;
        }

        let kind = if wc_scan::is_wallpaper_engine_source(&canonical_path) {
            "wallpaper_engine_workshop"
        } else {
            "directory"
        };
        let display_name = default_source_name(&canonical_path, kind);
        let recursive = i64::from(kind == "directory");
        conn.execute(
            "UPDATE sources
             SET path = ?1,
                 display_name = CASE WHEN trim(display_name) = '' THEN ?2 ELSE display_name END,
                 kind = ?3,
                 recursive = ?4,
                 availability = CASE
                     WHEN availability IN ('unknown', 'available', 'offline') THEN availability
                     ELSE 'unknown'
                 END
             WHERE id = ?5",
            params![canonical_path, display_name, kind, recursive, survivor_id],
        )
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    }

    // Preserve explicit v1 ownership even if the path is outside the source.
    conn.execute(
        "INSERT OR IGNORE INTO wallpaper_sources (wallpaper_id, source_id, last_seen_at)
         SELECT wallpapers.id, sources.id, wallpapers.last_seen
         FROM wallpapers
         JOIN sources ON sources.id = wallpapers.source_id
         WHERE wallpapers.source_id IS NOT NULL",
        [],
    )
    .map_err(|e| WcError::Sqlite(e.to_string()))?;
    merge_wallpaper_aliases(conn)?;
    // Then add every component-aware containment membership for overlaps.
    backfill_containment_memberships(conn)?;
    conn.execute("UPDATE wallpapers SET source_id = NULL", [])
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    Ok(())
}

fn merge_wallpaper_aliases(conn: &Connection) -> Result<(), WcError> {
    let wallpaper_rows = {
        let mut stmt = conn
            .prepare("SELECT id, path FROM wallpapers ORDER BY id")
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| WcError::Sqlite(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        rows
    };
    let mut aliases: BTreeMap<String, Vec<(i64, String)>> = BTreeMap::new();
    for (id, path) in wallpaper_rows {
        aliases
            .entry(canonical_entry_path(&path))
            .or_default()
            .push((id, path));
    }

    for (canonical_path, mut rows) in aliases {
        rows.sort_by_key(|(id, _)| *id);
        let survivor_id = rows[0].0;
        for (alias_id, alias_path) in rows.iter().skip(1) {
            conn.execute(
                "INSERT OR IGNORE INTO wallpaper_sources (wallpaper_id, source_id, last_seen_at)
                 SELECT ?1, source_id, last_seen_at
                 FROM wallpaper_sources WHERE wallpaper_id = ?2",
                params![survivor_id, alias_id],
            )
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
            merge_wallpaper_metadata(conn, survivor_id, *alias_id)?;
            migrate_wallpaper_path_references(conn, alias_path, &canonical_path)?;
            conn.execute(
                "DELETE FROM wallpaper_sources WHERE wallpaper_id = ?1",
                params![alias_id],
            )
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
            conn.execute("DELETE FROM wallpapers WHERE id = ?1", params![alias_id])
                .map_err(|e| WcError::Sqlite(e.to_string()))?;
        }
        migrate_wallpaper_path_references(conn, &rows[0].1, &canonical_path)?;
        conn.execute(
            "UPDATE wallpapers SET path = ?1 WHERE id = ?2",
            params![canonical_path, survivor_id],
        )
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    }
    Ok(())
}

fn merge_wallpaper_metadata(
    conn: &Connection,
    survivor_id: i64,
    alias_id: i64,
) -> Result<(), WcError> {
    conn.execute(
        "UPDATE wallpapers SET
             type = CASE WHEN trim(type) = '' THEN
                 (SELECT type FROM wallpapers WHERE id = ?2) ELSE type END,
             ext = CASE WHEN trim(ext) = '' THEN
                 (SELECT ext FROM wallpapers WHERE id = ?2) ELSE ext END,
             backend = CASE WHEN trim(backend) = '' THEN
                 (SELECT backend FROM wallpapers WHERE id = ?2) ELSE backend END,
             size = MAX(size, (SELECT size FROM wallpapers WHERE id = ?2)),
             mtime = MAX(mtime, (SELECT mtime FROM wallpapers WHERE id = ?2)),
             resolution = CASE WHEN trim(resolution) IN ('', '?', '?x?') THEN
                 (SELECT resolution FROM wallpapers WHERE id = ?2) ELSE resolution END,
             project_type = CASE WHEN trim(project_type) = '' THEN
                 (SELECT project_type FROM wallpapers WHERE id = ?2) ELSE project_type END,
             preview_path = CASE WHEN trim(preview_path) = '' THEN
                 (SELECT preview_path FROM wallpapers WHERE id = ?2) ELSE preview_path END,
             workshop_id = CASE WHEN trim(workshop_id) = '' THEN
                 (SELECT workshop_id FROM wallpapers WHERE id = ?2) ELSE workshop_id END,
             title = CASE WHEN trim(title) = '' THEN
                 (SELECT title FROM wallpapers WHERE id = ?2) ELSE title END,
             we_file = CASE WHEN trim(we_file) = '' THEN
                 (SELECT we_file FROM wallpapers WHERE id = ?2) ELSE we_file END,
             unsupported_reason = CASE WHEN trim(unsupported_reason) = '' THEN
                 (SELECT unsupported_reason FROM wallpapers WHERE id = ?2)
                 ELSE unsupported_reason END,
             last_seen = MAX(last_seen, (SELECT last_seen FROM wallpapers WHERE id = ?2)),
             added_at = CASE
                 WHEN added_at = '' THEN (SELECT added_at FROM wallpapers WHERE id = ?2)
                 WHEN (SELECT added_at FROM wallpapers WHERE id = ?2) = '' THEN added_at
                 ELSE MIN(added_at, (SELECT added_at FROM wallpapers WHERE id = ?2))
             END
         WHERE id = ?1",
        params![survivor_id, alias_id],
    )
    .map_err(|e| WcError::Sqlite(e.to_string()))?;
    Ok(())
}

fn migrate_wallpaper_path_references(
    conn: &Connection,
    old_path: &str,
    canonical_path: &str,
) -> Result<(), WcError> {
    if old_path == canonical_path {
        return Ok(());
    }
    conn.execute(
        "INSERT OR IGNORE INTO favorites (path, added_at)
         SELECT ?1, added_at FROM favorites WHERE path = ?2",
        params![canonical_path, old_path],
    )
    .map_err(|e| WcError::Sqlite(e.to_string()))?;
    conn.execute("DELETE FROM favorites WHERE path = ?1", params![old_path])
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    conn.execute(
        "UPDATE history SET path = ?1 WHERE path = ?2",
        params![canonical_path, old_path],
    )
    .map_err(|e| WcError::Sqlite(e.to_string()))?;
    conn.execute(
        "UPDATE state SET value = ?1 WHERE key = 'current' AND value = ?2",
        params![canonical_path, old_path],
    )
    .map_err(|e| WcError::Sqlite(e.to_string()))?;
    let display_state_exists = conn
        .query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM sqlite_master
                 WHERE type = 'table' AND name = 'display_state'
             )",
            [],
            |row| row.get::<_, bool>(0),
        )
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    if display_state_exists {
        conn.execute(
            "UPDATE display_state SET wallpaper_path = ?1 WHERE wallpaper_path = ?2",
            params![canonical_path, old_path],
        )
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    }
    Ok(())
}

fn backfill_containment_memberships(conn: &Connection) -> Result<(), WcError> {
    let sources = {
        let mut stmt = conn
            .prepare("SELECT id, path FROM sources ORDER BY id")
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| WcError::Sqlite(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        rows
    };
    let wallpapers = {
        let mut stmt = conn
            .prepare("SELECT id, path, last_seen FROM wallpapers ORDER BY id")
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(|e| WcError::Sqlite(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        rows
    };

    for (wallpaper_id, wallpaper_path, last_seen) in wallpapers {
        let canonical_wallpaper = canonical_entry_path(&wallpaper_path);
        for (source_id, source_path) in &sources {
            if Path::new(&canonical_wallpaper).starts_with(Path::new(source_path)) {
                conn.execute(
                    "INSERT OR IGNORE INTO wallpaper_sources
                     (wallpaper_id, source_id, last_seen_at) VALUES (?1, ?2, ?3)",
                    params![wallpaper_id, source_id, last_seen],
                )
                .map_err(|e| WcError::Sqlite(e.to_string()))?;
            }
        }
    }
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

    let temp_path = reserve_migration_temp_path(&db_path)?;
    let migration = migrate_into_temp_db(cd, &temp_path);
    if let Err(err) = migration {
        cleanup_migration_temp(&temp_path);
        return Err(err);
    }
    publish_migration_temp(&temp_path, &db_path)
}

static MIGRATION_TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn migration_sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

fn cleanup_migration_sidecars(path: &Path) {
    for suffix in ["-wal", "-shm", "-journal"] {
        let _ = std::fs::remove_file(migration_sidecar(path, suffix));
    }
}

fn cleanup_migration_temp(path: &Path) {
    let _ = std::fs::remove_file(path);
    cleanup_migration_sidecars(path);
}

/// Atomically publish a completed same-directory migration without replacing
/// a database another process won the race to create.
fn publish_migration_temp(temp_path: &Path, final_path: &Path) -> Result<(), WcError> {
    match std::fs::hard_link(temp_path, final_path) {
        Ok(()) => {
            // The final hard link is already durable as a directory entry.
            // Failure to unlink the private temp name must not undo or damage
            // the successfully published database.
            cleanup_migration_temp(temp_path);
            Ok(())
        }
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            cleanup_migration_temp(temp_path);
            Err(WcError::Other(format!(
                "database already exists; completed migration was not published: {}",
                final_path.display()
            )))
        }
        Err(err) => {
            cleanup_migration_temp(temp_path);
            Err(WcError::Io(err))
        }
    }
}

fn reserve_migration_temp_path(db_path: &Path) -> Result<PathBuf, WcError> {
    let parent = db_path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = db_path
        .file_name()
        .unwrap_or_else(|| OsStr::new("wallpapers.db"));
    for _ in 0..100 {
        let counter = MIGRATION_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut name = OsString::from(".");
        name.push(file_name);
        name.push(format!(".migrate-{}-{counter}.tmp", std::process::id()));
        let path = parent.join(name);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => {
                drop(file);
                return Ok(path);
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(WcError::Io(err)),
        }
    }
    Err(WcError::Other(
        "could not reserve a temporary database for legacy migration".into(),
    ))
}

fn migrate_into_temp_db(cd: &ConfigDir, temp_path: &Path) -> Result<(), WcError> {
    let conn = Connection::open(temp_path).map_err(|e| WcError::Sqlite(e.to_string()))?;
    create_schema(&conn)?;
    let now = super::chrono_now();
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    {
        let conn: &Connection = &tx;

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
            let (path, display_name, kind, recursive) = super::sources::source_defaults(&path)?;
            conn.execute(
                "INSERT OR IGNORE INTO sources
             (path, added_at, display_name, kind, recursive, availability)
             VALUES (?1, ?2, ?3, ?4, ?5, 'unknown')",
                params![path, now, display_name, kind.as_str(), recursive],
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
            "INSERT OR REPLACE INTO db_meta (key, value) VALUES ('schema_version', ?1)",
            params![CURRENT_SCHEMA_VERSION.to_string()],
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

        // Wallpapers — import from library.tsv if present. This helper writes
        // through the caller's transaction and must not start a nested one.
        super::import_library_tsv_into(conn, cd)?;
        backfill_containment_memberships(conn)?;
    }
    tx.commit().map_err(|e| WcError::Sqlite(e.to_string()))?;

    // Publish only a self-contained main database. Switching out of WAL after
    // a checkpoint removes any dependency on temp-path sidecars before close.
    conn.execute_batch(
        "PRAGMA wal_checkpoint(TRUNCATE);
         PRAGMA journal_mode = DELETE;",
    )
    .map_err(|e| WcError::Sqlite(e.to_string()))?;
    drop(conn);

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
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA journal_mode = WAL;",
    )
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
    #[cfg(test)]
    RUNTIME_CONNECTION_OPEN_COUNT.with(|count| count.set(count.get() + 1));
    Ok(conn)
}

#[cfg(test)]
thread_local! {
    static RUNTIME_CONNECTION_OPEN_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_runtime_connection_open_count() {
    RUNTIME_CONNECTION_OPEN_COUNT.with(|count| count.set(0));
}

#[cfg(test)]
pub(crate) fn runtime_connection_open_count() -> usize {
    RUNTIME_CONNECTION_OPEN_COUNT.with(std::cell::Cell::get)
}

/// Fallible bootstrap: open/create the DB, apply the full schema, and set
/// runtime PRAGMAs. Surfaces schema/migration errors to callers that can
/// propagate them (for example [`crate::StorageApi::try_new`] and display-state
/// ConfigDir helpers).
pub fn try_ensure_sqlite_db(cd: &ConfigDir) -> Result<(), WcError> {
    let db = cd.db_path();
    let conn = Connection::open(&db).map_err(|e| WcError::Sqlite(e.to_string()))?;
    create_schema(&conn)?;
    apply_runtime_pragmas(&conn)?;
    Ok(())
}

/// Ensure wallpapers.db exists with the full schema.
/// No-op if the file already exists. Failures are logged and silently ignored
/// so that callers never get blocked by bootstrap failures.
pub fn ensure_sqlite_db(cd: &ConfigDir) {
    let _ = try_ensure_sqlite_db(cd);
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

    fn create_v1_schema(conn: &Connection) {
        conn.execute_batch(
            "PRAGMA user_version = 1;
             CREATE TABLE sources (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 path TEXT NOT NULL UNIQUE,
                 added_at TEXT NOT NULL DEFAULT (datetime('now'))
             );
             CREATE TABLE wallpapers (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 path TEXT NOT NULL,
                 type TEXT NOT NULL,
                 ext TEXT NOT NULL,
                 backend TEXT NOT NULL,
                 size INTEGER NOT NULL DEFAULT 0,
                 mtime INTEGER NOT NULL DEFAULT 0,
                 resolution TEXT NOT NULL DEFAULT '?x?',
                 project_type TEXT NOT NULL DEFAULT '',
                 preview_path TEXT NOT NULL DEFAULT '',
                 workshop_id TEXT NOT NULL DEFAULT '',
                 title TEXT NOT NULL DEFAULT '',
                 we_file TEXT NOT NULL DEFAULT '',
                 unsupported_reason TEXT NOT NULL DEFAULT '',
                 source_id INTEGER REFERENCES sources(id),
                 last_seen TEXT NOT NULL DEFAULT (datetime('now'))
             );",
        )
        .unwrap();
    }

    fn column_names(conn: &Connection, table: &str) -> Vec<String> {
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info({table})"))
            .unwrap();
        stmt.query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    }

    fn insert_v1_wallpaper(conn: &Connection, path: &str, source_id: Option<i64>) {
        conn.execute(
            "INSERT INTO wallpapers
             (path, type, ext, backend, size, mtime, resolution, source_id)
             VALUES (?1, 'image', 'jpg', 'awww', 1, 1, '1x1', ?2)",
            params![path, source_id],
        )
        .unwrap();
    }

    #[test]
    fn migrates_v1_to_v2_without_changing_source_or_wallpaper_identity() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("walls");
        std::fs::create_dir(&source).unwrap();
        let wallpaper = source.join("a.jpg");
        std::fs::write(&wallpaper, b"wallpaper").unwrap();
        let conn = Connection::open_in_memory().unwrap();
        create_v1_schema(&conn);
        conn.execute(
            "INSERT INTO sources (id, path) VALUES (7, ?1)",
            params![source.to_string_lossy()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO wallpapers (id, path, type, ext, backend, source_id)
             VALUES (11, ?1, 'image', 'jpg', 'awww', 7)",
            params![wallpaper.to_string_lossy()],
        )
        .unwrap();

        create_schema(&conn).unwrap();

        assert_eq!(
            conn.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            2
        );
        let source_row: (i64, String, String, String, i64, String) = conn
            .query_row(
                "SELECT id, path, display_name, kind, recursive, availability FROM sources",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(source_row.0, 7);
        assert_eq!(source_row.1, source.to_string_lossy());
        assert_eq!(source_row.2, "walls");
        assert_eq!(source_row.3, "directory");
        assert_eq!(source_row.4, 1);
        assert_eq!(source_row.5, "unknown");
        let wallpaper_row: (i64, String, Option<i64>, String) = conn
            .query_row(
                "SELECT id, path, source_id, added_at FROM wallpapers",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(wallpaper_row.0, 11);
        assert_eq!(wallpaper_row.1, wallpaper.to_string_lossy());
        assert_eq!(wallpaper_row.2, None);
        assert!(!wallpaper_row.3.is_empty());
        let membership: (i64, i64) = conn
            .query_row(
                "SELECT wallpaper_id, source_id FROM wallpaper_sources",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(membership, (11, 7));
        conn.execute(
            "INSERT INTO wallpapers (path, type, ext, backend)
             VALUES ('/walls/new.jpg', 'image', 'jpg', 'awww')",
            [],
        )
        .unwrap();
        let inserted_added_at: String = conn
            .query_row(
                "SELECT added_at FROM wallpapers WHERE path = '/walls/new.jpg'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!inserted_added_at.is_empty());
    }

    #[test]
    fn rejects_future_schema_without_modifying_it() {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "user_version", 3).unwrap();

        let err = create_schema(&conn).unwrap_err();

        assert!(err.to_string().contains("newer") || err.to_string().contains("version"));
        assert_eq!(
            conn.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            3
        );
        let table_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'sources'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_count, 0);
    }

    #[test]
    fn v2_migration_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("walls");
        std::fs::create_dir(&source).unwrap();
        let wallpaper = source.join("a.jpg");
        std::fs::write(&wallpaper, b"wallpaper").unwrap();
        let conn = Connection::open_in_memory().unwrap();
        create_v1_schema(&conn);
        conn.execute(
            "INSERT INTO sources (path) VALUES (?1)",
            params![source.to_string_lossy()],
        )
        .unwrap();
        insert_v1_wallpaper(&conn, &wallpaper.to_string_lossy(), Some(1));

        create_schema(&conn).unwrap();
        create_schema(&conn).unwrap();

        let counts: (i64, i64, i64) = conn
            .query_row(
                "SELECT
                    (SELECT COUNT(*) FROM sources),
                    (SELECT COUNT(*) FROM wallpapers),
                    (SELECT COUNT(*) FROM wallpaper_sources)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(counts, (1, 1, 1));
    }

    #[test]
    fn current_v2_schema_repairs_a_stale_db_meta_marker_without_replaying_migration() {
        let conn = Connection::open_in_memory().unwrap();
        create_schema(&conn).unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO db_meta (key, value) VALUES ('schema_version', '1')",
            [],
        )
        .unwrap();

        create_schema(&conn).unwrap();

        assert_eq!(
            conn.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            CURRENT_SCHEMA_VERSION
        );
        let marker: String = conn
            .query_row(
                "SELECT value FROM db_meta WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(marker, CURRENT_SCHEMA_VERSION.to_string());
    }

    #[test]
    fn overlapping_sources_both_receive_membership_using_component_boundaries() {
        let tmp = tempfile::tempdir().unwrap();
        let parent = tmp.path().join("walls");
        let child = parent.join("nested");
        let sibling_prefix = tmp.path().join("walls2");
        std::fs::create_dir_all(&child).unwrap();
        std::fs::create_dir_all(&sibling_prefix).unwrap();
        let wallpaper = child.join("a.jpg");
        std::fs::write(&wallpaper, b"wallpaper").unwrap();
        let conn = Connection::open_in_memory().unwrap();
        create_v1_schema(&conn);
        for path in [&parent, &child, &sibling_prefix] {
            conn.execute(
                "INSERT INTO sources (path) VALUES (?1)",
                params![path.to_string_lossy()],
            )
            .unwrap();
        }
        insert_v1_wallpaper(&conn, &wallpaper.to_string_lossy(), Some(2));

        create_schema(&conn).unwrap();

        let mut stmt = conn
            .prepare(
                "SELECT sources.path
                 FROM wallpaper_sources
                 JOIN sources ON sources.id = wallpaper_sources.source_id
                 ORDER BY sources.path",
            )
            .unwrap();
        let memberships = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            memberships,
            vec![
                parent.to_string_lossy().to_string(),
                child.to_string_lossy().to_string()
            ]
        );
    }

    #[test]
    fn unavailable_absolute_source_path_is_preserved_and_not_deleted_during_migration() {
        let tmp = tempfile::tempdir().unwrap();
        let missing_source = tmp.path().join("offline-library");
        let missing_wallpaper = missing_source.join("a.jpg");
        let conn = Connection::open_in_memory().unwrap();
        create_v1_schema(&conn);
        conn.execute(
            "INSERT INTO sources (id, path) VALUES (12, ?1)",
            params![missing_source.to_string_lossy()],
        )
        .unwrap();
        insert_v1_wallpaper(&conn, &missing_wallpaper.to_string_lossy(), Some(12));

        create_schema(&conn).unwrap();

        let source: (i64, String, String) = conn
            .query_row("SELECT id, path, availability FROM sources", [], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .unwrap();
        assert_eq!(source.0, 12);
        assert_eq!(source.1, missing_source.to_string_lossy());
        assert_eq!(source.2, "unknown");
        let membership_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM wallpaper_sources", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(membership_count, 1);
    }

    #[cfg(unix)]
    #[test]
    fn canonical_source_aliases_merge_into_lowest_id_without_losing_membership() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("walls");
        let alias = tmp.path().join("walls-link");
        std::fs::create_dir(&real).unwrap();
        std::os::unix::fs::symlink(&real, &alias).unwrap();
        let wallpaper = real.join("a.jpg");
        std::fs::write(&wallpaper, b"wallpaper").unwrap();
        let conn = Connection::open_in_memory().unwrap();
        create_v1_schema(&conn);
        conn.execute(
            "INSERT INTO sources (id, path) VALUES (3, ?1), (9, ?2)",
            params![alias.to_string_lossy(), real.to_string_lossy()],
        )
        .unwrap();
        insert_v1_wallpaper(&conn, &wallpaper.to_string_lossy(), Some(9));

        create_schema(&conn).unwrap();

        let source_row: (i64, String) = conn
            .query_row("SELECT id, path FROM sources", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap();
        assert_eq!(source_row, (3, real.to_string_lossy().to_string()));
        let membership_source: i64 = conn
            .query_row("SELECT source_id FROM wallpaper_sources", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(membership_source, 3);
    }

    #[cfg(unix)]
    #[test]
    fn canonical_wallpaper_aliases_merge_into_lowest_id_and_preserve_metadata_and_memberships() {
        let tmp = tempfile::tempdir().unwrap();
        let parent = tmp.path().join("walls");
        let child = parent.join("nested");
        std::fs::create_dir_all(&child).unwrap();
        let real = child.join("a.jpg");
        let alias = parent.join("a-link.jpg");
        std::fs::write(&real, b"wallpaper").unwrap();
        std::os::unix::fs::symlink(&real, &alias).unwrap();
        let conn = Connection::open_in_memory().unwrap();
        create_v1_schema(&conn);
        conn.execute(
            "INSERT INTO sources (id, path) VALUES (2, ?1), (6, ?2)",
            params![parent.to_string_lossy(), child.to_string_lossy()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO wallpapers
             (id, path, type, ext, backend, source_id, title, workshop_id)
             VALUES
             (4, ?1, 'image', 'jpg', 'awww', 2, '', ''),
             (8, ?2, 'image', 'jpg', 'awww', 6, 'Rich title', '123')",
            params![alias.to_string_lossy(), real.to_string_lossy()],
        )
        .unwrap();
        conn.execute_batch(
            "CREATE TABLE favorites (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 path TEXT NOT NULL UNIQUE,
                 added_at TEXT NOT NULL DEFAULT (datetime('now'))
             );
             CREATE TABLE state (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO favorites (path) VALUES (?1)",
            params![alias.to_string_lossy()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO state (key, value) VALUES ('current', ?1)",
            params![alias.to_string_lossy()],
        )
        .unwrap();

        create_schema(&conn).unwrap();

        let wallpaper: (i64, String, String, String) = conn
            .query_row(
                "SELECT id, path, title, workshop_id FROM wallpapers",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(wallpaper.0, 4);
        assert_eq!(wallpaper.1, real.to_string_lossy());
        assert_eq!(wallpaper.2, "Rich title");
        assert_eq!(wallpaper.3, "123");
        let membership_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM wallpaper_sources WHERE wallpaper_id = 4",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(membership_count, 2);
        let favorite: String = conn
            .query_row("SELECT path FROM favorites", [], |row| row.get(0))
            .unwrap();
        let current: String = conn
            .query_row("SELECT value FROM state WHERE key = 'current'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(favorite, real.to_string_lossy());
        assert_eq!(current, real.to_string_lossy());
        assert_eq!(wallpapers_fts_count(&conn).unwrap(), 1);
        check_wallpapers_fts_integrity(&conn).unwrap();
    }

    #[test]
    fn exact_duplicate_v1_wallpaper_paths_merge_before_unique_index_creation() {
        let conn = Connection::open_in_memory().unwrap();
        create_v1_schema(&conn);
        conn.execute(
            "INSERT INTO wallpapers
             (id, path, type, ext, backend, title)
             VALUES
             (2, '/walls/a.jpg', 'image', 'jpg', 'awww', ''),
             (5, '/walls/a.jpg', 'image', 'jpg', 'awww', 'Metadata')",
            [],
        )
        .unwrap();

        create_schema(&conn).unwrap();

        let row: (i64, String) = conn
            .query_row("SELECT id, title FROM wallpapers", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap();
        assert_eq!(row, (2, "Metadata".to_string()));
        let unique_index: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_index_list('wallpapers')
                 WHERE name = 'idx_wallpapers_path' AND \"unique\" = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(unique_index, 1);
    }

    #[test]
    fn migration_enables_and_enforces_wallpaper_source_foreign_keys() {
        let conn = Connection::open_in_memory().unwrap();

        create_schema(&conn).unwrap();

        assert_eq!(
            conn.pragma_query_value(None, "foreign_keys", |row| row.get::<_, i64>(0))
                .unwrap(),
            1
        );
        let err = conn
            .execute(
                "INSERT INTO wallpaper_sources (wallpaper_id, source_id) VALUES (999, 999)",
                [],
            )
            .unwrap_err();
        assert!(err.to_string().contains("FOREIGN KEY"));
    }

    #[test]
    fn failed_v1_migration_rolls_back_columns_and_version_marker() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "PRAGMA user_version = 1;
             CREATE TABLE sources (id INTEGER PRIMARY KEY);
             CREATE TABLE wallpapers (id INTEGER PRIMARY KEY);",
        )
        .unwrap();

        create_schema(&conn).unwrap_err();

        assert_eq!(
            conn.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert!(!column_names(&conn, "sources").contains(&"display_name".to_string()));
        assert!(!column_names(&conn, "wallpapers").contains(&"added_at".to_string()));
    }

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
    fn migrate_to_sqlite_imports_flat_sources_with_v2_defaults_and_consistent_markers() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        let offline_path = tmp.path().join("offline-walls");
        flat::sources_add(&cd, &offline_path.to_string_lossy()).unwrap();

        migrate_to_sqlite(&cd).unwrap();

        let sources = crate::sqlite::sources_list_typed(&cd).unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].path, offline_path.to_string_lossy());
        assert_eq!(sources[0].display_name, "offline-walls");
        assert_eq!(sources[0].kind, crate::sqlite::SourceKind::Directory);
        assert!(sources[0].recursive);
        assert_eq!(
            sources[0].availability,
            crate::sqlite::SourceAvailability::Unknown
        );
        let conn = Connection::open(cd.db_path()).unwrap();
        assert_eq!(
            conn.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            CURRENT_SCHEMA_VERSION
        );
        let marker: String = conn
            .query_row(
                "SELECT value FROM db_meta WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(marker, CURRENT_SCHEMA_VERSION.to_string());
    }

    #[test]
    fn migrate_to_sqlite_failure_leaves_no_final_db_and_can_retry_without_losing_flat_state() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        let source = tmp.path().join("walls");
        std::fs::create_dir(&source).unwrap();
        flat::write_lines(
            &cd.sources_path(),
            &[
                source.to_string_lossy().to_string(),
                "relative-source".into(),
            ],
        )
        .unwrap();
        flat::favorites_add(&cd, "/walls/favorite.jpg").unwrap();
        flat::history_add(&cd, "/walls/history.jpg", 100).unwrap();
        flat::current_write(&cd, "/walls/current.jpg").unwrap();
        flat::last_backend_write(&cd, "awww").unwrap();

        let err = migrate_to_sqlite(&cd).unwrap_err();

        assert!(err.to_string().contains("absolute"), "{err}");
        assert!(
            !cd.db_path().exists(),
            "failed migration published a database"
        );
        let leftovers = std::fs::read_dir(&cd.path)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains("migrate"))
            .collect::<Vec<_>>();
        assert!(
            leftovers.is_empty(),
            "temporary migration files: {leftovers:?}"
        );

        flat::write_lines(&cd.sources_path(), &[source.to_string_lossy().to_string()]).unwrap();
        migrate_to_sqlite(&cd).unwrap();

        let conn = Connection::open(cd.db_path()).unwrap();
        let source_path: String = conn
            .query_row("SELECT path FROM sources", [], |row| row.get(0))
            .unwrap();
        assert_eq!(source_path, source.to_string_lossy());
        let favorite: String = conn
            .query_row("SELECT path FROM favorites", [], |row| row.get(0))
            .unwrap();
        let history: String = conn
            .query_row("SELECT path FROM history", [], |row| row.get(0))
            .unwrap();
        let current: String = conn
            .query_row("SELECT value FROM state WHERE key = 'current'", [], |row| {
                row.get(0)
            })
            .unwrap();
        let backend: String = conn
            .query_row(
                "SELECT value FROM state WHERE key = 'last_backend'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(favorite, "/walls/favorite.jpg");
        assert_eq!(history, "/walls/history.jpg");
        assert_eq!(current, "/walls/current.jpg");
        assert_eq!(backend, "awww");
        let journal_mode: String = conn
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .unwrap();
        assert_eq!(journal_mode, "delete");
        drop(conn);
        assert!(!migration_sidecar(&cd.db_path(), "-wal").exists());
        assert!(!migration_sidecar(&cd.db_path(), "-shm").exists());
    }

    #[test]
    fn concurrent_temp_publish_is_atomic_no_replace_and_cleans_both_candidates() {
        use std::sync::{Arc, Barrier};

        let tmp = tempfile::tempdir().unwrap();
        let final_path = tmp.path().join("wallpapers.db");
        let temp_a = tmp.path().join("migration-a.tmp");
        let temp_b = tmp.path().join("migration-b.tmp");
        std::fs::write(&temp_a, b"candidate-a").unwrap();
        std::fs::write(&temp_b, b"candidate-b").unwrap();
        for path in [&temp_a, &temp_b] {
            std::fs::write(migration_sidecar(path, "-wal"), b"wal").unwrap();
            std::fs::write(migration_sidecar(path, "-shm"), b"shm").unwrap();
        }
        let barrier = Arc::new(Barrier::new(3));
        let spawn_publisher = |temp_path: PathBuf| {
            let barrier = barrier.clone();
            let final_path = final_path.clone();
            std::thread::spawn(move || {
                barrier.wait();
                publish_migration_temp(&temp_path, &final_path)
            })
        };
        let publisher_a = spawn_publisher(temp_a.clone());
        let publisher_b = spawn_publisher(temp_b.clone());

        barrier.wait();
        let result_a = publisher_a.join().unwrap();
        let result_b = publisher_b.join().unwrap();

        assert_eq!(
            usize::from(result_a.is_ok()) + usize::from(result_b.is_ok()),
            1
        );
        let loser_error = if let Err(err) = result_a {
            err
        } else {
            result_b.unwrap_err()
        };
        assert!(
            loser_error.to_string().contains("already exists")
                || loser_error.to_string().contains("already published"),
            "{loser_error}"
        );
        let published = std::fs::read(&final_path).unwrap();
        assert!(published == b"candidate-a" || published == b"candidate-b");
        for path in [&temp_a, &temp_b] {
            assert!(!path.exists(), "candidate temp was not cleaned: {path:?}");
            assert!(!migration_sidecar(path, "-wal").exists());
            assert!(!migration_sidecar(path, "-shm").exists());
        }
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
        assert_eq!(
            conn.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            0,
            "FTS failure must roll back the v2 version marker"
        );
        let sources_table: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'sources'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(sources_table, 0, "schema changes must roll back together");
    }

    #[test]
    fn try_ensure_sqlite_db_surfaces_schema_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        // Regular table occupying the FTS name makes rebuild fail during create_schema.
        let conn = Connection::open(cd.db_path()).unwrap();
        conn.execute("CREATE TABLE wallpapers_fts(dummy TEXT)", [])
            .unwrap();
        drop(conn);

        let err = try_ensure_sqlite_db(&cd).expect_err("poisoned FTS must surface");
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
            .query_row("SELECT COUNT(*) FROM wallpapers", [], |row| {
                row.get::<_, i64>(0)
            })
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

    #[test]
    fn open_runtime_connection_enables_foreign_keys() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        try_ensure_sqlite_db(&cd).unwrap();

        let conn = open_runtime_connection(&cd).unwrap();

        assert_eq!(
            conn.pragma_query_value(None, "foreign_keys", |row| row.get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
}
