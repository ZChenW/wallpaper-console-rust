//! wc-storage — unified storage API. Runtime storage is SQLite-only.

pub mod flat;
pub mod sqlite;
pub mod tsv;
pub mod we_compat;

use wc_core::config::ConfigDir;
use wc_core::error::WcError;
use wc_core::types::StorageBackend;

/// Runtime storage is SQLite-only. Legacy flat files may still be imported
/// during initialization.
pub fn storage_backend_mode(_config_dir: &std::path::Path) -> StorageBackend {
    StorageBackend::Sqlite
}

/// Unified storage API. All reads and writes go through SQLite.
pub struct StorageApi {
    pub cd: ConfigDir,
    pub mode: StorageBackend,
}

impl StorageApi {
    pub fn new(cd: ConfigDir) -> Self {
        cd.init().ok();

        if !cd.db_path().exists() {
            sqlite::migrate_to_sqlite(&cd).ok();
        }
        sqlite::ensure_sqlite_db(&cd);
        wc_core::config::write_config_value(&cd.path, "storage_backend", "sqlite").ok();
        sqlite::sqlite_config_set(&cd, "storage_backend", "sqlite").ok();

        StorageApi {
            cd,
            mode: StorageBackend::Sqlite,
        }
    }

    /// Re-read the backend mode from config (after config-set).
    pub fn refresh_mode(&mut self) {
        self.mode = StorageBackend::Sqlite;
        wc_core::config::write_config_value(&self.cd.path, "storage_backend", "sqlite").ok();
        sqlite::sqlite_config_set(&self.cd, "storage_backend", "sqlite").ok();
        sqlite::ensure_sqlite_db(&self.cd);
    }

    // ── Reads (always SQLite) ─────────────────────────────────────────

    pub fn config_get(&self, key: &str, default: &str) -> String {
        if key == "storage_backend" {
            return "sqlite".to_string();
        }
        self._sqlite_config_get(key, default)
    }

    fn _sqlite_config_get(&self, key: &str, default: &str) -> String {
        sqlite::ensure_sqlite_db(&self.cd);
        let db = self.cd.db_path();
        if !db.exists() {
            return default.to_string();
        }
        match rusqlite::Connection::open(&db) {
            Ok(conn) => conn
                .query_row("SELECT value FROM config WHERE key=?1", [key], |row| {
                    row.get::<_, String>(0)
                })
                .unwrap_or_else(|_| default.to_string()),
            _ => default.to_string(),
        }
    }

    pub fn sources_list(&self) -> Result<Vec<String>, WcError> {
        sqlite::ensure_sqlite_db(&self.cd);
        let conn = rusqlite::Connection::open(self.cd.db_path())
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        let mut stmt = conn
            .prepare("SELECT path FROM sources ORDER BY path")
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        let rows: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| WcError::Sqlite(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        let normalized: Vec<String> = rows
            .into_iter()
            .map(|p| wc_scan::normalize_source_path(&p))
            .collect();
        let mut seen = std::collections::HashSet::new();
        Ok(normalized
            .into_iter()
            .filter(|p| seen.insert(flat::try_canonicalize(p)))
            .collect())
    }

    pub fn favorites_list(&self) -> Result<Vec<String>, WcError> {
        sqlite::ensure_sqlite_db(&self.cd);
        let conn = rusqlite::Connection::open(self.cd.db_path())
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        let mut stmt = conn
            .prepare("SELECT path FROM favorites ORDER BY path")
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        let result: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| WcError::Sqlite(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        Ok(result)
    }

    pub fn history_list(&self) -> Result<Vec<String>, WcError> {
        sqlite::ensure_sqlite_db(&self.cd);
        let conn = rusqlite::Connection::open(self.cd.db_path())
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        let mut stmt = conn
            .prepare("SELECT path FROM history ORDER BY id DESC")
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        let rows: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| WcError::Sqlite(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        let mut seen = std::collections::HashSet::new();
        Ok(rows
            .into_iter()
            .filter(|p| seen.insert(flat::try_canonicalize(p)))
            .collect())
    }

    pub fn current_read(&self) -> Result<Option<String>, WcError> {
        sqlite::ensure_sqlite_db(&self.cd);
        let conn = rusqlite::Connection::open(self.cd.db_path())
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        Ok(conn
            .query_row("SELECT value FROM state WHERE key='current'", [], |row| {
                row.get(0)
            })
            .ok())
    }

    pub fn last_backend_read(&self) -> Result<Option<String>, WcError> {
        sqlite::ensure_sqlite_db(&self.cd);
        let conn = rusqlite::Connection::open(self.cd.db_path())
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        Ok(conn
            .query_row(
                "SELECT value FROM state WHERE key='last_backend'",
                [],
                |row| row.get(0),
            )
            .ok())
    }

    // ── Writes (always SQLite) ────────────────────────────────────────

    pub fn config_set(&self, key: &str, value: &str) -> Result<(), WcError> {
        let value = wc_core::config_normalizer::normalize_config_value(key, value);
        sqlite::sqlite_config_set(&self.cd, key, &value)?;
        wc_core::config::write_config_value(&self.cd.path, key, &value).ok();
        Ok(())
    }

    pub fn sources_add(&self, path: &str) -> Result<bool, WcError> {
        let path = wc_scan::normalize_source_path(path);
        sqlite::sqlite_source_add(&self.cd, &path)
    }

    pub fn sources_remove(&self, path: &str) -> Result<bool, WcError> {
        let path = wc_scan::normalize_source_path(path);
        sqlite::sqlite_source_remove_canonical(&self.cd, &path)
    }

    pub fn favorites_add(&self, path: &str) -> Result<bool, WcError> {
        sqlite::sqlite_favorite_add(&self.cd, path)
    }

    pub fn favorites_remove(&self, path: &str) -> Result<(), WcError> {
        sqlite::sqlite_favorite_remove(&self.cd, path)
    }

    pub fn history_add(&self, path: &str, backend: &str) -> Result<(), WcError> {
        let canon = flat::try_canonicalize(path);
        sqlite::sqlite_history_add(&self.cd, &canon, backend, 100)
    }

    pub fn history_clear(&self) -> Result<(), WcError> {
        sqlite::sqlite_history_clear(&self.cd)
    }

    pub fn current_write(&self, path: &str) -> Result<(), WcError> {
        sqlite::sqlite_state_write(&self.cd, "current", path)
    }

    pub fn last_backend_write(&self, backend: &str) -> Result<(), WcError> {
        sqlite::sqlite_state_write(&self.cd, "last_backend", backend)
    }

    pub fn runtime_state_clear(&self) -> Result<(), WcError> {
        sqlite::sqlite_state_delete(&self.cd, "current")?;
        sqlite::sqlite_state_delete(&self.cd, "last_backend")?;
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
    fn sqlite_config_get_reads_keys_with_apostrophes() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        let storage = StorageApi::new(cd);

        storage
            .config_set("artist's_key", "artist's value")
            .unwrap();

        assert_eq!(
            storage.config_get("artist's_key", "missing"),
            "artist's value"
        );
    }

    #[test]
    fn source_normalization_collapses_we_project_to_root() {
        let root = tempfile::tempdir().unwrap();
        let marker = root.path().join("steamapps/workshop/content/431960");
        std::fs::create_dir_all(&marker).unwrap();
        let project = marker.join("123456");
        std::fs::create_dir_all(&project).unwrap();

        let cd = ConfigDir {
            path: root.path().join("wallpaper-console"),
        };
        cd.init().unwrap();

        let storage = StorageApi::new(cd);
        storage.sources_add(&project.to_string_lossy()).unwrap();
        let list = storage.sources_list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(
            list[0],
            marker.to_string_lossy().to_string(),
            "project dir should collapse to workshop root"
        );
    }

    #[test]
    fn source_remove_cleans_raw_sqlite_project_level_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();

        let root = tmp.path().join("steamapps/workshop/content/431960");
        std::fs::create_dir_all(&root).unwrap();
        let project = root.join("123456");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("project.json"), b"{}").unwrap();

        let root_str = root.to_string_lossy().to_string();
        let proj_str = project.to_string_lossy().to_string();

        {
            let db = cd.db_path();
            let conn = rusqlite::Connection::open(&db).unwrap();
            crate::sqlite::ensure_sqlite_db(&cd);
            conn.execute(
                "INSERT INTO sources (path) VALUES (?1)",
                rusqlite::params![proj_str],
            )
            .unwrap();
        }

        let s = StorageApi::new(cd);
        assert!(s.sources_remove(&root_str).unwrap());
        let remaining = s.sources_list().unwrap();
        assert!(
            remaining.is_empty(),
            "all sources should be gone, got: {:?}",
            remaining
        );
    }

    #[test]
    fn source_remove_cleans_raw_flat_project_level_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();

        let root = tmp.path().join("steamapps/workshop/content/431960");
        std::fs::create_dir_all(&root).unwrap();
        let project = root.join("123456");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("project.json"), b"{}").unwrap();

        let root_str = root.to_string_lossy().to_string();
        let proj_str = project.to_string_lossy().to_string();

        flat::write_lines(&cd.sources_path(), &[proj_str.clone()]).unwrap();

        let s = StorageApi::new(cd);
        assert!(s.sources_remove(&root_str).unwrap());
        let remaining = s.sources_list().unwrap();
        assert!(
            remaining.is_empty(),
            "all sources should be gone, got: {:?}",
            remaining
        );
    }

    #[test]
    fn runtime_state_clear_removes_current_and_last_backend_but_preserves_history() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = wc_core::ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        let storage = StorageApi::new(cd);

        storage.current_write("/walls/current.jpg").unwrap();
        storage.last_backend_write("awww").unwrap();
        storage.history_add("/walls/current.jpg", "awww").unwrap();

        storage.runtime_state_clear().unwrap();

        assert_eq!(storage.current_read().unwrap(), None);
        assert_eq!(storage.last_backend_read().unwrap(), None);
        assert_eq!(
            storage.history_list().unwrap(),
            vec!["/walls/current.jpg".to_string()]
        );
    }

    #[test]
    fn source_remove_canonical_cleans_both_root_and_project_level() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();

        let root = tmp.path().join("steamapps/workshop/content/431960");
        std::fs::create_dir_all(&root).unwrap();
        let project = root.join("123456");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("project.json"), b"{}").unwrap();

        let root_str = root.to_string_lossy().to_string();
        let proj_str = project.to_string_lossy().to_string();

        let s = StorageApi::new(cd);
        s.sources_add(&root_str).unwrap();
        s.sources_add(&proj_str).unwrap();

        let all = s.sources_list().unwrap();
        assert_eq!(all.len(), 1, "normalized should dedupe to one: {:?}", all);

        {
            let db = s.cd.db_path();
            let conn = rusqlite::Connection::open(&db).unwrap();
            conn.execute(
                "INSERT INTO sources (path) VALUES (?1)",
                rusqlite::params![proj_str],
            )
            .unwrap();
        }

        assert!(s.sources_remove(&root_str).unwrap());
        let remaining = s.sources_list().unwrap();
        assert!(
            remaining.is_empty(),
            "both root and project should be gone, got: {:?}",
            remaining
        );
    }

    #[test]
    fn source_normalization_dedupes_canonical_duplicates() {
        let root = tempfile::tempdir().unwrap();
        let real = root.path().join("walls");
        std::fs::create_dir_all(&real).unwrap();
        let link = root.path().join("walls-link");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let cd = ConfigDir {
            path: root.path().join("wallpaper-console"),
        };
        cd.init().unwrap();

        let storage = StorageApi::new(cd);
        storage.sources_add(&real.to_string_lossy()).unwrap();
        storage.sources_add(&link.to_string_lossy()).unwrap();
        let list = storage.sources_list().unwrap();
        assert_eq!(list.len(), 1, "canonical duplicates should be deduped");
    }

    #[test]
    fn source_remove_canonical_cleans_project_level_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();

        let root = tmp.path().join("steamapps/workshop/content/431960");
        std::fs::create_dir_all(&root).unwrap();
        let project = root.join("1234567890");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("project.json"), b"{}").unwrap();

        let root_str = root.to_string_lossy().to_string();
        let proj_str = project.to_string_lossy().to_string();

        let s = StorageApi::new(cd);
        s.sources_add(&root_str).unwrap();
        s.sources_add(&proj_str).unwrap();

        assert!(s.sources_remove(&root_str).unwrap());
        let remaining = s.sources_list().unwrap();
        assert!(
            remaining.is_empty(),
            "all sources should be gone, got: {:?}",
            remaining
        );
    }

    #[test]
    fn legacy_file_config_is_repaired_to_sqlite() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        std::fs::write(cd.config_path(), "storage_backend=file\n").unwrap();

        let storage = StorageApi::new(cd);

        assert_eq!(storage.mode, StorageBackend::Sqlite);
        assert_eq!(storage.config_get("storage_backend", "file"), "sqlite");
        assert_eq!(
            wc_core::config::read_config_value(&storage.cd.path, "storage_backend", "file"),
            "sqlite"
        );
    }

    #[test]
    fn missing_sqlite_db_is_created_for_empty_reads() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();

        let storage = StorageApi::new(cd);

        assert!(storage.cd.db_path().exists());
        assert_eq!(storage.sources_list().unwrap(), Vec::<String>::new());
        assert_eq!(storage.favorites_list().unwrap(), Vec::<String>::new());
        assert_eq!(storage.history_list().unwrap(), Vec::<String>::new());
        assert_eq!(storage.current_read().unwrap(), None);
        assert_eq!(storage.last_backend_read().unwrap(), None);
    }

    #[test]
    fn storage_backend_config_set_is_forced_to_sqlite() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        let storage = StorageApi::new(cd);

        storage.config_set("storage_backend", "hybrid").unwrap();

        assert_eq!(storage.config_get("storage_backend", "missing"), "sqlite");
        assert_eq!(
            wc_core::config::read_config_value(&storage.cd.path, "storage_backend", "missing"),
            "sqlite"
        );
    }

    #[test]
    fn sqlite_config_table_is_normalized_after_legacy_import() {
        // Regression: P2-3 — after initializing from legacy storage_backend=file,
        // the SQLite config table must contain storage_backend=sqlite.
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        std::fs::write(cd.config_path(), "storage_backend=file\n").unwrap();

        let storage = StorageApi::new(cd);

        let conn = rusqlite::Connection::open(storage.cd.db_path()).unwrap();
        let db_value: String = conn
            .query_row(
                "SELECT value FROM config WHERE key='storage_backend'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            db_value, "sqlite",
            "SQLite config table must be normalized to sqlite"
        );
    }

    #[test]
    fn config_set_normalizes_runtime_sensitive_values() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        let storage = StorageApi::new(cd);

        storage
            .config_set("wallpaper_transition_fps", "999")
            .unwrap();
        assert_eq!(
            storage.config_get("wallpaper_transition_fps", "missing"),
            "240"
        );

        storage
            .config_set("awww_transition_duration", "-1")
            .unwrap();
        assert_eq!(
            storage.config_get("awww_transition_duration", "missing"),
            "1"
        );

        storage
            .config_set("linux_wallpaperengine_target_mode", "window")
            .unwrap();
        assert_eq!(
            storage.config_get("linux_wallpaperengine_target_mode", "missing"),
            "auto"
        );
    }
}
