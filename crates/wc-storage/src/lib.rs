//! wc-storage — unified storage API dispatching on storage_backend_mode.

pub mod flat;
pub mod mirror;
pub mod sqlite;
pub mod tsv;
pub mod we_compat;

use std::path::Path;
use wc_core::config::ConfigDir;
use wc_core::error::WcError;
use wc_core::types::StorageBackend;

/// Determine the active storage backend mode.
pub fn storage_backend_mode(config_dir: &Path) -> StorageBackend {
    let raw = wc_core::config::read_config_value(config_dir, "storage_backend", "sqlite");
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
        if matches!(mode, StorageBackend::Sqlite) {
            if cd.db_path().exists() {
                sqlite::ensure_sqlite_db(&cd);
            } else {
                sqlite::migrate_to_sqlite(&cd).ok();
            }
        }
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
        let raw = match self.mode {
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
                rows
            }
            _ => flat::sources_list(&self.cd)?,
        };
        let mut seen = std::collections::HashSet::new();
        let deduped: Vec<String> = raw
            .into_iter()
            .filter(|p| seen.insert(flat::try_canonicalize(p)))
            .collect();
        Ok(deduped)
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
        let raw = match self.mode {
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
                rows
            }
            _ => flat::history_list(&self.cd)?,
        };
        let mut seen = std::collections::HashSet::new();
        let deduped: Vec<String> = raw
            .into_iter()
            .filter(|p| seen.insert(flat::try_canonicalize(p)))
            .collect();
        Ok(deduped)
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
        if matches!(self.mode, StorageBackend::Sqlite) {
            sqlite::sqlite_config_set(&self.cd, key, value)?;
            wc_core::config::write_config_value(&self.cd.path, key, value).ok();
        } else {
            wc_core::config::write_config_value(&self.cd.path, key, value)?;
            mirror::mirror_config_set(&self.cd, key, value).ok();
        }
        Ok(())
    }

    pub fn sources_add(&self, path: &str) -> Result<bool, WcError> {
        match self.mode {
            StorageBackend::Sqlite => {
                // SQLite must succeed first — GUI reads from DB in sqlite mode.
                // Flat-file write follows only as a compatibility copy.
                let added = sqlite::sqlite_source_add(&self.cd, path)?;
                // Sync flat as a best-effort compatibility copy.
                flat::sources_add(&self.cd, path).ok();
                Ok(added)
            }
            _ => {
                let added = flat::sources_add(&self.cd, path)?;
                if added {
                    mirror::mirror_source_add(&self.cd, path).ok();
                }
                Ok(added)
            }
        }
    }

    pub fn sources_remove(&self, path: &str) -> Result<bool, WcError> {
        match self.mode {
            StorageBackend::Sqlite => {
                // SQLite must succeed first.
                let removed = sqlite::sqlite_source_remove(&self.cd, path)?;
                // Sync flat as a best-effort compatibility copy.
                flat::sources_remove(&self.cd, path).ok();
                Ok(removed)
            }
            _ => {
                let removed = flat::sources_remove(&self.cd, path)?;
                if removed {
                    mirror::mirror_source_remove(&self.cd, path).ok();
                }
                Ok(removed)
            }
        }
    }

    pub fn favorites_add(&self, path: &str) -> Result<bool, WcError> {
        if matches!(self.mode, StorageBackend::Sqlite) {
            let added = sqlite::sqlite_favorite_add(&self.cd, path)?;
            flat::favorites_add(&self.cd, path).ok();
            Ok(added)
        } else {
            let added = flat::favorites_add(&self.cd, path)?;
            if added {
                mirror::mirror_favorite_add(&self.cd, path).ok();
            }
            Ok(added)
        }
    }

    pub fn favorites_remove(&self, path: &str) -> Result<(), WcError> {
        if matches!(self.mode, StorageBackend::Sqlite) {
            sqlite::sqlite_favorite_remove(&self.cd, path)?;
            flat::favorites_remove(&self.cd, path).ok();
        } else {
            flat::favorites_remove(&self.cd, path)?;
            mirror::mirror_favorite_remove(&self.cd, path).ok();
        }
        Ok(())
    }

    pub fn history_add(&self, path: &str, backend: &str) -> Result<(), WcError> {
        let canon = flat::try_canonicalize(path);
        if matches!(self.mode, StorageBackend::Sqlite) {
            sqlite::sqlite_history_add(&self.cd, &canon, backend, 100)?;
            flat::history_add(&self.cd, &canon, 100).ok();
        } else {
            flat::history_add(&self.cd, &canon, 100)?;
            mirror::mirror_history_add(&self.cd, &canon, backend).ok();
            mirror::mirror_history_trim(&self.cd, 100).ok();
        }
        Ok(())
    }

    pub fn history_clear(&self) -> Result<(), WcError> {
        if matches!(self.mode, StorageBackend::Sqlite) {
            sqlite::sqlite_history_clear(&self.cd)?;
            flat::history_clear(&self.cd).ok();
        } else {
            flat::history_clear(&self.cd)?;
            mirror::mirror_history_clear(&self.cd).ok();
        }
        Ok(())
    }

    pub fn current_write(&self, path: &str) -> Result<(), WcError> {
        if matches!(self.mode, StorageBackend::Sqlite) {
            sqlite::sqlite_state_write(&self.cd, "current", path)?;
            flat::current_write(&self.cd, path).ok();
        } else {
            flat::current_write(&self.cd, path)?;
            mirror::mirror_current_write(&self.cd, path).ok();
        }
        Ok(())
    }

    pub fn last_backend_write(&self, backend: &str) -> Result<(), WcError> {
        if matches!(self.mode, StorageBackend::Sqlite) {
            sqlite::sqlite_state_write(&self.cd, "last_backend", backend)?;
            flat::last_backend_write(&self.cd, backend).ok();
        } else {
            flat::last_backend_write(&self.cd, backend)?;
            mirror::mirror_last_backend_write(&self.cd, backend).ok();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_mode_auto_migrates_legacy_flat_state() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        wc_core::config::write_config_value(&cd.path, "storage_backend", "sqlite").unwrap();
        flat::sources_add(&cd, "/walls").unwrap();
        flat::favorites_add(&cd, "/walls/a.jpg").unwrap();
        flat::history_add(&cd, "/walls/b.jpg", 100).unwrap();
        flat::current_write(&cd, "/walls/current.jpg").unwrap();
        flat::last_backend_write(&cd, "awww").unwrap();

        let storage = StorageApi::new(cd);

        assert_eq!(storage.mode, StorageBackend::Sqlite);
        assert!(storage.cd.db_path().exists());
        assert_eq!(storage.sources_list().unwrap(), vec!["/walls".to_string()]);
        assert_eq!(
            storage.favorites_list().unwrap(),
            vec!["/walls/a.jpg".to_string()]
        );
        assert_eq!(
            storage.history_list().unwrap(),
            vec!["/walls/b.jpg".to_string()]
        );
        assert_eq!(
            storage.current_read().unwrap().as_deref(),
            Some("/walls/current.jpg")
        );
        assert_eq!(
            storage.last_backend_read().unwrap().as_deref(),
            Some("awww")
        );
    }

    #[test]
    fn storage_backend_mode_defaults_to_sqlite_when_config_missing() {
        let tmp = tempfile::tempdir().unwrap();

        assert_eq!(storage_backend_mode(tmp.path()), StorageBackend::Sqlite);
    }

    #[test]
    fn sqlite_mirror_active_uses_the_supplied_config_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        wc_core::config::write_config_value(&cd.path, "storage_backend", "hybrid").unwrap();

        assert!(!sqlite_mirror_active(&cd.path));
        sqlite::ensure_sqlite_db(&cd);
        assert!(sqlite_mirror_active(&cd.path));
    }
}

/// Check whether SQLite mirror writes are active.
pub fn sqlite_mirror_active(config_dir: &Path) -> bool {
    let mode = storage_backend_mode(config_dir);
    matches!(mode, StorageBackend::Hybrid | StorageBackend::Sqlite)
        && config_dir.join("wallpapers.db").exists()
}
