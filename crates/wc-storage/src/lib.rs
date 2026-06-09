//! wc-storage — flat-file and SQLite storage, hybrid mirror, migration, verify.

pub mod flat;
pub mod mirror;
pub mod sqlite;

use std::path::Path;
use wc_core::config::ConfigDir;
use wc_core::types::StorageBackend;

/// Determine the active storage backend mode.
pub fn storage_backend_mode(config_dir: &Path) -> StorageBackend {
    let raw = wc_core::config::read_config_value(config_dir, "storage_backend", "file");
    raw.parse::<StorageBackend>()
        .unwrap_or(StorageBackend::File)
}

/// Check whether SQLite mirror writes are active.
pub fn sqlite_mirror_active(config_dir: &Path) -> bool {
    let mode = storage_backend_mode(config_dir);
    matches!(mode, StorageBackend::Hybrid | StorageBackend::Sqlite)
        && ConfigDir::new()
            .map(|c| c.db_path().exists())
            .unwrap_or(false)
}
