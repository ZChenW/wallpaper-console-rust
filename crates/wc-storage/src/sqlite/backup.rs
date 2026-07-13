use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use rusqlite::{params, Connection, OpenFlags};
use wc_core::config::ConfigDir;
use wc_core::error::WcError;

use super::chrono_now_compact;
use super::connection::acquire_maintenance_lock;
use super::schema::{
    check_wallpapers_fts_integrity, create_schema, open_runtime_connection, rebuild_wallpapers_fts,
    validate_current_schema_objects, wallpapers_count, wallpapers_fts_count,
    CURRENT_PERSISTENT_TABLES, CURRENT_SCHEMA_VERSION, FTS_SCHEMA_VERSION,
};
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
/// - `Ok(VerifyResult::OkWithWarnings(w))` — flat-file compatibility copies
///   have drifted; SQLite is authoritative and fine
/// - `Ok(VerifyResult::Failed(e))` — SQLite schema/FTS integrity error
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
            "wallpapers.db not found; initialize SQLite storage first.".into(),
        ));
    }
    let conn = open_runtime_connection(cd)?;

    let mut warnings: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    for table in CURRENT_PERSISTENT_TABLES {
        if !table_exists(&conn, "main", table)? {
            return Err(WcError::Sqlite(format!(
                "required table {table} is missing"
            )));
        }
    }
    if let Err(error) = validate_current_schema_objects(&conn) {
        errors.push(format!("schema: {error}"));
    }
    if let Err(error) = integrity_check(&conn, "verify") {
        errors.push(format!("integrity: {error}"));
    }
    let mut foreign_keys = conn
        .prepare("PRAGMA foreign_key_check")
        .map_err(|error| WcError::Sqlite(error.to_string()))?;
    if foreign_keys
        .query([])
        .map_err(|error| WcError::Sqlite(error.to_string()))?
        .next()
        .map_err(|error| WcError::Sqlite(error.to_string()))?
        .is_some()
    {
        errors.push("foreign_keys".into());
    }
    if !errors.is_empty() {
        return Ok(VerifyResult::Failed(errors));
    }

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

    // Favorites — any drift from flat files is a legacy-compat warning,
    // not a data error. SQLite is the authoritative runtime store.
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
            warnings.push("favorites (legacy flat compat)".into());
        }
    }

    // History — any drift from flat files is a legacy-compat warning.
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
        let flat_norm = canonical_unique_sorted(&flat_hist);
        let db_norm = canonical_unique_sorted(&db_hist);
        if flat_norm != db_norm {
            warnings.push("history (legacy flat compat)".into());
        }
    }

    // State: current — any drift from flat files is a legacy-compat warning.
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
            warnings.push("current (legacy flat compat)".into());
        }
    }

    // State: last_backend — any drift from flat files is a legacy-compat warning.
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
            warnings.push("last_backend (legacy flat compat)".into());
        }
    }

    // Search index: data integrity; stale FTS silently breaks GUI/CLI search.
    {
        let wallpaper_count = wallpapers_count(&conn)?;
        let fts_count = wallpapers_fts_count(&conn)?;
        if wallpaper_count != fts_count || check_wallpapers_fts_integrity(&conn).is_err() {
            errors.push("wallpapers_fts".into());
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

static MAINTENANCE_PATH_COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique_database_sibling(db_path: &Path, label: &str) -> PathBuf {
    let counter = MAINTENANCE_PATH_COUNTER.fetch_add(1, Ordering::Relaxed);
    let file_name = db_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("wallpapers.db");
    db_path.with_file_name(format!(
        ".{file_name}.{label}-{}-{counter}",
        std::process::id()
    ))
}

fn visible_database_backup(db_path: &Path, label: &str) -> PathBuf {
    let counter = MAINTENANCE_PATH_COUNTER.fetch_add(1, Ordering::Relaxed);
    db_path.with_extension(format!(
        "db.{label}.{}-{}-{counter}",
        chrono_now_compact(),
        std::process::id()
    ))
}

fn database_sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

fn remove_database_sidecars(path: &Path) -> Result<(), WcError> {
    for suffix in ["-wal", "-shm", "-journal"] {
        match std::fs::remove_file(database_sidecar(path, suffix)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(WcError::Io(error)),
        }
    }
    Ok(())
}

fn cleanup_database(path: &Path) {
    let _ = std::fs::remove_file(path);
    for suffix in ["-wal", "-shm", "-journal"] {
        let _ = std::fs::remove_file(database_sidecar(path, suffix));
    }
}

struct TemporaryDatabase {
    path: PathBuf,
    published: bool,
}

impl TemporaryDatabase {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            published: false,
        }
    }

    fn publish(mut self, destination: &Path) -> Result<(), WcError> {
        std::fs::rename(&self.path, destination).map_err(WcError::Io)?;
        self.published = true;
        Ok(())
    }

    fn preserve(mut self) -> PathBuf {
        let path = self.path.clone();
        self.published = true;
        path
    }
}

impl Drop for TemporaryDatabase {
    fn drop(&mut self) {
        if !self.published {
            cleanup_database(&self.path);
        }
    }
}

fn reserve_empty_temporary_database<F>(
    mut next_temporary_path: F,
) -> Result<TemporaryDatabase, WcError>
where
    F: FnMut() -> PathBuf,
{
    loop {
        let path = next_temporary_path();
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => {
                drop(file);
                return Ok(TemporaryDatabase::new(path));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(WcError::Io(error)),
        }
    }
}

fn reserve_visible_database_backup(
    db_path: &Path,
    label: &str,
) -> Result<TemporaryDatabase, WcError> {
    reserve_empty_temporary_database(|| visible_database_backup(db_path, label))
}

fn close_connection(connection: Connection) -> Result<(), WcError> {
    connection
        .close()
        .map_err(|(_, error)| WcError::Sqlite(error.to_string()))
}

fn database_version(connection: &Connection) -> Result<i64, WcError> {
    connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|error| WcError::Sqlite(error.to_string()))
}

fn reject_future_schema(connection: &Connection) -> Result<i64, WcError> {
    let version = database_version(connection)?;
    if version > CURRENT_SCHEMA_VERSION {
        return Err(WcError::Sqlite(format!(
            "database schema version {version} is newer than supported version {CURRENT_SCHEMA_VERSION}"
        )));
    }
    Ok(version)
}

fn vacuum_into(connection: &Connection, destination: &Path) -> Result<(), WcError> {
    let destination = destination.to_string_lossy().into_owned();
    connection
        .execute("VACUUM INTO ?1", params![destination])
        .map(|_| ())
        .map_err(|error| WcError::Sqlite(error.to_string()))
}

fn checkpoint_wal(connection: &Connection, context: &str) -> Result<(), WcError> {
    let (busy, _, _): (i64, i64, i64) = connection
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .map_err(|error| WcError::Sqlite(error.to_string()))?;
    if busy != 0 {
        return Err(WcError::Other(format!(
            "{context}: WAL checkpoint remained busy"
        )));
    }
    Ok(())
}

fn quote_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn table_exists(connection: &Connection, schema: &str, table: &str) -> Result<bool, WcError> {
    let sql = format!(
        "SELECT COUNT(*) FROM {}.sqlite_master WHERE type = 'table' AND name = ?1",
        quote_identifier(schema)
    );
    connection
        .query_row(&sql, [table], |row| row.get::<_, i64>(0))
        .map(|count| count > 0)
        .map_err(|error| WcError::Sqlite(error.to_string()))
}

fn table_columns(
    connection: &Connection,
    schema: &str,
    table: &str,
) -> Result<Vec<String>, WcError> {
    // Deliberately use table_info rather than table_xinfo: repair copies stored
    // columns such as author, while generated columns such as filename must be
    // omitted so the fresh target schema can recompute them from path.
    let sql = format!(
        "PRAGMA {}.table_info({})",
        quote_identifier(schema),
        quote_identifier(table)
    );
    let mut statement = connection
        .prepare(&sql)
        .map_err(|error| WcError::Sqlite(error.to_string()))?;
    let columns = statement
        .query_map([], |row| row.get(1))
        .map_err(|error| WcError::Sqlite(error.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| WcError::Sqlite(error.to_string()))?;
    Ok(columns)
}

fn copy_common_table_columns(
    connection: &Connection,
    source_schema: &str,
    table: &str,
) -> Result<bool, WcError> {
    if !table_exists(connection, source_schema, table)? {
        return Ok(false);
    }
    let target_columns = table_columns(connection, "main", table)?;
    let source_columns: std::collections::HashSet<String> =
        table_columns(connection, source_schema, table)?
            .into_iter()
            .collect();
    let common: Vec<String> = target_columns
        .into_iter()
        .filter(|column| source_columns.contains(column))
        .collect();
    if common.is_empty() {
        return Err(WcError::Other(format!(
            "repair: no compatible columns for persistent table {table}"
        )));
    }
    let columns = common
        .iter()
        .map(|column| quote_identifier(column))
        .collect::<Vec<_>>()
        .join(", ");
    let filter = if table == "db_meta" {
        " WHERE key NOT IN
          ('schema_version', 'fts_schema_version', 'fts_rebuilt_at')"
    } else {
        ""
    };
    let sql = format!(
        "INSERT INTO main.{table_name} ({columns})
         SELECT {columns} FROM {source_schema}.{table_name}{filter}",
        table_name = quote_identifier(table),
        source_schema = quote_identifier(source_schema),
    );
    connection
        .execute(&sql, [])
        .map_err(|error| WcError::Sqlite(error.to_string()))?;
    Ok(true)
}

fn copy_persistent_data(connection: &Connection, source_path: &Path) -> Result<(), WcError> {
    connection
        .execute(
            "ATTACH DATABASE ?1 AS source_db",
            [source_path.to_string_lossy()],
        )
        .map_err(|error| WcError::Sqlite(error.to_string()))?;
    let source_has_memberships = table_exists(connection, "source_db", "wallpaper_sources")?;
    connection
        .execute_batch("PRAGMA foreign_keys = OFF;")
        .map_err(|error| WcError::Sqlite(error.to_string()))?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| WcError::Sqlite(error.to_string()))?;
    for table in CURRENT_PERSISTENT_TABLES {
        copy_common_table_columns(&transaction, "source_db", table)?;
    }
    if table_exists(&transaction, "source_db", "sqlite_sequence")? {
        transaction
            .execute("DELETE FROM main.sqlite_sequence", [])
            .map_err(|error| WcError::Sqlite(error.to_string()))?;
        transaction
            .execute(
                "INSERT INTO main.sqlite_sequence (name, seq)
                 SELECT name, seq FROM source_db.sqlite_sequence
                 WHERE name IN ('sources', 'wallpapers', 'favorites', 'history')",
                [],
            )
            .map_err(|error| WcError::Sqlite(error.to_string()))?;
    }
    transaction
        .commit()
        .map_err(|error| WcError::Sqlite(error.to_string()))?;
    connection
        .execute_batch("PRAGMA foreign_keys = ON; DETACH DATABASE source_db;")
        .map_err(|error| WcError::Sqlite(error.to_string()))?;

    if !source_has_memberships {
        connection
            .execute(
                "INSERT OR IGNORE INTO wallpaper_sources
                 (wallpaper_id, source_id, last_seen_at)
                 SELECT id, source_id, last_seen FROM wallpapers WHERE source_id IS NOT NULL",
                [],
            )
            .map_err(|error| WcError::Sqlite(error.to_string()))?;
    }
    rebuild_wallpapers_fts(connection)?;
    connection
        .execute(
            "INSERT OR REPLACE INTO db_meta (key, value)
             VALUES ('schema_version', ?1)",
            [CURRENT_SCHEMA_VERSION.to_string()],
        )
        .map_err(|error| WcError::Sqlite(error.to_string()))?;
    connection
        .execute(
            "INSERT OR REPLACE INTO db_meta (key, value)
             VALUES ('fts_schema_version', ?1)",
            [FTS_SCHEMA_VERSION],
        )
        .map_err(|error| WcError::Sqlite(error.to_string()))?;
    connection
        .pragma_update(None, "user_version", CURRENT_SCHEMA_VERSION)
        .map_err(|error| WcError::Sqlite(error.to_string()))?;
    Ok(())
}

fn integrity_check(connection: &Connection, context: &str) -> Result<(), WcError> {
    let mut statement = connection
        .prepare("PRAGMA integrity_check")
        .map_err(|error| WcError::Sqlite(error.to_string()))?;
    let results = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| WcError::Sqlite(error.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| WcError::Sqlite(error.to_string()))?;
    if results.as_slice() != ["ok"] {
        return Err(WcError::Other(format!(
            "{context}: integrity_check failed: {}",
            results.join(", ")
        )));
    }
    Ok(())
}

fn validate_current_database(connection: &Connection, context: &str) -> Result<(), WcError> {
    let version = database_version(connection)?;
    if version != CURRENT_SCHEMA_VERSION {
        return Err(WcError::Other(format!(
            "{context}: expected schema version {CURRENT_SCHEMA_VERSION}, found {version}"
        )));
    }
    for table in CURRENT_PERSISTENT_TABLES {
        if !table_exists(connection, "main", table)? {
            return Err(WcError::Other(format!(
                "{context}: required table {table} is missing"
            )));
        }
    }
    validate_current_schema_objects(connection)?;
    integrity_check(connection, context)?;
    let mut foreign_keys = connection
        .prepare("PRAGMA foreign_key_check")
        .map_err(|error| WcError::Sqlite(error.to_string()))?;
    if foreign_keys
        .query([])
        .map_err(|error| WcError::Sqlite(error.to_string()))?
        .next()
        .map_err(|error| WcError::Sqlite(error.to_string()))?
        .is_some()
    {
        return Err(WcError::Other(format!(
            "{context}: foreign_key_check failed"
        )));
    }
    let wallpaper_count = wallpapers_count(connection)?;
    let fts_count = wallpapers_fts_count(connection)?;
    if wallpaper_count != fts_count || check_wallpapers_fts_integrity(connection).is_err() {
        return Err(WcError::Other(format!(
            "{context}: wallpapers FTS integrity check failed"
        )));
    }
    let schema_marker: String = connection
        .query_row(
            "SELECT value FROM db_meta WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        )
        .map_err(|error| WcError::Sqlite(error.to_string()))?;
    if schema_marker != CURRENT_SCHEMA_VERSION.to_string() {
        return Err(WcError::Other(format!(
            "{context}: schema marker is {schema_marker}, expected {CURRENT_SCHEMA_VERSION}"
        )));
    }
    Ok(())
}

fn finalize_temporary_database(
    connection: Connection,
    path: &Path,
    context: &str,
) -> Result<(), WcError> {
    validate_current_database(&connection, context)?;
    checkpoint_wal(&connection, context)?;
    let journal_mode: String = connection
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .map_err(|error| WcError::Sqlite(error.to_string()))?;
    if !journal_mode.eq_ignore_ascii_case("delete") {
        connection
            .execute_batch("PRAGMA journal_mode = DELETE;")
            .map_err(|error| WcError::Sqlite(error.to_string()))?;
    }
    close_connection(connection)?;
    remove_database_sidecars(path)
}

const RESTORE_REQUIRED_TABLES: &[&str] = &[
    "db_meta",
    "config",
    "sources",
    "wallpapers",
    "favorites",
    "history",
    "state",
];

fn validate_restore_source(connection: &Connection, context: &str) -> Result<(), WcError> {
    reject_future_schema(connection)?;
    integrity_check(connection, context)?;
    for table in RESTORE_REQUIRED_TABLES {
        if !table_exists(connection, "main", table)? {
            return Err(WcError::Other(format!(
                "{context}: required table {table} is missing"
            )));
        }
    }
    Ok(())
}

fn stage_restore_candidate(
    db_path: &Path,
    backup_path: &Path,
) -> Result<TemporaryDatabase, WcError> {
    stage_restore_candidate_with_paths(backup_path, || {
        unique_database_sibling(db_path, "restore.tmp")
    })
}

fn stage_restore_candidate_with_paths<F>(
    backup_path: &Path,
    mut next_temporary_path: F,
) -> Result<TemporaryDatabase, WcError>
where
    F: FnMut() -> PathBuf,
{
    let temporary = reserve_empty_temporary_database(&mut next_temporary_path)?;
    let source = Connection::open_with_flags(backup_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| WcError::Other(format!("not a valid SQLite database: {error}")))?;
    source
        .busy_timeout(Duration::from_secs(5))
        .map_err(|error| WcError::Sqlite(error.to_string()))?;
    validate_restore_source(&source, "restore source")?;
    vacuum_into(&source, &temporary.path)?;
    close_connection(source)?;

    let candidate = Connection::open_with_flags(&temporary.path, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|error| WcError::Sqlite(error.to_string()))?;
    candidate
        .busy_timeout(Duration::from_secs(5))
        .map_err(|error| WcError::Sqlite(error.to_string()))?;
    if database_version(&candidate)? < CURRENT_SCHEMA_VERSION {
        create_schema(&candidate)?;
    }
    rebuild_wallpapers_fts(&candidate)?;
    finalize_temporary_database(candidate, &temporary.path, "restore candidate")?;
    Ok(temporary)
}

fn consistent_backup(connection: &Connection, destination: &Path) -> Result<(), WcError> {
    if let Err(error) = vacuum_into(connection, destination) {
        cleanup_database(destination);
        return Err(error);
    }
    Ok(())
}

fn migrate_repair_source_if_needed(
    db_path: &Path,
    backup_path: &Path,
    source_version: i64,
) -> Result<Option<TemporaryDatabase>, WcError> {
    if source_version >= CURRENT_SCHEMA_VERSION {
        return Ok(None);
    }
    let migrated =
        reserve_empty_temporary_database(|| unique_database_sibling(db_path, "repair-source.tmp"))?;
    std::fs::copy(backup_path, &migrated.path).map_err(WcError::Io)?;
    let connection =
        Connection::open(&migrated.path).map_err(|error| WcError::Sqlite(error.to_string()))?;
    connection
        .busy_timeout(Duration::from_secs(5))
        .map_err(|error| WcError::Sqlite(error.to_string()))?;
    create_schema(&connection)?;
    finalize_temporary_database(connection, &migrated.path, "repair migration source")?;
    Ok(Some(migrated))
}

/// Repair the SQLite database without importing from flat files.
/// Rebuilds schema and FTS while preserving existing SQLite data.
pub fn repair(cd: &ConfigDir) -> Result<(), WcError> {
    repair_with_seam(cd, || {})
}

fn repair_with_seam(cd: &ConfigDir, after_exclusive_lock: impl FnOnce()) -> Result<(), WcError> {
    repair_with_candidates(cd, after_exclusive_lock, || {
        unique_database_sibling(&cd.db_path(), "repair.tmp")
    })
}

fn repair_with_candidates<F>(
    cd: &ConfigDir,
    after_exclusive_lock: impl FnOnce(),
    mut next_temporary_path: F,
) -> Result<(), WcError>
where
    F: FnMut() -> PathBuf,
{
    let _maintenance = acquire_maintenance_lock(cd)?;
    after_exclusive_lock();
    let db_path = cd.db_path();
    if !db_path.exists() {
        return Err(WcError::Other(
            "wallpapers.db not found; initialize SQLite storage first.".into(),
        ));
    }

    let source = Connection::open(&db_path).map_err(|error| WcError::Sqlite(error.to_string()))?;
    source
        .busy_timeout(Duration::from_secs(5))
        .map_err(|error| WcError::Sqlite(error.to_string()))?;
    let source_version = reject_future_schema(&source)?;

    let backup = reserve_visible_database_backup(&db_path, "bak")?;
    vacuum_into(&source, &backup.path)?;
    let backup_path = backup.preserve();
    checkpoint_wal(&source, "repair source")?;
    close_connection(source)?;
    remove_database_sidecars(&db_path)?;

    let migrated_source = migrate_repair_source_if_needed(&db_path, &backup_path, source_version)?;
    let copy_source_path = migrated_source
        .as_ref()
        .map(|source| source.path.as_path())
        .unwrap_or(&backup_path);

    let temporary = reserve_empty_temporary_database(&mut next_temporary_path)?;
    let rebuilt =
        Connection::open(&temporary.path).map_err(|error| WcError::Sqlite(error.to_string()))?;
    create_schema(&rebuilt)?;
    copy_persistent_data(&rebuilt, copy_source_path)?;
    finalize_temporary_database(rebuilt, &temporary.path, "repair candidate")?;
    temporary.publish(&db_path)
}

#[deprecated(note = "use repair(); resync no longer imports flat files")]
pub fn resync(cd: &ConfigDir) -> Result<(), WcError> {
    repair(cd)
}

/// Export SQLite back to flat files atomically.
pub fn export_flat(cd: &ConfigDir) -> Result<(), WcError> {
    let db_path = cd.db_path();
    if !db_path.exists() {
        return Err(WcError::Other(
            "wallpapers.db not found; initialize SQLite storage first.".into(),
        ));
    }
    let conn = open_runtime_connection(cd)?;

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
    let source = open_runtime_connection(cd)?;
    let backup_file = reserve_visible_database_backup(&db_path, "bak")?;
    let result = (|| {
        consistent_backup(&source, &backup_file.path)?;
        let candidate =
            Connection::open_with_flags(&backup_file.path, OpenFlags::SQLITE_OPEN_READ_WRITE)
                .map_err(|error| WcError::Sqlite(error.to_string()))?;
        finalize_temporary_database(candidate, &backup_file.path, "backup candidate")
    })();
    drop(source);
    result?;
    let backup_path = backup_file.preserve();
    Ok(backup_path.to_string_lossy().to_string())
}

/// Restore wallpapers.db from a backup file. Backs up current DB first.
pub fn restore(cd: &ConfigDir, backup_path: &Path) -> Result<(), WcError> {
    if !backup_path.exists() {
        return Err(WcError::Other(format!(
            "backup file not found: {}",
            backup_path.display()
        )));
    }
    let db_path = cd.db_path();
    let candidate = stage_restore_candidate(&db_path, backup_path)?;
    let _maintenance = acquire_maintenance_lock(cd)?;

    if db_path.exists() {
        let current = Connection::open_with_flags(&db_path, OpenFlags::SQLITE_OPEN_READ_WRITE)
            .map_err(|error| WcError::Sqlite(error.to_string()))?;
        current
            .busy_timeout(Duration::from_secs(5))
            .map_err(|error| WcError::Sqlite(error.to_string()))?;
        reject_future_schema(&current)?;
        let previous_file = reserve_visible_database_backup(&db_path, "pre-restore")?;
        consistent_backup(&current, &previous_file.path)?;
        checkpoint_wal(&current, "restore current database")?;
        close_connection(current)?;
        remove_database_sidecars(&db_path)?;
        let _previous_path = previous_file.preserve();
    }
    remove_database_sidecars(&db_path)?;
    candidate.publish(&db_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn restore_validation_fixture() -> (tempfile::TempDir, ConfigDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        crate::sqlite::try_ensure_sqlite_db(&cd).unwrap();
        let current = rusqlite::Connection::open(cd.db_path()).unwrap();
        current
            .execute(
                "INSERT INTO config (key, value) VALUES ('restore-sentinel', 'backup-value')",
                [],
            )
            .unwrap();
        drop(current);
        let backup_path = PathBuf::from(backup(&cd).unwrap());
        let current = rusqlite::Connection::open(cd.db_path()).unwrap();
        current
            .execute(
                "UPDATE config SET value = 'live-current' WHERE key = 'restore-sentinel'",
                [],
            )
            .unwrap();
        drop(current);
        (tmp, cd, backup_path)
    }

    fn assert_malformed_restore_rejected(cd: &ConfigDir, backup_path: &Path) {
        let result = restore(cd, backup_path);

        assert!(
            result.is_err(),
            "malformed current-version backup must be rejected"
        );
        let current = rusqlite::Connection::open(cd.db_path()).unwrap();
        assert_eq!(
            current
                .query_row(
                    "SELECT value FROM config WHERE key = 'restore-sentinel'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "live-current"
        );
    }

    fn rewrite_wallpapers_table_sql(path: &Path, rewrite: fn(&str) -> String) {
        let connection = rusqlite::Connection::open(path).unwrap();
        let original = connection
            .query_row(
                "SELECT sql FROM sqlite_master
                 WHERE type = 'table' AND name = 'wallpapers'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap();
        let rewritten = rewrite(&original);
        assert_ne!(rewritten, original, "test mutation must change table SQL");
        connection
            .execute_batch("PRAGMA writable_schema = ON;")
            .unwrap();
        connection
            .execute(
                "UPDATE sqlite_master SET sql = ?1
                 WHERE type = 'table' AND name = 'wallpapers'",
                [rewritten],
            )
            .unwrap();
        connection
            .execute_batch("PRAGMA writable_schema = OFF;")
            .unwrap();
        let schema_version = connection
            .pragma_query_value(None, "schema_version", |row| row.get::<_, i64>(0))
            .unwrap();
        connection
            .pragma_update(None, "schema_version", schema_version + 1)
            .unwrap();
    }

    fn ordinary_filename_column(sql: &str) -> String {
        replace_generated_filename_definition(sql, "filename TEXT")
    }

    fn stored_filename_column(sql: &str) -> String {
        sql.replacen(") VIRTUAL", ") STORED", 1)
    }

    fn wrong_filename_expression(sql: &str) -> String {
        sql.replacen(
            "substr(path, length(rtrim(path, replace(path, '/', ''))) + 1)",
            "path",
            1,
        )
    }

    fn replace_generated_filename_definition(sql: &str, replacement: &str) -> String {
        let start = sql.find("filename").expect("filename definition");
        let relative_end = sql[start..]
            .find(") VIRTUAL")
            .expect("virtual filename suffix");
        let end = start + relative_end + ") VIRTUAL".len();
        let mut rewritten = sql.to_string();
        rewritten.replace_range(start..end, replacement);
        rewritten
    }

    type FilenameSchemaMutation = fn(&str) -> String;
    const MALFORMED_FILENAME_MUTATIONS: [(&str, FilenameSchemaMutation); 3] = [
        ("ordinary", ordinary_filename_column),
        ("stored", stored_filename_column),
        ("wrong-expression", wrong_filename_expression),
    ];

    fn query_rows(
        conn: &rusqlite::Connection,
        sql: &str,
        column_count: usize,
    ) -> Vec<Vec<rusqlite::types::Value>> {
        let mut stmt = conn.prepare(sql).unwrap();
        stmt.query_map([], |row| {
            (0..column_count)
                .map(|index| row.get(index))
                .collect::<Result<Vec<rusqlite::types::Value>, _>>()
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
    }

    fn persistent_snapshot(
        conn: &rusqlite::Connection,
    ) -> std::collections::BTreeMap<&'static str, Vec<Vec<rusqlite::types::Value>>> {
        std::collections::BTreeMap::from([
            (
                "config",
                query_rows(conn, "SELECT key, value FROM config ORDER BY key", 2),
            ),
            (
                "sources",
                query_rows(
                    conn,
                    "SELECT id, path, display_name, kind, recursive, availability, added_at
                     FROM sources ORDER BY id",
                    7,
                ),
            ),
            (
                "wallpapers",
                query_rows(
                    conn,
                    "SELECT id, path, type, ext, backend, size, mtime, resolution,
                            project_type, preview_path, workshop_id, title, we_file,
                            unsupported_reason, source_id, last_seen, added_at, author
                     FROM wallpapers ORDER BY id",
                    18,
                ),
            ),
            (
                "wallpaper_sources",
                query_rows(
                    conn,
                    "SELECT wallpaper_id, source_id, last_seen_at
                     FROM wallpaper_sources ORDER BY wallpaper_id, source_id",
                    3,
                ),
            ),
            (
                "favorites",
                query_rows(
                    conn,
                    "SELECT id, path, added_at FROM favorites ORDER BY id",
                    3,
                ),
            ),
            (
                "history",
                query_rows(
                    conn,
                    "SELECT id, path, backend, applied_at FROM history ORDER BY id",
                    4,
                ),
            ),
            (
                "state",
                query_rows(conn, "SELECT key, value FROM state ORDER BY key", 2),
            ),
            (
                "display_state",
                query_rows(
                    conn,
                    "SELECT target_key, wallpaper_path, backend, updated_at
                     FROM display_state ORDER BY target_key",
                    4,
                ),
            ),
            (
                "db_meta",
                query_rows(
                    conn,
                    "SELECT key, value, updated_at FROM db_meta
                     WHERE key NOT IN
                         ('schema_version', 'fts_schema_version', 'fts_rebuilt_at')
                     ORDER BY key",
                    3,
                ),
            ),
            (
                "sqlite_sequence",
                query_rows(
                    conn,
                    "SELECT name, seq FROM sqlite_sequence ORDER BY name",
                    2,
                ),
            ),
        ])
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
    fn verify_warns_when_favorites_drift() {
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
            matches!(result, crate::sqlite::VerifyResult::OkWithWarnings(ref w) if w.iter().any(|s| s.contains("favorites"))),
            "favorites drift should be a warning, not an error: {:?}",
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
    fn verify_warns_when_history_drifts() {
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
            let conn = rusqlite::Connection::open(cd.db_path()).unwrap();
            conn.execute(
                "INSERT INTO history (path, backend, applied_at) VALUES (?1, 'test', 0)",
                rusqlite::params!["/walls/extra.jpg"],
            )
            .unwrap();
        }

        let result = crate::sqlite::verify(&cd).unwrap();
        assert!(
            matches!(result, crate::sqlite::VerifyResult::OkWithWarnings(ref w) if w.iter().any(|s| s.contains("history"))),
            "history drift should be a warning, not an error: {:?}",
            result
        );
    }

    #[test]
    fn verify_fails_on_fts_drift_even_with_config_warnings() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        wc_core::config::write_config_value(&cd.path, "storage_backend", "sqlite").unwrap();
        crate::sqlite::migrate_to_sqlite(&cd).unwrap();

        // Insert a wallpaper so there is a real mismatch with corrupted FTS
        {
            let conn = rusqlite::Connection::open(cd.db_path()).unwrap();
            conn.execute(
                "INSERT INTO wallpapers (path, type, ext, backend, size, mtime, resolution, title, workshop_id)
                 VALUES ('/walls/test.jpg', 'image', 'jpg', 'awww', 1, 1, '1x1', 'Test', '111')",
                [],
            )
            .unwrap();
            // Corrupt FTS: remove the auto-inserted FTS row to create a count mismatch
            conn.execute(
                "INSERT INTO wallpapers_fts(wallpapers_fts) VALUES ('delete-all')",
                [],
            )
            .unwrap();
        }

        // Config drift (warning) + FTS drift (fatal)
        wc_core::config::write_config_value(&cd.path, "extra_config", "val").unwrap();

        let result = crate::sqlite::verify(&cd).unwrap();
        assert!(
            matches!(result, crate::sqlite::VerifyResult::Failed(_)),
            "FTS errors should take priority over config warnings, got: {:?}",
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
        let conn = rusqlite::Connection::open(cd.db_path()).expect("should open db");
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
    fn verify_returns_err_when_membership_table_is_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        crate::sqlite::try_ensure_sqlite_db(&cd).unwrap();
        let connection = rusqlite::Connection::open(cd.db_path()).unwrap();
        connection
            .execute_batch("PRAGMA foreign_keys = OFF; DROP TABLE wallpaper_sources;")
            .unwrap();
        drop(connection);

        let result = verify(&cd);

        assert!(
            matches!(result, Err(WcError::Sqlite(ref error)) if error.contains("wallpaper_sources")),
            "missing membership table must not verify successfully: {result:?}"
        );
    }

    #[test]
    fn verify_fails_when_memberships_have_foreign_key_orphans() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        crate::sqlite::try_ensure_sqlite_db(&cd).unwrap();
        let connection = rusqlite::Connection::open(cd.db_path()).unwrap();
        connection
            .execute_batch("PRAGMA foreign_keys = OFF;")
            .unwrap();
        connection
            .execute(
                "INSERT INTO wallpaper_sources (wallpaper_id, source_id)
                 VALUES (999, 999)",
                [],
            )
            .unwrap();
        drop(connection);

        let result = verify(&cd).unwrap();

        assert!(
            matches!(result, VerifyResult::Failed(ref errors) if errors.contains(&"foreign_keys".to_string())),
            "orphaned membership must fail verification: {result:?}"
        );
    }

    #[test]
    fn verify_fails_on_an_unexpected_data_changing_trigger() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        crate::sqlite::try_ensure_sqlite_db(&cd).unwrap();
        let connection = rusqlite::Connection::open(cd.db_path()).unwrap();
        connection
            .execute_batch(
                "CREATE TRIGGER delete_favorites_after_wallpaper_insert
                 AFTER INSERT ON wallpapers
                 BEGIN
                     DELETE FROM favorites;
                 END;",
            )
            .unwrap();
        drop(connection);

        let result = verify(&cd).unwrap();

        assert!(
            matches!(result, VerifyResult::Failed(ref errors) if errors.iter().any(|error| error.starts_with("schema:"))),
            "unexpected data-changing trigger must fail verification: {result:?}"
        );
    }

    #[test]
    fn verify_fails_when_wallpaper_fts_count_drifts() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        wc_core::config::write_config_value(&cd.path, "storage_backend", "sqlite").unwrap();
        crate::sqlite::ensure_sqlite_db(&cd);
        let conn = rusqlite::Connection::open(cd.db_path()).unwrap();
        conn.execute(
            "INSERT INTO wallpapers (path, type, ext, backend, size, mtime, resolution, title, workshop_id)
             VALUES ('/walls/a.jpg', 'image', 'jpg', 'awww', 1, 1, '1x1', 'A', '111')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO wallpapers_fts(wallpapers_fts) VALUES ('delete-all')",
            [],
        )
        .unwrap();

        let result = crate::sqlite::verify(&cd).unwrap();

        match result {
            crate::sqlite::VerifyResult::Failed(errors) => {
                assert!(errors.contains(&"wallpapers_fts".to_string()), "{errors:?}");
            }
            other => panic!("expected FTS drift failure, got {other:?}"),
        }
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
            let conn = rusqlite::Connection::open(cd.db_path()).unwrap();
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
        flat::write_lines(&cd.history_path(), std::slice::from_ref(&sym_str)).unwrap();
        flat::favorites_add(&cd, "/walls/fav.jpg").unwrap();
        flat::sources_add(&cd, "/walls").unwrap();

        crate::sqlite::migrate_to_sqlite(&cd).unwrap();

        // SQLite history has the real path (different string, same canonical)
        {
            let conn = rusqlite::Connection::open(cd.db_path()).unwrap();
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
            let conn = rusqlite::Connection::open(cd.db_path()).unwrap();
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

    #[test]
    fn verify_sqlite_only_state_drift_is_warning_not_failed() {
        // Regression: P1-1 — write favorite/history/current via SQLite only,
        // leave flat files empty, verify() must return OkWithWarnings, not Failed.
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        crate::sqlite::ensure_sqlite_db(&cd);

        // Write data directly to SQLite
        {
            let conn = rusqlite::Connection::open(cd.db_path()).unwrap();
            conn.execute(
                "INSERT INTO favorites (path, added_at) VALUES ('/sqlite-only-fav.jpg', '2024-01-01T00:00:00')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO history (path, backend, applied_at) VALUES ('/sqlite-only-hist.jpg', 'awww', '2024-01-01T00:00:00')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT OR REPLACE INTO state (key, value) VALUES ('current', '/sqlite-only-cur.jpg')",
                [],
            )
            .unwrap();
        }

        let result = crate::sqlite::verify(&cd).unwrap();
        assert!(
            matches!(result, crate::sqlite::VerifyResult::OkWithWarnings(_)),
            "SQLite-only data with empty flat files should be OkWithWarnings, not Failed: {:?}",
            result
        );
    }

    #[test]
    fn repair_preserves_sqlite_only_data() {
        // Regression: P1-2 — create data in SQLite only, leave flat files empty,
        // call repair(), assert SQLite rows are preserved.
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        crate::sqlite::ensure_sqlite_db(&cd);

        {
            let conn = rusqlite::Connection::open(cd.db_path()).unwrap();
            conn.execute(
                "INSERT INTO sources (path, added_at) VALUES ('/src', '2024-01-01T00:00:00')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO favorites (path, added_at) VALUES ('/fav.jpg', '2024-01-01T00:00:00')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO history (path, backend, applied_at) VALUES ('/hist.jpg', 'awww', '2024-01-01T00:00:00')",
                [],
            )
            .unwrap();
        }

        crate::sqlite::repair(&cd).unwrap();

        let conn = rusqlite::Connection::open(cd.db_path()).unwrap();
        let src_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM sources", [], |r| r.get(0))
            .unwrap();
        let fav_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM favorites", [], |r| r.get(0))
            .unwrap();
        let hist_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM history", [], |r| r.get(0))
            .unwrap();
        assert_eq!(src_count, 1, "sources should be preserved after repair");
        assert_eq!(fav_count, 1, "favorites should be preserved after repair");
        assert_eq!(hist_count, 1, "history should be preserved after repair");

        // Flat files should still be empty (repair does not export)
        let flat_src = flat::sources_list(&cd).unwrap();
        let flat_fav = flat::favorites_list(&cd).unwrap();
        let flat_hist = flat::history_list(&cd).unwrap();
        assert!(
            flat_src.is_empty(),
            "flat sources should be empty after SQLite-only repair"
        );
        assert!(
            flat_fav.is_empty(),
            "flat favorites should be empty after SQLite-only repair"
        );
        assert!(
            flat_hist.is_empty(),
            "flat history should be empty after SQLite-only repair"
        );
    }

    #[test]
    fn repair_preserves_all_display_state_rows_exactly() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        crate::sqlite::ensure_sqlite_db(&cd);

        let expected = vec![
            (
                "DP-1".to_string(),
                "/walls/dp.mp4".to_string(),
                "mpvpaper".to_string(),
                "2026-07-13T04:05:06Z".to_string(),
            ),
            (
                crate::sqlite::ALL_DISPLAYS_TARGET_KEY.to_string(),
                "/walls/all.jpg".to_string(),
                "awww".to_string(),
                "2026-07-13T01:02:03Z".to_string(),
            ),
            (
                "eDP-1".to_string(),
                "/walls/edp.jpg".to_string(),
                "awww".to_string(),
                "2026-07-13T07:08:09Z".to_string(),
            ),
        ];
        {
            let conn = rusqlite::Connection::open(cd.db_path()).unwrap();
            conn.execute(
                "INSERT OR REPLACE INTO state (key, value) VALUES ('current', '/legacy.jpg')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT OR REPLACE INTO state (key, value) VALUES ('last_backend', 'awww')",
                [],
            )
            .unwrap();
            for (target_key, wallpaper_path, backend, updated_at) in &expected {
                conn.execute(
                    "INSERT INTO display_state
                     (target_key, wallpaper_path, backend, updated_at)
                     VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params![target_key, wallpaper_path, backend, updated_at],
                )
                .unwrap();
            }
        }

        crate::sqlite::repair(&cd).unwrap();
        crate::sqlite::try_ensure_sqlite_db(&cd).unwrap();

        let conn = rusqlite::Connection::open(cd.db_path()).unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT target_key, wallpaper_path, backend, updated_at
                 FROM display_state ORDER BY target_key",
            )
            .unwrap();
        let actual = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(actual, expected);
    }

    #[test]
    fn repair_preserves_named_rows_and_migration_marker_without_resurrecting_all_displays() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        crate::sqlite::ensure_sqlite_db(&cd);

        {
            let conn = rusqlite::Connection::open(cd.db_path()).unwrap();
            conn.execute(
                "INSERT OR REPLACE INTO state (key, value) VALUES ('current', '/legacy.jpg')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT OR REPLACE INTO state (key, value) VALUES ('last_backend', 'awww')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO display_state
                 (target_key, wallpaper_path, backend, updated_at)
                 VALUES ('DP-1', '/walls/dp.mp4', 'mpvpaper', '2026-07-13T10:11:12Z')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO display_state
                 (target_key, wallpaper_path, backend, updated_at)
                 VALUES ('eDP-1', '/walls/edp.jpg', 'awww', '2026-07-13T13:14:15Z')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT OR REPLACE INTO db_meta (key, value, updated_at)
                 VALUES (?1, '1', '2026-07-13T16:17:18Z')",
                rusqlite::params![crate::sqlite::LEGACY_DISPLAY_STATE_MIGRATED_META_KEY],
            )
            .unwrap();
        }

        crate::sqlite::repair(&cd).unwrap();
        // Re-open through the startup schema path. Losing the durable marker
        // here must not let retained legacy state recreate All Displays.
        crate::sqlite::try_ensure_sqlite_db(&cd).unwrap();

        let conn = rusqlite::Connection::open(cd.db_path()).unwrap();
        let rows: Vec<(String, String, String, String)> = {
            let mut stmt = conn
                .prepare(
                    "SELECT target_key, wallpaper_path, backend, updated_at
                     FROM display_state ORDER BY target_key",
                )
                .unwrap();
            stmt.query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
        };
        assert_eq!(
            rows,
            vec![
                (
                    "DP-1".to_string(),
                    "/walls/dp.mp4".to_string(),
                    "mpvpaper".to_string(),
                    "2026-07-13T10:11:12Z".to_string(),
                ),
                (
                    "eDP-1".to_string(),
                    "/walls/edp.jpg".to_string(),
                    "awww".to_string(),
                    "2026-07-13T13:14:15Z".to_string(),
                ),
            ]
        );

        let marker: (String, String) = conn
            .query_row(
                "SELECT value, updated_at FROM db_meta WHERE key = ?1",
                rusqlite::params![crate::sqlite::LEGACY_DISPLAY_STATE_MIGRATED_META_KEY],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            marker,
            ("1".to_string(), "2026-07-13T16:17:18Z".to_string())
        );
        assert_eq!(
            rows.iter()
                .filter(|(target, _, _, _)| { target == crate::sqlite::ALL_DISPLAYS_TARGET_KEY })
                .count(),
            0,
            "legacy keys must not resurrect All Displays after repair"
        );
    }

    #[test]
    fn repair_preserves_v3_fields_ids_memberships_and_sequence_high_water() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        crate::sqlite::try_ensure_sqlite_db(&cd).unwrap();
        let conn = rusqlite::Connection::open(cd.db_path()).unwrap();
        conn.execute(
            "INSERT INTO config (key, value) VALUES ('custom-config', 'custom-value')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sources
             (id, path, display_name, kind, recursive, availability, added_at)
             VALUES (41, '/sources/workshop', 'Workshop', 'wallpaper_engine_workshop',
                     0, 'offline', '2025-01-02T03:04:05Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO wallpapers
             (id, path, type, ext, backend, size, mtime, resolution,
              project_type, preview_path, workshop_id, title, we_file,
              unsupported_reason, source_id, last_seen, added_at, author)
             VALUES (73, '/sources/workshop/123', 'we_scene', 'scene',
                     'linux-wallpaperengine', 12345, 67890, 'WE', 'we_scene',
                     '/sources/workshop/123/preview.gif', '123', 'Scene Title',
                     'scene.json', 'renderer-limit', 41,
                     '2025-02-03T04:05:06Z', '2025-02-01T01:02:03Z',
                     'Scene Studio')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO wallpaper_sources (wallpaper_id, source_id, last_seen_at)
             VALUES (73, 41, '2025-02-04T05:06:07Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO favorites (id, path, added_at)
             VALUES (12, '/sources/workshop/123', '2025-03-01T00:00:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO history (id, path, backend, applied_at)
             VALUES (25, '/sources/workshop/123', 'linux-wallpaperengine',
                     '2025-03-02T00:00:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO state (key, value) VALUES ('custom-state', 'state-value')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO display_state (target_key, wallpaper_path, backend, updated_at)
             VALUES ('DP-9', '/sources/workshop/123', 'linux-wallpaperengine',
                     '2025-03-03T00:00:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO db_meta (key, value, updated_at)
             VALUES ('custom-persistent', 'meta-value', '2025-03-04T00:00:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO db_meta (key, value, updated_at)
             VALUES ('resynced_at', 'legacy-repair-marker', '2025-03-05T00:00:00Z')",
            [],
        )
        .unwrap();
        for (table, sequence) in [
            ("sources", 141_i64),
            ("wallpapers", 173),
            ("favorites", 112),
            ("history", 125),
        ] {
            conn.execute(
                "UPDATE sqlite_sequence SET seq = ?2 WHERE name = ?1",
                rusqlite::params![table, sequence],
            )
            .unwrap();
        }
        let before = persistent_snapshot(&conn);
        drop(conn);

        repair(&cd).unwrap();

        let conn = rusqlite::Connection::open(cd.db_path()).unwrap();
        let after = persistent_snapshot(&conn);
        assert_eq!(after, before);
        assert_eq!(
            conn.query_row(
                "SELECT author, filename FROM wallpapers WHERE id = 73",
                [],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .unwrap(),
            ("Scene Studio".into(), "123".into())
        );
        assert_eq!(
            conn.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            crate::sqlite::CURRENT_SCHEMA_VERSION
        );
        assert_eq!(
            conn.query_row(
                "SELECT value FROM db_meta WHERE key = 'schema_version'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
            crate::sqlite::CURRENT_SCHEMA_VERSION.to_string()
        );
        assert_eq!(
            wallpapers_count(&conn).unwrap(),
            wallpapers_fts_count(&conn).unwrap()
        );
        check_wallpapers_fts_integrity(&conn).unwrap();
    }

    #[test]
    fn repair_rebuilds_malformed_filename_columns_and_preserves_author() {
        for (label, mutation) in MALFORMED_FILENAME_MUTATIONS {
            let tmp = tempfile::tempdir().unwrap();
            let cd = ConfigDir {
                path: tmp.path().join(format!("wallpaper-console-{label}")),
            };
            cd.init().unwrap();
            crate::sqlite::try_ensure_sqlite_db(&cd).unwrap();
            let connection = rusqlite::Connection::open(cd.db_path()).unwrap();
            connection
                .execute(
                    "INSERT INTO wallpapers
                     (id, path, type, ext, backend, author)
                     VALUES (9, '/walls/repaired.jpg', 'image', 'jpg', 'awww',
                             'Repair Studio')",
                    [],
                )
                .unwrap();
            drop(connection);
            rewrite_wallpapers_table_sql(&cd.db_path(), mutation);

            repair(&cd).unwrap_or_else(|error| panic!("{label} repair failed: {error}"));

            let repaired = rusqlite::Connection::open(cd.db_path()).unwrap();
            assert_eq!(
                repaired
                    .query_row(
                        "SELECT author, filename FROM wallpapers WHERE id = 9",
                        [],
                        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                    )
                    .unwrap(),
                ("Repair Studio".into(), "repaired.jpg".into()),
                "{label}"
            );
            let hidden: i64 = repaired
                .query_row(
                    "SELECT hidden FROM pragma_table_xinfo('wallpapers')
                     WHERE name = 'filename'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(hidden, 2, "{label}");
        }
    }

    #[test]
    fn repair_migrates_v1_source_kinds_overlaps_and_duplicate_wallpapers_before_copying() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        let parent = tmp.path().join("walls");
        let child = parent.join("nested");
        let workshop = tmp.path().join("Steam/steamapps/workshop/content/431960");
        std::fs::create_dir_all(&child).unwrap();
        std::fs::create_dir_all(&workshop).unwrap();
        let wallpaper = child.join("same.jpg");
        std::fs::write(&wallpaper, b"wallpaper").unwrap();
        let connection = rusqlite::Connection::open(cd.db_path()).unwrap();
        connection
            .execute_batch(
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
        for (id, path) in [(1_i64, &parent), (2, &child), (3, &workshop)] {
            connection
                .execute(
                    "INSERT INTO sources (id, path) VALUES (?1, ?2)",
                    rusqlite::params![id, path.to_string_lossy()],
                )
                .unwrap();
        }
        for id in [10_i64, 11_i64] {
            connection
                .execute(
                    "INSERT INTO wallpapers
                     (id, path, type, ext, backend, source_id)
                     VALUES (?1, ?2, 'image', 'jpg', 'awww', 2)",
                    rusqlite::params![id, wallpaper.to_string_lossy()],
                )
                .unwrap();
        }
        drop(connection);

        repair(&cd).unwrap();

        let repaired = rusqlite::Connection::open(cd.db_path()).unwrap();
        assert_eq!(
            repaired
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            CURRENT_SCHEMA_VERSION
        );
        let sources = query_rows(
            &repaired,
            "SELECT id, kind, recursive FROM sources ORDER BY id",
            3,
        );
        assert_eq!(
            sources,
            vec![
                vec![1.into(), String::from("directory").into(), 1.into()],
                vec![2.into(), String::from("directory").into(), 1.into()],
                vec![
                    3.into(),
                    String::from("wallpaper_engine_workshop").into(),
                    0.into(),
                ],
            ]
        );
        assert_eq!(
            repaired
                .query_row("SELECT COUNT(*) FROM wallpapers", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            1
        );
        assert_eq!(
            repaired
                .query_row(
                    "SELECT COUNT(*) FROM wallpapers WHERE id = 10 AND source_id IS NULL",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
        assert_eq!(
            repaired
                .query_row("SELECT COUNT(*) FROM wallpaper_sources", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            2
        );
    }

    #[test]
    fn repair_does_not_publish_rows_from_a_stale_temporary_database() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        crate::sqlite::try_ensure_sqlite_db(&cd).unwrap();
        let stale_path = cd.path.join("stale-repair.db");
        let stale = rusqlite::Connection::open(&stale_path).unwrap();
        create_schema(&stale).unwrap();
        stale
            .execute(
                "INSERT INTO config (key, value) VALUES ('stale-temp-row', 'must-not-publish')",
                [],
            )
            .unwrap();
        drop(stale);
        let fresh_path = cd.path.join("fresh-repair.db");
        let mut candidates = vec![stale_path, fresh_path].into_iter();

        repair_with_candidates(&cd, || {}, || candidates.next().unwrap()).unwrap();

        let repaired = rusqlite::Connection::open(cd.db_path()).unwrap();
        let stale_count: i64 = repaired
            .query_row(
                "SELECT COUNT(*) FROM config WHERE key = 'stale-temp-row'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stale_count, 0);
    }

    #[test]
    fn restore_candidate_skips_and_preserves_a_stale_temporary_database() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        crate::sqlite::try_ensure_sqlite_db(&cd).unwrap();
        let backup_path = PathBuf::from(backup(&cd).unwrap());
        let stale_path = cd.path.join("stale-restore.db");
        std::fs::write(&stale_path, b"must remain untouched").unwrap();
        let fresh_path = cd.path.join("fresh-restore.db");
        let mut candidates = vec![stale_path.clone(), fresh_path.clone()].into_iter();

        let candidate =
            stage_restore_candidate_with_paths(&backup_path, || candidates.next().unwrap())
                .unwrap();

        assert_eq!(candidate.path, fresh_path);
        assert_eq!(
            std::fs::read(&stale_path).unwrap(),
            b"must remain untouched"
        );
        let candidate_connection = rusqlite::Connection::open(&candidate.path).unwrap();
        assert_eq!(
            candidate_connection
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            crate::sqlite::CURRENT_SCHEMA_VERSION
        );
        drop(candidate_connection);
        drop(candidate);
        assert!(!fresh_path.exists());
    }

    #[test]
    fn repair_rejects_future_schema_without_modifying_database() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        crate::sqlite::try_ensure_sqlite_db(&cd).unwrap();
        let future_version = crate::sqlite::CURRENT_SCHEMA_VERSION + 1;
        let conn = rusqlite::Connection::open(cd.db_path()).unwrap();
        conn.execute(
            "INSERT INTO config (key, value) VALUES ('future-sentinel', 'untouched')",
            [],
        )
        .unwrap();
        conn.execute_batch("PRAGMA journal_mode = DELETE;").unwrap();
        conn.pragma_update(None, "user_version", future_version)
            .unwrap();
        drop(conn);

        let error = repair(&cd).expect_err("repair must not downgrade a future schema");

        assert!(
            error.to_string().contains("newer") || error.to_string().contains("version"),
            "{error}"
        );
        let conn = rusqlite::Connection::open(cd.db_path()).unwrap();
        assert_eq!(
            conn.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            future_version
        );
        assert_eq!(
            conn.query_row(
                "SELECT value FROM config WHERE key = 'future-sentinel'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
            "untouched"
        );
        assert_eq!(
            conn.pragma_query_value(None, "journal_mode", |row| row.get::<_, String>(0))
                .unwrap(),
            "delete"
        );
    }

    #[test]
    fn backup_includes_committed_rows_still_present_only_in_wal() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        crate::sqlite::try_ensure_sqlite_db(&cd).unwrap();
        let writer = crate::sqlite::open_runtime_connection(&cd).unwrap();
        writer
            .execute_batch("PRAGMA wal_autocheckpoint = 0;")
            .unwrap();
        writer
            .execute(
                "INSERT INTO config (key, value) VALUES ('wal-sentinel', 'committed')",
                [],
            )
            .unwrap();
        assert!(
            std::fs::metadata(cd.db_path().with_extension("db-wal"))
                .map(|metadata| metadata.len() > 0)
                .unwrap_or(false),
            "test requires a non-empty WAL"
        );

        let backup_path = backup(&cd).unwrap();

        let backup_conn = rusqlite::Connection::open(backup_path).unwrap();
        assert_eq!(
            backup_conn
                .query_row(
                    "SELECT value FROM config WHERE key = 'wal-sentinel'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "committed"
        );
        drop(writer);
    }

    #[test]
    fn restore_rejects_invalid_backup_without_modifying_current_database() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        crate::sqlite::try_ensure_sqlite_db(&cd).unwrap();
        let conn = rusqlite::Connection::open(cd.db_path()).unwrap();
        conn.execute(
            "INSERT INTO config (key, value) VALUES ('restore-sentinel', 'current')",
            [],
        )
        .unwrap();
        drop(conn);
        let invalid = tmp.path().join("invalid.db");
        std::fs::write(&invalid, b"this is not sqlite").unwrap();

        let result = restore(&cd, &invalid);

        assert!(result.is_err(), "invalid restore input must be rejected");
        let conn = rusqlite::Connection::open(cd.db_path()).unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT value FROM config WHERE key = 'restore-sentinel'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
            "current"
        );
    }

    #[test]
    fn restore_removes_stale_sidecars_when_the_main_database_is_missing() {
        let (_tmp, cd, backup_path) = restore_validation_fixture();
        std::fs::remove_file(cd.db_path()).unwrap();
        for suffix in ["-wal", "-shm", "-journal"] {
            std::fs::write(database_sidecar(&cd.db_path(), suffix), b"stale sidecar").unwrap();
        }

        restore(&cd, &backup_path).unwrap();

        for suffix in ["-wal", "-shm", "-journal"] {
            assert!(!database_sidecar(&cd.db_path(), suffix).exists());
        }
        let restored = rusqlite::Connection::open(cd.db_path()).unwrap();
        assert_eq!(
            restored
                .query_row(
                    "SELECT value FROM config WHERE key = 'restore-sentinel'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "backup-value"
        );
    }

    #[test]
    fn restore_rejects_current_version_backup_missing_required_schema_objects() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        crate::sqlite::try_ensure_sqlite_db(&cd).unwrap();
        let current = rusqlite::Connection::open(cd.db_path()).unwrap();
        current
            .execute(
                "INSERT INTO config (key, value) VALUES ('restore-sentinel', 'backup-value')",
                [],
            )
            .unwrap();
        drop(current);
        let backup_path = PathBuf::from(backup(&cd).unwrap());
        let malformed = rusqlite::Connection::open(&backup_path).unwrap();
        malformed
            .execute_batch(
                "DROP TRIGGER wallpapers_ai;
                 DROP TRIGGER wallpapers_ad;
                 DROP TRIGGER wallpapers_au;
                 DROP TRIGGER wallpapers_added_at_ai;
                 DROP INDEX idx_wallpapers_path;
                 DROP INDEX idx_wallpapers_type;",
            )
            .unwrap();
        malformed
            .execute(
                "UPDATE config SET value = 'malformed-backup' WHERE key = 'restore-sentinel'",
                [],
            )
            .unwrap();
        drop(malformed);
        let current = rusqlite::Connection::open(cd.db_path()).unwrap();
        current
            .execute(
                "UPDATE config SET value = 'live-current' WHERE key = 'restore-sentinel'",
                [],
            )
            .unwrap();
        drop(current);

        let result = restore(&cd, &backup_path);

        assert!(
            result.is_err(),
            "malformed current-version backup must be rejected"
        );
        let current = rusqlite::Connection::open(cd.db_path()).unwrap();
        assert_eq!(
            current
                .query_row(
                    "SELECT value FROM config WHERE key = 'restore-sentinel'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "live-current"
        );
    }

    #[test]
    fn restore_rejects_current_version_backup_with_modified_fts_definition() {
        let (_tmp, cd, backup_path) = restore_validation_fixture();
        let malformed = rusqlite::Connection::open(&backup_path).unwrap();
        malformed
            .execute_batch(
                "DROP TABLE wallpapers_fts;
                 CREATE VIRTUAL TABLE wallpapers_fts USING fts5(
                     path,
                     title,
                     workshop_id,
                     project_type,
                     content='wallpapers',
                     content_rowid='id',
                     tokenize='porter ascii'
                 );
                 INSERT INTO wallpapers_fts(wallpapers_fts) VALUES ('rebuild');
                 UPDATE config SET value = 'malformed-backup'
                 WHERE key = 'restore-sentinel';",
            )
            .unwrap();
        drop(malformed);

        assert_malformed_restore_rejected(&cd, &backup_path);
    }

    #[test]
    fn restore_rejects_current_version_backup_with_unexpected_data_changing_trigger() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        crate::sqlite::try_ensure_sqlite_db(&cd).unwrap();
        let current = rusqlite::Connection::open(cd.db_path()).unwrap();
        current
            .execute(
                "INSERT INTO config (key, value) VALUES ('restore-sentinel', 'backup-value')",
                [],
            )
            .unwrap();
        current
            .execute("INSERT INTO favorites (path) VALUES ('/keep.jpg')", [])
            .unwrap();
        drop(current);
        let backup_path = PathBuf::from(backup(&cd).unwrap());
        let malformed = rusqlite::Connection::open(&backup_path).unwrap();
        malformed
            .execute_batch(
                "CREATE TRIGGER delete_favorites_after_wallpaper_insert
                 AFTER INSERT ON wallpapers
                 BEGIN
                     DELETE FROM favorites;
                 END;
                 UPDATE config SET value = 'malformed-backup'
                 WHERE key = 'restore-sentinel';",
            )
            .unwrap();
        drop(malformed);
        let current = rusqlite::Connection::open(cd.db_path()).unwrap();
        current
            .execute(
                "UPDATE config SET value = 'live-current' WHERE key = 'restore-sentinel'",
                [],
            )
            .unwrap();
        drop(current);

        let result = restore(&cd, &backup_path);

        assert!(
            result.is_err(),
            "a current-version backup with an unexpected trigger must be rejected"
        );
        let current = rusqlite::Connection::open(cd.db_path()).unwrap();
        assert_eq!(
            current
                .query_row(
                    "SELECT value FROM config WHERE key = 'restore-sentinel'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "live-current"
        );
        assert_eq!(
            current
                .query_row(
                    "SELECT COUNT(*) FROM favorites WHERE path = '/keep.jpg'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
    }

    #[test]
    fn restore_rejects_current_version_backup_with_modified_required_trigger() {
        let (_tmp, cd, backup_path) = restore_validation_fixture();
        let malformed = rusqlite::Connection::open(&backup_path).unwrap();
        malformed
            .execute_batch(
                "DROP TRIGGER wallpapers_ai;
                 CREATE TRIGGER wallpapers_ai
                 AFTER INSERT ON wallpapers
                 BEGIN
                     DELETE FROM favorites;
                 END;
                 UPDATE config SET value = 'malformed-backup'
                 WHERE key = 'restore-sentinel';",
            )
            .unwrap();
        drop(malformed);

        assert_malformed_restore_rejected(&cd, &backup_path);
    }

    #[test]
    fn restore_rejects_current_version_backup_with_wrong_required_index_definition() {
        let (_tmp, cd, backup_path) = restore_validation_fixture();
        let malformed = rusqlite::Connection::open(&backup_path).unwrap();
        malformed
            .execute_batch(
                "DROP INDEX idx_wallpapers_mtime;
                 CREATE INDEX idx_wallpapers_mtime ON wallpapers(size);
                 UPDATE config SET value = 'malformed-backup'
                 WHERE key = 'restore-sentinel';",
            )
            .unwrap();
        drop(malformed);

        assert_malformed_restore_rejected(&cd, &backup_path);
    }

    #[test]
    fn restore_rejects_current_version_backup_without_source_value_checks() {
        let (_tmp, cd, backup_path) = restore_validation_fixture();
        let malformed = rusqlite::Connection::open(&backup_path).unwrap();
        malformed
            .execute_batch(
                "PRAGMA foreign_keys = OFF;
                 PRAGMA legacy_alter_table = ON;
                 ALTER TABLE sources RENAME TO sources_old;
                 CREATE TABLE sources (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     path TEXT NOT NULL UNIQUE,
                     added_at TEXT NOT NULL DEFAULT (datetime('now')),
                     display_name TEXT NOT NULL DEFAULT '',
                     kind TEXT NOT NULL DEFAULT 'directory',
                     recursive INTEGER NOT NULL DEFAULT 1,
                     availability TEXT NOT NULL DEFAULT 'unknown'
                 );
                 INSERT INTO sources
                     (id, path, added_at, display_name, kind, recursive, availability)
                     SELECT id, path, added_at, display_name, kind, recursive, availability
                     FROM sources_old;
                 DROP TABLE sources_old;
                 PRAGMA legacy_alter_table = OFF;
                 UPDATE config SET value = 'malformed-backup'
                 WHERE key = 'restore-sentinel';",
            )
            .unwrap();
        drop(malformed);

        assert_malformed_restore_rejected(&cd, &backup_path);
    }

    #[test]
    fn restore_rejects_current_version_backup_with_narrowed_source_value_checks() {
        let (_tmp, cd, backup_path) = restore_validation_fixture();
        let malformed = rusqlite::Connection::open(&backup_path).unwrap();
        malformed
            .execute_batch(
                "PRAGMA foreign_keys = OFF;
                 PRAGMA legacy_alter_table = ON;
                 ALTER TABLE sources RENAME TO sources_old;
                 CREATE TABLE sources (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     path TEXT NOT NULL UNIQUE,
                     added_at TEXT NOT NULL DEFAULT (datetime('now')),
                     display_name TEXT NOT NULL DEFAULT '',
                     kind TEXT NOT NULL DEFAULT 'directory' CHECK (kind = 'directory'),
                     recursive INTEGER NOT NULL DEFAULT 1 CHECK (recursive = 1),
                     availability TEXT NOT NULL DEFAULT 'unknown'
                         CHECK (availability = 'unknown')
                 );
                 INSERT INTO sources
                     (id, path, added_at, display_name, kind, recursive, availability)
                     SELECT id, path, added_at, display_name, kind, recursive, availability
                     FROM sources_old;
                 DROP TABLE sources_old;
                 PRAGMA legacy_alter_table = OFF;
                 UPDATE config SET value = 'malformed-backup'
                 WHERE key = 'restore-sentinel';",
            )
            .unwrap();
        drop(malformed);

        assert_malformed_restore_rejected(&cd, &backup_path);
    }

    #[test]
    fn restore_rejects_current_version_backup_with_narrowed_wallpaper_type_check() {
        let (_tmp, cd, backup_path) = restore_validation_fixture();
        let malformed = rusqlite::Connection::open(&backup_path).unwrap();
        malformed
            .execute_batch(
                "PRAGMA foreign_keys = OFF;
                 PRAGMA legacy_alter_table = ON;
                 ALTER TABLE wallpapers RENAME TO wallpapers_old;
                 CREATE TABLE wallpapers (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     path TEXT NOT NULL,
                     type TEXT NOT NULL CHECK/**/(type = 'image'),
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
                 INSERT INTO wallpapers
                     (id, path, type, ext, backend, size, mtime, resolution,
                      project_type, preview_path, workshop_id, title, we_file,
                      unsupported_reason, source_id, last_seen, added_at)
                     SELECT id, path, type, ext, backend, size, mtime, resolution,
                            project_type, preview_path, workshop_id, title, we_file,
                            unsupported_reason, source_id, last_seen, added_at
                     FROM wallpapers_old;
                 DROP TABLE wallpapers_old;
                 PRAGMA legacy_alter_table = OFF;",
            )
            .unwrap();
        create_schema(&malformed).unwrap();
        malformed
            .execute(
                "UPDATE config SET value = 'malformed-backup'
                 WHERE key = 'restore-sentinel'",
                [],
            )
            .unwrap();
        drop(malformed);

        assert_malformed_restore_rejected(&cd, &backup_path);
    }

    #[test]
    fn restore_rejects_current_version_backup_with_partial_path_uniqueness() {
        let (_tmp, cd, backup_path) = restore_validation_fixture();
        let malformed = rusqlite::Connection::open(&backup_path).unwrap();
        malformed
            .execute_batch(
                "PRAGMA foreign_keys = OFF;
                 PRAGMA legacy_alter_table = ON;
                 ALTER TABLE sources RENAME TO sources_old;
                 CREATE TABLE sources (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     path TEXT NOT NULL,
                     added_at TEXT NOT NULL DEFAULT (datetime('now')),
                     display_name TEXT NOT NULL DEFAULT '',
                     kind TEXT NOT NULL DEFAULT 'directory'
                         CHECK (kind IN ('directory', 'wallpaper_engine_workshop')),
                     recursive INTEGER NOT NULL DEFAULT 1 CHECK (recursive IN (0, 1)),
                     availability TEXT NOT NULL DEFAULT 'unknown'
                         CHECK (availability IN ('unknown', 'available', 'offline'))
                 );
                 CREATE UNIQUE INDEX sources_path_partial
                     ON sources(path) WHERE path <> '/dup';
                 INSERT INTO sources
                     (id, path, added_at, display_name, kind, recursive, availability)
                     SELECT id, path, added_at, display_name, kind, recursive, availability
                     FROM sources_old;
                 DROP TABLE sources_old;
                 PRAGMA legacy_alter_table = OFF;
                 UPDATE config SET value = 'malformed-backup'
                 WHERE key = 'restore-sentinel';",
            )
            .unwrap();
        drop(malformed);

        assert_malformed_restore_rejected(&cd, &backup_path);
    }

    #[test]
    fn restore_rejects_current_version_backup_without_monotonic_source_ids() {
        let (_tmp, cd, backup_path) = restore_validation_fixture();
        let malformed = rusqlite::Connection::open(&backup_path).unwrap();
        malformed
            .execute_batch(
                "PRAGMA foreign_keys = OFF;
                 PRAGMA legacy_alter_table = ON;
                 ALTER TABLE sources RENAME TO sources_old;
                 CREATE TABLE sources (
                     id INTEGER PRIMARY KEY,
                     path TEXT NOT NULL UNIQUE,
                     added_at TEXT NOT NULL DEFAULT (datetime('now')),
                     display_name TEXT NOT NULL DEFAULT '',
                     kind TEXT NOT NULL DEFAULT 'directory'
                         CHECK (kind IN ('directory', 'wallpaper_engine_workshop')),
                     recursive INTEGER NOT NULL DEFAULT 1 CHECK (recursive IN (0, 1)),
                     availability TEXT NOT NULL DEFAULT 'unknown'
                         CHECK (availability IN ('unknown', 'available', 'offline'))
                 );
                 INSERT INTO sources
                     (id, path, added_at, display_name, kind, recursive, availability)
                     SELECT id, path, added_at, display_name, kind, recursive, availability
                     FROM sources_old;
                 DROP TABLE sources_old;
                 PRAGMA legacy_alter_table = OFF;
                 UPDATE config SET value = 'malformed-backup'
                 WHERE key = 'restore-sentinel';",
            )
            .unwrap();
        drop(malformed);

        assert_malformed_restore_rejected(&cd, &backup_path);
    }

    #[test]
    fn restore_rejects_current_version_backup_missing_required_table_column() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        crate::sqlite::try_ensure_sqlite_db(&cd).unwrap();
        let current = rusqlite::Connection::open(cd.db_path()).unwrap();
        current
            .execute(
                "INSERT INTO config (key, value) VALUES ('restore-sentinel', 'backup-value')",
                [],
            )
            .unwrap();
        drop(current);
        let backup_path = PathBuf::from(backup(&cd).unwrap());
        let malformed = rusqlite::Connection::open(&backup_path).unwrap();
        malformed
            .execute_batch("ALTER TABLE wallpaper_sources DROP COLUMN last_seen_at;")
            .unwrap();
        malformed
            .execute(
                "UPDATE config SET value = 'malformed-backup' WHERE key = 'restore-sentinel'",
                [],
            )
            .unwrap();
        drop(malformed);
        let current = rusqlite::Connection::open(cd.db_path()).unwrap();
        current
            .execute(
                "UPDATE config SET value = 'live-current' WHERE key = 'restore-sentinel'",
                [],
            )
            .unwrap();
        drop(current);

        let result = restore(&cd, &backup_path);

        assert!(
            result.is_err(),
            "malformed current-version backup must be rejected"
        );
        let current = rusqlite::Connection::open(cd.db_path()).unwrap();
        assert_eq!(
            current
                .query_row(
                    "SELECT value FROM config WHERE key = 'restore-sentinel'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "live-current"
        );
    }

    #[test]
    fn restore_rejects_current_version_backup_missing_required_column_default() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        crate::sqlite::try_ensure_sqlite_db(&cd).unwrap();
        let current = rusqlite::Connection::open(cd.db_path()).unwrap();
        current
            .execute(
                "INSERT INTO config (key, value) VALUES ('restore-sentinel', 'backup-value')",
                [],
            )
            .unwrap();
        drop(current);
        let backup_path = PathBuf::from(backup(&cd).unwrap());
        let malformed = rusqlite::Connection::open(&backup_path).unwrap();
        malformed
            .execute_batch(
                "ALTER TABLE favorites RENAME TO favorites_old;
                 CREATE TABLE favorites (
                     id       INTEGER PRIMARY KEY AUTOINCREMENT,
                     path     TEXT NOT NULL UNIQUE,
                     added_at TEXT NOT NULL
                 );
                 INSERT INTO favorites (id, path, added_at)
                     SELECT id, path, added_at FROM favorites_old;
                 DROP TABLE favorites_old;",
            )
            .unwrap();
        malformed
            .execute(
                "UPDATE config SET value = 'malformed-backup' WHERE key = 'restore-sentinel'",
                [],
            )
            .unwrap();
        drop(malformed);
        let current = rusqlite::Connection::open(cd.db_path()).unwrap();
        current
            .execute(
                "UPDATE config SET value = 'live-current' WHERE key = 'restore-sentinel'",
                [],
            )
            .unwrap();
        drop(current);

        let result = restore(&cd, &backup_path);

        assert!(
            result.is_err(),
            "malformed current-version backup must be rejected"
        );
        let current = rusqlite::Connection::open(cd.db_path()).unwrap();
        assert_eq!(
            current
                .query_row(
                    "SELECT value FROM config WHERE key = 'restore-sentinel'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "live-current"
        );
    }

    #[test]
    fn restore_rejects_current_version_backup_with_required_extra_column() {
        let (_tmp, cd, backup_path) = restore_validation_fixture();
        let malformed = rusqlite::Connection::open(&backup_path).unwrap();
        malformed
            .execute_batch(
                "ALTER TABLE favorites RENAME TO favorites_old;
                 CREATE TABLE favorites (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     path TEXT NOT NULL UNIQUE,
                     added_at TEXT NOT NULL DEFAULT (datetime('now')),
                     required_extra TEXT NOT NULL
                 );
                 INSERT INTO favorites (id, path, added_at, required_extra)
                     SELECT id, path, added_at, 'legacy' FROM favorites_old;
                 DROP TABLE favorites_old;
                 UPDATE config SET value = 'malformed-backup'
                 WHERE key = 'restore-sentinel';",
            )
            .unwrap();
        drop(malformed);

        assert_malformed_restore_rejected(&cd, &backup_path);
    }

    #[test]
    fn restore_rejects_malformed_current_filename_column_without_replacing_live_database() {
        for (label, mutation) in MALFORMED_FILENAME_MUTATIONS {
            let (_tmp, cd, backup_path) = restore_validation_fixture();
            rewrite_wallpapers_table_sql(&backup_path, mutation);

            let result = restore(&cd, &backup_path);

            assert!(result.is_err(), "{label} filename column was accepted");
            let current = rusqlite::Connection::open(cd.db_path()).unwrap();
            assert_eq!(
                current
                    .query_row(
                        "SELECT value FROM config WHERE key = 'restore-sentinel'",
                        [],
                        |row| row.get::<_, String>(0),
                    )
                    .unwrap(),
                "live-current",
                "{label}"
            );
        }
    }

    #[test]
    fn restore_migrates_valid_v2_data_to_v3_without_replaying_source_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        let old_path = tmp.path().join("valid-v2.db");
        let old = rusqlite::Connection::open(&old_path).unwrap();
        create_schema(&old).unwrap();
        old.execute(
            "INSERT INTO sources
             (id, path, display_name, kind, recursive, availability, added_at)
             VALUES (7, '/old/walls', 'Old walls', 'directory', 0, 'offline',
                     '2024-01-01')",
            [],
        )
        .unwrap();
        old.execute(
            "INSERT INTO wallpapers
             (id, path, type, ext, backend, added_at)
             VALUES (11, '/old/walls/migrated.jpg', 'image', 'jpg', 'awww',
                     '2024-02-02')",
            [],
        )
        .unwrap();
        old.execute(
            "INSERT INTO wallpaper_sources (wallpaper_id, source_id, last_seen_at)
             VALUES (11, 7, '2024-03-03')",
            [],
        )
        .unwrap();
        old.execute(
            "INSERT INTO favorites (id, path, added_at)
             VALUES (13, '/old/walls/migrated.jpg', '2024-04-04')",
            [],
        )
        .unwrap();
        old.execute_batch(
            "ALTER TABLE wallpapers DROP COLUMN filename;
             ALTER TABLE wallpapers DROP COLUMN author;
             PRAGMA user_version = 2;
             UPDATE db_meta SET value = '2' WHERE key = 'schema_version';",
        )
        .unwrap();
        drop(old);

        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        crate::sqlite::try_ensure_sqlite_db(&cd).unwrap();

        restore(&cd, &old_path).unwrap();

        let restored = rusqlite::Connection::open(cd.db_path()).unwrap();
        assert_eq!(
            restored
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            CURRENT_SCHEMA_VERSION
        );
        assert_eq!(
            restored
                .query_row(
                    "SELECT id, display_name, recursive, availability FROM sources",
                    [],
                    |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, i64>(2)?,
                            row.get::<_, String>(3)?,
                        ))
                    },
                )
                .unwrap(),
            (7, "Old walls".into(), 0, "offline".into())
        );
        assert_eq!(
            restored
                .query_row(
                    "SELECT id, added_at, author, filename FROM wallpapers",
                    [],
                    |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                        ))
                    },
                )
                .unwrap(),
            (
                11,
                "2024-02-02".into(),
                String::new(),
                "migrated.jpg".into()
            )
        );
        assert_eq!(
            restored
                .query_row("SELECT COUNT(*) FROM wallpaper_sources", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            1
        );
        assert_eq!(
            restored
                .query_row("SELECT COUNT(*) FROM favorites", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            1
        );
    }

    #[test]
    fn backup_and_restore_round_trip_complete_v3_state() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        crate::sqlite::try_ensure_sqlite_db(&cd).unwrap();
        let conn = rusqlite::Connection::open(cd.db_path()).unwrap();
        conn.execute(
            "INSERT INTO config (key, value) VALUES ('round-trip', 'before')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sources
             (id, path, added_at, display_name, kind, recursive, availability)
             VALUES (41, '/workshop/content/431960', '2025-04-01T00:00:00Z',
                     'Workshop', 'wallpaper_engine_workshop', 0, 'offline')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO wallpapers
             (id, path, type, ext, backend, size, mtime, resolution,
              project_type, preview_path, workshop_id, title, we_file,
              unsupported_reason, source_id, last_seen, added_at, author)
             VALUES (73, '/workshop/content/431960/123', 'scene', 'json',
                     'linux-wallpaperengine', 987, 654, '2560x1440', 'scene',
                     '/workshop/content/431960/123/preview.jpg', '123', 'Round Trip',
                     '/workshop/content/431960/123/project.json', '', 41,
                     '2025-04-02T00:00:00Z', '2025-04-03T00:00:00Z',
                     'Round Trip Studio')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO wallpaper_sources (wallpaper_id, source_id, last_seen_at)
             VALUES (73, 41, '2025-04-04T00:00:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO favorites (id, path, added_at)
             VALUES (12, '/workshop/content/431960/123', '2025-04-05T00:00:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO history (id, path, backend, applied_at)
             VALUES (25, '/workshop/content/431960/123', 'linux-wallpaperengine',
                     '2025-04-06T00:00:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO state (key, value) VALUES ('round-trip-state', 'preserved')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO display_state (target_key, wallpaper_path, backend, updated_at)
             VALUES ('DP-7', '/workshop/content/431960/123', 'linux-wallpaperengine',
                     '2025-04-07T00:00:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO db_meta (key, value, updated_at)
             VALUES ('round-trip-meta', 'preserved', '2025-04-08T00:00:00Z')",
            [],
        )
        .unwrap();
        for (table, sequence) in [
            ("sources", 141_i64),
            ("wallpapers", 173),
            ("favorites", 112),
            ("history", 125),
        ] {
            conn.execute(
                "UPDATE sqlite_sequence SET seq = ?2 WHERE name = ?1",
                rusqlite::params![table, sequence],
            )
            .unwrap();
        }
        let before = persistent_snapshot(&conn);
        drop(conn);

        let backup_path = PathBuf::from(backup(&cd).unwrap());
        let conn = rusqlite::Connection::open(cd.db_path()).unwrap();
        conn.execute(
            "UPDATE config SET value = 'after' WHERE key = 'round-trip'",
            [],
        )
        .unwrap();
        conn.execute("DELETE FROM display_state WHERE target_key = 'DP-7'", [])
            .unwrap();
        drop(conn);

        restore(&cd, &backup_path).unwrap();

        let conn = rusqlite::Connection::open(cd.db_path()).unwrap();
        assert_eq!(persistent_snapshot(&conn), before);
        assert_eq!(
            conn.query_row(
                "SELECT author, filename FROM wallpapers WHERE id = 73",
                [],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .unwrap(),
            ("Round Trip Studio".into(), "123".into())
        );
        assert_eq!(
            wallpapers_count(&conn).unwrap(),
            wallpapers_fts_count(&conn).unwrap()
        );
        check_wallpapers_fts_integrity(&conn).unwrap();
        assert!(std::fs::read_dir(&cd.path)
            .unwrap()
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().into_string().ok())
            .any(|name| name.contains("pre-restore")));
    }

    #[test]
    fn restore_rejects_future_backup_without_modifying_current_database() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        crate::sqlite::try_ensure_sqlite_db(&cd).unwrap();
        let future_backup = PathBuf::from(backup(&cd).unwrap());
        let future = rusqlite::Connection::open(&future_backup).unwrap();
        future
            .pragma_update(
                None,
                "user_version",
                crate::sqlite::CURRENT_SCHEMA_VERSION + 1,
            )
            .unwrap();
        drop(future);
        let current = rusqlite::Connection::open(cd.db_path()).unwrap();
        current
            .execute(
                "INSERT INTO config (key, value) VALUES ('future-restore', 'untouched')",
                [],
            )
            .unwrap();
        drop(current);

        let error = restore(&cd, &future_backup)
            .expect_err("restore must not accept a database from a newer build");

        assert!(
            error.to_string().contains("newer") || error.to_string().contains("version"),
            "{error}"
        );
        let current = rusqlite::Connection::open(cd.db_path()).unwrap();
        assert_eq!(
            current
                .query_row(
                    "SELECT value FROM config WHERE key = 'future-restore'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "untouched"
        );
    }

    #[test]
    fn repair_exclusive_lock_blocks_runtime_open_until_replacement_finishes() {
        use std::sync::mpsc::{self, RecvTimeoutError};
        use std::time::Duration;

        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("wallpaper-console");
        let cd = ConfigDir { path: path.clone() };
        cd.init().unwrap();
        crate::sqlite::try_ensure_sqlite_db(&cd).unwrap();

        let repair_cd = ConfigDir { path: path.clone() };
        let (locked_tx, locked_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let repair_thread = std::thread::spawn(move || {
            repair_with_seam(&repair_cd, || {
                locked_tx.send(()).unwrap();
                release_rx.recv().unwrap();
            })
            .map_err(|error| error.to_string())
        });
        locked_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("repair must acquire its exclusive lock");

        let writer_cd = ConfigDir { path };
        let (writer_tx, writer_rx) = mpsc::channel();
        let writer_thread = std::thread::spawn(move || {
            let result = (|| {
                let writer = crate::sqlite::open_runtime_connection(&writer_cd)?;
                writer
                    .execute(
                        "INSERT INTO config (key, value) VALUES ('new-inode-writer', 'committed')",
                        [],
                    )
                    .map_err(|error| WcError::Sqlite(error.to_string()))?;
                Ok::<_, WcError>(())
            })();
            writer_tx
                .send(result.map_err(|error| error.to_string()))
                .unwrap();
        });
        assert!(matches!(
            writer_rx.recv_timeout(Duration::from_millis(150)),
            Err(RecvTimeoutError::Timeout)
        ));

        release_tx.send(()).unwrap();
        repair_thread.join().unwrap().unwrap();
        writer_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("runtime writer must resume after repair publishes")
            .unwrap();
        writer_thread.join().unwrap();

        let conn = rusqlite::Connection::open(cd.db_path()).unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT value FROM config WHERE key = 'new-inode-writer'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
            "committed"
        );
    }

    #[test]
    fn repair_waits_for_an_existing_runtime_writer_before_replacing_database() {
        use std::sync::mpsc::{self, RecvTimeoutError};
        use std::time::Duration;

        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("wallpaper-console");
        let cd = ConfigDir { path: path.clone() };
        cd.init().unwrap();
        crate::sqlite::try_ensure_sqlite_db(&cd).unwrap();
        let writer = crate::sqlite::open_runtime_connection(&cd).unwrap();
        writer
            .execute(
                "INSERT INTO config (key, value) VALUES ('writer-sentinel', 'committed')",
                [],
            )
            .unwrap();

        let repair_cd = ConfigDir { path };
        let (tx, rx) = mpsc::channel();
        let repair_thread = std::thread::spawn(move || {
            tx.send(repair(&repair_cd).map_err(|error| error.to_string()))
                .unwrap();
        });
        let early = rx.recv_timeout(Duration::from_millis(150));
        let was_blocked = matches!(&early, Err(RecvTimeoutError::Timeout));
        drop(writer);
        let outcome = match early {
            Ok(outcome) => outcome,
            Err(RecvTimeoutError::Timeout) => rx
                .recv_timeout(Duration::from_secs(3))
                .expect("repair must resume after the runtime writer closes"),
            Err(RecvTimeoutError::Disconnected) => panic!("repair thread disconnected"),
        };
        repair_thread.join().unwrap();

        assert!(
            was_blocked,
            "repair must take the exclusive maintenance lock before rebuilding"
        );
        outcome.expect("repair must succeed after the writer releases its shared lock");
        let conn = rusqlite::Connection::open(cd.db_path()).unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT value FROM config WHERE key = 'writer-sentinel'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
            "committed"
        );
    }
}
