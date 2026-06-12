//! Mirror writes from flat files to SQLite (best-effort).

use rusqlite::{params, Connection};
use wc_core::config::ConfigDir;
use wc_core::error::WcError;

use crate::sqlite_mirror_active;

fn mirror_conn(cd: &ConfigDir) -> Result<Option<Connection>, WcError> {
    if !sqlite_mirror_active(&cd.path) {
        return Ok(None);
    }
    let conn = Connection::open(cd.db_path()).map_err(|e| WcError::Sqlite(e.to_string()))?;
    Ok(Some(conn))
}

fn mirror_exec(conn: &Connection, sql: &str) -> Result<(), WcError> {
    conn.execute(sql, []).map_err(|e| {
        eprintln!("wallpaper-console: SQLite mirror write failed (flat-file write succeeded)");
        WcError::Sqlite(e.to_string())
    })?;
    Ok(())
}

pub fn mirror_config_set(cd: &ConfigDir, key: &str, value: &str) -> Result<(), WcError> {
    let Some(conn) = mirror_conn(cd)? else {
        return Ok(());
    };
    conn.execute(
        "INSERT OR REPLACE INTO config (key, value) VALUES (?1, ?2)",
        params![key, value],
    )
    .map_err(|e| {
        eprintln!("wallpaper-console: SQLite mirror write failed (flat-file write succeeded)");
        WcError::Sqlite(e.to_string())
    })?;
    Ok(())
}

pub fn mirror_source_add(cd: &ConfigDir, path: &str) -> Result<(), WcError> {
    let Some(conn) = mirror_conn(cd)? else {
        return Ok(());
    };
    conn.execute(
        "INSERT OR IGNORE INTO sources (path) VALUES (?1)",
        params![path],
    )
    .map_err(|e| {
        eprintln!("wallpaper-console: SQLite mirror write failed (flat-file write succeeded)");
        WcError::Sqlite(e.to_string())
    })?;
    Ok(())
}

pub fn mirror_source_remove(cd: &ConfigDir, path: &str) -> Result<(), WcError> {
    let Some(conn) = mirror_conn(cd)? else {
        return Ok(());
    };
    conn.execute("DELETE FROM sources WHERE path = ?1", params![path])
        .map_err(|e| {
            eprintln!("wallpaper-console: SQLite mirror write failed (flat-file write succeeded)");
            WcError::Sqlite(e.to_string())
        })?;
    Ok(())
}

pub fn mirror_favorite_add(cd: &ConfigDir, path: &str) -> Result<(), WcError> {
    let Some(conn) = mirror_conn(cd)? else {
        return Ok(());
    };
    conn.execute(
        "INSERT OR IGNORE INTO favorites (path) VALUES (?1)",
        params![path],
    )
    .map_err(|e| {
        eprintln!("wallpaper-console: SQLite mirror write failed (flat-file write succeeded)");
        WcError::Sqlite(e.to_string())
    })?;
    Ok(())
}

pub fn mirror_favorite_remove(cd: &ConfigDir, path: &str) -> Result<(), WcError> {
    let Some(conn) = mirror_conn(cd)? else {
        return Ok(());
    };
    conn.execute("DELETE FROM favorites WHERE path = ?1", params![path])
        .map_err(|e| {
            eprintln!("wallpaper-console: SQLite mirror write failed (flat-file write succeeded)");
            WcError::Sqlite(e.to_string())
        })?;
    Ok(())
}

pub fn mirror_history_add(cd: &ConfigDir, path: &str, backend: &str) -> Result<(), WcError> {
    let Some(conn) = mirror_conn(cd)? else {
        return Ok(());
    };
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
    tx.commit().map_err(|e| {
        eprintln!("wallpaper-console: SQLite mirror write failed (flat-file write succeeded)");
        WcError::Sqlite(e.to_string())
    })?;
    Ok(())
}

pub fn mirror_history_clear(cd: &ConfigDir) -> Result<(), WcError> {
    let Some(conn) = mirror_conn(cd)? else {
        return Ok(());
    };
    mirror_exec(&conn, "DELETE FROM history")
}

pub fn mirror_history_trim(cd: &ConfigDir, max_entries: usize) -> Result<(), WcError> {
    let Some(conn) = mirror_conn(cd)? else {
        return Ok(());
    };
    // Deduplicate: keep only the latest id per path
    mirror_exec(
        &conn,
        "DELETE FROM history WHERE id NOT IN (SELECT MAX(id) FROM history GROUP BY path)",
    )?;
    // Trim to max_entries (newest first)
    let sql = format!(
        "DELETE FROM history WHERE id NOT IN (SELECT id FROM history ORDER BY id DESC LIMIT {})",
        max_entries
    );
    mirror_exec(&conn, &sql)
}

pub fn mirror_current_write(cd: &ConfigDir, path: &str) -> Result<(), WcError> {
    let Some(conn) = mirror_conn(cd)? else {
        return Ok(());
    };
    conn.execute(
        "INSERT OR REPLACE INTO state (key, value) VALUES ('current', ?1)",
        params![path],
    )
    .map_err(|e| {
        eprintln!("wallpaper-console: SQLite mirror write failed (flat-file write succeeded)");
        WcError::Sqlite(e.to_string())
    })?;
    Ok(())
}

pub fn mirror_last_backend_write(cd: &ConfigDir, backend: &str) -> Result<(), WcError> {
    let Some(conn) = mirror_conn(cd)? else {
        return Ok(());
    };
    conn.execute(
        "INSERT OR REPLACE INTO state (key, value) VALUES ('last_backend', ?1)",
        params![backend],
    )
    .map_err(|e| {
        eprintln!("wallpaper-console: SQLite mirror write failed (flat-file write succeeded)");
        WcError::Sqlite(e.to_string())
    })?;
    Ok(())
}
