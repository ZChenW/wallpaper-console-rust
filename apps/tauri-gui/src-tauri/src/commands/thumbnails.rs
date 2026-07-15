use super::common::{
    fail, format_bytes, ok, storage, CommandResult, ThumbnailCacheDto, ThumbnailDto,
};
use super::path_guard;

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
