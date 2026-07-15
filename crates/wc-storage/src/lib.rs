//! wc-storage — unified storage API. Runtime storage is SQLite-only.

pub mod flat;
pub mod sqlite;
mod sqlite_error;
pub mod tsv;
pub mod we_compat;

pub use sqlite::{SourceAvailability, SourceKind, SourceRecord};
pub(crate) use sqlite_error::sqlite_err;

use wc_config::ConfigDirExt;
use wc_core::config::ConfigDir;
use wc_core::error::WcError;
use wc_core::types::StorageBackend;

/// Unified storage API. All reads and writes go through SQLite.
pub struct StorageApi {
    pub cd: ConfigDir,
    pub mode: StorageBackend,
}

impl StorageApi {
    /// Fallible initializer: surfaces errors from `cd.init()`, SQLite
    /// migration, and config writes instead of swallowing them with `.ok()`.
    /// Use this in production paths that can propagate errors. Test helpers
    /// and existing call sites should keep using [`StorageApi::new`].
    pub fn try_new(cd: ConfigDir) -> Result<Self, WcError> {
        cd.init()?;

        sqlite::ensure_or_import_legacy_flat(&cd)?;
        wc_config::write_config_value(&cd.path, "storage_backend", "sqlite")?;
        sqlite::sqlite_config_set(&cd, "storage_backend", "sqlite")?;

        Ok(StorageApi {
            cd,
            mode: StorageBackend::Sqlite,
        })
    }

    /// Compatibility wrapper around [`StorageApi::try_new`] that panics on
    /// failure. Kept for the many existing test call sites that expect the
    /// historical panicking-on-failure behavior.
    pub fn new(cd: ConfigDir) -> Self {
        Self::try_new(cd).expect("storage initialization failed")
    }

    // ── Reads (always SQLite) ─────────────────────────────────────────

    pub fn config_get(&self, key: &str, default: &str) -> String {
        if key == "storage_backend" {
            return "sqlite".to_string();
        }
        self._sqlite_config_get(key, default)
    }

    /// Load renderer preferences once and clamp them to the compatibility
    /// matrix defined by `wc-core`.
    pub fn backend_routing(&self) -> wc_core::backend_routing::BackendRouting {
        let (image, gif, video) =
            if self.cd.db_path().exists() || sqlite::try_ensure_sqlite_db(&self.cd).is_ok() {
                match sqlite::open_runtime_connection(&self.cd) {
                    Ok(conn) => {
                        let read = |key: &str, default: &str| -> String {
                            match conn.query_row(
                                "SELECT value FROM config WHERE key=?1",
                                [key],
                                |row| row.get::<_, String>(0),
                            ) {
                                Ok(value) => value,
                                Err(rusqlite::Error::QueryReturnedNoRows) => default.to_string(),
                                Err(err) => {
                                    log::warn!("backend_routing({key}): read failed: {err}");
                                    default.to_string()
                                }
                            }
                        };
                        (
                            read("image_backend", "awww"),
                            read("gif_backend", "awww"),
                            read("video_backend", "mpvpaper"),
                        )
                    }
                    Err(err) => {
                        log::warn!("backend_routing: open connection failed: {err}");
                        (
                            "awww".to_string(),
                            "awww".to_string(),
                            "mpvpaper".to_string(),
                        )
                    }
                }
            } else {
                (
                    "awww".to_string(),
                    "awww".to_string(),
                    "mpvpaper".to_string(),
                )
            };
        wc_core::backend_routing::BackendRouting::from_raw(&image, &gif, &video)
    }

    fn _sqlite_config_get(&self, key: &str, default: &str) -> String {
        // Avoid try_ensure on the hot path: it takes an exclusive schema lock and
        // would invalidate the process-wide runtime connection cache on every read.
        // Startup paths (StorageApi::try_new) already bootstrap the database.
        if !self.cd.db_path().exists() {
            if let Err(err) = sqlite::try_ensure_sqlite_db(&self.cd) {
                log::warn!("config_get({key}): failed to ensure sqlite db: {err}");
                return default.to_string();
            }
        }
        match sqlite::open_runtime_connection(&self.cd) {
            Ok(conn) => {
                match conn.query_row("SELECT value FROM config WHERE key=?1", [key], |row| {
                    row.get::<_, String>(0)
                }) {
                    Ok(value) => value,
                    Err(rusqlite::Error::QueryReturnedNoRows) => default.to_string(),
                    Err(err) => {
                        log::warn!("config_get({key}): read failed: {err}");
                        default.to_string()
                    }
                }
            }
            Err(err) => {
                log::warn!("config_get({key}): open connection failed: {err}");
                default.to_string()
            }
        }
    }

    pub fn sources_list(&self) -> Result<Vec<String>, WcError> {
        sqlite::source_paths_list_compat(&self.cd)
    }

    pub fn favorites_list(&self) -> Result<Vec<String>, WcError> {
        if !self.cd.db_path().exists() {
            sqlite::try_ensure_sqlite_db(&self.cd)?;
        }
        let conn = sqlite::open_runtime_connection(&self.cd)?;
        let mut stmt = conn
            .prepare("SELECT path FROM favorites ORDER BY path")
            .map_err(sqlite_err)?;
        let result: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .map_err(sqlite_err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sqlite_err)?;
        Ok(result)
    }

    pub fn current_read(&self) -> Result<Option<String>, WcError> {
        if !self.cd.db_path().exists() {
            sqlite::try_ensure_sqlite_db(&self.cd)?;
        }
        let conn = sqlite::open_runtime_connection(&self.cd)?;
        Ok(conn
            .query_row("SELECT value FROM state WHERE key='current'", [], |row| {
                row.get(0)
            })
            .ok())
    }

    pub fn last_backend_read(&self) -> Result<Option<String>, WcError> {
        if !self.cd.db_path().exists() {
            sqlite::try_ensure_sqlite_db(&self.cd)?;
        }
        let conn = sqlite::open_runtime_connection(&self.cd)?;
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
        if let Err(err) = wc_config::write_config_value(&self.cd.path, key, &value) {
            log::warn!("config_set({key}): flat config write failed: {err}");
        }
        Ok(())
    }

    pub fn sources_add(&self, path: &str) -> Result<bool, WcError> {
        sqlite::sqlite_source_add(&self.cd, path)
    }

    pub fn sources_remove(&self, path: &str) -> Result<bool, WcError> {
        sqlite::sqlite_source_remove_canonical(&self.cd, path)
    }

    pub fn source_records(&self) -> Result<Vec<sqlite::SourceRecord>, WcError> {
        sqlite::sources_list_typed(&self.cd)
    }

    pub fn source_create(&self, path: &str) -> Result<sqlite::SourceRecord, WcError> {
        sqlite::source_create(&self.cd, path).map(|(source, _)| source)
    }

    pub fn source_rename(
        &self,
        id: i64,
        display_name: &str,
    ) -> Result<sqlite::SourceRecord, WcError> {
        sqlite::source_rename(&self.cd, id, display_name)
    }

    pub fn source_set_recursive(
        &self,
        id: i64,
        recursive: bool,
    ) -> Result<sqlite::SourceRecord, WcError> {
        sqlite::source_set_recursive(&self.cd, id, recursive)
    }

    pub fn source_set_availability(
        &self,
        id: i64,
        availability: sqlite::SourceAvailability,
    ) -> Result<sqlite::SourceRecord, WcError> {
        sqlite::source_set_availability(&self.cd, id, availability)
    }

    pub fn source_remove_by_id(&self, id: i64) -> Result<sqlite::SourceRecord, WcError> {
        sqlite::source_remove_by_id(&self.cd, id)
    }

    pub fn favorites_add(&self, path: &str) -> Result<bool, WcError> {
        sqlite::sqlite_favorite_add(&self.cd, path)
    }

    pub fn favorites_remove(&self, path: &str) -> Result<(), WcError> {
        sqlite::sqlite_favorite_remove(&self.cd, path)
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

    // ── Per-display wallpaper state ───────────────────────────────────

    pub fn display_state_get(
        &self,
        target: &sqlite::DisplayStateTarget,
    ) -> Result<Option<sqlite::DisplayStateRow>, WcError> {
        sqlite::display_state_get_cd(&self.cd, target)
    }

    pub fn display_state_list(&self) -> Result<Vec<sqlite::DisplayStateRow>, WcError> {
        sqlite::display_state_list_cd(&self.cd)
    }

    pub fn display_state_upsert(
        &self,
        target: &sqlite::DisplayStateTarget,
        wallpaper_path: &str,
        backend: &str,
    ) -> Result<(), WcError> {
        sqlite::display_state_upsert_cd(&self.cd, target, wallpaper_path, backend)
    }

    pub fn display_state_delete(
        &self,
        target: &sqlite::DisplayStateTarget,
    ) -> Result<bool, WcError> {
        sqlite::display_state_delete_cd(&self.cd, target)
    }

    pub fn display_state_replace_all(
        &self,
        rows: &[(sqlite::DisplayStateTarget, String, String)],
    ) -> Result<(), WcError> {
        sqlite::display_state_replace_all_cd(&self.cd, rows)
    }

    /// Same as [`Self::display_state_replace_all`] with a pre-commit test seam.
    pub fn display_state_replace_all_seam(
        &self,
        rows: &[(sqlite::DisplayStateTarget, String, String)],
        before_commit: &mut dyn FnMut() -> Result<(), WcError>,
    ) -> Result<(), WcError> {
        sqlite::display_state_replace_all_cd_with_seam(&self.cd, rows, before_commit)
    }

    /// Atomically commit All Displays display_state (plus retained disconnected
    /// named rows) and optionally legacy `current` / `last_backend` keys.
    pub fn display_state_commit_all_displays_with_legacy(
        &self,
        wallpaper_path: &str,
        backend: &str,
        retain_rows: &[(sqlite::DisplayStateTarget, String, String)],
        sync_legacy: bool,
    ) -> Result<(), WcError> {
        sqlite::display_state_commit_all_displays_with_legacy_cd(
            &self.cd,
            wallpaper_path,
            backend,
            retain_rows,
            sync_legacy,
            None,
        )
    }

    /// Same as [`Self::display_state_commit_all_displays_with_legacy`] with a
    /// pre-commit seam for failure-injection tests.
    pub fn display_state_commit_all_displays_with_legacy_seam(
        &self,
        wallpaper_path: &str,
        backend: &str,
        retain_rows: &[(sqlite::DisplayStateTarget, String, String)],
        sync_legacy: bool,
        before_commit: &mut dyn FnMut() -> Result<(), WcError>,
    ) -> Result<(), WcError> {
        sqlite::display_state_commit_all_displays_with_legacy_cd(
            &self.cd,
            wallpaper_path,
            backend,
            retain_rows,
            sync_legacy,
            Some(before_commit),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn insert_history(cd: &ConfigDir, path: &str, backend: &str) {
        let conn = sqlite::open_runtime_connection(cd).unwrap();
        conn.execute(
            "INSERT INTO history (path, backend) VALUES (?1, ?2)",
            [path, backend],
        )
        .unwrap();
    }

    fn history_rows(cd: &ConfigDir) -> Vec<(String, String)> {
        let conn = sqlite::open_runtime_connection(cd).unwrap();
        let mut stmt = conn
            .prepare("SELECT path, backend FROM history ORDER BY id")
            .unwrap();
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    }

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
            history_rows(&storage.cd),
            vec![("/walls/b.jpg".to_string(), "unknown".to_string())]
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
    fn try_new_surfaces_fallible_sqlite_bootstrap_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        let conn = rusqlite::Connection::open(cd.db_path()).unwrap();
        conn.execute("CREATE TABLE wallpapers_fts(dummy TEXT)", [])
            .unwrap();
        drop(conn);

        match StorageApi::try_new(cd) {
            Ok(_) => panic!("bootstrap errors must surface"),
            Err(err) => assert!(
                err.to_string().contains("wallpapers_fts") || err.to_string().contains("table"),
                "{err}"
            ),
        }
    }

    #[test]
    fn result_reads_reject_future_schema_without_changing_marker_or_data() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        let future_version = sqlite::CURRENT_SCHEMA_VERSION + 1;
        let conn = rusqlite::Connection::open(cd.db_path()).unwrap();
        sqlite::create_schema(&conn).unwrap();
        conn.execute(
            "INSERT INTO favorites (path) VALUES ('/walls/sentinel.jpg')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO history (path, backend) VALUES ('/walls/sentinel.jpg', 'awww')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO state (key, value) VALUES ('current', '/walls/sentinel.jpg')",
            [],
        )
        .unwrap();
        conn.pragma_update(None, "user_version", future_version)
            .unwrap();
        drop(conn);
        let storage = StorageApi {
            cd,
            mode: StorageBackend::Sqlite,
        };

        let results = [
            storage.favorites_list().map(|_| ()),
            storage.current_read().map(|_| ()),
            storage.last_backend_read().map(|_| ()),
        ];

        let conn = rusqlite::Connection::open(storage.cd.db_path()).unwrap();
        let version = conn
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap();
        let sentinel_rows: i64 = conn
            .query_row(
                "SELECT
                    (SELECT COUNT(*) FROM favorites WHERE path = '/walls/sentinel.jpg') +
                    (SELECT COUNT(*) FROM history WHERE path = '/walls/sentinel.jpg') +
                    (SELECT COUNT(*) FROM state
                     WHERE key = 'current' AND value = '/walls/sentinel.jpg')",
                [],
                |row| row.get(0),
            )
            .unwrap();

        for result in results {
            let error = result.expect_err("future-schema Result read must be rejected");
            assert!(
                error.to_string().contains("newer") || error.to_string().contains("version"),
                "{error}"
            );
        }
        assert_eq!(version, future_version);
        assert_eq!(sentinel_rows, 3);
    }

    #[test]
    fn storage_api_migrates_legacy_pair_into_all_displays_without_deleting_keys() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        let storage = StorageApi::new(cd);
        storage.current_write("/walls/current.jpg").unwrap();
        storage.last_backend_write("awww").unwrap();

        // Re-open path that runs ensure_sqlite_db / create_schema migration.
        let storage = StorageApi::new(ConfigDir {
            path: storage.cd.path.clone(),
        });

        let row = storage
            .display_state_get(&sqlite::DisplayStateTarget::AllDisplays)
            .unwrap()
            .expect("All Displays migrated");
        assert_eq!(row.wallpaper_path, "/walls/current.jpg");
        assert_eq!(row.backend, "awww");
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

        flat::write_lines(&cd.sources_path(), std::slice::from_ref(&proj_str)).unwrap();

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
        insert_history(&storage.cd, "/walls/current.jpg", "awww");

        storage.runtime_state_clear().unwrap();

        assert_eq!(storage.current_read().unwrap(), None);
        assert_eq!(storage.last_backend_read().unwrap(), None);
        assert_eq!(
            history_rows(&storage.cd),
            vec![("/walls/current.jpg".to_string(), "awww".to_string())]
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
            wc_config::read_config_value(&storage.cd.path, "storage_backend", "file"),
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
        assert!(history_rows(&storage.cd).is_empty());
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
            wc_config::read_config_value(&storage.cd.path, "storage_backend", "missing"),
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

    #[test]
    fn backend_routing_reads_and_safely_normalizes_all_renderer_preferences() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = StorageApi::new(ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        });
        storage.config_set("image_backend", "mpvpaper").unwrap();
        storage.config_set("gif_backend", "invalid").unwrap();
        storage.config_set("video_backend", "awww").unwrap();

        let routing = storage.backend_routing();

        assert_eq!(
            routing.backend_for(wc_core::types::FileType::Image),
            wc_core::types::Backend::Mpvpaper
        );
        assert_eq!(
            routing.backend_for(wc_core::types::FileType::Gif),
            wc_core::types::Backend::Awww
        );
        assert_eq!(
            routing.backend_for(wc_core::types::FileType::Video),
            wc_core::types::Backend::Mpvpaper
        );
    }

    #[test]
    fn repeated_config_and_page_reads_reuse_runtime_connection() {
        use sqlite::{
            browser_library_page, invalidate_cached_connections,
            reset_runtime_connection_open_count, runtime_connection_open_count, LibraryBrowserPage,
            LibraryBrowserQuery, LibraryBrowserSort, LibraryBrowserType,
        };

        let tmp = tempfile::tempdir().unwrap();
        let storage = StorageApi::new(ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        });
        storage.config_set("image_backend", "awww").unwrap();
        storage.config_set("gif_backend", "awww").unwrap();
        storage.config_set("video_backend", "mpvpaper").unwrap();

        invalidate_cached_connections();
        reset_runtime_connection_open_count();

        assert_eq!(storage.config_get("image_backend", "missing"), "awww");
        assert_eq!(storage.config_get("gif_backend", "missing"), "awww");
        assert_eq!(storage.config_get("video_backend", "missing"), "mpvpaper");
        let _routing = storage.backend_routing();

        let page: LibraryBrowserPage = browser_library_page(
            &storage.cd,
            &LibraryBrowserQuery {
                source_id: None,
                type_filter: LibraryBrowserType::Usable,
                favorites_only: false,
                search: String::new(),
                sort: LibraryBrowserSort::RecentlyAdded,
                offset: 0,
                limit: 20,
            },
        )
        .unwrap();
        assert_eq!(page.total, 0);

        let opens_after_reads = runtime_connection_open_count();
        assert!(
            opens_after_reads >= 1,
            "expected at least one runtime open, got {opens_after_reads}"
        );
        assert!(
            opens_after_reads < 8,
            "repeated config/page reads should reuse the cached connection, opened {opens_after_reads} times"
        );

        // Maintenance must be able to take exclusive after invalidating the idle cache.
        sqlite::take_exclusive_maintenance_lock(&storage.cd).unwrap();
    }

    #[test]
    fn try_new_returns_err_when_config_dir_path_is_a_file() {
        let tmp = tempfile::tempdir().unwrap();
        // Create a regular FILE where the config dir would live, so
        // `cd.init()` cannot create_dir_all on it as a directory.
        let conflict = tmp.path().join("wallpaper-console");
        std::fs::write(&conflict, b"not a directory").unwrap();

        let cd = ConfigDir { path: conflict };
        let result = StorageApi::try_new(cd);

        assert!(
            result.is_err(),
            "try_new should surface the init error, got Ok"
        );
    }
}
