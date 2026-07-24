use super::common::{
    fail, format_bytes, ok, storage, CommandResult, ThumbnailCacheDto, ThumbnailDto,
};
use super::path_guard;
use rusqlite::params;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use tauri::Manager;
use wc_storage::StorageApi;

fn preview_asset_scope_cache() -> &'static Mutex<HashSet<PathBuf>> {
    static CACHE: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashSet::new()))
}

fn allow_preview_asset(app: &tauri::AppHandle, canonical: &Path) -> Result<(), String> {
    let mut granted = preview_asset_scope_cache()
        .lock()
        .map_err(|_| "preview asset scope cache lock poisoned".to_string())?;
    if granted.contains(canonical) {
        return Ok(());
    }
    app.asset_protocol_scope()
        .allow_file(canonical)
        .map_err(|error| error.to_string())?;
    granted.insert(canonical.to_path_buf());
    Ok(())
}

fn ensure_preview_asset_path(
    path: &Path,
    wallpaper_path: &str,
    storage: &StorageApi,
) -> Result<PathBuf, String> {
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("cannot resolve path {}: {error}", path.display()))?;
    let requested_wallpaper = path_guard::ensure_command_wallpaper_path(wallpaper_path, storage)?;
    let connection = wc_storage::sqlite::open_runtime_connection(&storage.cd)
        .map_err(|error| error.to_string())?;
    let canonical_wallpaper_text = requested_wallpaper.to_string_lossy();
    let mut statement = connection
        .prepare(
            "SELECT path, preview_path
             FROM wallpapers
             WHERE path = ?1 OR path = ?2",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(
            params![wallpaper_path, canonical_wallpaper_text.as_ref()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
        )
        .map_err(|error| error.to_string())?;
    for row in rows {
        let (recorded_wallpaper, recorded_preview) = row.map_err(|error| error.to_string())?;
        if recorded_preview_asset_matches(
            &canonical,
            &requested_wallpaper,
            &recorded_wallpaper,
            recorded_preview.as_deref(),
        ) {
            return Ok(canonical);
        }
    }

    Err("path is not the recorded wallpaper or its recorded project preview".into())
}

fn recorded_preview_asset_matches(
    candidate: &Path,
    requested_wallpaper: &Path,
    recorded_wallpaper: &str,
    recorded_preview: Option<&str>,
) -> bool {
    let Ok(recorded_wallpaper) = Path::new(recorded_wallpaper).canonicalize() else {
        return false;
    };
    if recorded_wallpaper != requested_wallpaper {
        return false;
    }
    if candidate == recorded_wallpaper {
        return true;
    }

    let project_root = if recorded_wallpaper.is_dir() {
        recorded_wallpaper.as_path()
    } else {
        let Some(parent) = recorded_wallpaper.parent() else {
            return false;
        };
        parent
    };
    let Some(recorded_preview) = recorded_preview else {
        return false;
    };
    let Ok(recorded_preview) = Path::new(recorded_preview).canonicalize() else {
        return false;
    };
    recorded_preview == candidate && recorded_preview.starts_with(project_root)
}

fn authorize_preview_asset_with<Validate, Allow>(
    path: &Path,
    validate: Validate,
    allow: Allow,
) -> Result<String, String>
where
    Validate: FnOnce(&Path) -> Result<PathBuf, String>,
    Allow: FnOnce(&Path) -> Result<(), String>,
{
    let canonical = validate(path)?;
    allow(&canonical)?;
    Ok(canonical.to_string_lossy().into_owned())
}

fn authorize_thumbnail_asset_with<Allow>(
    thumbnail: Option<&str>,
    cache_dir: &Path,
    allow: Allow,
) -> Result<Option<String>, String>
where
    Allow: FnOnce(&Path) -> Result<(), String>,
{
    let Some(thumbnail) = thumbnail else {
        return Ok(None);
    };
    let canonical = path_guard::ensure_path_in_config_dir(Path::new(thumbnail), cache_dir)?;
    allow(&canonical)?;
    Ok(Some(canonical.to_string_lossy().into_owned()))
}

#[tauri::command]
pub async fn preview_asset_authorize(
    app: tauri::AppHandle,
    path: String,
    wallpaper_path: String,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let storage = storage()?;
        authorize_preview_asset_with(
            Path::new(&path),
            |candidate| ensure_preview_asset_path(candidate, &wallpaper_path, storage),
            |canonical| allow_preview_asset(&app, canonical),
        )
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn thumbnail_for(app: tauri::AppHandle, path: String) -> Result<ThumbnailDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let s = storage()?;
        let canonical = path_guard::ensure_command_wallpaper_path(&path, s)?;
        let canonical = canonical.to_string_lossy().into_owned();
        let mode = s.config_get("gui_thumbnail_mode", "cache");
        if mode == "icon" {
            return Ok(ThumbnailDto {
                path: canonical,
                thumbnail: None,
                cache_hit: false,
                failure_reason: None,
            });
        }
        let ttl = s
            .config_get("gui_thumbnail_failure_ttl_secs", "900")
            .parse()
            .unwrap_or(900);
        let cache_dir = s.cd.gui_thumbnail_cache_dir();
        let result = wc_preview::thumbnail_for_with_failure_ttl(&cache_dir, &canonical, ttl);
        let thumbnail =
            authorize_thumbnail_asset_with(result.thumbnail.as_deref(), &cache_dir, |thumbnail| {
                allow_preview_asset(&app, thumbnail)
            })?;
        Ok(ThumbnailDto {
            path: result.path,
            thumbnail,
            cache_hit: result.cache_hit,
            failure_reason: result.failure_reason.map(|r| r.as_str().to_string()),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod preview_asset_tests {
    use super::{
        authorize_preview_asset_with, authorize_thumbnail_asset_with, ensure_preview_asset_path,
    };
    use crate::commands::path_guard;
    use rusqlite::params;
    use std::fs;
    use std::path::{Path, PathBuf};

    #[test]
    fn preview_asset_validation_failure_never_mutates_the_scope() {
        let mut allowed = false;
        let result = authorize_preview_asset_with(
            Path::new("/outside/secret.png"),
            |_| Err("path is outside configured wallpaper sources".into()),
            |_| {
                allowed = true;
                Ok(())
            },
        );

        assert_eq!(
            result.unwrap_err(),
            "path is outside configured wallpaper sources"
        );
        assert!(!allowed);
    }

    #[test]
    fn preview_asset_scope_receives_only_the_canonical_validated_file() {
        let canonical = PathBuf::from("/sources/walls/real.png");
        let mut allowed = None;
        let result = authorize_preview_asset_with(
            Path::new("/sources/walls/../walls/real.png"),
            |_| Ok(canonical.clone()),
            |path| {
                allowed = Some(path.to_path_buf());
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(result, canonical.to_string_lossy());
        assert_eq!(allowed, Some(canonical));
    }

    #[test]
    fn preview_asset_allows_only_files_under_a_configured_source() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source");
        let outside = tmp.path().join("outside");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&outside).unwrap();
        let wallpaper = source.join("wall.jpg");
        let secret = outside.join("secret.jpg");
        fs::write(&wallpaper, b"wallpaper").unwrap();
        fs::write(&secret, b"secret").unwrap();
        let roots = vec![source];
        let mut allowed = Vec::new();

        let accepted = authorize_preview_asset_with(
            &wallpaper,
            |candidate| path_guard::ensure_path_in_sources(candidate, &roots),
            |canonical| {
                allowed.push(canonical.to_path_buf());
                Ok(())
            },
        )
        .unwrap();
        let rejected = authorize_preview_asset_with(
            &secret,
            |candidate| path_guard::ensure_path_in_sources(candidate, &roots),
            |canonical| {
                allowed.push(canonical.to_path_buf());
                Ok(())
            },
        );

        assert_eq!(
            accepted,
            wallpaper.canonicalize().unwrap().to_string_lossy()
        );
        assert!(rejected.is_err());
        assert_eq!(allowed, vec![wallpaper.canonicalize().unwrap()]);
    }

    #[test]
    fn preview_asset_accepts_an_exact_recorded_preview_outside_current_sources() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = wc_storage::StorageApi::new(wc_core::ConfigDir {
            path: tmp.path().join("config"),
        });
        let orphan = tmp.path().join("removed-source");
        fs::create_dir_all(&orphan).unwrap();
        let preview = orphan.join("preview.jpg");
        let unrelated = orphan.join("unrelated.jpg");
        fs::write(
            orphan.join("project.json"),
            r#"{"type":"scene","preview":"preview.jpg"}"#,
        )
        .unwrap();
        fs::write(&preview, b"preview").unwrap();
        fs::write(&unrelated, b"secret").unwrap();
        let scanned = wc_scan::read_we_project_info(&orphan).unwrap();
        let connection = wc_storage::sqlite::open_runtime_connection(&storage.cd).unwrap();
        connection
            .execute(
                "INSERT INTO wallpapers (path, type, ext, backend, preview_path)
                 VALUES (?1, 'we_scene', 'json', 'linux-wallpaperengine', ?2)",
                params![
                    orphan.to_string_lossy(),
                    scanned.preview_path.as_deref().unwrap()
                ],
            )
            .unwrap();

        let accepted =
            ensure_preview_asset_path(&preview, &orphan.to_string_lossy(), &storage).unwrap();
        let rejected = ensure_preview_asset_path(&unrelated, &orphan.to_string_lossy(), &storage);

        assert_eq!(accepted, preview.canonicalize().unwrap());
        assert!(rejected.is_err());
    }

    #[test]
    fn preview_asset_rejects_unrelated_file_inside_a_configured_source() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = wc_storage::StorageApi::new(wc_core::ConfigDir {
            path: tmp.path().join("config"),
        });
        let source = tmp.path().join("source");
        fs::create_dir_all(&source).unwrap();
        let wallpaper = source.join("wallpaper.jpg");
        let unrelated = source.join("private.txt");
        fs::write(&wallpaper, b"wallpaper").unwrap();
        fs::write(&unrelated, b"private").unwrap();
        storage
            .sources_add(source.to_string_lossy().as_ref())
            .unwrap();
        let connection = wc_storage::sqlite::open_runtime_connection(&storage.cd).unwrap();
        connection
            .execute(
                "INSERT INTO wallpapers (path, type, ext, backend)
                 VALUES (?1, 'image', 'jpg', 'awww')",
                [wallpaper.to_string_lossy().as_ref()],
            )
            .unwrap();

        let accepted =
            ensure_preview_asset_path(&wallpaper, &wallpaper.to_string_lossy(), &storage).unwrap();
        let rejected =
            ensure_preview_asset_path(&unrelated, &wallpaper.to_string_lossy(), &storage);

        assert_eq!(accepted, wallpaper.canonicalize().unwrap());
        assert!(rejected.is_err());
    }

    #[test]
    fn preview_asset_rejects_recorded_path_outside_the_project_root() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = wc_storage::StorageApi::new(wc_core::ConfigDir {
            path: tmp.path().join("config"),
        });
        let project = tmp.path().join("project");
        let outside = tmp.path().join("outside.jpg");
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("project.json"), b"{}").unwrap();
        fs::write(&outside, b"outside").unwrap();
        let connection = wc_storage::sqlite::open_runtime_connection(&storage.cd).unwrap();
        connection
            .execute(
                "INSERT INTO wallpapers (path, type, ext, backend, preview_path)
                 VALUES (?1, 'we_scene', 'json', 'linux-wallpaperengine', ?2)",
                params![
                    project.to_string_lossy(),
                    project.join("../outside.jpg").to_string_lossy()
                ],
            )
            .unwrap();

        let result = ensure_preview_asset_path(&outside, &project.to_string_lossy(), &storage);

        assert!(result.is_err());
    }

    #[cfg(unix)]
    #[test]
    fn preview_asset_rejects_recorded_symlink_escape() {
        use std::os::unix::fs::symlink;

        let tmp = tempfile::tempdir().unwrap();
        let storage = wc_storage::StorageApi::new(wc_core::ConfigDir {
            path: tmp.path().join("config"),
        });
        let project = tmp.path().join("project");
        let outside = tmp.path().join("outside.jpg");
        let linked_preview = project.join("preview.jpg");
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("project.json"), b"{}").unwrap();
        fs::write(&outside, b"outside").unwrap();
        symlink(&outside, &linked_preview).unwrap();
        let connection = wc_storage::sqlite::open_runtime_connection(&storage.cd).unwrap();
        connection
            .execute(
                "INSERT INTO wallpapers (path, type, ext, backend, preview_path)
                 VALUES (?1, 'we_scene', 'json', 'linux-wallpaperengine', ?2)",
                params![project.to_string_lossy(), linked_preview.to_string_lossy()],
            )
            .unwrap();

        let result =
            ensure_preview_asset_path(&linked_preview, &project.to_string_lossy(), &storage);

        assert!(result.is_err());
    }

    #[test]
    fn thumbnail_asset_authorization_accepts_only_the_active_cache_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = tmp.path().join("custom-xdg").join("gui-thumbnails");
        let outside = tmp.path().join("outside");
        fs::create_dir_all(&cache).unwrap();
        fs::create_dir_all(&outside).unwrap();
        let thumbnail = cache.join("thumb.webp");
        let secret = outside.join("secret.webp");
        fs::write(&thumbnail, b"thumbnail").unwrap();
        fs::write(&secret, b"secret").unwrap();
        let mut allowed = Vec::new();

        let accepted = authorize_thumbnail_asset_with(
            Some(thumbnail.to_string_lossy().as_ref()),
            &cache,
            |canonical| {
                allowed.push(canonical.to_path_buf());
                Ok(())
            },
        )
        .unwrap();
        let rejected = authorize_thumbnail_asset_with(
            Some(secret.to_string_lossy().as_ref()),
            &cache,
            |canonical| {
                allowed.push(canonical.to_path_buf());
                Ok(())
            },
        );

        assert_eq!(
            accepted.as_deref(),
            Some(thumbnail.canonicalize().unwrap().to_string_lossy().as_ref())
        );
        assert!(rejected.is_err());
        assert_eq!(allowed, vec![thumbnail.canonicalize().unwrap()]);
    }
}

#[tauri::command]
pub async fn thumbnail_cache_status() -> Result<ThumbnailCacheDto, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let s = storage()?;
        let dir = s.cd.gui_thumbnail_cache_dir();
        let info = wc_preview::thumbnail_cache_info(&dir);
        let cleanup_days = s
            .config_get("gui_thumbnail_cleanup_days", "30")
            .parse()
            .unwrap_or(30);
        Ok(ThumbnailCacheDto {
            dir: dir.to_string_lossy().to_string(),
            size: format_bytes(info.total_bytes),
            entries: info.entries,
            oldest_mtime: info.oldest_mtime,
            newest_mtime: info.newest_mtime,
            failure_entries: info.failure_entries,
            cleanup_days,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn thumbnail_cache_clear() -> CommandResult {
    tauri::async_runtime::spawn_blocking(|| match storage() {
        Ok(s) => {
            let removed = wc_preview::thumbnail_cache_cleanup_all(&s.cd.gui_thumbnail_cache_dir());
            ok(format!("Removed {} thumbnail cache file(s).", removed))
        }
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn thumbnail_cache_cleanup_old(days: u64) -> CommandResult {
    tauri::async_runtime::spawn_blocking(move || match storage() {
        Ok(s) => {
            let removed =
                wc_preview::thumbnail_cache_cleanup_old(&s.cd.gui_thumbnail_cache_dir(), days);
            ok(format!("Removed {} old thumbnail cache file(s).", removed))
        }
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}
