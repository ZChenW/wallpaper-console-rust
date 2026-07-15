use crate::sqlite_err;
use rusqlite::params;
#[cfg(test)]
use wc_config::ConfigDirExt;
use wc_core::config::ConfigDir;
use wc_core::error::WcError;

use super::schema::{open_runtime_connection, try_ensure_sqlite_db};

/// Add a source directly to the SQLite sources table.
/// Auto-creates the database if it does not exist.
pub fn sqlite_source_add(cd: &ConfigDir, path: &str) -> Result<bool, WcError> {
    super::sources::source_create(cd, path).map(|(_, created)| created)
}

/// Remove all source rows that normalize to the same WE root or canonical path as `path`.
pub fn sqlite_source_remove_canonical(cd: &ConfigDir, path: &str) -> Result<bool, WcError> {
    super::sources::source_remove_canonical_compat(cd, path)
}

pub fn sqlite_config_set(cd: &ConfigDir, key: &str, value: &str) -> Result<(), WcError> {
    try_ensure_sqlite_db(cd)?;
    let conn = open_runtime_connection(cd)?;
    conn.execute(
        "INSERT OR REPLACE INTO config (key, value) VALUES (?1, ?2)",
        params![key, value],
    )
    .map_err(sqlite_err)?;
    Ok(())
}

pub fn sqlite_favorite_add(cd: &ConfigDir, path: &str) -> Result<bool, WcError> {
    try_ensure_sqlite_db(cd)?;
    let conn = open_runtime_connection(cd)?;
    let n = conn
        .execute(
            "INSERT OR IGNORE INTO favorites (path) VALUES (?1)",
            params![path],
        )
        .map_err(sqlite_err)?;
    Ok(n > 0)
}

pub fn sqlite_favorite_remove(cd: &ConfigDir, path: &str) -> Result<(), WcError> {
    try_ensure_sqlite_db(cd)?;
    let conn = open_runtime_connection(cd)?;
    conn.execute("DELETE FROM favorites WHERE path = ?1", params![path])
        .map_err(sqlite_err)?;
    Ok(())
}

pub fn sqlite_state_write(cd: &ConfigDir, key: &str, value: &str) -> Result<(), WcError> {
    try_ensure_sqlite_db(cd)?;
    let conn = open_runtime_connection(cd)?;
    conn.execute(
        "INSERT OR REPLACE INTO state (key, value) VALUES (?1, ?2)",
        params![key, value],
    )
    .map_err(sqlite_err)?;
    Ok(())
}

pub fn sqlite_state_delete(cd: &ConfigDir, key: &str) -> Result<(), WcError> {
    try_ensure_sqlite_db(cd)?;
    let conn = open_runtime_connection(cd)?;
    conn.execute("DELETE FROM state WHERE key = ?1", params![key])
        .map_err(sqlite_err)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::*;
    use crate::sqlite::{create_schema, CURRENT_SCHEMA_VERSION};

    #[test]
    fn future_schema_rejects_config_favorite_and_state_writes_without_mutation() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        let future_version = CURRENT_SCHEMA_VERSION + 1;
        let conn = Connection::open(cd.db_path()).unwrap();
        create_schema(&conn).unwrap();
        conn.execute(
            "INSERT INTO config (key, value) VALUES ('sentinel', 'config-value')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO favorites (path) VALUES ('/walls/sentinel.jpg')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO state (key, value) VALUES ('sentinel', 'state-value')",
            [],
        )
        .unwrap();
        conn.pragma_update(None, "user_version", future_version)
            .unwrap();
        drop(conn);

        let results = [
            sqlite_config_set(&cd, "new-key", "new-value"),
            sqlite_favorite_add(&cd, "/walls/new.jpg").map(|_| ()),
            sqlite_state_write(&cd, "new-state", "new-value"),
        ];

        let conn = Connection::open(cd.db_path()).unwrap();
        let version = conn
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap();
        let sentinel_config: String = conn
            .query_row(
                "SELECT value FROM config WHERE key = 'sentinel'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let sentinel_state: String = conn
            .query_row(
                "SELECT value FROM state WHERE key = 'sentinel'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let unexpected_rows: i64 = conn
            .query_row(
                "SELECT
                    (SELECT COUNT(*) FROM config WHERE key = 'new-key') +
                    (SELECT COUNT(*) FROM favorites WHERE path = '/walls/new.jpg') +
                    (SELECT COUNT(*) FROM state WHERE key = 'new-state')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let sentinel_favorite: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM favorites WHERE path = '/walls/sentinel.jpg'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        for result in results {
            let error = result.expect_err("future-schema write must be rejected");
            assert!(
                error.to_string().contains("newer") || error.to_string().contains("version"),
                "{error}"
            );
        }
        assert_eq!(version, future_version);
        assert_eq!(sentinel_config, "config-value");
        assert_eq!(sentinel_state, "state-value");
        assert_eq!(sentinel_favorite, 1);
        assert_eq!(unexpected_rows, 0);
    }
}
