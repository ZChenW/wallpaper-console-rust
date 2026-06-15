use rusqlite::{params, Connection};
use wc_core::config::ConfigDir;
use wc_core::error::WcError;
use wc_core::types::WallpaperEntry;

use super::schema::ensure_sqlite_db;

pub struct LibraryReplaceSession {
    conn: Connection,
    #[allow(dead_code)]
    batch_size: usize,
    inserted: usize,
}

pub fn library_replace_session_start(cd: &ConfigDir) -> Result<LibraryReplaceSession, WcError> {
    ensure_sqlite_db(cd);
    let conn = Connection::open(cd.db_path()).map_err(|e| WcError::Sqlite(e.to_string()))?;
    conn.execute(
        "CREATE TEMP TABLE IF NOT EXISTS wallpapers_stage AS SELECT * FROM wallpapers WHERE 0",
        [],
    )
    .map_err(|e| WcError::Sqlite(e.to_string()))?;
    conn.execute("DELETE FROM wallpapers_stage", [])
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    Ok(LibraryReplaceSession {
        conn,
        batch_size: 250,
        inserted: 0,
    })
}

pub fn library_replace_session_push(
    session: &mut LibraryReplaceSession,
    entries: &[WallpaperEntry],
) -> Result<(), WcError> {
    let tx = session
        .conn
        .unchecked_transaction()
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    for entry in entries {
        let project = entry.project.as_ref();
        tx.execute(
            "INSERT INTO wallpapers_stage
                 (path, type, ext, backend, size, mtime, resolution,
                  project_type, preview_path, workshop_id, title, we_file, unsupported_reason)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                entry.path.as_str(),
                entry.file_type.as_str(),
                entry.ext.as_str(),
                entry.backend.as_str(),
                entry.size as i64,
                entry.mtime as i64,
                entry.resolution.as_str(),
                project.map(|p| p.project_type.as_str()).unwrap_or(""),
                project
                    .and_then(|p| p.preview_path.as_deref())
                    .unwrap_or(""),
                project.and_then(|p| p.workshop_id.as_deref()).unwrap_or(""),
                project.and_then(|p| p.title.as_deref()).unwrap_or(""),
                project.and_then(|p| p.we_file.as_deref()).unwrap_or(""),
                project
                    .and_then(|p| p.unsupported_reason.as_deref())
                    .unwrap_or(""),
            ],
        )
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    }
    tx.commit().map_err(|e| WcError::Sqlite(e.to_string()))?;
    session.inserted += entries.len();
    Ok(())
}

pub fn library_replace_session_commit(session: LibraryReplaceSession) -> Result<usize, WcError> {
    let tx = session
        .conn
        .unchecked_transaction()
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    tx.execute("DELETE FROM wallpapers", [])
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    tx.execute(
        "INSERT INTO wallpapers
         (path, type, ext, backend, size, mtime, resolution,
          project_type, preview_path, workshop_id, title, we_file, unsupported_reason)
         SELECT path, type, ext, backend, size, mtime, resolution,
                project_type, preview_path, workshop_id, title, we_file, unsupported_reason
         FROM wallpapers_stage",
        [],
    )
    .map_err(|e| WcError::Sqlite(e.to_string()))?;
    tx.commit().map_err(|e| WcError::Sqlite(e.to_string()))?;
    Ok(session.inserted)
}

pub fn library_replace_session_abort(session: LibraryReplaceSession) -> Result<(), WcError> {
    drop(session);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;
    use wc_core::types::{Backend, FileType, WallpaperEntry, WallpaperProject};

    fn test_entry(path: &str) -> WallpaperEntry {
        WallpaperEntry {
            path: Utf8PathBuf::from(path),
            file_type: FileType::Image,
            ext: "jpg".into(),
            backend: Backend::Awww,
            size: 100,
            mtime: 200,
            resolution: "2x2".into(),
            project: None,
        }
    }

    #[test]
    fn library_replace_session_commit_replaces_rows_only_at_commit() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        crate::sqlite::ensure_sqlite_db(&cd);

        let conn = rusqlite::Connection::open(cd.db_path()).unwrap();
        conn.execute(
            "INSERT INTO wallpapers (path, type, ext, backend, size, mtime, resolution)
             VALUES ('/walls/old.jpg', 'image', 'jpg', 'awww', 1, 1, '1x1')",
            [],
        )
        .unwrap();

        let mut session = library_replace_session_start(&cd).unwrap();
        let new_entry = WallpaperEntry {
            path: Utf8PathBuf::from("/walls/new.jpg"),
            file_type: FileType::Image,
            ext: "jpg".into(),
            backend: Backend::Awww,
            size: 100,
            mtime: 200,
            resolution: "2x2".into(),
            project: None,
        };
        library_replace_session_push(&mut session, &[new_entry]).unwrap();

        let count_before: usize = conn
            .query_row("SELECT COUNT(*) FROM wallpapers", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count_before, 1);

        let inserted = library_replace_session_commit(session).unwrap();
        assert_eq!(inserted, 1);

        let count_after: usize = conn
            .query_row("SELECT COUNT(*) FROM wallpapers", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count_after, 1);
    }

    #[test]
    fn library_replace_session_abort_preserves_existing_library() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        crate::sqlite::ensure_sqlite_db(&cd);

        let conn = rusqlite::Connection::open(cd.db_path()).unwrap();
        conn.execute(
            "INSERT INTO wallpapers (path, type, ext, backend, size, mtime, resolution)
             VALUES ('/walls/old.jpg', 'image', 'jpg', 'awww', 1, 1, '1x1')",
            [],
        )
        .unwrap();

        let mut session = library_replace_session_start(&cd).unwrap();
        let new_entry = WallpaperEntry {
            path: Utf8PathBuf::from("/walls/new.jpg"),
            file_type: FileType::Image,
            ext: "jpg".into(),
            backend: Backend::Awww,
            size: 100,
            mtime: 200,
            resolution: "2x2".into(),
            project: None,
        };
        library_replace_session_push(&mut session, &[new_entry]).unwrap();

        library_replace_session_abort(session).unwrap();

        let count: usize = conn
            .query_row("SELECT COUNT(*) FROM wallpapers", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);

        let path: String = conn
            .query_row("SELECT path FROM wallpapers", [], |r| r.get(0))
            .unwrap();
        assert_eq!(path, "/walls/old.jpg");
    }

    #[test]
    fn library_replace_session_push_rolls_back_failed_batch() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        crate::sqlite::ensure_sqlite_db(&cd);

        let mut session = library_replace_session_start(&cd).unwrap();
        session
            .conn
            .execute_batch(
                "CREATE TEMP TRIGGER fail_wallpapers_stage_insert
                 BEFORE INSERT ON wallpapers_stage
                 WHEN NEW.path = '/walls/bad.jpg'
                 BEGIN
                     SELECT RAISE(ABORT, 'forced stage failure');
                 END;",
            )
            .unwrap();

        let result = library_replace_session_push(
            &mut session,
            &[test_entry("/walls/good.jpg"), test_entry("/walls/bad.jpg")],
        );

        assert!(result.is_err(), "batch should fail on the injected trigger");
        let staged_count: usize = session
            .conn
            .query_row("SELECT COUNT(*) FROM wallpapers_stage", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            staged_count, 0,
            "failed batches must not leave partial rows"
        );
        assert_eq!(
            session.inserted, 0,
            "failed batches must not change counters"
        );
    }

    #[test]
    fn library_replace_session_preserves_we_metadata() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        crate::sqlite::ensure_sqlite_db(&cd);

        let mut session = library_replace_session_start(&cd).unwrap();
        let entry = WallpaperEntry {
            path: Utf8PathBuf::from("/steamapps/workshop/content/431960/3558034522"),
            file_type: FileType::WeScene,
            ext: "scene".into(),
            backend: Backend::LinuxWallpaperEngine,
            size: 42,
            mtime: 1234,
            resolution: "WE".into(),
            project: Some(WallpaperProject {
                project_type: "we_scene".into(),
                preview_path: Some(
                    "/steamapps/workshop/content/431960/3558034522/preview.gif".into(),
                ),
                workshop_id: Some("3558034522".into()),
                title: Some("Scene title".into()),
                we_file: Some("scene.json".into()),
                backend: Some("linux-wallpaperengine".into()),
                unsupported_reason: None,
            }),
        };
        library_replace_session_push(&mut session, &[entry]).unwrap();
        let inserted = library_replace_session_commit(session).unwrap();
        assert_eq!(inserted, 1);

        let conn = rusqlite::Connection::open(cd.db_path()).unwrap();
        let (project_type, preview_path, workshop_id, title): (String, String, String, String) =
            conn.query_row(
                "SELECT project_type, preview_path, workshop_id, title FROM wallpapers",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(project_type, "we_scene");
        assert!(preview_path.ends_with("preview.gif"));
        assert_eq!(workshop_id, "3558034522");
        assert_eq!(title, "Scene title");
    }
}
