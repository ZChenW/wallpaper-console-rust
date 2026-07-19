use crate::sqlite_err;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(test)]
use wc_config::ConfigDirExt;
use wc_core::config::ConfigDir;
use wc_core::error::WcError;

use crate::flat;

use super::connection::{
    acquire_maintenance_lock, invalidate_cached_connections, open_or_create_connection,
    open_runtime_connection as open_runtime_connection_with_version,
};
pub use super::connection::{apply_runtime_pragmas, RuntimeConnection};

const WALLPAPER_QUERY_INDEXES_SQL: &str = "
    CREATE UNIQUE INDEX IF NOT EXISTS idx_wallpapers_path ON wallpapers(path);
    CREATE INDEX IF NOT EXISTS idx_wallpapers_type ON wallpapers(type);
    CREATE INDEX IF NOT EXISTS idx_wallpapers_mtime ON wallpapers(mtime DESC, path ASC);
    CREATE INDEX IF NOT EXISTS idx_wallpapers_size ON wallpapers(size DESC, path ASC);
    CREATE INDEX IF NOT EXISTS idx_wallpapers_type_mtime ON wallpapers(type, mtime DESC, path ASC);
    CREATE INDEX IF NOT EXISTS idx_wallpapers_type_size ON wallpapers(type, size DESC, path ASC);
    CREATE INDEX IF NOT EXISTS idx_wallpapers_added_at ON wallpapers(added_at DESC, id DESC);
";
pub const FTS_SCHEMA_VERSION: &str = "2";
pub const CURRENT_SCHEMA_VERSION: i64 = 6;
pub(crate) const CURRENT_PERSISTENT_TABLES: &[&str] = &[
    "config",
    "sources",
    "wallpapers",
    "wallpaper_sources",
    "favorites",
    "history",
    "state",
    "display_state",
    "source_refresh_state",
    "db_meta",
];

/// Open a runtime connection that rejects databases created by newer builds.
pub fn open_runtime_connection(cd: &ConfigDir) -> Result<RuntimeConnection, WcError> {
    open_runtime_connection_with_version(cd, CURRENT_SCHEMA_VERSION)
}

/// Create the wallpaper-console SQLite schema.
pub fn create_schema(conn: &Connection) -> Result<(), WcError> {
    // Foreign-key enforcement is connection-local and cannot be enabled from
    // inside a transaction.
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .map_err(sqlite_err)?;
    let version = conn
        .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
        .map_err(sqlite_err)?;
    if version > CURRENT_SCHEMA_VERSION {
        return Err(WcError::SchemaTooNew {
            supported: CURRENT_SCHEMA_VERSION,
            observed: version,
        });
    }
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA temp_store = MEMORY;",
    )
    .map_err(sqlite_err)?;

    let tx = conn.unchecked_transaction().map_err(sqlite_err)?;
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
            added_at   TEXT NOT NULL DEFAULT (datetime('now')),
            author     TEXT NOT NULL DEFAULT '',
            filename   TEXT GENERATED ALWAYS AS (
                substr(path, length(rtrim(path, replace(path, '/', ''))) + 1)
            ) VIRTUAL
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

        CREATE TABLE IF NOT EXISTS source_refresh_state (
            source_id             INTEGER PRIMARY KEY
                                  REFERENCES sources(id) ON DELETE CASCADE,
            last_success_at       INTEGER,
            dirty                INTEGER NOT NULL DEFAULT 1 CHECK (dirty IN (0, 1)),
            failure_category     TEXT,
            consecutive_failures INTEGER NOT NULL DEFAULT 0 CHECK (consecutive_failures >= 0),
            next_retry_at         INTEGER
        );

        CREATE TABLE IF NOT EXISTS library_fts_state (
            singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
            status TEXT NOT NULL CHECK (status IN ('pending', 'ready')),
            revision INTEGER NOT NULL,
            next_wallpaper_id INTEGER NOT NULL
        ) STRICT;
        INSERT OR IGNORE INTO library_fts_state
            (singleton, status, revision, next_wallpaper_id)
            VALUES (1, 'pending', -1, 0);

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
        .map_err(sqlite_err)?;
        ensure_wallpaper_metadata_columns(&tx)?;
        drop_wallpapers_fts_triggers(&tx)?;
        ensure_v2_columns(&tx)?;
        ensure_wallpaper_sources_schema(&tx)?;
        if version < 2 {
            migrate_sources_and_memberships(&tx)?;
            // Alias merging runs with FTS triggers intentionally suspended.
            // Force one rebuild after commit even if the old database already
            // carried the current FTS marker.
            tx.execute("DELETE FROM db_meta WHERE key = 'fts_schema_version'", [])
                .map_err(sqlite_err)?;
        }
        ensure_v3_columns(&tx)?;
        tx.execute(
            "INSERT OR IGNORE INTO db_meta (key, value) VALUES ('library_revision', '0')",
            [],
        )
        .map_err(sqlite_err)?;
        if version < CURRENT_SCHEMA_VERSION {
            tx.pragma_update(None, "user_version", CURRENT_SCHEMA_VERSION)
                .map_err(sqlite_err)?;
        }
        tx.execute(
            "INSERT INTO db_meta (key, value) VALUES ('schema_version', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value
             WHERE db_meta.value != excluded.value",
            params![CURRENT_SCHEMA_VERSION.to_string()],
        )
        .map_err(sqlite_err)?;
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
    tx.commit().map_err(sqlite_err)?;

    // Browser FTS is derived state. Builds without trigram support or a
    // corrupt derived index continue with exact LIKE search and can rebuild
    // later without blocking database open/migration.
    let _ = super::library_fts::create_library_fts_schema(conn);

    super::display_state::ensure_display_state(conn)?;
    Ok(())
}

fn drop_wallpapers_fts_triggers(conn: &Connection) -> Result<(), WcError> {
    conn.execute_batch(
        "DROP TRIGGER IF EXISTS wallpapers_ai;
         DROP TRIGGER IF EXISTS wallpapers_ad;
         DROP TRIGGER IF EXISTS wallpapers_au;",
    )
    .map_err(sqlite_err)?;
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
    .map_err(sqlite_err)?;
    Ok(())
}

const REQUIRED_CURRENT_INDEXES: &[&str] = &[
    "idx_wallpapers_path",
    "idx_wallpapers_type",
    "idx_wallpapers_mtime",
    "idx_wallpapers_size",
    "idx_wallpapers_type_mtime",
    "idx_wallpapers_type_size",
    "idx_wallpapers_added_at",
    "idx_wallpaper_sources_source",
];

static SCHEMA_VALIDATION_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ColumnSignature {
    name: String,
    declared_type: String,
    not_null: bool,
    default_value: Option<String>,
    primary_key_position: i64,
    hidden: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ForeignKeySignature {
    target_table: String,
    from_column: String,
    to_column: String,
    on_update: String,
    on_delete: String,
    match_mode: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct TriggerSignature {
    name: String,
    table_name: String,
    normalized_sql: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct IndexColumnSignature {
    column_id: i64,
    name: Option<String>,
    descending: bool,
    collation: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct IndexSignature {
    table_name: String,
    unique: bool,
    origin: String,
    partial: bool,
    columns: Vec<IndexColumnSignature>,
}

fn normalize_schema_fragment(fragment: &str) -> String {
    let chars = fragment.chars().collect::<Vec<_>>();
    let mut tokens = Vec::new();
    let mut index = 0;

    while index < chars.len() {
        let current = chars[index];
        if current.is_whitespace() {
            index += 1;
            continue;
        }
        if current == '-' && chars.get(index + 1) == Some(&'-') {
            index += 2;
            while index < chars.len() && chars[index] != '\n' {
                index += 1;
            }
            continue;
        }
        if current == '/' && chars.get(index + 1) == Some(&'*') {
            index += 2;
            while index + 1 < chars.len() && !(chars[index] == '*' && chars[index + 1] == '/') {
                index += 1;
            }
            index = usize::min(index + 2, chars.len());
            continue;
        }
        if matches!(current, '\'' | '"' | '`') {
            let quote = current;
            let start = index;
            index += 1;
            while index < chars.len() {
                if chars[index] == quote {
                    if chars.get(index + 1) == Some(&quote) {
                        index += 2;
                        continue;
                    }
                    index += 1;
                    break;
                }
                index += 1;
            }
            tokens.push(chars[start..index].iter().collect::<String>());
            continue;
        }
        if current == '[' {
            let start = index;
            index += 1;
            while index < chars.len() && chars[index] != ']' {
                index += 1;
            }
            index = usize::min(index + 1, chars.len());
            tokens.push(chars[start..index].iter().collect::<String>());
            continue;
        }
        if current.is_alphanumeric() || matches!(current, '_' | '$') {
            let start = index;
            index += 1;
            while index < chars.len()
                && (chars[index].is_alphanumeric() || matches!(chars[index], '_' | '$'))
            {
                index += 1;
            }
            tokens.push(
                chars[start..index]
                    .iter()
                    .collect::<String>()
                    .to_ascii_lowercase(),
            );
            continue;
        }
        tokens.push(current.to_string());
        index += 1;
    }

    tokens.join(" ")
}

fn schema_object_sql(conn: &Connection, object_type: &str, name: &str) -> Result<String, WcError> {
    conn.query_row(
        "SELECT sql FROM sqlite_master WHERE type = ?1 AND name = ?2",
        params![object_type, name],
        |row| row.get::<_, String>(0),
    )
    .map(|sql| normalize_schema_fragment(&sql))
    .map_err(sqlite_err)
}

fn table_column_signatures(
    conn: &Connection,
    table: &str,
) -> Result<Vec<ColumnSignature>, WcError> {
    let mut statement = conn
        .prepare(
            "SELECT name, type, \"notnull\", dflt_value, pk, hidden
             FROM pragma_table_xinfo(?1)
             ORDER BY name",
        )
        .map_err(sqlite_err)?;
    let signatures = statement
        .query_map([table], |row| {
            Ok(ColumnSignature {
                name: row.get(0)?,
                declared_type: row.get::<_, String>(1)?.trim().to_ascii_uppercase(),
                not_null: row.get::<_, i64>(2)? != 0,
                default_value: row
                    .get::<_, Option<String>>(3)?
                    .map(|value| value.trim().to_string()),
                primary_key_position: row.get(4)?,
                hidden: row.get(5)?,
            })
        })
        .map_err(sqlite_err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sqlite_err)?;
    Ok(signatures)
}

fn table_foreign_key_signatures(
    conn: &Connection,
    table: &str,
) -> Result<Vec<ForeignKeySignature>, WcError> {
    let mut statement = conn
        .prepare(
            "SELECT \"table\", \"from\", \"to\", on_update, on_delete, \"match\"
             FROM pragma_foreign_key_list(?1)",
        )
        .map_err(sqlite_err)?;
    let mut signatures = statement
        .query_map([table], |row| {
            Ok(ForeignKeySignature {
                target_table: row.get(0)?,
                from_column: row.get(1)?,
                to_column: row.get(2)?,
                on_update: row.get(3)?,
                on_delete: row.get(4)?,
                match_mode: row.get(5)?,
            })
        })
        .map_err(sqlite_err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sqlite_err)?;
    signatures.sort();
    Ok(signatures)
}

fn table_unique_index_signatures(
    conn: &Connection,
    table: &str,
) -> Result<Vec<IndexSignature>, WcError> {
    let index_names = {
        let mut statement = conn
            .prepare("SELECT name FROM pragma_index_list(?1) WHERE \"unique\" = 1")
            .map_err(sqlite_err)?;
        let names = statement
            .query_map([table], |row| row.get::<_, String>(0))
            .map_err(sqlite_err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sqlite_err)?;
        names
    };
    let mut signatures = Vec::with_capacity(index_names.len());
    for index_name in index_names {
        let signature = named_index_signature(conn, &index_name)?.ok_or_else(|| {
            WcError::Other(format!(
                "current schema index {index_name} disappeared during validation"
            ))
        })?;
        signatures.push(signature);
    }
    signatures.sort();
    Ok(signatures)
}

fn trigger_signatures(conn: &Connection) -> Result<Vec<TriggerSignature>, WcError> {
    let mut statement = conn
        .prepare(
            "SELECT name, tbl_name, sql
             FROM sqlite_master
             WHERE type = 'trigger'
             ORDER BY name",
        )
        .map_err(sqlite_err)?;
    let signatures = statement
        .query_map([], |row| {
            let sql = row.get::<_, String>(2)?;
            Ok(TriggerSignature {
                name: row.get(0)?,
                table_name: row.get(1)?,
                normalized_sql: sql.split_whitespace().collect::<Vec<_>>().join(" "),
            })
        })
        .map_err(sqlite_err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sqlite_err)?;
    Ok(signatures)
}

fn named_index_signature(
    conn: &Connection,
    index_name: &str,
) -> Result<Option<IndexSignature>, WcError> {
    let table_name = conn
        .query_row(
            "SELECT tbl_name FROM sqlite_master WHERE type = 'index' AND name = ?1",
            [index_name],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(sqlite_err)?;
    let Some(table_name) = table_name else {
        return Ok(None);
    };
    let (unique, origin, partial) = conn
        .query_row(
            "SELECT \"unique\", origin, partial
             FROM pragma_index_list(?1)
             WHERE name = ?2",
            params![table_name, index_name],
            |row| {
                Ok((
                    row.get::<_, i64>(0)? != 0,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)? != 0,
                ))
            },
        )
        .map_err(sqlite_err)?;
    let mut statement = conn
        .prepare(
            "SELECT cid, name, \"desc\", coll
             FROM pragma_index_xinfo(?1)
             WHERE key = 1
             ORDER BY seqno",
        )
        .map_err(sqlite_err)?;
    let columns = statement
        .query_map([index_name], |row| {
            Ok(IndexColumnSignature {
                column_id: row.get(0)?,
                name: row.get(1)?,
                descending: row.get::<_, i64>(2)? != 0,
                collation: row.get(3)?,
            })
        })
        .map_err(sqlite_err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sqlite_err)?;
    Ok(Some(IndexSignature {
        table_name,
        unique,
        origin,
        partial,
        columns,
    }))
}

fn validate_table_shapes_against_current_schema(conn: &Connection) -> Result<(), WcError> {
    let reference = Connection::open_in_memory().map_err(sqlite_err)?;
    create_schema(&reference)?;
    for table in CURRENT_PERSISTENT_TABLES {
        let expected_columns = table_column_signatures(&reference, table)?;
        let actual_columns = table_column_signatures(conn, table)?;
        if actual_columns.len() != expected_columns.len() {
            return Err(WcError::Other(format!(
                "current schema table {table} has unexpected columns"
            )));
        }
        for expected in expected_columns {
            let actual = actual_columns
                .iter()
                .find(|column| column.name == expected.name);
            let migrated_wallpaper_added_at_default = table == &"wallpapers"
                && expected.name == "added_at"
                && actual.is_some_and(|column| {
                    column.declared_type == expected.declared_type
                        && column.not_null == expected.not_null
                        && column.primary_key_position == expected.primary_key_position
                        && column.default_value.as_deref() == Some("''")
                        && column.hidden == expected.hidden
                });
            if actual != Some(&expected) && !migrated_wallpaper_added_at_default {
                return Err(WcError::Other(format!(
                    "current schema table {table} is missing or changes required column {}",
                    expected.name
                )));
            }
        }

        let expected_table_sql = schema_object_sql(&reference, "table", table)?;
        let actual_table_sql = schema_object_sql(conn, "table", table)?;
        let migrated_wallpapers_sql = table == &"wallpapers"
            && actual_table_sql
                == expected_table_sql.replace(
                    &normalize_schema_fragment("added_at TEXT NOT NULL DEFAULT (datetime('now'))"),
                    &normalize_schema_fragment("added_at TEXT NOT NULL DEFAULT ''"),
                );
        if actual_table_sql != expected_table_sql && !migrated_wallpapers_sql {
            return Err(WcError::Other(format!(
                "current schema table {table} definition differs from the current schema"
            )));
        }

        let expected_foreign_keys = table_foreign_key_signatures(&reference, table)?;
        let actual_foreign_keys = table_foreign_key_signatures(conn, table)?;
        if actual_foreign_keys != expected_foreign_keys {
            return Err(WcError::Other(format!(
                "current schema table {table} has invalid foreign keys"
            )));
        }

        let expected_unique_indexes = table_unique_index_signatures(&reference, table)?;
        let actual_unique_indexes = table_unique_index_signatures(conn, table)?;
        if actual_unique_indexes != expected_unique_indexes {
            return Err(WcError::Other(format!(
                "current schema table {table} has invalid unique indexes"
            )));
        }
    }

    let expected_triggers = trigger_signatures(&reference)?;
    let actual_triggers = trigger_signatures(conn)?;
    if actual_triggers != expected_triggers {
        return Err(WcError::Other(
            "current schema has invalid or unexpected trigger definitions".into(),
        ));
    }
    for index_name in REQUIRED_CURRENT_INDEXES {
        let expected = named_index_signature(&reference, index_name)?;
        let actual = named_index_signature(conn, index_name)?;
        if actual != expected {
            return Err(WcError::Other(format!(
                "current schema has a missing or invalid required index {index_name}"
            )));
        }
    }
    let expected_fts = schema_object_sql(&reference, "table", "wallpapers_fts")?;
    let actual_fts = schema_object_sql(conn, "table", "wallpapers_fts")?;
    if actual_fts != expected_fts {
        return Err(WcError::Other(
            "current schema has an invalid wallpapers_fts virtual table definition".into(),
        ));
    }
    Ok(())
}

/// Validate the non-table objects and write-time invariants required by the
/// current schema without replaying migrations or leaving test rows behind.
pub(crate) fn validate_current_schema_objects(conn: &Connection) -> Result<(), WcError> {
    validate_table_shapes_against_current_schema(conn)?;

    let (counter, path, source_path) = loop {
        let counter = SCHEMA_VALIDATION_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = format!(
            "/__wc_schema_validation_{}_{}.jpg",
            std::process::id(),
            counter
        );
        let source_path = format!(
            "/__wc_schema_validation_source_{}_{}",
            std::process::id(),
            counter
        );
        let collision = conn
            .query_row(
                "SELECT
                     EXISTS(SELECT 1 FROM wallpapers WHERE path IN (?1, ?1 || '-next')),
                     EXISTS(SELECT 1 FROM sources WHERE path IN (?2, ?2 || '-next'))",
                params![path, source_path],
                |row| Ok((row.get::<_, bool>(0)?, row.get::<_, bool>(1)?)),
            )
            .map_err(sqlite_err)?;
        if !collision.0 && !collision.1 {
            break (counter, path, source_path);
        }
    };
    let inserted_token = format!("wcschemainsert{}{}", std::process::id(), counter);
    let updated_token = format!("wcschemaupdate{}{}", std::process::id(), counter);
    let transaction = conn.unchecked_transaction().map_err(sqlite_err)?;
    let validation = (|| {
        let invalid_sources = transaction
            .query_row(
                "SELECT COUNT(*) FROM sources
                 WHERE kind NOT IN ('directory', 'wallpaper_engine_workshop')
                    OR recursive NOT IN (0, 1)
                    OR availability NOT IN ('unknown', 'available', 'offline')",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map_err(sqlite_err)?;
        if invalid_sources != 0 {
            return Err(WcError::Other(
                "current schema contains invalid typed source values".into(),
            ));
        }
        transaction
            .execute(
                "INSERT INTO sources
                 (path, display_name, kind, recursive, availability)
                 VALUES (?1, 'schema validation', 'directory', 1, 'unknown')",
                params![source_path],
            )
            .map_err(sqlite_err)?;
        let source_id = transaction.last_insert_rowid();
        for kind in ["directory", "wallpaper_engine_workshop"] {
            for recursive in [0_i64, 1_i64] {
                for availability in ["unknown", "available", "offline"] {
                    transaction
                        .execute(
                            "UPDATE sources
                             SET kind = ?2, recursive = ?3, availability = ?4
                             WHERE id = ?1",
                            params![source_id, kind, recursive, availability],
                        )
                        .map_err(|error| {
                            WcError::Other(format!(
                                "current schema rejects a supported source value combination: {error}"
                            ))
                        })?;
                }
            }
        }
        for (column, sql) in [
            (
                "kind",
                "UPDATE sources SET kind = '__invalid__' WHERE id = ?1",
            ),
            (
                "recursive",
                "UPDATE sources SET recursive = 2 WHERE id = ?1",
            ),
            (
                "availability",
                "UPDATE sources SET availability = '__invalid__' WHERE id = ?1",
            ),
        ] {
            if transaction.execute(sql, params![source_id]).is_ok() {
                return Err(WcError::Other(format!(
                    "current schema does not enforce the sources.{column} value domain"
                )));
            }
        }
        transaction
            .execute("DELETE FROM sources WHERE id = ?1", params![source_id])
            .map_err(sqlite_err)?;
        transaction
            .execute(
                "INSERT INTO sources
                 (path, display_name, kind, recursive, availability)
                 VALUES (?1 || '-next', 'schema validation', 'directory', 1, 'unknown')",
                params![source_path],
            )
            .map_err(sqlite_err)?;
        if transaction.last_insert_rowid() <= source_id {
            return Err(WcError::Other(
                "current schema does not preserve monotonic source identities".into(),
            ));
        }
        transaction
            .execute(
                "INSERT INTO wallpapers
                 (path, type, ext, backend, title)
                 VALUES (?1, 'image', 'jpg', 'awww', ?2)",
                params![path, inserted_token],
            )
            .map_err(sqlite_err)?;
        let wallpaper_id = transaction.last_insert_rowid();
        let (added_at, author, filename) = transaction
            .query_row(
                "SELECT added_at, author, filename FROM wallpapers WHERE id = ?1",
                params![wallpaper_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .map_err(sqlite_err)?;
        if added_at.is_empty() {
            return Err(WcError::Other(
                "current schema does not populate wallpapers.added_at".into(),
            ));
        }
        let expected_filename = path.rsplit('/').next().unwrap_or(&path);
        if !author.is_empty() || filename != expected_filename {
            return Err(WcError::Other(
                "current schema does not default author or derive filename".into(),
            ));
        }
        let inserted_fts = transaction
            .query_row(
                "SELECT COUNT(*) FROM wallpapers_fts WHERE wallpapers_fts MATCH ?1",
                params![inserted_token],
                |row| row.get::<_, i64>(0),
            )
            .map_err(sqlite_err)?;
        if inserted_fts != 1 {
            return Err(WcError::Other(
                "current schema does not index inserted wallpapers".into(),
            ));
        }
        if transaction
            .execute(
                "INSERT INTO wallpapers (path, type, ext, backend)
                 VALUES (?1, 'image', 'jpg', 'awww')",
                params![path],
            )
            .is_ok()
        {
            return Err(WcError::Other(
                "current schema does not enforce unique wallpaper paths".into(),
            ));
        }
        transaction
            .execute(
                "UPDATE wallpapers SET title = ?1 WHERE id = ?2",
                params![updated_token, wallpaper_id],
            )
            .map_err(sqlite_err)?;
        let old_fts = transaction
            .query_row(
                "SELECT COUNT(*) FROM wallpapers_fts WHERE wallpapers_fts MATCH ?1",
                params![inserted_token],
                |row| row.get::<_, i64>(0),
            )
            .map_err(sqlite_err)?;
        let updated_fts = transaction
            .query_row(
                "SELECT COUNT(*) FROM wallpapers_fts WHERE wallpapers_fts MATCH ?1",
                params![updated_token],
                |row| row.get::<_, i64>(0),
            )
            .map_err(sqlite_err)?;
        if old_fts != 0 || updated_fts != 1 {
            return Err(WcError::Other(
                "current schema does not update wallpaper search rows".into(),
            ));
        }
        transaction
            .execute(
                "DELETE FROM wallpapers WHERE id = ?1",
                params![wallpaper_id],
            )
            .map_err(sqlite_err)?;
        let deleted_fts = transaction
            .query_row(
                "SELECT COUNT(*) FROM wallpapers_fts WHERE wallpapers_fts MATCH ?1",
                params![updated_token],
                |row| row.get::<_, i64>(0),
            )
            .map_err(sqlite_err)?;
        if deleted_fts != 0 {
            return Err(WcError::Other(
                "current schema does not remove deleted wallpaper search rows".into(),
            ));
        }
        transaction
            .execute(
                "INSERT INTO wallpapers
                 (path, type, ext, backend)
                 VALUES (?1 || '-next', 'image', 'jpg', 'awww')",
                params![path],
            )
            .map_err(sqlite_err)?;
        if transaction.last_insert_rowid() <= wallpaper_id {
            return Err(WcError::Other(
                "current schema does not preserve monotonic wallpaper identities".into(),
            ));
        }
        Ok(())
    })();
    transaction.rollback().map_err(sqlite_err)?;
    validation
}

fn table_columns(
    conn: &Connection,
    table: &str,
) -> Result<std::collections::HashSet<String>, WcError> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_xinfo({table})"))
        .map_err(sqlite_err)?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(sqlite_err)?
        .collect::<Result<std::collections::HashSet<_>, _>>()
        .map_err(sqlite_err)?;
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
            .map_err(sqlite_err)?;
        }
    }

    let wallpaper_columns = table_columns(conn, "wallpapers")?;
    if !wallpaper_columns.contains("added_at") {
        // SQLite cannot add a column with a non-constant datetime default.
        conn.execute_batch("ALTER TABLE wallpapers ADD COLUMN added_at TEXT NOT NULL DEFAULT ''; ")
            .map_err(sqlite_err)?;
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
    .map_err(sqlite_err)?;
    Ok(())
}

fn ensure_v3_columns(conn: &Connection) -> Result<(), WcError> {
    let wallpaper_columns = table_columns(conn, "wallpapers")?;
    if !wallpaper_columns.contains("author") {
        conn.execute_batch("ALTER TABLE wallpapers ADD COLUMN author TEXT NOT NULL DEFAULT ''; ")
            .map_err(sqlite_err)?;
    }
    if !wallpaper_columns.contains("filename") {
        conn.execute_batch(
            "ALTER TABLE wallpapers ADD COLUMN filename TEXT GENERATED ALWAYS AS (
                 substr(path, length(rtrim(path, replace(path, '/', ''))) + 1)
             ) VIRTUAL;",
        )
        .map_err(sqlite_err)?;
    }
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
    .map_err(sqlite_err)?;
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
            .map_err(sqlite_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(sqlite_err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sqlite_err)?;
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
            .map_err(sqlite_err)?;
            conn.execute(
                "UPDATE wallpapers SET source_id = ?1 WHERE source_id = ?2",
                params![survivor_id, alias_id],
            )
            .map_err(sqlite_err)?;
            conn.execute(
                "DELETE FROM wallpaper_sources WHERE source_id = ?1",
                params![alias_id],
            )
            .map_err(sqlite_err)?;
            conn.execute("DELETE FROM sources WHERE id = ?1", params![alias_id])
                .map_err(sqlite_err)?;
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
        .map_err(sqlite_err)?;
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
    .map_err(sqlite_err)?;
    merge_wallpaper_aliases(conn)?;
    // Then add every component-aware containment membership for overlaps.
    backfill_containment_memberships(conn)?;
    conn.execute("UPDATE wallpapers SET source_id = NULL", [])
        .map_err(sqlite_err)?;
    Ok(())
}

fn merge_wallpaper_aliases(conn: &Connection) -> Result<(), WcError> {
    let wallpaper_rows = {
        let mut stmt = conn
            .prepare("SELECT id, path FROM wallpapers ORDER BY id")
            .map_err(sqlite_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(sqlite_err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sqlite_err)?;
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
            .map_err(sqlite_err)?;
            merge_wallpaper_metadata(conn, survivor_id, *alias_id)?;
            migrate_wallpaper_path_references(conn, alias_path, &canonical_path)?;
            conn.execute(
                "DELETE FROM wallpaper_sources WHERE wallpaper_id = ?1",
                params![alias_id],
            )
            .map_err(sqlite_err)?;
            conn.execute("DELETE FROM wallpapers WHERE id = ?1", params![alias_id])
                .map_err(sqlite_err)?;
        }
        migrate_wallpaper_path_references(conn, &rows[0].1, &canonical_path)?;
        conn.execute(
            "UPDATE wallpapers SET path = ?1 WHERE id = ?2",
            params![canonical_path, survivor_id],
        )
        .map_err(sqlite_err)?;
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
    .map_err(sqlite_err)?;
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
    .map_err(sqlite_err)?;
    conn.execute("DELETE FROM favorites WHERE path = ?1", params![old_path])
        .map_err(sqlite_err)?;
    conn.execute(
        "UPDATE history SET path = ?1 WHERE path = ?2",
        params![canonical_path, old_path],
    )
    .map_err(sqlite_err)?;
    conn.execute(
        "UPDATE state SET value = ?1 WHERE key = 'current' AND value = ?2",
        params![canonical_path, old_path],
    )
    .map_err(sqlite_err)?;
    let display_state_exists = conn
        .query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM sqlite_master
                 WHERE type = 'table' AND name = 'display_state'
             )",
            [],
            |row| row.get::<_, bool>(0),
        )
        .map_err(sqlite_err)?;
    if display_state_exists {
        conn.execute(
            "UPDATE display_state SET wallpaper_path = ?1 WHERE wallpaper_path = ?2",
            params![canonical_path, old_path],
        )
        .map_err(sqlite_err)?;
    }
    Ok(())
}

fn backfill_containment_memberships(conn: &Connection) -> Result<(), WcError> {
    let sources = {
        let mut stmt = conn
            .prepare("SELECT id, path FROM sources ORDER BY id")
            .map_err(sqlite_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(sqlite_err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sqlite_err)?;
        rows
    };
    let wallpapers = {
        let mut stmt = conn
            .prepare("SELECT id, path, last_seen FROM wallpapers ORDER BY id")
            .map_err(sqlite_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(sqlite_err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sqlite_err)?;
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
                .map_err(sqlite_err)?;
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
        .map_err(sqlite_err)?;
        conn.execute(
            "INSERT OR REPLACE INTO db_meta (key, value) VALUES ('fts_rebuilt_at', datetime('now'))",
            [],
        )
        .map_err(sqlite_err)?;
    }
    Ok(())
}

pub fn wallpapers_count(conn: &Connection) -> Result<i64, WcError> {
    conn.query_row("SELECT COUNT(*) FROM wallpapers", [], |row| row.get(0))
        .map_err(sqlite_err)
}

pub fn wallpapers_fts_count(conn: &Connection) -> Result<i64, WcError> {
    conn.query_row("SELECT COUNT(*) FROM wallpapers_fts", [], |row| row.get(0))
        .map_err(sqlite_err)
}

pub fn check_wallpapers_fts_integrity(conn: &Connection) -> Result<(), WcError> {
    conn.execute(
        "INSERT INTO wallpapers_fts(wallpapers_fts, rank) VALUES ('integrity-check', 1)",
        [],
    )
    .map(|_| ())
    .map_err(sqlite_err)
}

pub fn rebuild_wallpapers_fts(conn: &Connection) -> Result<(), WcError> {
    conn.execute(
        "INSERT INTO wallpapers_fts(wallpapers_fts) VALUES ('rebuild')",
        [],
    )
    .map_err(sqlite_err)?;
    Ok(())
}

/// Add project metadata columns to older wallpapers tables.
pub fn ensure_wallpaper_metadata_columns(conn: &Connection) -> Result<(), WcError> {
    let mut stmt = conn
        .prepare("PRAGMA table_info(wallpapers)")
        .map_err(sqlite_err)?;
    let columns: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(sqlite_err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sqlite_err)?;
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
            .map_err(sqlite_err)?;
        }
    }
    Ok(())
}

/// Ensure existing databases have the indexes needed by paged GUI queries.
pub fn ensure_wallpaper_query_indexes(conn: &Connection) -> Result<(), WcError> {
    ensure_wallpaper_metadata_columns(conn)?;
    conn.execute_batch(WALLPAPER_QUERY_INDEXES_SQL)
        .map_err(sqlite_err)?;
    Ok(())
}

/// Migrate flat files into wallpapers.db (one-shot operation).
pub fn migrate_to_sqlite(cd: &ConfigDir) -> Result<(), WcError> {
    let _maintenance = acquire_maintenance_lock(cd)?;
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
    let conn = Connection::open(temp_path).map_err(sqlite_err)?;
    create_schema(&conn)?;
    let now = super::chrono_now();
    let tx = conn.unchecked_transaction().map_err(sqlite_err)?;
    {
        let conn: &Connection = &tx;

        // Config
        let config_map = wc_config::parse_config_file(&cd.config_path())?;
        for (key, value) in &config_map {
            conn.execute(
                "INSERT OR REPLACE INTO config (key, value) VALUES (?1, ?2)",
                params![key, value],
            )
            .map_err(sqlite_err)?;
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
            .map_err(sqlite_err)?;
        }

        // Favorites
        for path in flat::favorites_list(cd)? {
            conn.execute(
                "INSERT OR IGNORE INTO favorites (path, added_at) VALUES (?1, ?2)",
                params![path, now],
            )
            .map_err(sqlite_err)?;
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
            .map_err(sqlite_err)?;
        }

        // State
        if let Some(cur) = flat::current_read(cd)? {
            conn.execute(
                "INSERT OR REPLACE INTO state (key, value) VALUES ('current', ?1)",
                params![cur],
            )
            .map_err(sqlite_err)?;
        }
        if let Some(be) = flat::last_backend_read(cd)? {
            conn.execute(
                "INSERT OR REPLACE INTO state (key, value) VALUES ('last_backend', ?1)",
                params![be],
            )
            .map_err(sqlite_err)?;
        }

        // Meta
        conn.execute(
            "INSERT OR REPLACE INTO db_meta (key, value) VALUES ('schema_version', ?1)",
            params![CURRENT_SCHEMA_VERSION.to_string()],
        )
        .map_err(sqlite_err)?;
        conn.execute(
            "INSERT OR REPLACE INTO db_meta (key, value) VALUES ('migrated_at', ?1)",
            params![now],
        )
        .map_err(sqlite_err)?;
        conn.execute(
            "INSERT OR REPLACE INTO db_meta (key, value) VALUES ('source_runtime_dir', ?1)",
            params![cd.path.to_string_lossy().as_ref()],
        )
        .map_err(sqlite_err)?;

        // Wallpapers — import from library.tsv if present. This helper writes
        // through the caller's transaction and must not start a nested one.
        super::import_library_tsv_into(conn, cd)?;
        backfill_containment_memberships(conn)?;
    }
    tx.commit().map_err(sqlite_err)?;

    // Publish only a self-contained main database. Switching out of WAL after
    // a checkpoint removes any dependency on temp-path sidecars before close.
    conn.execute_batch(
        "PRAGMA wal_checkpoint(TRUNCATE);
         PRAGMA journal_mode = DELETE;",
    )
    .map_err(sqlite_err)?;
    drop(conn);

    Ok(())
}

/// Fallible bootstrap: open/create the DB, apply the full schema, and set
/// runtime PRAGMAs. Surfaces schema/migration errors to callers that can
/// propagate them (for example [`crate::StorageApi::try_new`] and display-state
/// ConfigDir helpers).
pub fn try_ensure_sqlite_db(cd: &ConfigDir) -> Result<(), WcError> {
    try_ensure_sqlite_db_with_seam(cd, || {})
}

fn try_ensure_sqlite_db_with_seam(
    cd: &ConfigDir,
    before_create_schema: impl FnOnce(),
) -> Result<(), WcError> {
    // Same-version fast path: if the DB file already exists, probe its
    // version under a shared schema lock first. Only fall through to
    // exclusive when the schema is older or the probe fails for a
    // non-SchemaTooNew reason.
    if cd.db_path().exists() {
        match try_warm_ensure_sqlite_db(cd) {
            Ok(true) => return Ok(()),
            // `false` is the only signal that an older schema needs the
            // exclusive migration path. Lock/IO/SQLite failures are not
            // evidence of an old schema and must remain bounded.
            Ok(false) => {
                invalidate_cached_connections();
            }
            Err(error) => return Err(error),
        }
    }
    // Fresh database or older schema requiring migration: acquire exclusive
    // schema lock and bootstrap/migrate.
    let conn = open_or_create_connection(cd)?;
    before_create_schema();
    create_schema(&conn)?;
    apply_runtime_pragmas(&conn)?;
    Ok(())
}

/// Ensure wallpapers.db exists with the full schema.
/// No-op if the file already exists. Failures are logged as warnings so
/// callers are not blocked by bootstrap failures.
pub fn ensure_sqlite_db(cd: &ConfigDir) {
    if let Err(err) = try_ensure_sqlite_db(cd) {
        log::warn!("ensure_sqlite_db failed: {err}");
    }
}

/// Ensure SQLite exists, importing legacy flat files only when the DB is absent.
/// Returns `true` if legacy flat files were imported into a newly created DB,
/// `false` if the DB already existed and was only ensured/repaired.
pub fn ensure_or_import_legacy_flat(cd: &ConfigDir) -> Result<bool, WcError> {
    ensure_or_import_legacy_flat_with_seam(cd, || {})
}

fn ensure_or_import_legacy_flat_with_seam(
    cd: &ConfigDir,
    before_migrate: impl FnOnce(),
) -> Result<bool, WcError> {
    if cd.db_path().exists() {
        // Same-version fast path: inspect the schema version without
        // acquiring an exclusive schema lock. Only enter the full
        // migration path when the DB needs an upgrade.
        match try_warm_ensure_sqlite_db(cd) {
            Ok(true) => return Ok(false),
            Ok(false) => {}
            // A failed shared warm probe must not be retried through the
            // exclusive migration path. In particular this avoids turning a
            // two-second contention timeout into a roughly nine-second one.
            Err(error) => return Err(error),
        }
        // DB exists but the fast path rejected it (wrong version or
        // validation failure). Fall through to the full migration path
        // which acquires exclusive schema lock and repairs/migrates.
        try_ensure_sqlite_db(cd)?;
        return Ok(false);
    }
    before_migrate();
    match migrate_to_sqlite(cd) {
        Ok(()) => {
            try_ensure_sqlite_db(cd)?;
            Ok(true)
        }
        Err(_) if cd.db_path().exists() => {
            // Another normal startup may have published a complete database
            // while this caller waited for the exclusive migration lock.
            try_ensure_sqlite_db(cd)?;
            Ok(false)
        }
        Err(error) => Err(error),
    }
}

/// Inexpensive warm-start check: opens the DB with shared locks only,
/// verifies the schema version is current, applies runtime PRAGMAs, and
/// returns `Ok(true)` when no migration work is required. Returns
/// `Ok(false)` or an error when the DB needs the full migration path.
fn try_warm_ensure_sqlite_db(cd: &ConfigDir) -> Result<bool, WcError> {
    use super::connection::open_runtime_connection as open_runtime;
    let conn = open_runtime(cd, CURRENT_SCHEMA_VERSION)?;
    let version = conn
        .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
        .map_err(sqlite_err)?;
    if version == CURRENT_SCHEMA_VERSION {
        apply_runtime_pragmas(&conn)?;
        Ok(true)
    } else {
        // Older schema — drop the shared lock so the caller can acquire
        // exclusive and migrate.
        drop(conn);
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source_refresh_state_table_exists(conn: &Connection) -> bool {
        conn.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM sqlite_master
                 WHERE type = 'table' AND name = 'source_refresh_state'
             )",
            [],
            |row| row.get::<_, bool>(0),
        )
        .unwrap()
    }

    #[test]
    fn fresh_v6_schema_initializes_revision_refresh_and_derived_search_state() {
        let conn = Connection::open_in_memory().unwrap();

        create_schema(&conn).unwrap();

        assert_eq!(CURRENT_SCHEMA_VERSION, 6);
        assert_eq!(crate::sqlite::read_library_revision(&conn).unwrap(), 0);
        assert!(source_refresh_state_table_exists(&conn));
        assert_eq!(
            conn.query_row(
                "SELECT status FROM library_fts_state WHERE singleton = 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
            "pending"
        );
    }

    #[test]
    fn v3_migration_preserves_data_and_initializes_library_revision() {
        let conn = Connection::open_in_memory().unwrap();
        create_v3_schema(&conn);
        conn.execute(
            "INSERT INTO favorites (path) VALUES ('/walls/kept.jpg')",
            [],
        )
        .unwrap();

        create_schema(&conn).unwrap();

        assert_eq!(
            conn.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            CURRENT_SCHEMA_VERSION
        );
        assert_eq!(crate::sqlite::read_library_revision(&conn).unwrap(), 0);
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM favorites WHERE path = '/walls/kept.jpg'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            1
        );
    }

    #[test]
    fn v4_migration_adds_source_refresh_state_without_changing_library_revision() {
        let conn = Connection::open_in_memory().unwrap();
        create_schema(&conn).unwrap();
        conn.execute("DROP TABLE source_refresh_state", []).unwrap();
        conn.execute(
            "UPDATE db_meta SET value = '41' WHERE key = 'library_revision'",
            [],
        )
        .unwrap();
        conn.pragma_update(None, "user_version", 4).unwrap();
        conn.execute(
            "UPDATE db_meta SET value = '4' WHERE key = 'schema_version'",
            [],
        )
        .unwrap();

        create_schema(&conn).unwrap();

        assert_eq!(
            conn.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            CURRENT_SCHEMA_VERSION
        );
        assert!(source_refresh_state_table_exists(&conn));
        assert_eq!(crate::sqlite::read_library_revision(&conn).unwrap(), 41);
    }

    fn create_v3_schema(conn: &Connection) {
        create_schema(conn).unwrap();
        conn.execute("DELETE FROM db_meta WHERE key = 'library_revision'", [])
            .unwrap();
        conn.pragma_update(None, "user_version", 3).unwrap();
        conn.execute(
            "UPDATE db_meta SET value = '3' WHERE key = 'schema_version'",
            [],
        )
        .unwrap();
    }

    #[test]
    fn schema_normalization_ignores_formatting_and_comments_but_not_constraint_tokens() {
        let canonical = "CREATE TABLE sample (type TEXT CHECK (type IN ('image', 'video')))";
        let formatted =
            "create table sample(type text CHECK/**/(type IN ('image','video'))) -- harmless\n";
        let narrowed = "create table sample(type text CHECK/**/(type = 'image'))";

        assert_eq!(
            normalize_schema_fragment(canonical),
            normalize_schema_fragment(formatted)
        );
        assert_ne!(
            normalize_schema_fragment(canonical),
            normalize_schema_fragment(narrowed)
        );
    }

    #[test]
    fn runtime_open_rejects_future_schema_before_changing_journal_mode() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        let future_version = CURRENT_SCHEMA_VERSION + 1;
        let conn = Connection::open(cd.db_path()).unwrap();
        create_schema(&conn).unwrap();
        conn.execute_batch("PRAGMA journal_mode = DELETE;").unwrap();
        conn.pragma_update(None, "user_version", future_version)
            .unwrap();
        drop(conn);

        let error = open_runtime_connection(&cd).err();

        let conn = Connection::open(cd.db_path()).unwrap();
        let version = conn
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap();
        let journal_mode = conn
            .pragma_query_value(None, "journal_mode", |row| row.get::<_, String>(0))
            .unwrap();
        let error = error.expect("direct runtime opener must reject a future schema");
        assert!(
            error.to_string().contains("newer") || error.to_string().contains("version"),
            "{error}"
        );
        assert_eq!(version, future_version);
        assert_eq!(journal_mode, "delete");
    }

    #[test]
    fn runtime_open_rechecks_version_after_waiting_for_schema_upgrade() {
        use std::sync::mpsc::{self, RecvTimeoutError};
        use std::time::Duration;

        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("wallpaper-console");
        let cd = ConfigDir { path: path.clone() };
        cd.init().unwrap();
        try_ensure_sqlite_db(&cd).unwrap();
        let future_version = CURRENT_SCHEMA_VERSION + 1;

        let upgrader_cd = ConfigDir { path: path.clone() };
        let (upgraded_tx, upgraded_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let upgrader = std::thread::spawn(move || -> Result<(), WcError> {
            let conn = open_or_create_connection(&upgrader_cd)?;
            conn.pragma_update(None, "user_version", future_version)
                .map_err(sqlite_err)?;
            upgraded_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Ok(())
        });
        upgraded_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("schema upgrader must hold the exclusive lock after bumping the version");

        let runtime_cd = ConfigDir { path };
        let (outcome_tx, outcome_rx) = mpsc::channel();
        let runtime = std::thread::spawn(move || {
            let outcome = open_runtime_connection(&runtime_cd)
                .map(drop)
                .map_err(|error| error.to_string());
            outcome_tx.send(outcome).unwrap();
        });

        let early = outcome_rx.recv_timeout(Duration::from_millis(150));
        let was_blocked = matches!(&early, Err(RecvTimeoutError::Timeout));
        release_tx.send(()).unwrap();
        upgrader.join().unwrap().unwrap();
        let outcome = match early {
            Ok(outcome) => outcome,
            Err(RecvTimeoutError::Timeout) => outcome_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("runtime opener must resume after the schema upgrade unlocks"),
            Err(RecvTimeoutError::Disconnected) => panic!("runtime opener disconnected"),
        };
        runtime.join().unwrap();

        assert!(
            was_blocked,
            "runtime opener must wait for the schema-exclusive upgrader"
        );
        let error = outcome.expect_err("runtime opener must reject the upgraded future schema");
        assert!(
            error.contains("newer") || error.contains("version"),
            "{error}"
        );
        let conn = Connection::open(cd.db_path()).unwrap();
        assert_eq!(
            conn.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            future_version
        );
    }

    #[test]
    fn runtime_open_waits_for_bootstrap_and_only_observes_complete_schema() {
        use std::sync::mpsc::{self, RecvTimeoutError};
        use std::time::Duration;

        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("wallpaper-console");
        let cd = ConfigDir { path: path.clone() };
        cd.init().unwrap();

        let bootstrap_cd = ConfigDir { path: path.clone() };
        let (opened_tx, opened_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let bootstrap = std::thread::spawn(move || -> Result<(), WcError> {
            try_ensure_sqlite_db_with_seam(&bootstrap_cd, || {
                opened_tx.send(()).unwrap();
                release_rx.recv().unwrap();
            })
        });

        opened_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("bootstrap must pause after opening the main database");
        assert!(cd.db_path().exists());

        let runtime_cd = ConfigDir { path };
        let (observed_tx, observed_rx) = mpsc::channel();
        let runtime = std::thread::spawn(move || -> Result<(), WcError> {
            let conn = open_runtime_connection(&runtime_cd)?;
            let version = conn
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .map_err(sqlite_err)?;
            let core_tables: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master
                     WHERE type = 'table'
                       AND name IN ('config', 'sources', 'wallpapers', 'wallpaper_sources', 'state')",
                    [],
                    |row| row.get(0),
                )
                .map_err(sqlite_err)?;
            observed_tx.send((version, core_tables)).unwrap();
            Ok(())
        });

        let early = observed_rx.recv_timeout(Duration::from_millis(150));
        let was_blocked = matches!(&early, Err(RecvTimeoutError::Timeout));
        release_tx.send(()).unwrap();
        bootstrap.join().unwrap().unwrap();
        let observed = match early {
            Ok(observed) => observed,
            Err(RecvTimeoutError::Timeout) => observed_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("runtime open must resume after bootstrap publishes the schema"),
            Err(RecvTimeoutError::Disconnected) => panic!("runtime opener disconnected"),
        };
        runtime.join().unwrap().unwrap();

        assert!(
            was_blocked,
            "runtime open must wait while bootstrap holds the exclusive schema lock"
        );
        assert_eq!(observed, (CURRENT_SCHEMA_VERSION, 5));
    }

    #[test]
    fn concurrent_ensure_or_import_treats_the_winning_migration_as_normal_startup() {
        use std::sync::{Arc, Barrier};

        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("wallpaper-console");
        let cd = ConfigDir { path: path.clone() };
        cd.init().unwrap();
        let barrier = Arc::new(Barrier::new(2));

        let handles: Vec<_> = (0..2)
            .map(|_| {
                let path = path.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let cd = ConfigDir { path };
                    ensure_or_import_legacy_flat_with_seam(&cd, || {
                        barrier.wait();
                    })
                })
            })
            .collect();
        let results: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();

        assert!(
            results.iter().all(Result::is_ok),
            "both normal startups must succeed: {results:?}"
        );
        let mut imported: Vec<bool> = results.into_iter().map(Result::unwrap).collect();
        imported.sort_unstable();
        assert_eq!(imported, vec![false, true]);
        try_ensure_sqlite_db(&cd).unwrap();
    }

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

    fn create_v2_schema(conn: &Connection) {
        conn.execute_batch(
            "PRAGMA user_version = 2;
             CREATE TABLE sources (
                 id           INTEGER PRIMARY KEY AUTOINCREMENT,
                 path         TEXT NOT NULL UNIQUE,
                 added_at     TEXT NOT NULL DEFAULT (datetime('now')),
                 display_name TEXT NOT NULL DEFAULT '',
                 kind         TEXT NOT NULL DEFAULT 'directory'
                              CHECK (kind IN ('directory', 'wallpaper_engine_workshop')),
                 recursive    INTEGER NOT NULL DEFAULT 1
                              CHECK (recursive IN (0, 1)),
                 availability TEXT NOT NULL DEFAULT 'unknown'
                              CHECK (availability IN ('unknown', 'available', 'offline'))
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
                 last_seen TEXT NOT NULL DEFAULT (datetime('now')),
                 added_at TEXT NOT NULL DEFAULT (datetime('now'))
             );
             CREATE TABLE wallpaper_sources (
                 wallpaper_id INTEGER NOT NULL REFERENCES wallpapers(id) ON DELETE CASCADE,
                 source_id INTEGER NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
                 last_seen_at TEXT NOT NULL DEFAULT (datetime('now')),
                 PRIMARY KEY (wallpaper_id, source_id)
             );
             CREATE TABLE favorites (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 path TEXT NOT NULL UNIQUE,
                 added_at TEXT NOT NULL DEFAULT (datetime('now'))
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
    fn fresh_schema_has_author_and_generated_filename() {
        let conn = Connection::open_in_memory().unwrap();

        create_schema(&conn).unwrap();

        assert_eq!(
            conn.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            CURRENT_SCHEMA_VERSION
        );
        let columns = {
            let mut stmt = conn.prepare("PRAGMA table_xinfo(wallpapers)").unwrap();
            stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, i64>(6)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
        };
        assert!(columns.contains(&(
            "author".to_string(),
            "TEXT".to_string(),
            1,
            Some("''".to_string()),
            0,
        )));
        assert!(columns.contains(&("filename".to_string(), "TEXT".to_string(), 0, None, 2,)));

        conn.execute(
            "INSERT INTO wallpapers (path, type, ext, backend)
             VALUES ('/one/first.jpg', 'image', 'jpg', 'awww')",
            [],
        )
        .unwrap();
        let inserted: (String, String) = conn
            .query_row("SELECT author, filename FROM wallpapers", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap();
        assert_eq!(inserted, (String::new(), "first.jpg".to_string()));

        conn.execute("UPDATE wallpapers SET path = '/two/renamed.png'", [])
            .unwrap();
        assert_eq!(
            conn.query_row("SELECT filename FROM wallpapers", [], |row| {
                row.get::<_, String>(0)
            })
            .unwrap(),
            "renamed.png"
        );
    }

    #[test]
    fn migrates_v1_to_v3_without_changing_source_or_wallpaper_identity() {
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
            CURRENT_SCHEMA_VERSION
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
        let wallpaper_row: (i64, String, Option<i64>, String, String, String) = conn
            .query_row(
                "SELECT id, path, source_id, added_at, author, filename FROM wallpapers",
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
        assert_eq!(wallpaper_row.0, 11);
        assert_eq!(wallpaper_row.1, wallpaper.to_string_lossy());
        assert_eq!(wallpaper_row.2, None);
        assert!(!wallpaper_row.3.is_empty());
        assert_eq!(wallpaper_row.4, "");
        assert_eq!(wallpaper_row.5, "a.jpg");
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
    fn migrates_v2_to_v3_without_replaying_v1_source_migration() {
        let conn = Connection::open_in_memory().unwrap();
        create_v2_schema(&conn);
        conn.execute(
            "INSERT INTO sources
             (id, path, display_name, kind, recursive, availability, added_at)
             VALUES (7, '/walls', 'My walls', 'directory', 0, 'offline', '2025-01-01')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO wallpapers
             (id, path, type, ext, backend, source_id, added_at)
             VALUES (11, '/walls/kept.jpg', 'image', 'jpg', 'awww', NULL, '2025-02-03')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO wallpaper_sources (wallpaper_id, source_id, last_seen_at)
             VALUES (11, 7, '2025-02-04')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO favorites (id, path, added_at)
             VALUES (13, '/walls/kept.jpg', '2025-02-05')",
            [],
        )
        .unwrap();

        create_schema(&conn).unwrap();

        assert_eq!(
            conn.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            CURRENT_SCHEMA_VERSION
        );
        let source: (i64, String, i64, String) = conn
            .query_row(
                "SELECT id, display_name, recursive, availability FROM sources",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(source, (7, "My walls".into(), 0, "offline".into()));
        let wallpaper: (i64, String, String, String) = conn
            .query_row(
                "SELECT id, added_at, author, filename FROM wallpapers",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            wallpaper,
            (11, "2025-02-03".into(), String::new(), "kept.jpg".into())
        );
        assert_eq!(
            conn.query_row(
                "SELECT wallpaper_id, source_id, last_seen_at FROM wallpaper_sources",
                [],
                |row| Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?
                )),
            )
            .unwrap(),
            (11, 7, "2025-02-04".into())
        );
        assert_eq!(
            conn.query_row("SELECT id, path, added_at FROM favorites", [], |row| Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?
            )),)
                .unwrap(),
            (13, "/walls/kept.jpg".into(), "2025-02-05".into())
        );
    }

    #[test]
    fn rejects_future_schema_without_modifying_it() {
        let conn = Connection::open_in_memory().unwrap();
        let future_version = CURRENT_SCHEMA_VERSION + 1;
        conn.pragma_update(None, "user_version", future_version)
            .unwrap();

        let err = create_schema(&conn).unwrap_err();

        assert!(err.to_string().contains("newer") || err.to_string().contains("version"));
        assert_eq!(
            conn.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            future_version
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
    fn current_migration_is_idempotent() {
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
    fn current_v3_schema_repairs_a_stale_db_meta_marker_without_replaying_migration() {
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
        let wallpaper_columns = table_columns(&conn, "wallpapers").unwrap();
        assert!(!wallpaper_columns.contains("author"));
        assert!(!wallpaper_columns.contains("filename"));
    }

    #[test]
    fn migrate_to_sqlite_preserves_apostrophes_in_bound_values() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        wc_config::write_config_value(&cd.path, "custom_name", "artist's value").unwrap();
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

    #[test]
    fn ensure_or_import_legacy_flat_returns_schema_too_new_without_acquiring_exclusive_lock() {
        use std::sync::mpsc;
        use std::time::Duration;

        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        let future_version = CURRENT_SCHEMA_VERSION + 1;
        let conn = Connection::open(cd.db_path()).unwrap();
        create_schema(&conn).unwrap();
        conn.pragma_update(None, "user_version", future_version)
            .unwrap();
        drop(conn);

        // Hold a shared schema lock to prove ensure_or_import_legacy_flat
        // does NOT block waiting for exclusive access.
        let blocker_cd = ConfigDir {
            path: cd.path.clone(),
        };
        let (ready_tx, ready_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let blocker = std::thread::spawn(move || {
            let guard = crate::sqlite::connection::acquire_schema_shared_lock(&blocker_cd).unwrap();
            ready_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            drop(guard);
        });
        ready_rx.recv().unwrap();

        let started = std::time::Instant::now();
        let result = ensure_or_import_legacy_flat(&cd);
        let elapsed = started.elapsed();

        release_tx.send(()).unwrap();
        blocker.join().unwrap();

        match result {
            Err(WcError::SchemaTooNew {
                supported,
                observed,
            }) => {
                assert_eq!(supported, CURRENT_SCHEMA_VERSION);
                assert_eq!(observed, future_version);
            }
            other => panic!(
                "expected SchemaTooNew, got {:?}",
                other.as_ref().map(|_| &())
            ),
        }
        assert!(
            elapsed < Duration::from_millis(500),
            "SchemaTooNew must return immediately (took {elapsed:?}), not block on exclusive lock"
        );

        // Database must be unchanged.
        let conn = Connection::open(cd.db_path()).unwrap();
        let version = conn
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap();
        assert_eq!(version, future_version);
    }

    #[test]
    fn try_new_returns_schema_too_new_without_writing_config() {
        use std::time::Duration;

        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        let future_version = CURRENT_SCHEMA_VERSION + 1;
        let conn = Connection::open(cd.db_path()).unwrap();
        create_schema(&conn).unwrap();
        conn.pragma_update(None, "user_version", future_version)
            .unwrap();
        drop(conn);

        let path = cd.path.clone();
        let started = std::time::Instant::now();
        let result = crate::StorageApi::try_new(ConfigDir { path });
        let elapsed = started.elapsed();

        match result {
            Err(WcError::SchemaTooNew {
                supported,
                observed,
            }) => {
                assert_eq!(supported, CURRENT_SCHEMA_VERSION);
                assert_eq!(observed, future_version);
            }
            other => panic!(
                "expected SchemaTooNew, got {:?}",
                other.as_ref().map(|_| &())
            ),
        }
        assert!(
            elapsed < Duration::from_secs(1),
            "try_new must return SchemaTooNew quickly (took {elapsed:?})"
        );

        // Database version must be unchanged.
        let conn = Connection::open(cd.db_path()).unwrap();
        let version = conn
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap();
        assert_eq!(version, future_version);
    }

    // ── Same-version fast path tests ─────────────────────────────────────

    #[test]
    fn same_version_try_ensure_uses_shared_lock_not_exclusive() {
        use std::sync::mpsc;
        use std::time::{Duration, Instant};

        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        try_ensure_sqlite_db(&cd).unwrap();

        // Hold a shared schema lock from a separate thread.
        let blocker_cd = ConfigDir {
            path: cd.path.clone(),
        };
        let (ready_tx, ready_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let blocker = std::thread::spawn(move || {
            let guard = crate::sqlite::connection::acquire_schema_shared_lock(&blocker_cd).unwrap();
            ready_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            drop(guard);
        });
        ready_rx.recv().unwrap();

        // Same-version try_ensure_sqlite_db must use the fast path (shared
        // schema lock) and complete quickly without blocking on exclusive.
        let started = Instant::now();
        try_ensure_sqlite_db(&cd).unwrap();
        let elapsed = started.elapsed();

        release_tx.send(()).unwrap();
        blocker.join().unwrap();

        assert!(
            elapsed < Duration::from_millis(500),
            "same-version try_ensure must use shared lock (fast path), took {elapsed:?}"
        );
    }

    #[test]
    fn same_version_write_paths_succeed_under_shared_schema_lock() {
        use std::sync::mpsc;
        use std::time::{Duration, Instant};

        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        try_ensure_sqlite_db(&cd).unwrap();

        // Hold a shared schema lock from a separate thread.
        let blocker_cd = ConfigDir {
            path: cd.path.clone(),
        };
        let (ready_tx, ready_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let blocker = std::thread::spawn(move || {
            let guard = crate::sqlite::connection::acquire_schema_shared_lock(&blocker_cd).unwrap();
            ready_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            drop(guard);
        });
        ready_rx.recv().unwrap();

        // Each write path calls try_ensure_sqlite_db internally — after the
        // fast-path fix, none should request exclusive schema lock.
        let started = Instant::now();
        crate::sqlite::sqlite_config_set(&cd, "test_key", "test_value").unwrap();
        crate::sqlite::sqlite_favorite_add(&cd, "/walls/fast.jpg").unwrap();
        let elapsed = started.elapsed();

        release_tx.send(()).unwrap();
        blocker.join().unwrap();

        assert!(
            elapsed < Duration::from_millis(500),
            "same-version writes must use shared lock, took {elapsed:?}"
        );

        // Verify the writes actually persisted.
        let conn = open_runtime_connection(&cd).unwrap();
        let config_val: String = conn
            .query_row(
                "SELECT value FROM config WHERE key = 'test_key'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(config_val, "test_value");
        let fav_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM favorites WHERE path = '/walls/fast.jpg'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(fav_count, 1);
    }

    #[test]
    fn older_schema_migration_acquires_exclusive_and_completes_quickly() {
        use std::time::{Duration, Instant};

        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();

        // Create a v1-schema database.
        {
            let conn = Connection::open(cd.db_path()).unwrap();
            create_v1_schema(&conn);
            conn.execute("INSERT INTO sources (id, path) VALUES (1, '/walls')", [])
                .unwrap();
            insert_v1_wallpaper(&conn, "/walls/a.jpg", Some(1));
        }

        let started = Instant::now();
        try_ensure_sqlite_db(&cd).unwrap();
        let elapsed = started.elapsed();

        assert!(
            elapsed < Duration::from_secs(1),
            "older schema migration must complete <1s, took {elapsed:?}"
        );

        // Verify migration actually happened.
        let conn = Connection::open(cd.db_path()).unwrap();
        let version = conn
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap();
        assert_eq!(version, CURRENT_SCHEMA_VERSION);
    }

    #[test]
    fn exclusive_maintenance_blocks_ordinary_runtime_open_with_typed_timeout() {
        use std::time::{Duration, Instant};

        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        try_ensure_sqlite_db(&cd).unwrap();

        // Hold exclusive maintenance lock for the duration of this test.
        let _guard = crate::sqlite::connection::acquire_maintenance_lock(&cd).unwrap();

        // Ordinary runtime open should time out with typed LockTimeout.
        let started = Instant::now();
        let result = open_runtime_connection(&cd);
        let elapsed = started.elapsed();

        match result {
            Err(WcError::LockTimeout {
                category,
                operation,
                stage,
                waited,
                deadline,
            }) => {
                assert_eq!(category, wc_core::error::LockCategory::Maintenance);
                assert_eq!(operation, wc_core::error::LockOperation::Shared);
                assert!(
                    stage.contains("runtime"),
                    "stage should mention runtime, got: {stage}"
                );
                assert!(
                    waited >= Duration::from_millis(1500),
                    "should wait ~2s, waited {waited:?}"
                );
                assert!(
                    deadline >= Duration::from_secs(2),
                    "deadline should be ~2s, got {deadline:?}"
                );
            }
            other => panic!(
                "expected LockTimeout after ~2s, got {:?} after {elapsed:?}",
                other.as_ref().map(|_| &())
            ),
        }
        assert!(
            elapsed >= Duration::from_millis(1500),
            "should have waited ~2s, only took {elapsed:?}"
        );
    }

    #[test]
    fn warm_ensure_propagates_maintenance_timeout_without_exclusive_retry() {
        use std::time::{Duration, Instant};

        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        try_ensure_sqlite_db(&cd).unwrap();
        let _guard = crate::sqlite::connection::acquire_maintenance_lock(&cd).unwrap();

        for operation in [
            try_ensure_sqlite_db as fn(&ConfigDir) -> Result<(), WcError>,
            |cd| ensure_or_import_legacy_flat(cd).map(|_| ()),
        ] {
            let started = Instant::now();
            let error = operation(&cd).unwrap_err();
            let elapsed = started.elapsed();
            assert!(
                matches!(
                    error,
                    WcError::LockTimeout {
                        category: wc_core::error::LockCategory::Maintenance,
                        operation: wc_core::error::LockOperation::Shared,
                        ..
                    }
                ),
                "unexpected warm-probe error: {error}"
            );
            assert!(
                elapsed >= Duration::from_millis(1_500),
                "returned too early: {elapsed:?}"
            );
            assert!(
                elapsed < Duration::from_secs(3),
                "warm probe appears to have retried through the exclusive path: {elapsed:?}"
            );
        }
    }
}
