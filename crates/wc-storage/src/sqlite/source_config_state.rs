use crate::sqlite_err;
use rusqlite::{params, Connection, TransactionBehavior};
use std::collections::HashMap;
#[cfg(test)]
use wc_config::ConfigDirExt;
use wc_core::behavior_setting::{
    BehaviorSettings, BehaviorSettingsPatch, BehaviorSettingsSnapshot, BEHAVIOR_SETTING_KEYS,
};
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

fn behavior_settings_from_connection(connection: &Connection) -> Result<BehaviorSettings, WcError> {
    let defaults = wc_core::config::default_config();
    let mut values = HashMap::new();
    let mut statement = connection
        .prepare("SELECT value FROM config WHERE key = ?1")
        .map_err(sqlite_err)?;
    for key in BEHAVIOR_SETTING_KEYS {
        let value = statement
            .query_row([key], |row| row.get::<_, String>(0))
            .or_else(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => {
                    Ok(defaults.get(*key).cloned().unwrap_or_default())
                }
                other => Err(other),
            })
            .map_err(sqlite_err)?;
        values.insert((*key).to_string(), value);
    }
    Ok(BehaviorSettings::from_config(&values))
}

pub fn read_behavior_settings(cd: &ConfigDir) -> Result<BehaviorSettingsSnapshot, WcError> {
    try_ensure_sqlite_db(cd)?;
    let connection = open_runtime_connection(cd)?;
    Ok(behavior_settings_from_connection(&connection)?.snapshot())
}

pub fn update_behavior_settings(
    cd: &ConfigDir,
    expected_revision: &str,
    patch: &BehaviorSettingsPatch,
) -> Result<BehaviorSettingsSnapshot, WcError> {
    try_ensure_sqlite_db(cd)?;
    let mut connection = open_runtime_connection(cd)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sqlite_err)?;
    let current = behavior_settings_from_connection(&transaction)?;
    let observed = wc_core::behavior_setting::behavior_settings_revision(&current);
    if observed != expected_revision {
        return Err(WcError::ConfigRevisionChanged {
            expected: expected_revision.to_string(),
            observed,
        });
    }
    let next = current.apply_patch(patch);
    {
        let mut statement = transaction
            .prepare("INSERT OR REPLACE INTO config (key, value) VALUES (?1, ?2)")
            .map_err(sqlite_err)?;
        for (key, value) in next.config_entries() {
            statement.execute(params![key, value]).map_err(sqlite_err)?;
        }
    }
    transaction.commit().map_err(sqlite_err)?;

    let entries = next.config_entries();
    if let Err(error) = wc_config::write_config_values(
        &cd.path,
        entries.iter().map(|(key, value)| (*key, value.as_str())),
    ) {
        log::warn!("behavior settings flat config mirror failed: {error}");
    }
    Ok(next.snapshot())
}

pub fn sqlite_favorite_add(cd: &ConfigDir, path: &str) -> Result<bool, WcError> {
    try_ensure_sqlite_db(cd)?;
    let mut conn = open_runtime_connection(cd)?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sqlite_err)?;
    let n = tx
        .execute(
            "INSERT OR IGNORE INTO favorites (path) VALUES (?1)",
            params![path],
        )
        .map_err(sqlite_err)?;
    if n > 0 {
        super::bump_library_revision(&tx)?;
    }
    tx.commit().map_err(sqlite_err)?;
    Ok(n > 0)
}

pub fn sqlite_favorite_remove(cd: &ConfigDir, path: &str) -> Result<(), WcError> {
    try_ensure_sqlite_db(cd)?;
    let mut conn = open_runtime_connection(cd)?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sqlite_err)?;
    let changed = tx
        .execute("DELETE FROM favorites WHERE path = ?1", params![path])
        .map_err(sqlite_err)?;
    if changed > 0 {
        super::bump_library_revision(&tx)?;
    }
    tx.commit().map_err(sqlite_err)?;
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

/// Atomically publish the legacy runtime state pair.
pub fn sqlite_runtime_state_write_pair(
    cd: &ConfigDir,
    current: &str,
    last_backend: &str,
) -> Result<(), WcError> {
    try_ensure_sqlite_db(cd)?;
    let mut conn = open_runtime_connection(cd)?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sqlite_err)?;
    tx.execute(
        "INSERT OR REPLACE INTO state (key, value) VALUES ('current', ?1)",
        params![current],
    )
    .map_err(sqlite_err)?;
    tx.execute(
        "INSERT OR REPLACE INTO state (key, value) VALUES ('last_backend', ?1)",
        params![last_backend],
    )
    .map_err(sqlite_err)?;
    tx.commit().map_err(sqlite_err)?;
    Ok(())
}

pub fn sqlite_state_delete(cd: &ConfigDir, key: &str) -> Result<(), WcError> {
    try_ensure_sqlite_db(cd)?;
    let conn = open_runtime_connection(cd)?;
    conn.execute("DELETE FROM state WHERE key = ?1", params![key])
        .map_err(sqlite_err)?;
    Ok(())
}

/// Atomically clear the legacy runtime state pair.
pub fn sqlite_runtime_state_clear(cd: &ConfigDir) -> Result<(), WcError> {
    try_ensure_sqlite_db(cd)?;
    let mut conn = open_runtime_connection(cd)?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sqlite_err)?;
    tx.execute(
        "DELETE FROM state WHERE key IN ('current', 'last_backend')",
        [],
    )
    .map_err(sqlite_err)?;
    tx.commit().map_err(sqlite_err)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::*;
    use crate::sqlite::{create_schema, CURRENT_SCHEMA_VERSION};

    #[test]
    fn favorite_mutations_bump_once_while_noops_and_config_state_do_not() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        crate::sqlite::try_ensure_sqlite_db(&cd).unwrap();
        let revision = || {
            let conn = Connection::open(cd.db_path()).unwrap();
            crate::sqlite::read_library_revision(&conn).unwrap()
        };

        assert_eq!(revision(), 0);
        assert!(sqlite_favorite_add(&cd, "/walls/a.jpg").unwrap());
        assert_eq!(revision(), 1);
        assert!(!sqlite_favorite_add(&cd, "/walls/a.jpg").unwrap());
        assert_eq!(revision(), 1);
        sqlite_config_set(&cd, "theme", "dark").unwrap();
        sqlite_state_write(&cd, "current", "/walls/a.jpg").unwrap();
        assert_eq!(revision(), 1);
        sqlite_favorite_remove(&cd, "/walls/a.jpg").unwrap();
        assert_eq!(revision(), 2);
        sqlite_favorite_remove(&cd, "/walls/a.jpg").unwrap();
        assert_eq!(revision(), 2);
    }

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

    #[test]
    fn runtime_state_pair_write_rolls_back_when_the_second_key_fails() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        crate::sqlite::try_ensure_sqlite_db(&cd).unwrap();
        sqlite_runtime_state_write_pair(&cd, "/walls/old.jpg", "awww").unwrap();
        let conn = Connection::open(cd.db_path()).unwrap();
        conn.execute_batch(
            "CREATE TRIGGER reject_last_backend_update
             BEFORE INSERT ON state
             WHEN NEW.key = 'last_backend'
             BEGIN
                 SELECT RAISE(ABORT, 'injected last_backend failure');
             END;",
        )
        .unwrap();
        drop(conn);

        let error = sqlite_runtime_state_write_pair(&cd, "/walls/new.jpg", "mpvpaper").unwrap_err();
        assert!(error.to_string().contains("injected last_backend failure"));

        let conn = Connection::open(cd.db_path()).unwrap();
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
        assert_eq!(current, "/walls/old.jpg");
        assert_eq!(backend, "awww");
    }

    #[test]
    fn runtime_state_clear_rolls_back_when_delete_fails() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        crate::sqlite::try_ensure_sqlite_db(&cd).unwrap();
        sqlite_runtime_state_write_pair(&cd, "/walls/old.jpg", "awww").unwrap();
        let conn = Connection::open(cd.db_path()).unwrap();
        conn.execute_batch(
            "CREATE TRIGGER reject_last_backend_delete
             BEFORE DELETE ON state
             WHEN OLD.key = 'last_backend'
             BEGIN
                 SELECT RAISE(ABORT, 'injected last_backend delete failure');
             END;",
        )
        .unwrap();
        drop(conn);

        let error = sqlite_runtime_state_clear(&cd).unwrap_err();
        assert!(error
            .to_string()
            .contains("injected last_backend delete failure"));

        let conn = Connection::open(cd.db_path()).unwrap();
        let remaining: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM state
                 WHERE key IN ('current', 'last_backend')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(remaining, 2);
    }
}
