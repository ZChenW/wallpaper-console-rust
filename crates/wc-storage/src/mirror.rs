//! Mirror writes from flat files to SQLite (best-effort).

use rusqlite::Connection;
use wc_core::config::ConfigDir;
use wc_core::error::WcError;

use crate::sqlite::sqlite_escape;
use crate::sqlite_mirror_active;

fn mirror_run(cd: &ConfigDir, sql: &str) -> Result<(), WcError> {
    if !sqlite_mirror_active(&cd.path) {
        return Ok(());
    }
    let conn = Connection::open(cd.db_path()).map_err(|e| WcError::Sqlite(e.to_string()))?;
    conn.execute(sql, []).map_err(|e| {
        // Best-effort: log, don't die
        eprintln!("wallpaper-console: SQLite mirror write failed (flat-file write succeeded)");
        WcError::Sqlite(e.to_string())
    })?;
    Ok(())
}

pub fn mirror_config_set(cd: &ConfigDir, key: &str, value: &str) -> Result<(), WcError> {
    let ek = sqlite_escape(key);
    let ev = sqlite_escape(value);
    let sql = format!(
        "INSERT OR REPLACE INTO config (key, value) VALUES ('{}', '{}')",
        ek, ev
    );
    mirror_run(cd, &sql)
}

pub fn mirror_source_add(cd: &ConfigDir, path: &str) -> Result<(), WcError> {
    let ep = sqlite_escape(path);
    let sql = format!("INSERT OR IGNORE INTO sources (path) VALUES ('{}')", ep);
    mirror_run(cd, &sql)
}

pub fn mirror_source_remove(cd: &ConfigDir, path: &str) -> Result<(), WcError> {
    let ep = sqlite_escape(path);
    let sql = format!("DELETE FROM sources WHERE path = '{}'", ep);
    mirror_run(cd, &sql)
}

pub fn mirror_favorite_add(cd: &ConfigDir, path: &str) -> Result<(), WcError> {
    let ep = sqlite_escape(path);
    let sql = format!("INSERT OR IGNORE INTO favorites (path) VALUES ('{}')", ep);
    mirror_run(cd, &sql)
}

pub fn mirror_favorite_remove(cd: &ConfigDir, path: &str) -> Result<(), WcError> {
    let ep = sqlite_escape(path);
    let sql = format!("DELETE FROM favorites WHERE path = '{}'", ep);
    mirror_run(cd, &sql)
}

pub fn mirror_history_add(cd: &ConfigDir, path: &str, backend: &str) -> Result<(), WcError> {
    let ep = sqlite_escape(path);
    let eb = sqlite_escape(backend);
    let sql = format!(
        "INSERT INTO history (path, backend) VALUES ('{}', '{}')",
        ep, eb
    );
    mirror_run(cd, &sql)
}

pub fn mirror_history_clear(cd: &ConfigDir) -> Result<(), WcError> {
    mirror_run(cd, "DELETE FROM history")
}

pub fn mirror_history_trim(cd: &ConfigDir, max_entries: usize) -> Result<(), WcError> {
    let sql = format!(
        "DELETE FROM history WHERE id NOT IN (SELECT id FROM history ORDER BY id DESC LIMIT {})",
        max_entries
    );
    mirror_run(cd, &sql)
}

pub fn mirror_current_write(cd: &ConfigDir, path: &str) -> Result<(), WcError> {
    let ep = sqlite_escape(path);
    let sql = format!(
        "INSERT OR REPLACE INTO state (key, value) VALUES ('current', '{}')",
        ep
    );
    mirror_run(cd, &sql)
}

pub fn mirror_last_backend_write(cd: &ConfigDir, backend: &str) -> Result<(), WcError> {
    let eb = sqlite_escape(backend);
    let sql = format!(
        "INSERT OR REPLACE INTO state (key, value) VALUES ('last_backend', '{}')",
        eb
    );
    mirror_run(cd, &sql)
}
