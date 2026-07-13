use rusqlite::{params, Connection};
use wc_core::config::ConfigDir;
use wc_core::error::WcError;

use super::schema::ensure_sqlite_db;

/// Add a source directly to the SQLite sources table.
/// Auto-creates the database if it does not exist.
pub fn sqlite_source_add(cd: &ConfigDir, path: &str) -> Result<bool, WcError> {
    super::sources::source_create(cd, path).map(|(_, created)| created)
}

/// Remove a source directly from the SQLite sources table.
/// Auto-creates the database if it does not exist.
pub fn sqlite_source_remove(cd: &ConfigDir, path: &str) -> Result<bool, WcError> {
    super::sources::source_remove_exact_path_compat(cd, path)
}

/// Remove all source rows that normalize to the same WE root or canonical path as `path`.
pub fn sqlite_source_remove_canonical(cd: &ConfigDir, path: &str) -> Result<bool, WcError> {
    super::sources::source_remove_canonical_compat(cd, path)
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

pub fn sqlite_state_delete(cd: &ConfigDir, key: &str) -> Result<(), WcError> {
    ensure_sqlite_db(cd);
    let conn = Connection::open(cd.db_path()).map_err(|e| WcError::Sqlite(e.to_string()))?;
    conn.execute("DELETE FROM state WHERE key = ?1", params![key])
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    Ok(())
}
