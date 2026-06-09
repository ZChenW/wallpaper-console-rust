//! wc-storage — unified storage API dispatching on storage_backend_mode.

pub mod flat;
pub mod mirror;
pub mod sqlite;

use std::path::Path;
use wc_core::config::ConfigDir;
use wc_core::error::WcError;
use wc_core::types::StorageBackend;

/// Determine the active storage backend mode.
pub fn storage_backend_mode(config_dir: &Path) -> StorageBackend {
    let raw = wc_core::config::read_config_value(config_dir, "storage_backend", "file");
    raw.parse::<StorageBackend>()
        .unwrap_or(StorageBackend::File)
}

/// Unified storage API. Reads dispatch on `storage_backend` mode;
/// writes always go flat-first then mirror to SQLite when active.
pub struct StorageApi {
    pub cd: ConfigDir,
    pub mode: StorageBackend,
}

impl StorageApi {
    pub fn new(cd: ConfigDir) -> Self {
        let mode = storage_backend_mode(&cd.path);
        StorageApi { cd, mode }
    }

    /// Re-read the backend mode from config (after config-set).
    pub fn refresh_mode(&mut self) {
        self.mode = storage_backend_mode(&self.cd.path);
    }

    // ── Reads (dispatch on mode) ───────────────────────────────────────

    pub fn config_get(&self, key: &str, default: &str) -> String {
        match self.mode {
            StorageBackend::Sqlite => {
                if key == "storage_backend" {
                    // Bootstrap-safe: always read from flat config
                    return wc_core::config::read_config_value(&self.cd.path, key, default);
                }
                self._sqlite_config_get(key, default)
            }
            _ => wc_core::config::read_config_value(&self.cd.path, key, default),
        }
    }

    fn _sqlite_config_get(&self, key: &str, default: &str) -> String {
        let db = self.cd.db_path();
        if !db.exists() {
            return default.to_string();
        }
        match rusqlite::Connection::open(&db) {
            Ok(conn) => {
                let ek = sqlite::sqlite_escape(key);
                let sql = format!("SELECT value FROM config WHERE key='{}'", ek);
                conn.query_row(&sql, [], |row| row.get::<_, String>(0))
                    .unwrap_or_else(|_| default.to_string())
            }
            _ => default.to_string(),
        }
    }

    pub fn sources_list(&self) -> Result<Vec<String>, WcError> {
        match self.mode {
            StorageBackend::Sqlite => {
                let db = self.cd.db_path();
                if !db.exists() {
                    return Err(WcError::Other(
                        "wallpapers.db not found. Run migrate-to-sqlite first.".into(),
                    ));
                }
                let conn =
                    rusqlite::Connection::open(&db).map_err(|e| WcError::Sqlite(e.to_string()))?;
                let mut stmt = conn
                    .prepare("SELECT path FROM sources ORDER BY path")
                    .map_err(|e| WcError::Sqlite(e.to_string()))?;
                let rows: Vec<String> = stmt
                    .query_map([], |row| row.get(0))
                    .map_err(|e| WcError::Sqlite(e.to_string()))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| WcError::Sqlite(e.to_string()))?;
                Ok(rows)
            }
            _ => flat::sources_list(&self.cd),
        }
    }

    pub fn favorites_list(&self) -> Result<Vec<String>, WcError> {
        match self.mode {
            StorageBackend::Sqlite => {
                let db = self.cd.db_path();
                if !db.exists() {
                    return Err(WcError::Other(
                        "wallpapers.db not found. Run migrate-to-sqlite first.".into(),
                    ));
                }
                let conn =
                    rusqlite::Connection::open(&db).map_err(|e| WcError::Sqlite(e.to_string()))?;
                let mut stmt = conn
                    .prepare("SELECT path FROM favorites ORDER BY path")
                    .map_err(|e| WcError::Sqlite(e.to_string()))?;
                let rows: Vec<String> = stmt
                    .query_map([], |row| row.get(0))
                    .map_err(|e| WcError::Sqlite(e.to_string()))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| WcError::Sqlite(e.to_string()))?;
                Ok(rows)
            }
            _ => flat::favorites_list(&self.cd),
        }
    }

    pub fn history_list(&self) -> Result<Vec<String>, WcError> {
        match self.mode {
            StorageBackend::Sqlite => {
                let db = self.cd.db_path();
                if !db.exists() {
                    return Err(WcError::Other(
                        "wallpapers.db not found. Run migrate-to-sqlite first.".into(),
                    ));
                }
                let conn =
                    rusqlite::Connection::open(&db).map_err(|e| WcError::Sqlite(e.to_string()))?;
                let mut stmt = conn
                    .prepare("SELECT path FROM history ORDER BY id DESC")
                    .map_err(|e| WcError::Sqlite(e.to_string()))?;
                let rows: Vec<String> = stmt
                    .query_map([], |row| row.get(0))
                    .map_err(|e| WcError::Sqlite(e.to_string()))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| WcError::Sqlite(e.to_string()))?;
                Ok(rows)
            }
            _ => flat::history_list(&self.cd),
        }
    }

    pub fn current_read(&self) -> Result<Option<String>, WcError> {
        match self.mode {
            StorageBackend::Sqlite => {
                let db = self.cd.db_path();
                if !db.exists() {
                    return Err(WcError::Other(
                        "wallpapers.db not found. Run migrate-to-sqlite first.".into(),
                    ));
                }
                let conn =
                    rusqlite::Connection::open(&db).map_err(|e| WcError::Sqlite(e.to_string()))?;
                Ok(conn
                    .query_row("SELECT value FROM state WHERE key='current'", [], |row| {
                        row.get(0)
                    })
                    .ok())
            }
            _ => flat::current_read(&self.cd),
        }
    }

    pub fn last_backend_read(&self) -> Result<Option<String>, WcError> {
        match self.mode {
            StorageBackend::Sqlite => {
                let db = self.cd.db_path();
                if !db.exists() {
                    return Err(WcError::Other(
                        "wallpapers.db not found. Run migrate-to-sqlite first.".into(),
                    ));
                }
                let conn =
                    rusqlite::Connection::open(&db).map_err(|e| WcError::Sqlite(e.to_string()))?;
                Ok(conn
                    .query_row(
                        "SELECT value FROM state WHERE key='last_backend'",
                        [],
                        |row| row.get(0),
                    )
                    .ok())
            }
            _ => flat::last_backend_read(&self.cd),
        }
    }

    // ── Writes (flat-first, then mirror) ──────────────────────────────

    pub fn config_set(&self, key: &str, value: &str) -> Result<(), WcError> {
        wc_core::config::write_config_value(&self.cd.path, key, value)?;
        mirror::mirror_config_set(&self.cd, key, value).ok();
        Ok(())
    }

    pub fn sources_add(&self, path: &str) -> Result<bool, WcError> {
        let added = flat::sources_add(&self.cd, path)?;
        if added {
            mirror::mirror_source_add(&self.cd, path).ok();
        }
        Ok(added)
    }

    pub fn sources_remove(&self, path: &str) -> Result<bool, WcError> {
        let removed = flat::sources_remove(&self.cd, path)?;
        if removed {
            mirror::mirror_source_remove(&self.cd, path).ok();
        }
        Ok(removed)
    }

    pub fn favorites_add(&self, path: &str) -> Result<bool, WcError> {
        let added = flat::favorites_add(&self.cd, path)?;
        if added {
            mirror::mirror_favorite_add(&self.cd, path).ok();
        }
        Ok(added)
    }

    pub fn favorites_remove(&self, path: &str) -> Result<(), WcError> {
        flat::favorites_remove(&self.cd, path)?;
        mirror::mirror_favorite_remove(&self.cd, path).ok();
        Ok(())
    }

    pub fn history_add(&self, path: &str, backend: &str) -> Result<(), WcError> {
        flat::history_add(&self.cd, path, 100)?;
        mirror::mirror_history_add(&self.cd, path, backend).ok();
        mirror::mirror_history_trim(&self.cd, 100).ok();
        Ok(())
    }

    pub fn history_clear(&self) -> Result<(), WcError> {
        flat::history_clear(&self.cd)?;
        mirror::mirror_history_clear(&self.cd).ok();
        Ok(())
    }

    pub fn current_write(&self, path: &str) -> Result<(), WcError> {
        flat::current_write(&self.cd, path)?;
        mirror::mirror_current_write(&self.cd, path).ok();
        Ok(())
    }

    pub fn last_backend_write(&self, backend: &str) -> Result<(), WcError> {
        flat::last_backend_write(&self.cd, backend)?;
        mirror::mirror_last_backend_write(&self.cd, backend).ok();
        Ok(())
    }
}

/// Check whether SQLite mirror writes are active.
pub fn sqlite_mirror_active(config_dir: &Path) -> bool {
    let mode = storage_backend_mode(config_dir);
    matches!(mode, StorageBackend::Hybrid | StorageBackend::Sqlite)
        && ConfigDir::new()
            .map(|c| c.db_path().exists())
            .unwrap_or(false)
}
