//! SQLite storage — schema, migration, verify, resync, backup, restore.

mod backup;
mod connection;
mod display_state;
mod library_fts;
mod library_page;
mod library_revision;
mod metadata_cache;
mod row_map;
mod scan_snapshot;
mod schema;
mod source_config_state;
mod source_reconcile;
mod source_refresh_state;
mod sources;
mod user_unsupported;

pub use backup::*;
pub use connection::{invalidate_cached_connections, RUNTIME_BUSY_TIMEOUT_MS};
#[cfg(test)]
pub(crate) use connection::{
    reset_runtime_connection_open_count, runtime_connection_open_count,
    take_exclusive_maintenance_lock,
};
pub use display_state::*;
pub use library_fts::*;
pub use library_page::*;
pub use library_revision::*;
pub use metadata_cache::*;
pub use row_map::wallpaper_entry_from_row;
pub use scan_snapshot::*;
pub use schema::*;
pub use source_config_state::*;
pub use source_reconcile::*;
pub use source_refresh_state::*;
pub use sources::*;
pub use user_unsupported::*;

use crate::sqlite_err;
use rusqlite::{params, Connection};
use wc_core::config::ConfigDir;
use wc_core::error::WcError;

// ── Helpers ───────────────────────────────────────────────────────────────

fn chrono_now() -> String {
    // ISO 8601 UTC timestamp
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let days_since_epoch = secs / 86400;
    let (y, m, d) = civil_from_days(days_since_epoch as i64);
    let time_secs = secs % 86400;
    let h = time_secs / 3600;
    let min = (time_secs % 3600) / 60;
    let s = time_secs % 60;
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}", y, m, d, h, min, s)
}

fn chrono_now_compact() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let days_since_epoch = secs / 86400;
    let (y, m, d) = civil_from_days(days_since_epoch as i64);
    let time_secs = secs % 86400;
    let h = time_secs / 3600;
    let min = (time_secs % 3600) / 60;
    let s = time_secs % 60;
    format!("{:04}{:02}{:02}-{:02}{:02}{:02}", y, m, d, h, min, s)
}

/// Convert days since Unix epoch to (year, month, day) — simplified Gregorian.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// Import library.tsv into the wallpapers table of an existing transaction.
/// Parses each line as: type\text\tbackend\tsize\tmtime\tresolution\tpath
/// Silently skips if the file doesn't exist or contains no valid rows.
/// The caller owns transaction boundaries.
fn import_library_tsv_into(conn: &Connection, cd: &ConfigDir) -> Result<(), WcError> {
    let tsv_path = cd.library_tsv_path();
    if !tsv_path.exists() {
        return Ok(());
    }
    let content = std::fs::read_to_string(&tsv_path).map_err(WcError::Io)?;
    let mut batch: Vec<(&str, &str, &str, &str, u64, u64, &str)> = Vec::new();
    for line in content.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 7 {
            continue;
        }
        let path = parts[6];
        let ftype = parts[0];
        let ext = parts[1];
        let backend = parts[2];
        let size: u64 = parts[3].parse().unwrap_or(0);
        let mtime: u64 = parts[4].parse().unwrap_or(0);
        let resolution = parts[5];
        batch.push((path, ftype, ext, backend, size, mtime, resolution));
    }
    if batch.is_empty() {
        return Ok(());
    }

    let mut stmt = conn
        .prepare(
            "INSERT OR IGNORE INTO wallpapers (path, type, ext, backend, size, mtime, resolution)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )
        .map_err(sqlite_err)?;
    for (path, ftype, ext, backend, size, mtime, resolution) in &batch {
        stmt.execute(params![
            path,
            ftype,
            ext,
            backend,
            *size as i64,
            *mtime as i64,
            resolution
        ])
        .map_err(sqlite_err)?;
    }
    Ok(())
}
