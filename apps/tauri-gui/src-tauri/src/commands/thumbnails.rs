use super::common::{
    fail, format_bytes, ok, storage, CommandResult, ThumbnailCacheDto, ThumbnailDto,
};
use super::path_guard;
use rusqlite::params;
use std::path::{Path, PathBuf};
use wc_storage::StorageApi;

fn ensure_preview_asset_path(
    path: &Path,
    wallpaper_path: &str,
    storage: &StorageApi,
) -> Result<PathBuf, String> {
    let roots = path_guard::load_source_roots(storage)?;
    if let Ok(canonical) = path_guard::ensure_path_in_sources(path, &roots) {
        return Ok(canonical);
    }

    let canonical = path
        .canonicalize()
        .map_err(|error| format!("cannot resolve path {}: {error}", path.display()))?;
    let connection = wc_storage::sqlite::open_runtime_connection(&storage.cd)
        .map_err(|error| error.to_string())?;
    let recorded = connection
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM wallpapers
                WHERE path = ?1 AND (path = ?2 OR preview_path = ?2)
            )",
            params![wallpaper_path, path.to_string_lossy()],
            |row| row.get::<_, bool>(0),
        )
        .map_err(|error| error.to_string())?;
    if recorded {
        Ok(canonical)
    } else {
        Err(
            "path is outside configured wallpaper sources and is not recorded for this wallpaper"
                .into(),
        )
    }
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

#[tauri::command]
pub async fn preview_asset_authorize(
    app: tauri::AppHandle,
    path: String,
    wallpaper_path: String,
) -> Result<String, String> {
    use tauri::Manager;

    tauri::async_runtime::spawn_blocking(move || {
        let storage = storage()?;
        authorize_preview_asset_with(
            Path::new(&path),
            |candidate| ensure_preview_asset_path(candidate, &wallpaper_path, storage),
            |canonical| {
                app.asset_protocol_scope()
                    .allow_file(canonical)
                    .map_err(|error| error.to_string())
            },
        )
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn thumbnail_for(path: String) -> Result<ThumbnailDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let s = storage()?;
        path_guard::ensure_command_wallpaper_path(&path, s)?;
        let mode = s.config_get("gui_thumbnail_mode", "cache");
        if mode == "icon" {
            return Ok(ThumbnailDto {
                path,
                thumbnail: None,
                cache_hit: false,
                failure_reason: None,
            });
        }
        let ttl = s
            .config_get("gui_thumbnail_failure_ttl_secs", "900")
            .parse()
            .unwrap_or(900);
        let result =
            wc_preview::thumbnail_for_with_failure_ttl(&s.cd.gui_thumbnail_cache_dir(), &path, ttl);
        Ok(ThumbnailDto {
            path: result.path,
            thumbnail: result.thumbnail,
            cache_hit: result.cache_hit,
            failure_reason: result.failure_reason.map(|r| r.as_str().to_string()),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod preview_asset_tests {
    use super::{authorize_preview_asset_with, ensure_preview_asset_path};
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
        let wallpaper = orphan.join("project.json");
        let preview = orphan.join("preview.jpg");
        let unrelated = orphan.join("unrelated.jpg");
        fs::write(&wallpaper, b"{}").unwrap();
        fs::write(&preview, b"preview").unwrap();
        fs::write(&unrelated, b"secret").unwrap();
        let connection = wc_storage::sqlite::open_runtime_connection(&storage.cd).unwrap();
        connection
            .execute(
                "INSERT INTO wallpapers (path, type, ext, backend, preview_path)
                 VALUES (?1, 'we_scene', 'json', 'linux-wallpaperengine', ?2)",
                params![wallpaper.to_string_lossy(), preview.to_string_lossy()],
            )
            .unwrap();

        let accepted =
            ensure_preview_asset_path(&preview, &wallpaper.to_string_lossy(), &storage).unwrap();
        let rejected =
            ensure_preview_asset_path(&unrelated, &wallpaper.to_string_lossy(), &storage);

        assert_eq!(accepted, preview.canonicalize().unwrap());
        assert!(rejected.is_err());
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
