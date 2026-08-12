use crate::sqlite_err;
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use wc_core::config::ConfigDir;
use wc_core::error::WcError;

use super::{open_runtime_connection, try_ensure_sqlite_db};

fn scene_identity_key(transaction: &Transaction<'_>, wallpaper_id: i64) -> Result<String, WcError> {
    transaction
        .query_row(
            "SELECT CASE
                        WHEN workshop_id <> '' THEN 'workshop:' || workshop_id
                        ELSE 'path:' || path
                    END
             FROM wallpapers
             WHERE id = ?1 AND type = 'we_scene'",
            [wallpaper_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(sqlite_err)?
        .ok_or_else(|| {
            WcError::Other(
                "only Wallpaper Engine scenes in the current Library can be moved to Unsupported"
                    .into(),
            )
        })
}

pub fn sqlite_user_unsupported_add(cd: &ConfigDir, wallpaper_id: i64) -> Result<bool, WcError> {
    try_ensure_sqlite_db(cd)?;
    let mut connection = open_runtime_connection(cd)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sqlite_err)?;
    let identity_key = scene_identity_key(&transaction, wallpaper_id)?;
    let changed = transaction
        .execute(
            "INSERT OR IGNORE INTO user_unsupported (identity_key) VALUES (?1)",
            params![identity_key],
        )
        .map_err(sqlite_err)?;
    if changed > 0 {
        super::bump_library_revision(&transaction)?;
    }
    transaction.commit().map_err(sqlite_err)?;
    Ok(changed > 0)
}

pub fn sqlite_user_unsupported_remove(cd: &ConfigDir, wallpaper_id: i64) -> Result<bool, WcError> {
    try_ensure_sqlite_db(cd)?;
    let mut connection = open_runtime_connection(cd)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sqlite_err)?;
    let identity_key = scene_identity_key(&transaction, wallpaper_id)?;
    let changed = transaction
        .execute(
            "DELETE FROM user_unsupported WHERE identity_key = ?1",
            [identity_key],
        )
        .map_err(sqlite_err)?;
    if changed > 0 {
        super::bump_library_revision(&transaction)?;
    }
    transaction.commit().map_err(sqlite_err)?;
    Ok(changed > 0)
}

pub fn sqlite_user_unsupported_contains_path(cd: &ConfigDir, path: &str) -> Result<bool, WcError> {
    if !cd.db_path().exists() {
        return Ok(false);
    }
    let connection = open_runtime_connection(cd)?;
    let indexed_match = connection
        .query_row(
            "SELECT EXISTS (
                 SELECT 1
                 FROM wallpapers wallpaper
                 JOIN user_unsupported excluded
                   ON excluded.identity_key = CASE
                       WHEN wallpaper.workshop_id <> ''
                           THEN 'workshop:' || wallpaper.workshop_id
                       ELSE 'path:' || wallpaper.path
                   END
                 WHERE wallpaper.path = ?1
             )",
            [path],
            |row| row.get(0),
        )
        .map_err(sqlite_err)?;
    if indexed_match {
        return Ok(true);
    }

    let workshop_identity = wc_scan::read_we_project_info(std::path::Path::new(path))
        .and_then(|project| project.workshop_id)
        .filter(|workshop_id| !workshop_id.is_empty())
        .map(|workshop_id| format!("workshop:{workshop_id}"));
    let identity_key = workshop_identity.unwrap_or_else(|| format!("path:{path}"));
    connection
        .query_row(
            "SELECT EXISTS (
                 SELECT 1 FROM user_unsupported WHERE identity_key = ?1
             )",
            [identity_key],
            |row| row.get(0),
        )
        .map_err(sqlite_err)
}
