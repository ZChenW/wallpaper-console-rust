//! Named wallpaper sources and their stable identity.
//!
//! This module owns the small source repository API. Callers work with typed
//! records; compatibility path-only APIs adapt to these operations elsewhere.

use std::collections::HashSet;
use std::path::Path;

use crate::sqlite_err;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
#[cfg(test)]
use wc_config::ConfigDirExt;
use wc_core::config::ConfigDir;
use wc_core::error::WcError;

use super::schema::{open_runtime_connection, try_ensure_sqlite_db, RuntimeConnection};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Directory,
    WallpaperEngineWorkshop,
}

impl SourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Directory => "directory",
            Self::WallpaperEngineWorkshop => "wallpaper_engine_workshop",
        }
    }

    fn parse(value: &str) -> Result<Self, WcError> {
        match value {
            "directory" => Ok(Self::Directory),
            "wallpaper_engine_workshop" => Ok(Self::WallpaperEngineWorkshop),
            other => Err(WcError::Sqlite(format!(
                "invalid source kind stored in database: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceAvailability {
    Unknown,
    Available,
    Offline,
}

impl SourceAvailability {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Available => "available",
            Self::Offline => "offline",
        }
    }

    fn parse(value: &str) -> Result<Self, WcError> {
        match value {
            "unknown" => Ok(Self::Unknown),
            "available" => Ok(Self::Available),
            "offline" => Ok(Self::Offline),
            other => Err(WcError::Sqlite(format!(
                "invalid source availability stored in database: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRecord {
    pub id: i64,
    pub path: String,
    pub display_name: String,
    pub kind: SourceKind,
    pub recursive: bool,
    pub availability: SourceAvailability,
    pub added_at: String,
}

fn source_connection(cd: &ConfigDir) -> Result<RuntimeConnection, WcError> {
    try_ensure_sqlite_db(cd)?;
    open_runtime_connection(cd)
}

fn source_from_values(
    id: i64,
    path: String,
    display_name: String,
    kind: String,
    recursive: i64,
    availability: String,
    added_at: String,
) -> Result<SourceRecord, WcError> {
    if display_name.trim().is_empty() {
        return Err(WcError::Sqlite(format!(
            "source {id} has a blank display name"
        )));
    }
    if !matches!(recursive, 0 | 1) {
        return Err(WcError::Sqlite(format!(
            "source {id} has invalid recursive value {recursive}"
        )));
    }
    Ok(SourceRecord {
        id,
        path,
        display_name,
        kind: SourceKind::parse(&kind)?,
        recursive: recursive == 1,
        availability: SourceAvailability::parse(&availability)?,
        added_at,
    })
}

fn source_get_from_conn(conn: &Connection, id: i64) -> Result<SourceRecord, WcError> {
    let values = conn
        .query_row(
            "SELECT id, path, display_name, kind, recursive, availability, added_at
             FROM sources WHERE id = ?1",
            params![id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                ))
            },
        )
        .optional()
        .map_err(sqlite_err)?
        .ok_or_else(|| WcError::Other(format!("source id {id} not found")))?;
    source_from_values(
        values.0, values.1, values.2, values.3, values.4, values.5, values.6,
    )
}

fn normalize_new_source_path(path: &str) -> Result<String, WcError> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(WcError::Other("source path must not be blank".into()));
    }
    if !Path::new(trimmed).is_absolute() {
        return Err(WcError::Other(format!(
            "source path must be absolute: {trimmed}"
        )));
    }
    Ok(current_source_identity(trimmed))
}

fn current_source_identity(path: &str) -> String {
    if std::fs::canonicalize(path).is_ok() {
        wc_scan::normalize_source_path(path)
    } else {
        // Offline/missing sources retain the supplied absolute identity. They
        // can be reconciled on a later successful scan without data loss.
        path.to_string()
    }
}

fn inferred_kind(path: &str) -> SourceKind {
    if wc_scan::is_wallpaper_engine_source(path) {
        SourceKind::WallpaperEngineWorkshop
    } else {
        SourceKind::Directory
    }
}

fn inferred_display_name(path: &str, kind: SourceKind) -> String {
    if kind == SourceKind::WallpaperEngineWorkshop {
        return "Wallpaper Engine".to_string();
    }
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(path)
        .to_string()
}

pub(crate) fn source_defaults(path: &str) -> Result<(String, String, SourceKind, bool), WcError> {
    let path = normalize_new_source_path(path)?;
    let kind = inferred_kind(&path);
    let display_name = inferred_display_name(&path, kind);
    let recursive = kind == SourceKind::Directory;
    Ok((path, display_name, kind, recursive))
}

pub fn sources_list_typed(cd: &ConfigDir) -> Result<Vec<SourceRecord>, WcError> {
    let conn = source_connection(cd)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, path, display_name, kind, recursive, availability, added_at
             FROM sources ORDER BY path, id",
        )
        .map_err(sqlite_err)?;
    let values = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
            ))
        })
        .map_err(sqlite_err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sqlite_err)?;
    values
        .into_iter()
        .map(|value| {
            source_from_values(
                value.0, value.1, value.2, value.3, value.4, value.5, value.6,
            )
        })
        .collect()
}

pub fn source_get(cd: &ConfigDir, id: i64) -> Result<SourceRecord, WcError> {
    let conn = source_connection(cd)?;
    source_get_from_conn(&conn, id)
}

/// Create a source with inferred defaults, returning the stable row and
/// whether this call inserted it. Existing canonical identities are idempotent.
pub fn source_create(cd: &ConfigDir, path: &str) -> Result<(SourceRecord, bool), WcError> {
    let (path, display_name, kind, recursive) = source_defaults(path)?;
    let mut conn = source_connection(cd)?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sqlite_err)?;
    let rows = {
        let mut stmt = tx
            .prepare("SELECT id, path FROM sources ORDER BY id")
            .map_err(sqlite_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(sqlite_err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sqlite_err)?;
        rows
    };
    let matching_ids: Vec<i64> = rows
        .into_iter()
        .filter_map(|(id, candidate)| (current_source_identity(&candidate) == path).then_some(id))
        .collect();

    let (id, created) = if matching_ids.is_empty() {
        tx.execute(
            "INSERT OR IGNORE INTO sources
             (path, display_name, kind, recursive, availability)
             VALUES (?1, ?2, ?3, ?4, 'unknown')",
            params![path, display_name, kind.as_str(), recursive],
        )
        .map_err(sqlite_err)?;
        let id = tx
            .query_row(
                "SELECT id FROM sources WHERE path = ?1",
                params![path],
                |row| row.get::<_, i64>(0),
            )
            .map_err(sqlite_err)?;
        (id, true)
    } else {
        let survivor_id = *matching_ids
            .iter()
            .min()
            .expect("matching source IDs are non-empty");
        for alias_id in matching_ids.iter().copied().filter(|id| *id != survivor_id) {
            tx.execute(
                "INSERT OR IGNORE INTO wallpaper_sources
                 (wallpaper_id, source_id, last_seen_at)
                 SELECT wallpaper_id, ?1, last_seen_at
                 FROM wallpaper_sources WHERE source_id = ?2",
                params![survivor_id, alias_id],
            )
            .map_err(sqlite_err)?;
            tx.execute(
                "UPDATE wallpapers SET source_id = ?1 WHERE source_id = ?2",
                params![survivor_id, alias_id],
            )
            .map_err(sqlite_err)?;
            tx.execute(
                "DELETE FROM wallpaper_sources WHERE source_id = ?1",
                params![alias_id],
            )
            .map_err(sqlite_err)?;
            tx.execute("DELETE FROM sources WHERE id = ?1", params![alias_id])
                .map_err(sqlite_err)?;
        }
        tx.execute(
            "UPDATE sources SET path = ?1 WHERE id = ?2",
            params![path, survivor_id],
        )
        .map_err(sqlite_err)?;
        (survivor_id, false)
    };
    let source = source_get_from_conn(&tx, id)?;
    tx.commit().map_err(sqlite_err)?;
    Ok((source, created))
}

pub fn source_rename(cd: &ConfigDir, id: i64, display_name: &str) -> Result<SourceRecord, WcError> {
    let display_name = display_name.trim();
    if display_name.is_empty() {
        return Err(WcError::Other(
            "source display name must not be blank".into(),
        ));
    }
    let conn = source_connection(cd)?;
    source_get_from_conn(&conn, id)?;
    conn.execute(
        "UPDATE sources SET display_name = ?1 WHERE id = ?2",
        params![display_name, id],
    )
    .map_err(sqlite_err)?;
    source_get_from_conn(&conn, id)
}

pub fn source_set_recursive(
    cd: &ConfigDir,
    id: i64,
    recursive: bool,
) -> Result<SourceRecord, WcError> {
    let conn = source_connection(cd)?;
    let source = source_get_from_conn(&conn, id)?;
    if source.kind == SourceKind::WallpaperEngineWorkshop {
        return Err(WcError::Other(
            "Wallpaper Engine Workshop recursion is managed by its specialized scanner".into(),
        ));
    }
    conn.execute(
        "UPDATE sources SET recursive = ?1 WHERE id = ?2",
        params![recursive, id],
    )
    .map_err(sqlite_err)?;
    source_get_from_conn(&conn, id)
}

pub fn source_set_availability(
    cd: &ConfigDir,
    id: i64,
    availability: SourceAvailability,
) -> Result<SourceRecord, WcError> {
    let conn = source_connection(cd)?;
    source_get_from_conn(&conn, id)?;
    conn.execute(
        "UPDATE sources SET availability = ?1 WHERE id = ?2",
        params![availability.as_str(), id],
    )
    .map_err(sqlite_err)?;
    source_get_from_conn(&conn, id)
}

pub fn source_remove_by_id(cd: &ConfigDir, id: i64) -> Result<SourceRecord, WcError> {
    source_remove_by_id_with_seam(cd, id, |_| Ok(()))
}

fn source_remove_by_id_with_seam<F>(
    cd: &ConfigDir,
    id: i64,
    before_commit: F,
) -> Result<SourceRecord, WcError>
where
    F: FnOnce(&Connection) -> Result<(), WcError>,
{
    let mut conn = source_connection(cd)?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sqlite_err)?;
    let source = source_get_from_conn(&tx, id)?;
    tx.execute(
        "UPDATE wallpapers SET source_id = NULL WHERE source_id = ?1",
        params![id],
    )
    .map_err(sqlite_err)?;
    tx.execute("DELETE FROM sources WHERE id = ?1", params![id])
        .map_err(sqlite_err)?;
    before_commit(&tx)?;
    tx.commit().map_err(sqlite_err)?;
    Ok(source)
}

pub(crate) fn source_paths_list_compat(cd: &ConfigDir) -> Result<Vec<String>, WcError> {
    let conn = source_connection(cd)?;
    let mut stmt = conn
        .prepare("SELECT path FROM sources ORDER BY path")
        .map_err(sqlite_err)?;
    let paths = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(sqlite_err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sqlite_err)?;
    let mut seen = HashSet::new();
    Ok(paths
        .into_iter()
        .map(|path| wc_scan::normalize_source_path(&path))
        .filter(|path| seen.insert(crate::flat::try_canonicalize(path)))
        .collect())
}

pub(crate) fn source_remove_canonical_compat(cd: &ConfigDir, path: &str) -> Result<bool, WcError> {
    let mut conn = source_connection(cd)?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sqlite_err)?;
    let target = wc_scan::normalize_source_path(path);
    let rows = {
        let mut stmt = tx
            .prepare("SELECT id, path FROM sources")
            .map_err(sqlite_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(sqlite_err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sqlite_err)?;
        rows
    };
    let ids: Vec<i64> = rows
        .into_iter()
        .filter_map(|(id, candidate)| {
            (wc_scan::normalize_source_path(&candidate) == target).then_some(id)
        })
        .collect();
    for id in &ids {
        tx.execute(
            "UPDATE wallpapers SET source_id = NULL WHERE source_id = ?1",
            params![id],
        )
        .map_err(sqlite_err)?;
        tx.execute("DELETE FROM sources WHERE id = ?1", params![id])
            .map_err(sqlite_err)?;
    }
    tx.commit().map_err(sqlite_err)?;
    Ok(!ids.is_empty())
}

#[cfg(test)]
mod tests {
    use rusqlite::{params, Connection};
    use wc_core::config::ConfigDir;

    use super::*;

    fn storage() -> (tempfile::TempDir, ConfigDir) {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        crate::sqlite::try_ensure_sqlite_db(&cd).unwrap();
        (tmp, cd)
    }

    #[test]
    fn create_directory_source_populates_stable_defaults() {
        let (tmp, cd) = storage();
        let path = tmp.path().join("my-walls");
        std::fs::create_dir(&path).unwrap();

        let (source, created) = source_create(&cd, &path.to_string_lossy()).unwrap();

        assert!(created);
        assert!(source.id > 0);
        assert_eq!(source.path, path.to_string_lossy());
        assert_eq!(source.display_name, "my-walls");
        assert_eq!(source.kind, SourceKind::Directory);
        assert!(source.recursive);
        assert_eq!(source.availability, SourceAvailability::Unknown);
        assert!(!source.added_at.is_empty());
        assert_eq!(sources_list_typed(&cd).unwrap(), vec![source]);
    }

    #[test]
    fn rename_preserves_id_path_and_membership() {
        let (tmp, cd) = storage();
        let source_path = tmp.path().join("walls");
        std::fs::create_dir(&source_path).unwrap();
        let (source, _) = source_create(&cd, &source_path.to_string_lossy()).unwrap();
        let conn = Connection::open(cd.db_path()).unwrap();
        conn.execute("PRAGMA foreign_keys = ON", []).unwrap();
        conn.execute(
            "INSERT INTO wallpapers (path, type, ext, backend)
             VALUES (?1, 'image', 'jpg', 'awww')",
            params![source_path.join("a.jpg").to_string_lossy()],
        )
        .unwrap();
        let wallpaper_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO wallpaper_sources (wallpaper_id, source_id) VALUES (?1, ?2)",
            params![wallpaper_id, source.id],
        )
        .unwrap();
        drop(conn);

        let renamed = source_rename(&cd, source.id, "Curated").unwrap();

        assert_eq!(renamed.id, source.id);
        assert_eq!(renamed.path, source.path);
        assert_eq!(renamed.display_name, "Curated");
        let conn = Connection::open(cd.db_path()).unwrap();
        let membership: (i64, i64) = conn
            .query_row(
                "SELECT wallpaper_id, source_id FROM wallpaper_sources",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(membership, (wallpaper_id, source.id));
    }

    #[test]
    fn rename_rejects_blank_names_and_unknown_ids() {
        let (_tmp, cd) = storage();

        let blank = source_rename(&cd, 999, "  ").unwrap_err();
        assert!(blank.to_string().contains("blank") || blank.to_string().contains("name"));
        let unknown = source_rename(&cd, 999, "Missing").unwrap_err();
        assert!(
            unknown.to_string().contains("not found") || unknown.to_string().contains("unknown")
        );
    }

    #[test]
    fn workshop_source_has_specialized_defaults_and_rejects_recursion_changes() {
        let (tmp, cd) = storage();
        let root = tmp.path().join("steamapps/workshop/content/431960");
        std::fs::create_dir_all(&root).unwrap();

        let (source, _) = source_create(&cd, &root.to_string_lossy()).unwrap();

        assert_eq!(source.kind, SourceKind::WallpaperEngineWorkshop);
        assert_eq!(source.display_name, "Wallpaper Engine");
        assert!(!source.recursive);
        let err = source_set_recursive(&cd, source.id, true).unwrap_err();
        assert!(err.to_string().contains("Workshop") || err.to_string().contains("recursive"));
        assert!(!source_get(&cd, source.id).unwrap().recursive);
    }

    #[test]
    fn availability_is_typed_and_unknown_ids_are_rejected() {
        let (tmp, cd) = storage();
        let path = tmp.path().join("walls");
        std::fs::create_dir(&path).unwrap();
        let (source, _) = source_create(&cd, &path.to_string_lossy()).unwrap();

        let offline = source_set_availability(&cd, source.id, SourceAvailability::Offline).unwrap();

        assert_eq!(offline.availability, SourceAvailability::Offline);
        assert!(source_set_availability(&cd, 999, SourceAvailability::Available).is_err());
        assert!(source_remove_by_id(&cd, 999).is_err());
    }

    #[test]
    fn removing_source_only_removes_membership_and_preserves_user_and_runtime_state() {
        let (tmp, cd) = storage();
        let path = tmp.path().join("walls");
        std::fs::create_dir(&path).unwrap();
        let wallpaper = path.join("a.jpg").to_string_lossy().to_string();
        let (source, _) = source_create(&cd, &path.to_string_lossy()).unwrap();
        let conn = Connection::open(cd.db_path()).unwrap();
        conn.execute("PRAGMA foreign_keys = ON", []).unwrap();
        conn.execute(
            "INSERT INTO wallpapers (path, type, ext, backend) VALUES (?1, 'image', 'jpg', 'awww')",
            params![wallpaper],
        )
        .unwrap();
        let wallpaper_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO wallpaper_sources (wallpaper_id, source_id) VALUES (?1, ?2)",
            params![wallpaper_id, source.id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO favorites (path) VALUES (?1)",
            params![wallpaper],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO state (key, value) VALUES ('current', ?1)",
            params![wallpaper],
        )
        .unwrap();
        drop(conn);

        let removed = source_remove_by_id(&cd, source.id).unwrap();

        assert_eq!(removed.id, source.id);
        let conn = Connection::open(cd.db_path()).unwrap();
        let counts: (i64, i64, i64, i64) = conn
            .query_row(
                "SELECT
                    (SELECT COUNT(*) FROM sources),
                    (SELECT COUNT(*) FROM wallpaper_sources),
                    (SELECT COUNT(*) FROM wallpapers),
                    (SELECT COUNT(*) FROM favorites)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(counts, (0, 0, 1, 1));
        let current: String = conn
            .query_row("SELECT value FROM state WHERE key = 'current'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(current, wallpaper);
    }

    #[test]
    fn source_removal_failure_rolls_back_source_and_membership() {
        let (tmp, cd) = storage();
        let path = tmp.path().join("walls");
        std::fs::create_dir(&path).unwrap();
        let (source, _) = source_create(&cd, &path.to_string_lossy()).unwrap();
        let conn = Connection::open(cd.db_path()).unwrap();
        conn.execute("PRAGMA foreign_keys = ON", []).unwrap();
        conn.execute(
            "INSERT INTO wallpapers (path, type, ext, backend)
             VALUES (?1, 'image', 'jpg', 'awww')",
            params![path.join("a.jpg").to_string_lossy()],
        )
        .unwrap();
        let wallpaper_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO wallpaper_sources (wallpaper_id, source_id) VALUES (?1, ?2)",
            params![wallpaper_id, source.id],
        )
        .unwrap();
        drop(conn);

        let result = source_remove_by_id_with_seam(&cd, source.id, |_| {
            Err(WcError::Other("injected before commit".into()))
        });

        assert!(result.unwrap_err().to_string().contains("injected"));
        assert_eq!(source_get(&cd, source.id).unwrap(), source);
        let conn = Connection::open(cd.db_path()).unwrap();
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM wallpaper_sources", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
            1
        );
    }

    #[test]
    fn removing_one_overlapping_source_preserves_the_other_membership() {
        let (tmp, cd) = storage();
        let parent = tmp.path().join("walls");
        let child = parent.join("nested");
        std::fs::create_dir_all(&child).unwrap();
        let (parent_source, _) = source_create(&cd, &parent.to_string_lossy()).unwrap();
        let (child_source, _) = source_create(&cd, &child.to_string_lossy()).unwrap();
        let conn = Connection::open(cd.db_path()).unwrap();
        conn.execute("PRAGMA foreign_keys = ON", []).unwrap();
        conn.execute(
            "INSERT INTO wallpapers (path, type, ext, backend)
             VALUES (?1, 'image', 'jpg', 'awww')",
            params![child.join("shared.jpg").to_string_lossy()],
        )
        .unwrap();
        let wallpaper_id = conn.last_insert_rowid();
        for source_id in [parent_source.id, child_source.id] {
            conn.execute(
                "INSERT INTO wallpaper_sources (wallpaper_id, source_id) VALUES (?1, ?2)",
                params![wallpaper_id, source_id],
            )
            .unwrap();
        }
        drop(conn);

        source_remove_by_id(&cd, parent_source.id).unwrap();

        let conn = Connection::open(cd.db_path()).unwrap();
        let remaining_source: i64 = conn
            .query_row("SELECT source_id FROM wallpaper_sources", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(remaining_source, child_source.id);
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM wallpapers", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }

    #[test]
    fn removing_source_clears_legacy_source_id_without_deleting_wallpaper() {
        let (tmp, cd) = storage();
        let path = tmp.path().join("walls");
        std::fs::create_dir(&path).unwrap();
        let (source, _) = source_create(&cd, &path.to_string_lossy()).unwrap();
        let conn = Connection::open(cd.db_path()).unwrap();
        conn.execute("PRAGMA foreign_keys = ON", []).unwrap();
        conn.execute(
            "INSERT INTO wallpapers (path, type, ext, backend, source_id)
             VALUES (?1, 'image', 'jpg', 'awww', ?2)",
            params![path.join("legacy.jpg").to_string_lossy(), source.id],
        )
        .unwrap();
        drop(conn);

        source_remove_by_id(&cd, source.id).unwrap();

        let conn = Connection::open(cd.db_path()).unwrap();
        let legacy_source_id: Option<i64> = conn
            .query_row("SELECT source_id FROM wallpapers", [], |row| row.get(0))
            .unwrap();
        assert_eq!(legacy_source_id, None);
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM wallpapers", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }

    #[cfg(unix)]
    #[test]
    fn create_reconciles_an_offline_alias_to_lowest_id_and_preserves_membership_and_settings() {
        let (tmp, cd) = storage();
        let link = tmp.path().join("offline-link");
        let real = tmp.path().join("real-walls");
        let (offline, created) = source_create(&cd, &link.to_string_lossy()).unwrap();
        assert!(created);
        source_rename(&cd, offline.id, "My offline collection").unwrap();
        source_set_recursive(&cd, offline.id, false).unwrap();
        source_set_availability(&cd, offline.id, SourceAvailability::Offline).unwrap();
        let conn = Connection::open(cd.db_path()).unwrap();
        conn.execute(
            "INSERT INTO wallpapers (path, type, ext, backend)
             VALUES ('/remembered/a.jpg', 'image', 'jpg', 'awww')",
            [],
        )
        .unwrap();
        let wallpaper_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO wallpaper_sources (wallpaper_id, source_id) VALUES (?1, ?2)",
            params![wallpaper_id, offline.id],
        )
        .unwrap();
        drop(conn);
        std::fs::create_dir(&real).unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let (reconciled, created) = source_create(&cd, &real.to_string_lossy()).unwrap();

        assert!(!created);
        assert_eq!(reconciled.id, offline.id);
        assert_eq!(reconciled.path, real.to_string_lossy());
        assert_eq!(reconciled.display_name, "My offline collection");
        assert!(!reconciled.recursive);
        assert_eq!(reconciled.availability, SourceAvailability::Offline);
        assert_eq!(reconciled.added_at, offline.added_at);
        assert_eq!(sources_list_typed(&cd).unwrap(), vec![reconciled]);
        let conn = Connection::open(cd.db_path()).unwrap();
        let membership: (i64, i64) = conn
            .query_row(
                "SELECT wallpaper_id, source_id FROM wallpaper_sources",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(membership, (wallpaper_id, offline.id));
    }
}
