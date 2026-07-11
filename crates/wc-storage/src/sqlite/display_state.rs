//! Per-display wallpaper state persistence.
//!
//! Stores one row per display target (named output or the reserved All Displays
//! target). Legacy `state.current` + `state.last_backend` migrate into All
//! Displays only when both keys are present and valid; legacy keys are kept.
//! A durable `db_meta` marker records that the one-time migration completed so
//! retained legacy keys cannot recreate All Displays after delete/replace.

use std::collections::HashSet;

use rusqlite::{params, Connection, OptionalExtension};
use wc_core::config::ConfigDir;
use wc_core::error::WcError;

use super::schema::{open_runtime_connection, try_ensure_sqlite_db};

/// Reserved storage key for the explicit All Displays target.
pub const ALL_DISPLAYS_TARGET_KEY: &str = "__all_displays__";

/// `db_meta` key marking that the one-time legacy display-state migration ran.
pub const LEGACY_DISPLAY_STATE_MIGRATED_META_KEY: &str = "legacy_display_state_migrated";

/// Identity of a persisted display wallpaper assignment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisplayStateTarget {
    /// Explicit all-display operation; never inferred from a named output.
    AllDisplays,
    /// Concrete Wayland/X output name (for example `eDP-1`).
    Output(String),
}

impl DisplayStateTarget {
    /// Encode this target as the primary-key string stored in SQLite.
    pub fn storage_key(&self) -> &str {
        match self {
            DisplayStateTarget::AllDisplays => ALL_DISPLAYS_TARGET_KEY,
            DisplayStateTarget::Output(name) => name.as_str(),
        }
    }

    /// Parse a storage key into a typed target.
    ///
    /// Rejects blank/whitespace names. The reserved All Displays key is never
    /// treated as a concrete output name.
    pub fn from_storage_key(key: &str) -> Result<Self, WcError> {
        if key == ALL_DISPLAYS_TARGET_KEY {
            return Ok(DisplayStateTarget::AllDisplays);
        }
        if is_blank(key) {
            return Err(WcError::Other(
                "display state target must not be blank".into(),
            ));
        }
        Ok(DisplayStateTarget::Output(key.to_string()))
    }

    /// Validate a caller-supplied target before write.
    ///
    /// Rejects blank output names and using the reserved All Displays key as an
    /// `Output` variant.
    pub fn validate(&self) -> Result<(), WcError> {
        match self {
            DisplayStateTarget::AllDisplays => Ok(()),
            DisplayStateTarget::Output(name) => {
                if is_blank(name) {
                    return Err(WcError::Other(
                        "display state output name must not be blank".into(),
                    ));
                }
                if name == ALL_DISPLAYS_TARGET_KEY {
                    return Err(WcError::Other(
                        "reserved All Displays key cannot be used as an output name".into(),
                    ));
                }
                Ok(())
            }
        }
    }
}

/// One persisted display wallpaper assignment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayStateRow {
    pub target: DisplayStateTarget,
    pub wallpaper_path: String,
    pub backend: String,
    pub updated_at: String,
}

fn is_blank(value: &str) -> bool {
    value.trim().is_empty()
}

/// Normalize a backend identifier for new display-state writes/migration.
///
/// Accepts the three supported backends and the intentional legacy alias
/// `swww` → `awww`. Rejects typos and unknown values so we never persist an
/// unrestorable backend id.
fn normalize_supported_backend(raw: &str) -> Result<&'static str, WcError> {
    match raw.trim() {
        "awww" | "swww" => Ok("awww"),
        "mpvpaper" => Ok("mpvpaper"),
        "linux-wallpaperengine" => Ok("linux-wallpaperengine"),
        _ => Err(WcError::Other(format!(
            "unsupported display state backend: {raw}"
        ))),
    }
}

fn validate_assignment(path: &str, backend: &str) -> Result<&'static str, WcError> {
    if is_blank(path) {
        return Err(WcError::Other(
            "display state wallpaper path must not be blank".into(),
        ));
    }
    if is_blank(backend) {
        return Err(WcError::Other(
            "display state backend must not be blank".into(),
        ));
    }
    normalize_supported_backend(backend)
}

fn map_row(
    target_key: String,
    path: String,
    backend: String,
    updated_at: String,
) -> Result<DisplayStateRow, WcError> {
    if is_blank(&path) {
        return Err(WcError::Other(
            "corrupted display state: wallpaper path is blank".into(),
        ));
    }
    if is_blank(&backend) {
        return Err(WcError::Other(
            "corrupted display state: backend is blank".into(),
        ));
    }
    let backend = match normalize_supported_backend(&backend) {
        Ok(normalized) => normalized.to_string(),
        Err(_) => {
            return Err(WcError::Other(format!(
                "corrupted display state: unsupported backend: {backend}"
            )));
        }
    };
    Ok(DisplayStateRow {
        target: DisplayStateTarget::from_storage_key(&target_key)?,
        wallpaper_path: path,
        backend,
        updated_at,
    })
}

/// Ensure the display_state table exists (safe for existing databases).
pub fn ensure_display_state_schema(conn: &Connection) -> Result<(), WcError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS display_state (
            target_key     TEXT PRIMARY KEY NOT NULL,
            wallpaper_path TEXT NOT NULL,
            backend        TEXT NOT NULL,
            updated_at     TEXT NOT NULL DEFAULT (datetime('now'))
        );",
    )
    .map_err(|e| WcError::Sqlite(e.to_string()))?;
    Ok(())
}

fn read_state_value(conn: &Connection, key: &str) -> Result<Option<String>, WcError> {
    let exists: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='state'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map(|n| n > 0)
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    if !exists {
        return Ok(None);
    }
    conn.query_row(
        "SELECT value FROM state WHERE key = ?1",
        params![key],
        |row| row.get(0),
    )
    .optional()
    .map_err(|e| WcError::Sqlite(e.to_string()))
}

fn migration_marker_present(conn: &Connection) -> Result<bool, WcError> {
    let exists: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='db_meta'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map(|n| n > 0)
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    if !exists {
        return Ok(false);
    }
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM db_meta WHERE key = ?1",
            params![LEGACY_DISPLAY_STATE_MIGRATED_META_KEY],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    Ok(value.is_some())
}

fn write_migration_marker(conn: &Connection) -> Result<(), WcError> {
    conn.execute(
        "INSERT OR REPLACE INTO db_meta (key, value) VALUES (?1, ?2)",
        params![LEGACY_DISPLAY_STATE_MIGRATED_META_KEY, "1"],
    )
    .map_err(|e| WcError::Sqlite(e.to_string()))?;
    Ok(())
}

fn display_state_row_count(conn: &Connection) -> Result<i64, WcError> {
    conn.query_row("SELECT COUNT(*) FROM display_state", [], |row| row.get(0))
        .map_err(|e| WcError::Sqlite(e.to_string()))
}

/// Migrate legacy `current` + `last_backend` into All Displays when eligible.
///
/// Rules:
/// - both keys must be present and non-blank
/// - backend must be a supported identifier (with `swww` → `awww`)
/// - never overwrite an existing All Displays row
/// - never delete legacy keys
/// - partial / invalid pairs remain eligible (no marker) until a complete outcome
/// - once complete (insert or existing user/new rows), write a durable `db_meta`
///   marker transactionally so retained legacy keys cannot resurrect All Displays
/// - concurrency-safe via a transaction around the decision/insert/marker
pub fn migrate_legacy_display_state(conn: &Connection) -> Result<(), WcError> {
    ensure_display_state_schema(conn)?;
    if migration_marker_present(conn)? {
        return Ok(());
    }

    // IMMEDIATE serializes concurrent startups on the write lock so busy_timeout
    // can wait instead of deadlocking deferred upgrade races. Manual begin keeps
    // the `&Connection` API used throughout storage helpers.
    conn.execute("BEGIN IMMEDIATE", [])
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    let result = (|| {
        if migration_marker_present(conn)? {
            return Ok(());
        }

        // User/new display_state rows mean migration is already past legacy seeding.
        if display_state_row_count(conn)? > 0 {
            write_migration_marker(conn)?;
            return Ok(());
        }

        let current = read_state_value(conn, "current")?;
        let last_backend = read_state_value(conn, "last_backend")?;

        let (Some(path), Some(backend)) = (current, last_backend) else {
            return Ok(());
        };
        if is_blank(&path) || is_blank(&backend) {
            return Ok(());
        }
        let Ok(normalized_backend) = normalize_supported_backend(&backend) else {
            // Deterministic: leave unmarked so a later valid backend can migrate.
            return Ok(());
        };

        let all = DisplayStateTarget::AllDisplays;
        conn.execute(
            "INSERT INTO display_state (target_key, wallpaper_path, backend, updated_at)
             SELECT ?1, ?2, ?3, datetime('now')
             ON CONFLICT(target_key) DO NOTHING",
            params![all.storage_key(), path, normalized_backend],
        )
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
        write_migration_marker(conn)?;
        Ok(())
    })();
    match &result {
        Ok(()) => {
            conn.execute("COMMIT", [])
                .map_err(|e| WcError::Sqlite(e.to_string()))?;
        }
        Err(_) => {
            let _ = conn.execute("ROLLBACK", []);
        }
    }
    result
}

/// Ensure schema and run idempotent legacy migration.
pub fn ensure_display_state(conn: &Connection) -> Result<(), WcError> {
    ensure_display_state_schema(conn)?;
    migrate_legacy_display_state(conn)?;
    Ok(())
}

/// Read one display state row by target.
pub fn display_state_get(
    conn: &Connection,
    target: &DisplayStateTarget,
) -> Result<Option<DisplayStateRow>, WcError> {
    target.validate()?;
    ensure_display_state_schema(conn)?;
    let row = conn
        .query_row(
            "SELECT target_key, wallpaper_path, backend, updated_at
             FROM display_state WHERE target_key = ?1",
            params![target.storage_key()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    match row {
        Some((key, path, backend, updated_at)) => {
            Ok(Some(map_row(key, path, backend, updated_at)?))
        }
        None => Ok(None),
    }
}

/// List all display state rows, ordered by target key.
pub fn display_state_list(conn: &Connection) -> Result<Vec<DisplayStateRow>, WcError> {
    ensure_display_state_schema(conn)?;
    let mut stmt = conn
        .prepare(
            "SELECT target_key, wallpaper_path, backend, updated_at
             FROM display_state ORDER BY target_key ASC",
        )
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    let mut out = Vec::new();
    for row in rows {
        let (key, path, backend, updated_at) = row.map_err(|e| WcError::Sqlite(e.to_string()))?;
        out.push(map_row(key, path, backend, updated_at)?);
    }
    Ok(out)
}

/// Insert or replace one display state row. Updates `updated_at`.
pub fn display_state_upsert(
    conn: &Connection,
    target: &DisplayStateTarget,
    wallpaper_path: &str,
    backend: &str,
) -> Result<(), WcError> {
    target.validate()?;
    let backend = validate_assignment(wallpaper_path, backend)?;
    ensure_display_state_schema(conn)?;
    conn.execute(
        "INSERT INTO display_state (target_key, wallpaper_path, backend, updated_at)
         VALUES (?1, ?2, ?3, datetime('now'))
         ON CONFLICT(target_key) DO UPDATE SET
            wallpaper_path = excluded.wallpaper_path,
            backend = excluded.backend,
            updated_at = excluded.updated_at",
        params![target.storage_key(), wallpaper_path, backend],
    )
    .map_err(|e| WcError::Sqlite(e.to_string()))?;
    Ok(())
}

/// Delete one display state row. Returns whether a row was removed.
pub fn display_state_delete(
    conn: &Connection,
    target: &DisplayStateTarget,
) -> Result<bool, WcError> {
    target.validate()?;
    ensure_display_state_schema(conn)?;
    let n = conn
        .execute(
            "DELETE FROM display_state WHERE target_key = ?1",
            params![target.storage_key()],
        )
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    Ok(n > 0)
}

/// Atomically replace the entire display_state table contents.
pub fn display_state_replace_all(
    conn: &Connection,
    rows: &[(DisplayStateTarget, String, String)],
) -> Result<(), WcError> {
    let mut seen = HashSet::new();
    let mut normalized: Vec<(&DisplayStateTarget, &str, &'static str)> =
        Vec::with_capacity(rows.len());
    for (target, path, backend) in rows {
        target.validate()?;
        let backend = validate_assignment(path, backend)?;
        if !seen.insert(target.storage_key().to_string()) {
            return Err(WcError::Other(format!(
                "duplicate display state target: {}",
                target.storage_key()
            )));
        }
        normalized.push((target, path.as_str(), backend));
    }
    ensure_display_state_schema(conn)?;
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    tx.execute("DELETE FROM display_state", [])
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    {
        let mut stmt = tx
            .prepare(
                "INSERT INTO display_state (target_key, wallpaper_path, backend, updated_at)
                 VALUES (?1, ?2, ?3, datetime('now'))",
            )
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        for (target, path, backend) in normalized {
            stmt.execute(params![target.storage_key(), path, backend])
                .map_err(|e| WcError::Sqlite(e.to_string()))?;
        }
    }
    tx.commit().map_err(|e| WcError::Sqlite(e.to_string()))?;
    Ok(())
}

fn open_display_state_connection(cd: &ConfigDir) -> Result<Connection, WcError> {
    try_ensure_sqlite_db(cd)?;
    open_runtime_connection(cd)
}

/// ConfigDir-backed helpers used by [`crate::StorageApi`].
pub fn display_state_get_cd(
    cd: &ConfigDir,
    target: &DisplayStateTarget,
) -> Result<Option<DisplayStateRow>, WcError> {
    let conn = open_display_state_connection(cd)?;
    ensure_display_state(&conn)?;
    display_state_get(&conn, target)
}

pub fn display_state_list_cd(cd: &ConfigDir) -> Result<Vec<DisplayStateRow>, WcError> {
    let conn = open_display_state_connection(cd)?;
    ensure_display_state(&conn)?;
    display_state_list(&conn)
}

pub fn display_state_upsert_cd(
    cd: &ConfigDir,
    target: &DisplayStateTarget,
    wallpaper_path: &str,
    backend: &str,
) -> Result<(), WcError> {
    let conn = open_display_state_connection(cd)?;
    ensure_display_state(&conn)?;
    display_state_upsert(&conn, target, wallpaper_path, backend)
}

pub fn display_state_delete_cd(
    cd: &ConfigDir,
    target: &DisplayStateTarget,
) -> Result<bool, WcError> {
    let conn = open_display_state_connection(cd)?;
    ensure_display_state(&conn)?;
    display_state_delete(&conn, target)
}

pub fn display_state_replace_all_cd(
    cd: &ConfigDir,
    rows: &[(DisplayStateTarget, String, String)],
) -> Result<(), WcError> {
    let conn = open_display_state_connection(cd)?;
    ensure_display_state(&conn)?;
    display_state_replace_all(&conn, rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::schema::create_schema;

    fn open_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        create_schema(&conn).unwrap();
        conn
    }

    fn state_value(conn: &Connection, key: &str) -> Option<String> {
        conn.query_row(
            "SELECT value FROM state WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )
        .optional()
        .unwrap()
    }

    fn set_state(conn: &Connection, key: &str, value: &str) {
        conn.execute(
            "INSERT OR REPLACE INTO state (key, value) VALUES (?1, ?2)",
            params![key, value],
        )
        .unwrap();
    }

    fn table_exists(conn: &Connection, name: &str) -> bool {
        conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
            params![name],
            |row| row.get::<_, i64>(0),
        )
        .unwrap()
            > 0
    }

    #[test]
    fn schema_creates_display_state_table_on_fresh_db() {
        let conn = open_db();
        assert!(
            table_exists(&conn, "display_state"),
            "create_schema must create display_state"
        );
    }

    #[test]
    fn schema_upgrades_existing_db_without_display_state() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE state (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE db_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        )
        .unwrap();
        assert!(!table_exists(&conn, "display_state"));

        ensure_display_state(&conn).expect("upgrade must add display_state");
        assert!(table_exists(&conn, "display_state"));
    }

    #[test]
    fn rejects_blank_output_target() {
        let err = DisplayStateTarget::Output(String::new())
            .validate()
            .unwrap_err();
        assert!(err.to_string().contains("blank"), "{err}");

        let err = DisplayStateTarget::Output("   ".into())
            .validate()
            .unwrap_err();
        assert!(err.to_string().contains("blank"), "{err}");

        let err = DisplayStateTarget::from_storage_key("").unwrap_err();
        assert!(err.to_string().contains("blank"), "{err}");
    }

    #[test]
    fn reserved_all_displays_key_is_not_an_output_name() {
        assert_eq!(
            DisplayStateTarget::from_storage_key(ALL_DISPLAYS_TARGET_KEY).unwrap(),
            DisplayStateTarget::AllDisplays
        );
        let err = DisplayStateTarget::Output(ALL_DISPLAYS_TARGET_KEY.into())
            .validate()
            .unwrap_err();
        assert!(
            err.to_string().contains("reserved") || err.to_string().contains("All Displays"),
            "{err}"
        );
    }

    #[test]
    fn upsert_get_list_delete_round_trip() {
        let conn = open_db();
        let edp = DisplayStateTarget::Output("eDP-1".into());
        let all = DisplayStateTarget::AllDisplays;

        display_state_upsert(&conn, &edp, "/walls/a.jpg", "awww").unwrap();
        display_state_upsert(&conn, &all, "/walls/b.jpg", "mpvpaper").unwrap();

        let got = display_state_get(&conn, &edp).unwrap().expect("eDP-1 row");
        assert_eq!(got.target, edp);
        assert_eq!(got.wallpaper_path, "/walls/a.jpg");
        assert_eq!(got.backend, "awww");
        assert!(!got.updated_at.is_empty());

        let list = display_state_list(&conn).unwrap();
        assert_eq!(list.len(), 2);
        assert!(list.iter().any(|r| r.target == all));
        assert!(list.iter().any(|r| r.target == edp));

        assert!(display_state_delete(&conn, &edp).unwrap());
        assert_eq!(display_state_get(&conn, &edp).unwrap(), None);
        assert!(!display_state_delete(&conn, &edp).unwrap());
    }

    #[test]
    fn upsert_rejects_blank_path_or_backend() {
        let conn = open_db();
        let target = DisplayStateTarget::Output("eDP-1".into());
        assert!(display_state_upsert(&conn, &target, "  ", "awww").is_err());
        assert!(display_state_upsert(&conn, &target, "/walls/a.jpg", "").is_err());
        assert!(display_state_list(&conn).unwrap().is_empty());
    }

    #[test]
    fn replace_all_is_atomic() {
        let conn = open_db();
        let edp = DisplayStateTarget::Output("eDP-1".into());
        display_state_upsert(&conn, &edp, "/walls/old.jpg", "awww").unwrap();

        display_state_replace_all(
            &conn,
            &[
                (
                    DisplayStateTarget::AllDisplays,
                    "/walls/all.jpg".into(),
                    "awww".into(),
                ),
                (
                    DisplayStateTarget::Output("HDMI-1".into()),
                    "/walls/hdmi.jpg".into(),
                    "mpvpaper".into(),
                ),
            ],
        )
        .unwrap();

        let list = display_state_list(&conn).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(display_state_get(&conn, &edp).unwrap(), None);
        let hdmi = DisplayStateTarget::Output("HDMI-1".into());
        let row = display_state_get(&conn, &hdmi).unwrap().unwrap();
        assert_eq!(row.wallpaper_path, "/walls/hdmi.jpg");
        assert_eq!(row.backend, "mpvpaper");
    }

    #[test]
    fn replace_all_rejects_invalid_row_and_preserves_previous() {
        let conn = open_db();
        let edp = DisplayStateTarget::Output("eDP-1".into());
        display_state_upsert(&conn, &edp, "/walls/old.jpg", "awww").unwrap();

        let err = display_state_replace_all(
            &conn,
            &[
                (
                    DisplayStateTarget::AllDisplays,
                    "/walls/all.jpg".into(),
                    "awww".into(),
                ),
                (
                    DisplayStateTarget::Output("  ".into()),
                    "/walls/bad.jpg".into(),
                    "awww".into(),
                ),
            ],
        )
        .unwrap_err();
        assert!(err.to_string().contains("blank"), "{err}");

        let list = display_state_list(&conn).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].wallpaper_path, "/walls/old.jpg");
    }

    #[test]
    fn legacy_pair_migrates_to_all_displays_when_both_valid() {
        let conn = open_db();
        set_state(&conn, "current", "/walls/current.jpg");
        set_state(&conn, "last_backend", "awww");

        migrate_legacy_display_state(&conn).unwrap();

        let row = display_state_get(&conn, &DisplayStateTarget::AllDisplays)
            .unwrap()
            .expect("All Displays row");
        assert_eq!(row.wallpaper_path, "/walls/current.jpg");
        assert_eq!(row.backend, "awww");
        assert_eq!(
            state_value(&conn, "current").as_deref(),
            Some("/walls/current.jpg")
        );
        assert_eq!(state_value(&conn, "last_backend").as_deref(), Some("awww"));
    }

    #[test]
    fn legacy_migration_is_idempotent_and_does_not_overwrite() {
        let conn = open_db();
        set_state(&conn, "current", "/walls/current.jpg");
        set_state(&conn, "last_backend", "awww");
        migrate_legacy_display_state(&conn).unwrap();

        display_state_upsert(
            &conn,
            &DisplayStateTarget::AllDisplays,
            "/walls/newer.jpg",
            "mpvpaper",
        )
        .unwrap();
        set_state(&conn, "current", "/walls/legacy-again.jpg");
        set_state(&conn, "last_backend", "linux-wallpaperengine");

        migrate_legacy_display_state(&conn).unwrap();
        migrate_legacy_display_state(&conn).unwrap();

        let row = display_state_get(&conn, &DisplayStateTarget::AllDisplays)
            .unwrap()
            .unwrap();
        assert_eq!(row.wallpaper_path, "/walls/newer.jpg");
        assert_eq!(row.backend, "mpvpaper");
    }

    #[test]
    fn partial_legacy_pair_is_left_untouched() {
        let conn = open_db();
        set_state(&conn, "current", "/walls/current.jpg");
        // last_backend missing

        migrate_legacy_display_state(&conn).unwrap();
        assert_eq!(
            display_state_get(&conn, &DisplayStateTarget::AllDisplays).unwrap(),
            None
        );
        assert!(display_state_list(&conn).unwrap().is_empty());
        assert_eq!(
            state_value(&conn, "current").as_deref(),
            Some("/walls/current.jpg")
        );

        set_state(&conn, "last_backend", "   ");
        migrate_legacy_display_state(&conn).unwrap();
        assert!(display_state_list(&conn).unwrap().is_empty());
        assert_eq!(
            state_value(&conn, "current").as_deref(),
            Some("/walls/current.jpg")
        );
        assert_eq!(state_value(&conn, "last_backend").as_deref(), Some("   "));
    }

    #[test]
    fn blank_legacy_current_does_not_create_display_state() {
        let conn = open_db();
        set_state(&conn, "current", "  ");
        set_state(&conn, "last_backend", "awww");

        migrate_legacy_display_state(&conn).unwrap();
        assert!(display_state_list(&conn).unwrap().is_empty());
        assert_eq!(state_value(&conn, "current").as_deref(), Some("  "));
        assert_eq!(state_value(&conn, "last_backend").as_deref(), Some("awww"));
    }

    #[test]
    fn ensure_display_state_runs_migration_on_startup_path() {
        let conn = open_db();
        set_state(&conn, "current", "/walls/current.jpg");
        set_state(&conn, "last_backend", "awww");

        ensure_display_state(&conn).unwrap();
        ensure_display_state(&conn).unwrap();

        let row = display_state_get(&conn, &DisplayStateTarget::AllDisplays)
            .unwrap()
            .unwrap();
        assert_eq!(row.wallpaper_path, "/walls/current.jpg");
        assert_eq!(row.backend, "awww");
        assert_eq!(
            state_value(&conn, "current").as_deref(),
            Some("/walls/current.jpg")
        );
    }

    #[test]
    fn upsert_rejects_unsupported_backend_typos() {
        let conn = open_db();
        let target = DisplayStateTarget::Output("eDP-1".into());
        let err = display_state_upsert(&conn, &target, "/walls/a.jpg", "aww")
            .expect_err("typo backend must be rejected");
        assert!(
            err.to_string().contains("backend") || err.to_string().contains("unsupported"),
            "{err}"
        );
        assert!(display_state_list(&conn).unwrap().is_empty());
    }

    #[test]
    fn upsert_normalizes_legacy_swww_to_awww() {
        let conn = open_db();
        let target = DisplayStateTarget::Output("eDP-1".into());
        display_state_upsert(&conn, &target, "/walls/a.jpg", "swww").unwrap();
        let row = display_state_get(&conn, &target).unwrap().unwrap();
        assert_eq!(row.backend, "awww");
    }

    #[test]
    fn upsert_accepts_supported_backends() {
        let conn = open_db();
        for (i, backend) in ["awww", "mpvpaper", "linux-wallpaperengine"]
            .into_iter()
            .enumerate()
        {
            let target = DisplayStateTarget::Output(format!("OUT-{i}"));
            display_state_upsert(&conn, &target, "/walls/a.jpg", backend).unwrap();
            assert_eq!(
                display_state_get(&conn, &target).unwrap().unwrap().backend,
                backend
            );
        }
    }

    #[test]
    fn legacy_swww_migrates_as_awww() {
        let conn = open_db();
        set_state(&conn, "current", "/walls/current.jpg");
        set_state(&conn, "last_backend", "swww");
        migrate_legacy_display_state(&conn).unwrap();
        let row = display_state_get(&conn, &DisplayStateTarget::AllDisplays)
            .unwrap()
            .unwrap();
        assert_eq!(row.backend, "awww");
        assert_eq!(state_value(&conn, "last_backend").as_deref(), Some("swww"));
    }

    #[test]
    fn legacy_typo_backend_does_not_create_display_state() {
        let conn = open_db();
        set_state(&conn, "current", "/walls/current.jpg");
        set_state(&conn, "last_backend", "aww");
        migrate_legacy_display_state(&conn).unwrap();
        assert!(display_state_list(&conn).unwrap().is_empty());
        assert_eq!(
            state_value(&conn, "current").as_deref(),
            Some("/walls/current.jpg")
        );
        assert_eq!(state_value(&conn, "last_backend").as_deref(), Some("aww"));
    }

    #[test]
    fn get_reports_corrupted_blank_wallpaper_path() {
        let conn = open_db();
        conn.execute(
            "INSERT INTO display_state (target_key, wallpaper_path, backend, updated_at)
             VALUES (?1, '  ', 'awww', datetime('now'))",
            params![ALL_DISPLAYS_TARGET_KEY],
        )
        .unwrap();
        let err = display_state_get(&conn, &DisplayStateTarget::AllDisplays).unwrap_err();
        assert!(
            err.to_string().contains("blank") || err.to_string().contains("corrupt"),
            "{err}"
        );
    }

    #[test]
    fn list_reports_corrupted_blank_backend() {
        let conn = open_db();
        conn.execute(
            "INSERT INTO display_state (target_key, wallpaper_path, backend, updated_at)
             VALUES ('eDP-1', '/walls/a.jpg', '', datetime('now'))",
            [],
        )
        .unwrap();
        let err = display_state_list(&conn).unwrap_err();
        assert!(
            err.to_string().contains("blank") || err.to_string().contains("corrupt"),
            "{err}"
        );
    }

    #[test]
    fn replace_all_rejects_duplicate_targets_before_mutating() {
        let conn = open_db();
        let edp = DisplayStateTarget::Output("eDP-1".into());
        display_state_upsert(&conn, &edp, "/walls/old.jpg", "awww").unwrap();

        let err = display_state_replace_all(
            &conn,
            &[
                (
                    DisplayStateTarget::Output("HDMI-1".into()),
                    "/walls/a.jpg".into(),
                    "awww".into(),
                ),
                (
                    DisplayStateTarget::Output("HDMI-1".into()),
                    "/walls/b.jpg".into(),
                    "mpvpaper".into(),
                ),
            ],
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("duplicate") || err.to_string().contains("target"),
            "{err}"
        );

        let list = display_state_list(&conn).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].wallpaper_path, "/walls/old.jpg");
    }

    #[test]
    fn replace_all_rolls_back_when_insert_fails_after_delete() {
        let conn = open_db();
        let edp = DisplayStateTarget::Output("eDP-1".into());
        display_state_upsert(&conn, &edp, "/walls/old.jpg", "awww").unwrap();

        // Abort after DELETE by failing the insert path via a DB trigger.
        conn.execute_batch(
            "CREATE TEMP TRIGGER display_state_test_abort_insert
             BEFORE INSERT ON display_state
             BEGIN
               SELECT RAISE(ABORT, 'test insert abort');
             END;",
        )
        .unwrap();

        let err = display_state_replace_all(
            &conn,
            &[(
                DisplayStateTarget::AllDisplays,
                "/walls/new.jpg".into(),
                "awww".into(),
            )],
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("abort") || err.to_string().contains("Sqlite"),
            "{err}"
        );

        let list = display_state_list(&conn).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].target, edp);
        assert_eq!(list[0].wallpaper_path, "/walls/old.jpg");
    }

    fn meta_value(conn: &Connection, key: &str) -> Option<String> {
        conn.query_row(
            "SELECT value FROM db_meta WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )
        .optional()
        .unwrap()
    }

    #[test]
    fn legacy_migration_writes_durable_db_meta_marker() {
        let conn = open_db();
        set_state(&conn, "current", "/walls/current.jpg");
        set_state(&conn, "last_backend", "awww");

        migrate_legacy_display_state(&conn).unwrap();

        assert!(
            meta_value(&conn, LEGACY_DISPLAY_STATE_MIGRATED_META_KEY).is_some(),
            "successful migration must record a durable db_meta marker"
        );
    }

    #[test]
    fn migrated_all_displays_stays_absent_after_delete_and_reopen() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        crate::sqlite::schema::try_ensure_sqlite_db(&cd).unwrap();
        {
            let conn = Connection::open(cd.db_path()).unwrap();
            set_state(&conn, "current", "/walls/current.jpg");
            set_state(&conn, "last_backend", "awww");
        }

        ensure_display_state(&crate::sqlite::schema::open_runtime_connection(&cd).unwrap())
            .unwrap();
        assert!(display_state_delete_cd(&cd, &DisplayStateTarget::AllDisplays).unwrap());

        // Helper reopen path re-runs ensure/migration; legacy keys must not resurrect.
        assert_eq!(
            display_state_get_cd(&cd, &DisplayStateTarget::AllDisplays).unwrap(),
            None
        );
        assert!(display_state_list_cd(&cd).unwrap().is_empty());
        assert_eq!(
            flat_state_via_storage(&cd, "current").as_deref(),
            Some("/walls/current.jpg")
        );
    }

    fn flat_state_via_storage(cd: &ConfigDir, key: &str) -> Option<String> {
        let conn = Connection::open(cd.db_path()).unwrap();
        state_value(&conn, key)
    }

    #[test]
    fn migrate_then_replace_with_named_rows_does_not_resurrect_all_displays() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        crate::sqlite::schema::try_ensure_sqlite_db(&cd).unwrap();
        {
            let conn = Connection::open(cd.db_path()).unwrap();
            set_state(&conn, "current", "/walls/current.jpg");
            set_state(&conn, "last_backend", "awww");
            ensure_display_state(&conn).unwrap();
            display_state_replace_all(
                &conn,
                &[(
                    DisplayStateTarget::Output("eDP-1".into()),
                    "/walls/edp.jpg".into(),
                    "mpvpaper".into(),
                )],
            )
            .unwrap();
        }

        let list = display_state_list_cd(&cd).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].target, DisplayStateTarget::Output("eDP-1".into()));
        assert_eq!(
            display_state_get_cd(&cd, &DisplayStateTarget::AllDisplays).unwrap(),
            None,
            "legacy current/last_backend must not recreate All Displays after replace_all"
        );
    }

    #[test]
    fn existing_user_display_rows_complete_migration_without_inserting_all_displays() {
        let conn = open_db();
        set_state(&conn, "current", "/walls/current.jpg");
        set_state(&conn, "last_backend", "awww");
        display_state_upsert(
            &conn,
            &DisplayStateTarget::Output("eDP-1".into()),
            "/walls/edp.jpg",
            "mpvpaper",
        )
        .unwrap();

        migrate_legacy_display_state(&conn).unwrap();

        assert!(meta_value(&conn, LEGACY_DISPLAY_STATE_MIGRATED_META_KEY).is_some());
        assert_eq!(
            display_state_get(&conn, &DisplayStateTarget::AllDisplays).unwrap(),
            None
        );
        let list = display_state_list(&conn).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].wallpaper_path, "/walls/edp.jpg");
    }

    #[test]
    fn partial_legacy_pair_remains_eligible_until_complete() {
        let conn = open_db();
        set_state(&conn, "current", "/walls/current.jpg");
        // last_backend missing — migration not complete
        migrate_legacy_display_state(&conn).unwrap();
        assert!(meta_value(&conn, LEGACY_DISPLAY_STATE_MIGRATED_META_KEY).is_none());

        set_state(&conn, "last_backend", "awww");
        migrate_legacy_display_state(&conn).unwrap();
        assert!(meta_value(&conn, LEGACY_DISPLAY_STATE_MIGRATED_META_KEY).is_some());
        let row = display_state_get(&conn, &DisplayStateTarget::AllDisplays)
            .unwrap()
            .expect("completed pair should migrate");
        assert_eq!(row.wallpaper_path, "/walls/current.jpg");
    }

    #[test]
    fn invalid_legacy_backend_stays_eligible_without_marker_or_row() {
        // Deterministic: typo does not create unrestorable state and does not
        // permanently seal migration; a later valid backend can still migrate.
        let conn = open_db();
        set_state(&conn, "current", "/walls/current.jpg");
        set_state(&conn, "last_backend", "aww");
        migrate_legacy_display_state(&conn).unwrap();
        assert!(display_state_list(&conn).unwrap().is_empty());
        assert!(meta_value(&conn, LEGACY_DISPLAY_STATE_MIGRATED_META_KEY).is_none());

        set_state(&conn, "last_backend", "awww");
        migrate_legacy_display_state(&conn).unwrap();
        assert!(meta_value(&conn, LEGACY_DISPLAY_STATE_MIGRATED_META_KEY).is_some());
        assert_eq!(
            display_state_get(&conn, &DisplayStateTarget::AllDisplays)
                .unwrap()
                .unwrap()
                .backend,
            "awww"
        );
    }

    #[test]
    fn get_normalizes_legacy_swww_backend_on_read() {
        let conn = open_db();
        conn.execute(
            "INSERT INTO display_state (target_key, wallpaper_path, backend, updated_at)
             VALUES (?1, '/walls/a.jpg', 'swww', datetime('now'))",
            params![ALL_DISPLAYS_TARGET_KEY],
        )
        .unwrap();
        let row = display_state_get(&conn, &DisplayStateTarget::AllDisplays)
            .unwrap()
            .unwrap();
        assert_eq!(row.backend, "awww");
    }

    #[test]
    fn list_reports_corrupted_unsupported_backend_typo() {
        let conn = open_db();
        conn.execute(
            "INSERT INTO display_state (target_key, wallpaper_path, backend, updated_at)
             VALUES ('eDP-1', '/walls/a.jpg', 'aww', datetime('now'))",
            [],
        )
        .unwrap();
        let err = display_state_list(&conn).unwrap_err();
        assert!(
            err.to_string().contains("corrupt")
                || err.to_string().contains("unsupported")
                || err.to_string().contains("backend"),
            "{err}"
        );
    }

    #[test]
    fn get_reports_corrupted_unsupported_backend_typo() {
        let conn = open_db();
        conn.execute(
            "INSERT INTO display_state (target_key, wallpaper_path, backend, updated_at)
             VALUES (?1, '/walls/a.jpg', 'not-a-backend', datetime('now'))",
            params![ALL_DISPLAYS_TARGET_KEY],
        )
        .unwrap();
        let err = display_state_get(&conn, &DisplayStateTarget::AllDisplays).unwrap_err();
        assert!(
            err.to_string().contains("corrupt")
                || err.to_string().contains("unsupported")
                || err.to_string().contains("backend"),
            "{err}"
        );
    }

    #[test]
    fn concurrent_legacy_migration_is_idempotent() {
        use std::sync::{Arc, Barrier};
        use std::thread;

        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        crate::sqlite::schema::try_ensure_sqlite_db(&cd).unwrap();
        {
            let conn = Connection::open(cd.db_path()).unwrap();
            set_state(&conn, "current", "/walls/current.jpg");
            set_state(&conn, "last_backend", "awww");
        }

        let barrier = Arc::new(Barrier::new(4));
        let mut handles = Vec::new();
        for _ in 0..4 {
            let cd = ConfigDir {
                path: cd.path.clone(),
            };
            let barrier = barrier.clone();
            handles.push(thread::spawn(move || {
                barrier.wait();
                let conn = crate::sqlite::schema::open_runtime_connection(&cd).unwrap();
                ensure_display_state(&conn).unwrap();
            }));
        }
        for h in handles {
            h.join().unwrap();
        }

        let list = display_state_list_cd(&cd).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].wallpaper_path, "/walls/current.jpg");
        assert_eq!(list[0].backend, "awww");
        {
            let conn = Connection::open(cd.db_path()).unwrap();
            assert!(meta_value(&conn, LEGACY_DISPLAY_STATE_MIGRATED_META_KEY).is_some());
        }
    }

    #[test]
    fn config_dir_helpers_wait_through_exclusive_transaction_lock() {
        use std::sync::{Arc, Barrier};
        use std::thread;
        use std::time::{Duration, Instant};

        use rusqlite::ErrorCode;

        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        crate::sqlite::schema::try_ensure_sqlite_db(&cd).unwrap();
        display_state_upsert_cd(
            &cd,
            &DisplayStateTarget::AllDisplays,
            "/walls/a.jpg",
            "awww",
        )
        .unwrap();

        let db_path = cd.db_path().to_path_buf();
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute_batch("PRAGMA journal_mode = DELETE;")
                .expect("switch test db to rollback journal for lock contention");
        }

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
            .query_row("SELECT COUNT(*) FROM display_state", [], |row| {
                row.get::<_, i64>(0)
            })
            .expect_err("read without busy_timeout should fail while exclusive lock is held");
        assert!(started.elapsed() < Duration::from_millis(100));
        match no_wait_err {
            rusqlite::Error::SqliteFailure(err, _) => {
                assert_eq!(err.code, ErrorCode::DatabaseBusy, "{no_wait_err}");
            }
            other => panic!("expected SQLITE_BUSY, got {other}"),
        }

        let row = display_state_get_cd(&cd, &DisplayStateTarget::AllDisplays)
            .expect("display_state ConfigDir helper should wait instead of SQLITE_BUSY")
            .expect("row present");
        writer.join().unwrap();
        assert_eq!(row.wallpaper_path, "/walls/a.jpg");
    }
}
