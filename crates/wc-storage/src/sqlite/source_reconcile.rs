use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use wc_core::config::ConfigDir;
use wc_core::error::WcError;
use wc_scan::{CompleteSourceScan, ScanSourceKind};

use super::schema::{open_runtime_connection, try_ensure_sqlite_db};

/// Changes made while publishing one authoritative source snapshot.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceReconcileReport {
    /// Unique entries published by the complete snapshot.
    pub indexed: usize,
    /// New physical wallpaper rows created by this reconcile.
    pub wallpapers_added: usize,
    /// Confirmed-missing orphan wallpaper rows physically removed.
    pub wallpapers_removed: usize,
    /// New `(wallpaper, source)` relationships created.
    pub memberships_added: usize,
    /// Relationships absent from this source's complete snapshot and removed.
    pub memberships_removed: usize,
    /// Favorites removed because their orphan paths were confirmed missing.
    pub favorites_removed: usize,
    /// Workshop IDs belonging to confirmed-missing orphan rows.
    pub removed_we_workshop_ids: Vec<String>,
}

/// Publish a complete scan for exactly one configured source.
///
/// Incomplete, offline, and cancelled scan outcomes cannot be passed to this
/// function: only `CompleteSourceScan` exposes an authoritative entry set.
pub fn reconcile_complete_source(
    cd: &ConfigDir,
    source_id: i64,
    snapshot: &CompleteSourceScan,
) -> Result<SourceReconcileReport, WcError> {
    reconcile_complete_source_with_presence(cd, source_id, snapshot, filesystem_path_presence)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PathPresence {
    Present,
    Missing,
    Unknown,
}

fn filesystem_path_presence(path: &Path) -> PathPresence {
    match path.try_exists() {
        Ok(true) => PathPresence::Present,
        Ok(false) => PathPresence::Missing,
        Err(_) => PathPresence::Unknown,
    }
}

fn enumeration_root_identity(path: &Path) -> std::path::PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn reconcile_complete_source_with_presence<P>(
    cd: &ConfigDir,
    source_id: i64,
    snapshot: &CompleteSourceScan,
    path_presence: P,
) -> Result<SourceReconcileReport, WcError>
where
    P: Fn(&Path) -> PathPresence,
{
    reconcile_complete_source_with_seams(cd, source_id, snapshot, path_presence, |_| Ok(()))
}

fn reconcile_complete_source_with_seams<P, F>(
    cd: &ConfigDir,
    source_id: i64,
    snapshot: &CompleteSourceScan,
    path_presence: P,
    before_commit: F,
) -> Result<SourceReconcileReport, WcError>
where
    P: Fn(&Path) -> PathPresence,
    F: FnOnce(&Connection) -> Result<(), WcError>,
{
    try_ensure_sqlite_db(cd)?;
    let mut conn = open_runtime_connection(cd)?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| WcError::Sqlite(error.to_string()))?;

    let source_config = tx
        .query_row(
            "SELECT path, kind, recursive FROM sources WHERE id = ?1",
            params![source_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()
        .map_err(|error| WcError::Sqlite(error.to_string()))?;
    let Some((source_path, source_kind, source_recursive)) = source_config else {
        return Err(WcError::Other(format!("source id {source_id} not found")));
    };

    let request = snapshot.request();
    let request_kind = match request.kind {
        ScanSourceKind::Directory => "directory",
        ScanSourceKind::WallpaperEngineWorkshop => "wallpaper_engine_workshop",
    };
    let configured_root = enumeration_root_identity(Path::new(&source_path));
    let snapshot_root = enumeration_root_identity(&request.path);
    let request_recursive = i64::from(request.recursive);
    if configured_root != snapshot_root
        || source_kind != request_kind
        || source_recursive != request_recursive
    {
        return Err(WcError::Other(format!(
            "source snapshot no longer matches configured source {source_id}"
        )));
    }

    tx.execute_batch(
        "CREATE TEMP TABLE wc_source_scan_stage (
             path               TEXT PRIMARY KEY,
             type               TEXT NOT NULL,
             ext                TEXT NOT NULL,
             backend            TEXT NOT NULL,
             size               INTEGER NOT NULL,
             mtime              INTEGER NOT NULL,
             resolution         TEXT NOT NULL,
             project_type       TEXT NOT NULL,
             preview_path       TEXT NOT NULL,
             workshop_id        TEXT NOT NULL,
             title              TEXT NOT NULL,
             we_file            TEXT NOT NULL,
             unsupported_reason TEXT NOT NULL
         );",
    )
    .map_err(|error| WcError::Sqlite(error.to_string()))?;

    for entry in snapshot.entries() {
        let project = entry.project.as_ref();
        tx.execute(
            "INSERT INTO wc_source_scan_stage
             (path, type, ext, backend, size, mtime, resolution, project_type,
              preview_path, workshop_id, title, we_file, unsupported_reason)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
             ON CONFLICT(path) DO UPDATE SET
                 type = excluded.type,
                 ext = excluded.ext,
                 backend = excluded.backend,
                 size = excluded.size,
                 mtime = excluded.mtime,
                 resolution = excluded.resolution,
                 project_type = excluded.project_type,
                 preview_path = excluded.preview_path,
                 workshop_id = excluded.workshop_id,
                 title = excluded.title,
                 we_file = excluded.we_file,
                 unsupported_reason = excluded.unsupported_reason",
            params![
                entry.path.as_str(),
                entry.file_type.as_str(),
                entry.ext.as_str(),
                entry.backend.as_str(),
                i64::try_from(entry.size).unwrap_or(i64::MAX),
                i64::try_from(entry.mtime).unwrap_or(i64::MAX),
                entry.resolution.as_str(),
                project
                    .map(|value| value.project_type.as_str())
                    .unwrap_or(""),
                project
                    .and_then(|value| value.preview_path.as_deref())
                    .unwrap_or(""),
                project
                    .and_then(|value| value.workshop_id.as_deref())
                    .unwrap_or(""),
                project
                    .and_then(|value| value.title.as_deref())
                    .unwrap_or(""),
                project
                    .and_then(|value| value.we_file.as_deref())
                    .unwrap_or(""),
                project
                    .and_then(|value| value.unsupported_reason.as_deref())
                    .unwrap_or(""),
            ],
        )
        .map_err(|error| WcError::Sqlite(error.to_string()))?;
    }

    let indexed = tx
        .query_row("SELECT COUNT(*) FROM wc_source_scan_stage", [], |row| {
            row.get::<_, i64>(0)
        })
        .map_err(|error| WcError::Sqlite(error.to_string()))?;
    let wallpapers_added = tx
        .query_row(
            "SELECT COUNT(*)
             FROM wc_source_scan_stage staged
             LEFT JOIN wallpapers wallpaper ON wallpaper.path = staged.path
             WHERE wallpaper.id IS NULL",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|error| WcError::Sqlite(error.to_string()))?;
    let memberships_added = tx
        .query_row(
            "SELECT COUNT(*)
             FROM wc_source_scan_stage staged
             LEFT JOIN wallpapers wallpaper ON wallpaper.path = staged.path
             LEFT JOIN wallpaper_sources membership
               ON membership.wallpaper_id = wallpaper.id
              AND membership.source_id = ?1
             WHERE membership.wallpaper_id IS NULL",
            params![source_id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|error| WcError::Sqlite(error.to_string()))?;

    tx.execute(
        "INSERT INTO wallpapers
         (path, type, ext, backend, size, mtime, resolution, project_type,
          preview_path, workshop_id, title, we_file, unsupported_reason, last_seen)
         SELECT path, type, ext, backend, size, mtime, resolution, project_type,
                preview_path, workshop_id, title, we_file, unsupported_reason, datetime('now')
         FROM wc_source_scan_stage
         WHERE true
         ON CONFLICT(path) DO UPDATE SET
             type = excluded.type,
             ext = excluded.ext,
             backend = excluded.backend,
             size = excluded.size,
             mtime = excluded.mtime,
             resolution = excluded.resolution,
             project_type = excluded.project_type,
             preview_path = excluded.preview_path,
             workshop_id = excluded.workshop_id,
             title = excluded.title,
             we_file = excluded.we_file,
             unsupported_reason = excluded.unsupported_reason,
             last_seen = excluded.last_seen",
        [],
    )
    .map_err(|error| WcError::Sqlite(error.to_string()))?;
    tx.execute(
        "INSERT INTO wallpaper_sources (wallpaper_id, source_id, last_seen_at)
         SELECT wallpaper.id, ?1, datetime('now')
         FROM wc_source_scan_stage staged
         JOIN wallpapers wallpaper ON wallpaper.path = staged.path
         WHERE true
         ON CONFLICT(wallpaper_id, source_id) DO UPDATE SET
             last_seen_at = excluded.last_seen_at",
        params![source_id],
    )
    .map_err(|error| WcError::Sqlite(error.to_string()))?;

    let removed_memberships = {
        let mut statement = tx
            .prepare(
                "SELECT wallpaper.id, wallpaper.path, wallpaper.workshop_id
                 FROM wallpaper_sources membership
                 JOIN wallpapers wallpaper ON wallpaper.id = membership.wallpaper_id
                 WHERE membership.source_id = ?1
                   AND NOT EXISTS (
                       SELECT 1 FROM wc_source_scan_stage staged
                       WHERE staged.path = wallpaper.path
                   )
                 ORDER BY wallpaper.id",
            )
            .map_err(|error| WcError::Sqlite(error.to_string()))?;
        let rows = statement
            .query_map(params![source_id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(|error| WcError::Sqlite(error.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| WcError::Sqlite(error.to_string()))?;
        rows
    };
    tx.execute(
        "DELETE FROM wallpaper_sources
         WHERE source_id = ?1
           AND NOT EXISTS (
               SELECT 1
               FROM wallpapers wallpaper
               JOIN wc_source_scan_stage staged ON staged.path = wallpaper.path
               WHERE wallpaper.id = wallpaper_sources.wallpaper_id
           )",
        params![source_id],
    )
    .map_err(|error| WcError::Sqlite(error.to_string()))?;

    let mut report = SourceReconcileReport {
        indexed: indexed.max(0) as usize,
        wallpapers_added: wallpapers_added.max(0) as usize,
        memberships_added: memberships_added.max(0) as usize,
        memberships_removed: removed_memberships.len(),
        ..SourceReconcileReport::default()
    };
    for (wallpaper_id, path, workshop_id) in removed_memberships {
        let remaining_memberships = tx
            .query_row(
                "SELECT COUNT(*) FROM wallpaper_sources WHERE wallpaper_id = ?1",
                params![wallpaper_id],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|error| WcError::Sqlite(error.to_string()))?;
        if remaining_memberships != 0 || path_presence(Path::new(&path)) != PathPresence::Missing {
            continue;
        }
        report.favorites_removed += tx
            .execute("DELETE FROM favorites WHERE path = ?1", params![path])
            .map_err(|error| WcError::Sqlite(error.to_string()))?;
        report.wallpapers_removed += tx
            .execute(
                "DELETE FROM wallpapers WHERE id = ?1",
                params![wallpaper_id],
            )
            .map_err(|error| WcError::Sqlite(error.to_string()))?;
        if !workshop_id.is_empty() {
            report.removed_we_workshop_ids.push(workshop_id);
        }
    }
    report.removed_we_workshop_ids.sort();
    report.removed_we_workshop_ids.dedup();

    tx.execute(
        "UPDATE sources SET availability = 'available' WHERE id = ?1",
        params![source_id],
    )
    .map_err(|error| WcError::Sqlite(error.to_string()))?;
    before_commit(&tx)?;
    tx.commit()
        .map_err(|error| WcError::Sqlite(error.to_string()))?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use rusqlite::Connection;
    use wc_core::config::ConfigDir;
    use wc_scan::{
        scan_source, CompleteSourceScan, ScanControl, ScanSourceKind, SourceScanOutcome,
        SourceScanRequest,
    };

    use super::*;
    use crate::sqlite::{
        source_create, source_get, source_set_availability, source_set_recursive,
        SourceAvailability,
    };

    fn storage() -> (tempfile::TempDir, ConfigDir) {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        crate::sqlite::try_ensure_sqlite_db(&cd).unwrap();
        (tmp, cd)
    }

    fn complete_scan(path: &Path, recursive: bool) -> CompleteSourceScan {
        let outcome = scan_source(
            &SourceScanRequest {
                path: path.to_path_buf(),
                kind: ScanSourceKind::Directory,
                recursive,
            },
            |_| ScanControl::Continue,
        );
        let SourceScanOutcome::Complete(snapshot) = outcome else {
            panic!("fixture scan must be complete");
        };
        snapshot
    }

    fn complete_workshop_scan(path: &Path) -> CompleteSourceScan {
        let outcome = scan_source(
            &SourceScanRequest {
                path: path.to_path_buf(),
                kind: ScanSourceKind::WallpaperEngineWorkshop,
                recursive: false,
            },
            |_| ScanControl::Continue,
        );
        let SourceScanOutcome::Complete(snapshot) = outcome else {
            panic!("workshop fixture scan must be complete");
        };
        snapshot
    }

    fn create_scene_project(root: &Path, workshop_id: &str) -> std::path::PathBuf {
        let project = root.join(workshop_id);
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("scene.json"), b"{}").unwrap();
        std::fs::write(project.join("preview.jpg"), b"preview fixture").unwrap();
        std::fs::write(
            project.join("project.json"),
            r#"{
                "type": "scene",
                "file": "scene.json",
                "preview": "preview.jpg",
                "title": "Fresh scene"
            }"#,
        )
        .unwrap();
        project
    }

    #[test]
    fn overlapping_sources_share_one_wallpaper_with_independent_memberships() {
        let (tmp, cd) = storage();
        let parent = tmp.path().join("walls");
        let child = parent.join("nested");
        std::fs::create_dir_all(&child).unwrap();
        std::fs::write(child.join("shared.jpg"), b"image fixture").unwrap();
        let (parent_source, _) = source_create(&cd, &parent.to_string_lossy()).unwrap();
        let (child_source, _) = source_create(&cd, &child.to_string_lossy()).unwrap();

        let parent_report =
            reconcile_complete_source(&cd, parent_source.id, &complete_scan(&parent, true))
                .unwrap();
        let child_report =
            reconcile_complete_source(&cd, child_source.id, &complete_scan(&child, true)).unwrap();

        assert_eq!(parent_report.wallpapers_added, 1);
        assert_eq!(parent_report.memberships_added, 1);
        assert_eq!(child_report.wallpapers_added, 0);
        assert_eq!(child_report.memberships_added, 1);
        let conn = Connection::open(cd.db_path()).unwrap();
        let counts: (i64, i64) = conn
            .query_row(
                "SELECT
                    (SELECT COUNT(*) FROM wallpapers),
                    (SELECT COUNT(*) FROM wallpaper_sources)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(counts, (1, 2));
    }

    #[test]
    fn refreshing_one_source_does_not_remove_another_sources_membership() {
        let (tmp, cd) = storage();
        let parent = tmp.path().join("walls");
        let child = parent.join("nested");
        std::fs::create_dir_all(&child).unwrap();
        let wallpaper = child.join("shared.jpg");
        std::fs::write(&wallpaper, b"image fixture").unwrap();
        let (parent_source, _) = source_create(&cd, &parent.to_string_lossy()).unwrap();
        let (child_source, _) = source_create(&cd, &child.to_string_lossy()).unwrap();
        reconcile_complete_source(&cd, parent_source.id, &complete_scan(&parent, true)).unwrap();
        reconcile_complete_source(&cd, child_source.id, &complete_scan(&child, true)).unwrap();
        std::fs::rename(&wallpaper, tmp.path().join("moved.jpg")).unwrap();

        let report =
            reconcile_complete_source(&cd, parent_source.id, &complete_scan(&parent, true))
                .unwrap();

        assert_eq!(report.memberships_removed, 1);
        assert_eq!(report.wallpapers_removed, 0);
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
    fn complete_empty_snapshot_removes_missing_orphan() {
        let (tmp, cd) = storage();
        let source_path = tmp.path().join("walls");
        std::fs::create_dir(&source_path).unwrap();
        let wallpaper = source_path.join("gone.jpg");
        std::fs::write(&wallpaper, b"image fixture").unwrap();
        let (source, _) = source_create(&cd, &source_path.to_string_lossy()).unwrap();
        reconcile_complete_source(&cd, source.id, &complete_scan(&source_path, true)).unwrap();
        std::fs::rename(&wallpaper, tmp.path().join("moved.jpg")).unwrap();

        let report =
            reconcile_complete_source(&cd, source.id, &complete_scan(&source_path, true)).unwrap();

        assert_eq!(report.indexed, 0);
        assert_eq!(report.memberships_removed, 1);
        assert_eq!(report.wallpapers_removed, 1);
        let conn = Connection::open(cd.db_path()).unwrap();
        let counts: (i64, i64) = conn
            .query_row(
                "SELECT
                    (SELECT COUNT(*) FROM wallpapers),
                    (SELECT COUNT(*) FROM wallpaper_sources)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(counts, (0, 0));
    }

    #[test]
    fn turning_recursion_off_preserves_existing_orphan_and_favorite() {
        let (tmp, cd) = storage();
        let source_path = tmp.path().join("walls");
        let nested = source_path.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        let wallpaper = nested.join("kept.jpg");
        std::fs::write(&wallpaper, b"image fixture").unwrap();
        let (source, _) = source_create(&cd, &source_path.to_string_lossy()).unwrap();
        reconcile_complete_source(&cd, source.id, &complete_scan(&source_path, true)).unwrap();
        let conn = Connection::open(cd.db_path()).unwrap();
        conn.execute(
            "INSERT INTO favorites (path) VALUES (?1)",
            params![wallpaper.to_string_lossy()],
        )
        .unwrap();
        drop(conn);
        source_set_recursive(&cd, source.id, false).unwrap();

        let report =
            reconcile_complete_source(&cd, source.id, &complete_scan(&source_path, false)).unwrap();

        assert_eq!(report.memberships_removed, 1);
        assert_eq!(report.wallpapers_removed, 0);
        assert_eq!(report.favorites_removed, 0);
        let conn = Connection::open(cd.db_path()).unwrap();
        let counts: (i64, i64, i64) = conn
            .query_row(
                "SELECT
                    (SELECT COUNT(*) FROM wallpapers),
                    (SELECT COUNT(*) FROM wallpaper_sources),
                    (SELECT COUNT(*) FROM favorites)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(counts, (1, 0, 1));
    }

    #[test]
    fn unknown_path_presence_preserves_orphan_and_favorite() {
        let (tmp, cd) = storage();
        let source_path = tmp.path().join("walls");
        let nested = source_path.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        let wallpaper = nested.join("unknown.jpg");
        std::fs::write(&wallpaper, b"image fixture").unwrap();
        let (source, _) = source_create(&cd, &source_path.to_string_lossy()).unwrap();
        reconcile_complete_source(&cd, source.id, &complete_scan(&source_path, true)).unwrap();
        let conn = Connection::open(cd.db_path()).unwrap();
        conn.execute(
            "INSERT INTO favorites (path) VALUES (?1)",
            params![wallpaper.to_string_lossy()],
        )
        .unwrap();
        drop(conn);
        source_set_recursive(&cd, source.id, false).unwrap();

        let report = reconcile_complete_source_with_presence(
            &cd,
            source.id,
            &complete_scan(&source_path, false),
            |_| PathPresence::Unknown,
        )
        .unwrap();

        assert_eq!(report.memberships_removed, 1);
        assert_eq!(report.wallpapers_removed, 0);
        assert_eq!(report.favorites_removed, 0);
        let conn = Connection::open(cd.db_path()).unwrap();
        let counts: (i64, i64) = conn
            .query_row(
                "SELECT
                    (SELECT COUNT(*) FROM wallpapers),
                    (SELECT COUNT(*) FROM favorites)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(counts, (1, 1));
    }

    #[test]
    fn missing_orphan_removes_favorite_but_preserves_runtime_state() {
        let (tmp, cd) = storage();
        let source_path = tmp.path().join("walls");
        std::fs::create_dir(&source_path).unwrap();
        let wallpaper = source_path.join("gone.jpg");
        let wallpaper_text = wallpaper.to_string_lossy().to_string();
        std::fs::write(&wallpaper, b"image fixture").unwrap();
        let (source, _) = source_create(&cd, &source_path.to_string_lossy()).unwrap();
        reconcile_complete_source(&cd, source.id, &complete_scan(&source_path, true)).unwrap();
        let conn = Connection::open(cd.db_path()).unwrap();
        conn.execute(
            "INSERT INTO favorites (path) VALUES (?1)",
            params![wallpaper_text],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO state (key, value) VALUES ('current', ?1)",
            params![wallpaper_text],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO display_state (target_key, wallpaper_path, backend)
             VALUES ('all', ?1, 'awww')",
            params![wallpaper_text],
        )
        .unwrap();
        drop(conn);
        std::fs::rename(&wallpaper, tmp.path().join("moved.jpg")).unwrap();

        let report =
            reconcile_complete_source(&cd, source.id, &complete_scan(&source_path, true)).unwrap();

        assert_eq!(report.favorites_removed, 1);
        assert_eq!(report.wallpapers_removed, 1);
        let conn = Connection::open(cd.db_path()).unwrap();
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM favorites", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        let current: String = conn
            .query_row("SELECT value FROM state WHERE key = 'current'", [], |row| {
                row.get(0)
            })
            .unwrap();
        let display_path: String = conn
            .query_row(
                "SELECT wallpaper_path FROM display_state WHERE target_key = 'all'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(current, wallpaper_text);
        assert_eq!(display_path, wallpaper_text);
    }

    #[test]
    fn metadata_refresh_preserves_wallpaper_id_and_added_at() {
        let (tmp, cd) = storage();
        let workshop_root = tmp.path().join("steamapps/workshop/content/431960");
        std::fs::create_dir_all(&workshop_root).unwrap();
        let project = create_scene_project(&workshop_root, "123456");
        let (source, _) = source_create(&cd, &workshop_root.to_string_lossy()).unwrap();
        let snapshot = complete_workshop_scan(&workshop_root);
        reconcile_complete_source(&cd, source.id, &snapshot).unwrap();
        let conn = Connection::open(cd.db_path()).unwrap();
        let wallpaper_id: i64 = conn
            .query_row("SELECT id FROM wallpapers", [], |row| row.get(0))
            .unwrap();
        conn.execute(
            "UPDATE wallpapers SET
                type = 'image', ext = 'old', backend = 'awww', size = 1, mtime = 2,
                resolution = 'old', project_type = 'old', preview_path = 'old',
                workshop_id = 'old', title = 'old', we_file = 'old',
                unsupported_reason = 'old', added_at = 'stable-added-at'",
            [],
        )
        .unwrap();
        drop(conn);

        let report = reconcile_complete_source(&cd, source.id, &snapshot).unwrap();

        assert_eq!(report.wallpapers_added, 0);
        assert_eq!(report.memberships_added, 0);
        let conn = Connection::open(cd.db_path()).unwrap();
        let row: (
            i64,
            String,
            String,
            String,
            String,
            String,
            String,
            String,
            String,
        ) = conn
            .query_row(
                "SELECT id, added_at, type, backend, project_type, preview_path,
                        workshop_id, title, we_file
                 FROM wallpapers",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(row.0, wallpaper_id);
        assert_eq!(row.1, "stable-added-at");
        assert_eq!(row.2, "we_scene");
        assert_eq!(row.3, "linux-wallpaperengine");
        assert_eq!(row.4, "we_scene");
        assert_eq!(row.5, project.join("preview.jpg").to_string_lossy());
        assert_eq!(row.6, "123456");
        assert_eq!(row.7, "Fresh scene");
        assert_eq!(row.8, "scene.json");
        assert_eq!(
            source_get(&cd, source.id).unwrap().availability,
            SourceAvailability::Available
        );
    }

    #[test]
    fn removed_wallpaper_engine_orphan_reports_workshop_id() {
        let (tmp, cd) = storage();
        let workshop_root = tmp.path().join("steamapps/workshop/content/431960");
        std::fs::create_dir_all(&workshop_root).unwrap();
        let project = create_scene_project(&workshop_root, "654321");
        let (source, _) = source_create(&cd, &workshop_root.to_string_lossy()).unwrap();
        reconcile_complete_source(&cd, source.id, &complete_workshop_scan(&workshop_root)).unwrap();
        std::fs::rename(&project, tmp.path().join("moved-project")).unwrap();

        let report =
            reconcile_complete_source(&cd, source.id, &complete_workshop_scan(&workshop_root))
                .unwrap();

        assert_eq!(report.wallpapers_removed, 1);
        assert_eq!(report.removed_we_workshop_ids, vec!["654321"]);
    }

    #[test]
    fn injected_failure_rolls_back_entries_memberships_favorites_and_availability() {
        let (tmp, cd) = storage();
        let source_path = tmp.path().join("walls");
        std::fs::create_dir(&source_path).unwrap();
        let wallpaper = source_path.join("old.jpg");
        std::fs::write(&wallpaper, b"image fixture").unwrap();
        let (source, _) = source_create(&cd, &source_path.to_string_lossy()).unwrap();
        reconcile_complete_source(&cd, source.id, &complete_scan(&source_path, true)).unwrap();
        source_set_availability(&cd, source.id, SourceAvailability::Offline).unwrap();
        let conn = Connection::open(cd.db_path()).unwrap();
        conn.execute(
            "INSERT INTO favorites (path) VALUES (?1)",
            params![wallpaper.to_string_lossy()],
        )
        .unwrap();
        drop(conn);
        std::fs::rename(&wallpaper, tmp.path().join("moved.jpg")).unwrap();

        let result = reconcile_complete_source_with_seams(
            &cd,
            source.id,
            &complete_scan(&source_path, true),
            |_| PathPresence::Missing,
            |_| Err(WcError::Other("injected before commit".into())),
        );

        assert!(result.unwrap_err().to_string().contains("injected"));
        let conn = Connection::open(cd.db_path()).unwrap();
        let counts: (i64, i64, i64) = conn
            .query_row(
                "SELECT
                    (SELECT COUNT(*) FROM wallpapers),
                    (SELECT COUNT(*) FROM wallpaper_sources),
                    (SELECT COUNT(*) FROM favorites)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(counts, (1, 1, 1));
        assert_eq!(
            source_get(&cd, source.id).unwrap().availability,
            SourceAvailability::Offline
        );
    }

    #[test]
    fn public_reconcile_signature_accepts_only_complete_snapshots() {
        let _typed_api: fn(
            &ConfigDir,
            i64,
            &CompleteSourceScan,
        ) -> Result<SourceReconcileReport, WcError> = reconcile_complete_source;

        let tmp = tempfile::tempdir().unwrap();
        let outcome = scan_source(
            &SourceScanRequest {
                path: tmp.path().join("offline"),
                kind: ScanSourceKind::Directory,
                recursive: true,
            },
            |_| ScanControl::Continue,
        );
        assert!(matches!(outcome, SourceScanOutcome::Offline(_)));

        let cancelled = scan_source(
            &SourceScanRequest {
                path: tmp.path().to_path_buf(),
                kind: ScanSourceKind::Directory,
                recursive: true,
            },
            |_| ScanControl::Cancel,
        );
        assert!(matches!(cancelled, SourceScanOutcome::Cancelled(_)));
    }

    #[test]
    fn snapshot_from_another_source_is_rejected_without_mutation() {
        let (tmp, cd) = storage();
        let source_a_path = tmp.path().join("source-a");
        let source_b_path = tmp.path().join("source-b");
        std::fs::create_dir_all(&source_a_path).unwrap();
        std::fs::create_dir_all(&source_b_path).unwrap();
        std::fs::write(source_a_path.join("a.jpg"), b"a").unwrap();
        std::fs::write(source_b_path.join("b.jpg"), b"b").unwrap();
        let (source_a, _) = source_create(&cd, &source_a_path.to_string_lossy()).unwrap();
        let (source_b, _) = source_create(&cd, &source_b_path.to_string_lossy()).unwrap();
        reconcile_complete_source(
            &cd,
            source_b.id,
            &complete_scan(&source_b_path, source_b.recursive),
        )
        .unwrap();
        source_set_availability(&cd, source_b.id, SourceAvailability::Offline).unwrap();

        let error = reconcile_complete_source(
            &cd,
            source_b.id,
            &complete_scan(&source_a_path, source_a.recursive),
        )
        .unwrap_err();

        assert!(error.to_string().contains("snapshot"));
        assert_eq!(
            source_get(&cd, source_b.id).unwrap().availability,
            SourceAvailability::Offline
        );
        let conn = Connection::open(cd.db_path()).unwrap();
        let membership_paths = conn
            .prepare(
                "SELECT wallpaper.path
                 FROM wallpaper_sources membership
                 JOIN wallpapers wallpaper ON wallpaper.id = membership.wallpaper_id
                 WHERE membership.source_id = ?1
                 ORDER BY wallpaper.path",
            )
            .unwrap()
            .query_map(params![source_b.id], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            membership_paths,
            vec![source_b_path.join("b.jpg").to_string_lossy().to_string()]
        );
    }

    #[test]
    fn stale_recursive_snapshot_is_rejected_without_mutation() {
        let (tmp, cd) = storage();
        let source_path = tmp.path().join("walls");
        let nested = source_path.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("nested.jpg"), b"nested").unwrap();
        let (source, _) = source_create(&cd, &source_path.to_string_lossy()).unwrap();
        let stale_snapshot = complete_scan(&source_path, true);
        source_set_recursive(&cd, source.id, false).unwrap();

        let error = reconcile_complete_source(&cd, source.id, &stale_snapshot).unwrap_err();

        assert!(error.to_string().contains("snapshot"));
        let conn = Connection::open(cd.db_path()).unwrap();
        let counts: (i64, i64) = conn
            .query_row(
                "SELECT
                    (SELECT COUNT(*) FROM wallpapers),
                    (SELECT COUNT(*) FROM wallpaper_sources)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(counts, (0, 0));
        assert_eq!(
            source_get(&cd, source.id).unwrap().availability,
            SourceAvailability::Unknown
        );
    }

    #[test]
    fn single_workshop_project_snapshot_cannot_replace_the_root_snapshot() {
        let (tmp, cd) = storage();
        let workshop_root = tmp.path().join("steamapps/workshop/content/431960");
        std::fs::create_dir_all(&workshop_root).unwrap();
        let first_project = create_scene_project(&workshop_root, "111111");
        let second_project = create_scene_project(&workshop_root, "222222");
        let (source, _) = source_create(&cd, &workshop_root.to_string_lossy()).unwrap();
        reconcile_complete_source(&cd, source.id, &complete_workshop_scan(&workshop_root)).unwrap();
        let conn = Connection::open(cd.db_path()).unwrap();
        conn.execute(
            "INSERT INTO favorites (path) VALUES (?1)",
            params![second_project.to_string_lossy()],
        )
        .unwrap();
        drop(conn);
        source_set_availability(&cd, source.id, SourceAvailability::Offline).unwrap();

        let error =
            reconcile_complete_source(&cd, source.id, &complete_workshop_scan(&first_project))
                .unwrap_err();

        assert!(error.to_string().contains("snapshot"));
        assert_eq!(
            source_get(&cd, source.id).unwrap().availability,
            SourceAvailability::Offline
        );
        let conn = Connection::open(cd.db_path()).unwrap();
        let counts: (i64, i64) = conn
            .query_row(
                "SELECT
                    (SELECT COUNT(*) FROM wallpaper_sources WHERE source_id = ?1),
                    (SELECT COUNT(*) FROM favorites)",
                params![source.id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(counts, (2, 1));
    }
}
