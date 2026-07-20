//! Path allowlisting for Tauri commands that accept filesystem paths.

use std::fs::File;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use rusqlite::params;
use wc_storage::StorageApi;

const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";

/// Component-level prefix check (avoids `/a/bc` matching `/a/b` via string starts_with).
pub fn path_is_under(path: &Path, root: &Path) -> bool {
    let path_components: Vec<Component<'_>> = path.components().collect();
    let root_components: Vec<Component<'_>> = root.components().collect();
    if root_components.len() > path_components.len() {
        return false;
    }
    path_components
        .iter()
        .zip(root_components.iter())
        .all(|(a, b)| a == b)
}

fn canonicalize_existing(path: &Path) -> Result<PathBuf, String> {
    path.canonicalize()
        .map_err(|error| format!("cannot resolve path {}: {}", path.display(), error))
}

/// Validate `path` lies under one of the configured wallpaper source roots.
pub fn ensure_path_in_sources(
    path: &Path,
    sources: &[impl AsRef<Path>],
) -> Result<PathBuf, String> {
    let canonical = canonicalize_existing(path)?;
    for source in sources {
        let Ok(source_canonical) = source.as_ref().canonicalize() else {
            continue;
        };
        if path_is_under(&canonical, &source_canonical) {
            return Ok(canonical);
        }
    }
    Err("path is outside configured wallpaper sources".into())
}

/// Validate `path` lies under the config directory (or a subdirectory).
pub fn ensure_path_in_config_dir(path: &Path, config_dir: &Path) -> Result<PathBuf, String> {
    let canonical = canonicalize_existing(path)?;
    let config_canonical = canonicalize_existing(config_dir)?;
    if path_is_under(&canonical, &config_canonical) {
        Ok(canonical)
    } else {
        Err("path is outside the config directory".into())
    }
}

/// Allow a path that is under a configured source **or** exactly matches a
/// recorded library / current / history / display-state path.
#[cfg(test)]
pub fn ensure_wallpaper_access_path(
    path: &Path,
    sources: &[impl AsRef<Path>],
    recorded_paths: &[impl AsRef<Path>],
) -> Result<PathBuf, String> {
    if let Ok(canonical) = ensure_path_in_sources(path, sources) {
        return Ok(canonical);
    }

    let canonical = canonicalize_existing(path)?;
    for recorded in recorded_paths {
        let Ok(recorded_canonical) = recorded.as_ref().canonicalize() else {
            continue;
        };
        if recorded_canonical == canonical {
            return Ok(canonical);
        }
    }

    Err("path is outside configured wallpaper sources".into())
}

/// SQLite restore may accept an arbitrary user-chosen file, but it must exist
/// and start with the SQLite header magic bytes.
pub fn ensure_sqlite_restore_file(path: &Path) -> Result<PathBuf, String> {
    if !path.is_file() {
        return Err(format!("backup file not found: {}", path.display()));
    }
    let mut file = File::open(path)
        .map_err(|error| format!("cannot open backup file {}: {}", path.display(), error))?;
    let mut header = [0u8; 16];
    file.read_exact(&mut header).map_err(|_| {
        format!(
            "backup file is too short to be a SQLite database: {}",
            path.display()
        )
    })?;
    if &header != SQLITE_MAGIC {
        return Err(format!(
            "backup file is not a valid SQLite database: {}",
            path.display()
        ));
    }
    Ok(path.to_path_buf())
}

/// Load source roots from storage for path allowlisting.
pub fn load_source_roots(storage: &StorageApi) -> Result<Vec<PathBuf>, String> {
    storage
        .sources_list()
        .map(|paths| paths.into_iter().map(PathBuf::from).collect())
        .map_err(|error| error.to_string())
}

fn stored_path_matches(stored: &str, path: &Path, canonical: &Path) -> bool {
    if stored.is_empty() {
        return false;
    }
    let original = path.to_string_lossy();
    let canon = canonical.to_string_lossy();
    if stored == original || stored == canon {
        return true;
    }
    Path::new(stored)
        .canonicalize()
        .ok()
        .is_some_and(|stored_canonical| stored_canonical == canonical)
}

fn path_exists_in_table(
    conn: &rusqlite::Connection,
    sql: &str,
    path: &Path,
    canonical: &Path,
) -> Result<bool, String> {
    let original = path.to_string_lossy();
    let canon = canonical.to_string_lossy();
    conn.query_row(sql, params![original.as_ref(), canon.as_ref()], |row| {
        row.get(0)
    })
    .map_err(|error| error.to_string())
}

/// Check whether `path` is recorded in library / current / history / display state.
pub fn path_is_recorded_wallpaper_path(
    storage: &StorageApi,
    path: &Path,
    canonical: &Path,
) -> Result<bool, String> {
    if let Some(current) = storage
        .current_read()
        .map_err(|error| error.to_string())?
        .filter(|value| !value.is_empty())
    {
        if stored_path_matches(&current, path, canonical) {
            return Ok(true);
        }
    }

    if !storage.cd.db_path().exists() {
        return Ok(false);
    }

    let conn = wc_storage::sqlite::open_runtime_connection(&storage.cd)
        .map_err(|error| error.to_string())?;

    if path_exists_in_table(
        &conn,
        "SELECT EXISTS(
            SELECT 1 FROM display_state
            WHERE wallpaper_path IN (?1, ?2) AND wallpaper_path != ''
        )",
        path,
        canonical,
    )? {
        return Ok(true);
    }
    if path_exists_in_table(
        &conn,
        "SELECT EXISTS(SELECT 1 FROM favorites WHERE path IN (?1, ?2))",
        path,
        canonical,
    )? {
        return Ok(true);
    }
    if path_exists_in_table(
        &conn,
        "SELECT EXISTS(SELECT 1 FROM wallpapers WHERE path IN (?1, ?2))",
        path,
        canonical,
    )? {
        return Ok(true);
    }
    if path_exists_in_table(
        &conn,
        "SELECT EXISTS(
            SELECT 1 FROM wallpapers
            WHERE preview_path IN (?1, ?2)
              AND preview_path IS NOT NULL
              AND preview_path != ''
        )",
        path,
        canonical,
    )? {
        return Ok(true);
    }
    path_exists_in_table(
        &conn,
        "SELECT EXISTS(SELECT 1 FROM history WHERE path IN (?1, ?2))",
        path,
        canonical,
    )
}

/// Convenience wrapper used by Tauri commands.
pub fn ensure_command_wallpaper_path(path: &str, storage: &StorageApi) -> Result<PathBuf, String> {
    let path = Path::new(path);
    let sources = load_source_roots(storage)?;
    if let Ok(canonical) = ensure_path_in_sources(path, &sources) {
        return Ok(canonical);
    }

    let canonical = canonicalize_existing(path)?;
    if path_is_recorded_wallpaper_path(storage, path, &canonical)? {
        Ok(canonical)
    } else {
        Err("path is outside configured wallpaper sources".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::symlink;
    use wc_config::ConfigDirExt;

    #[test]
    fn rejects_string_prefix_false_positive() {
        let root = Path::new("/a/b");
        let impostor = Path::new("/a/bc/file.jpg");
        assert!(!path_is_under(impostor, root));
    }

    #[test]
    fn accepts_genuine_subdirectory() {
        let root = Path::new("/a/b");
        let child = Path::new("/a/b/c/file.jpg");
        assert!(path_is_under(child, root));
    }

    #[test]
    fn ensure_path_in_sources_rejects_parent_traversal() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source");
        let outside = tmp.path().join("outside");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&outside).unwrap();
        let secret = outside.join("secret.jpg");
        fs::write(&secret, b"secret").unwrap();

        let traversal = source.join("..").join("outside").join("secret.jpg");
        let error = ensure_path_in_sources(&traversal, &[source]).unwrap_err();
        assert!(
            error.contains("outside configured wallpaper sources"),
            "{error}"
        );
    }

    #[test]
    fn ensure_path_in_sources_accepts_child() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source");
        fs::create_dir_all(source.join("nested")).unwrap();
        let file = source.join("nested").join("wall.jpg");
        fs::write(&file, b"jpg").unwrap();

        let allowed = ensure_path_in_sources(&file, &[source]).unwrap();
        assert_eq!(allowed, file.canonicalize().unwrap());
    }

    #[test]
    fn ensure_path_in_sources_rejects_symlink_escape() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source");
        let outside = tmp.path().join("outside");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&outside).unwrap();
        let secret = outside.join("secret.jpg");
        fs::write(&secret, b"secret").unwrap();
        let link = source.join("escape.jpg");
        symlink(&secret, &link).unwrap();

        let error = ensure_path_in_sources(&link, &[source]).unwrap_err();
        assert!(
            error.contains("outside configured wallpaper sources"),
            "{error}"
        );
    }

    #[test]
    fn ensure_path_in_config_dir_accepts_nested_file() {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        fs::create_dir_all(config.join("cache")).unwrap();
        let file = config.join("cache").join("thumb.jpg");
        fs::write(&file, b"thumb").unwrap();

        let allowed = ensure_path_in_config_dir(&file, &config).unwrap();
        assert_eq!(allowed, file.canonicalize().unwrap());
    }

    #[test]
    fn ensure_path_in_config_dir_rejects_outside() {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        let outside = tmp.path().join("elsewhere");
        fs::create_dir_all(&config).unwrap();
        fs::create_dir_all(&outside).unwrap();
        let file = outside.join("wallpapers.db.bak");
        fs::write(&file, b"nope").unwrap();

        let error = ensure_path_in_config_dir(&file, &config).unwrap_err();
        assert!(error.contains("outside the config directory"), "{error}");
    }

    #[test]
    fn wallpaper_access_allows_recorded_exact_path_outside_sources() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source");
        let orphan_dir = tmp.path().join("orphan");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&orphan_dir).unwrap();
        let orphan = orphan_dir.join("old.jpg");
        fs::write(&orphan, b"jpg").unwrap();

        let allowed =
            ensure_wallpaper_access_path(&orphan, &[source], std::slice::from_ref(&orphan))
                .unwrap();
        assert_eq!(allowed, orphan.canonicalize().unwrap());
    }

    #[test]
    fn sqlite_restore_rejects_non_sqlite_file() {
        let tmp = tempfile::tempdir().unwrap();
        let fake = tmp.path().join("fake.bak");
        fs::write(&fake, b"not a sqlite db!!!!").unwrap();
        let error = ensure_sqlite_restore_file(&fake).unwrap_err();
        assert!(error.contains("not a valid SQLite database"), "{error}");
    }

    #[test]
    fn sqlite_restore_accepts_sqlite_header() {
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("good.bak");
        let mut bytes = Vec::from(*SQLITE_MAGIC);
        bytes.extend_from_slice(&[0u8; 32]);
        fs::write(&db, bytes).unwrap();
        let allowed = ensure_sqlite_restore_file(&db).unwrap();
        assert_eq!(allowed, db);
    }

    #[test]
    fn sqlite_restore_rejects_missing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("missing.bak");
        let error = ensure_sqlite_restore_file(&missing).unwrap_err();
        assert!(error.contains("not found"), "{error}");
    }

    #[test]
    fn ensure_command_wallpaper_path_allows_recorded_orphan_path() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source");
        let orphan_dir = tmp.path().join("orphan");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&orphan_dir).unwrap();
        let orphan = orphan_dir.join("old.jpg");
        std::fs::write(&orphan, b"jpg").unwrap();

        let cd = wc_core::ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        wc_config::write_config_value(&cd.path, "storage_backend", "sqlite").unwrap();
        let storage = wc_storage::StorageApi::new(cd);
        storage
            .sources_add(source.to_string_lossy().as_ref())
            .unwrap();
        let conn = wc_storage::sqlite::open_runtime_connection(&storage.cd).unwrap();
        conn.execute(
            "INSERT INTO wallpapers (path, type, ext, backend) VALUES (?1, 'image', 'jpg', 'awww')",
            [orphan.to_string_lossy().as_ref()],
        )
        .unwrap();

        let allowed = ensure_command_wallpaper_path(&orphan.to_string_lossy(), &storage).unwrap();
        assert_eq!(allowed, orphan.canonicalize().unwrap());
    }

    #[test]
    fn ensure_command_wallpaper_path_rejects_unrecorded_outside_source() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source");
        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let secret = outside.join("secret.jpg");
        std::fs::write(&secret, b"secret").unwrap();

        let cd = wc_core::ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        wc_config::write_config_value(&cd.path, "storage_backend", "sqlite").unwrap();
        let storage = wc_storage::StorageApi::new(cd);
        storage
            .sources_add(source.to_string_lossy().as_ref())
            .unwrap();

        let error = ensure_command_wallpaper_path(&secret.to_string_lossy(), &storage).unwrap_err();
        assert!(
            error.contains("outside configured wallpaper sources"),
            "{error}"
        );
    }
}
