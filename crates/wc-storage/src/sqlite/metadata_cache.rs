use std::collections::HashMap;

use camino::Utf8PathBuf;
use wc_core::config::ConfigDir;
use wc_core::types::{Backend, FileType, WallpaperEntry, WallpaperProject};

use super::row_map::non_empty;
use super::schema::{ensure_wallpaper_metadata_columns, open_runtime_connection};

/// Load prior metadata from the SQLite wallpapers table into a HashMap keyed by
/// canonical path. Returns an empty cache if the database does not exist.
pub fn prior_metadata_cache_from_sqlite(cd: &ConfigDir) -> HashMap<String, WallpaperEntry> {
    let mut cache = HashMap::new();
    let db_path = cd.db_path();
    if !db_path.exists() {
        return cache;
    }
    let conn = match open_runtime_connection(cd) {
        Ok(c) => c,
        Err(err) => {
            log::warn!("prior metadata cache: open failed ({err}); full rescan will run");
            return cache;
        }
    };
    ensure_wallpaper_metadata_columns(&conn).ok();
    let mut stmt = match conn.prepare(
        "SELECT path, type, ext, backend, size, mtime, resolution,
                project_type, preview_path, workshop_id, title, we_file, unsupported_reason
         FROM wallpapers",
    ) {
        Ok(s) => s,
        Err(err) => {
            log::warn!("prior metadata cache: prepare failed ({err}); full rescan will run");
            return cache;
        }
    };
    let rows = match stmt.query_map([], |row| {
        let path: String = row.get(0)?;
        let ftype_s: String = row.get(1)?;
        let ext: String = row.get(2)?;
        let backend_s: String = row.get(3)?;
        let size: i64 = row.get(4)?;
        let mtime: i64 = row.get(5)?;
        let resolution: String = row.get(6)?;
        let project_type: String = row.get(7)?;
        let preview_path: String = row.get(8)?;
        let workshop_id: String = row.get(9)?;
        let title: String = row.get(10)?;
        let we_file: String = row.get(11)?;
        let unsupported_reason: String = row.get(12)?;
        Ok((
            path,
            ftype_s,
            ext,
            backend_s,
            size,
            mtime,
            resolution,
            project_type,
            preview_path,
            workshop_id,
            title,
            we_file,
            unsupported_reason,
        ))
    }) {
        Ok(r) => r,
        Err(_) => return cache,
    };
    for row in rows {
        let (
            path,
            ftype_s,
            ext,
            backend_s,
            size,
            mtime,
            resolution,
            project_type,
            preview_path,
            workshop_id,
            title,
            we_file,
            unsupported_reason,
        ) = match row {
            Ok(r) => r,
            Err(_) => continue,
        };
        let canon = std::fs::canonicalize(&path)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| path.clone());
        let file_type = match ftype_s.as_str() {
            "gif" => FileType::Gif,
            "video" => FileType::Video,
            "we_scene" => FileType::WeScene,
            "we_web" => FileType::WeWeb,
            "unsupported" => FileType::WeApplication,
            _ => FileType::Image,
        };
        let backend = match backend_s.as_str() {
            "mpvpaper" => Backend::Mpvpaper,
            "swaybg" => Backend::Swaybg,
            "feh" => Backend::Feh,
            "linux-wallpaperengine" => Backend::LinuxWallpaperEngine,
            "chromium-web" | "webkit-layer-shell" | "unsupported" => Backend::Unsupported,
            _ => Backend::Awww,
        };
        let project = if project_type.is_empty()
            && preview_path.is_empty()
            && workshop_id.is_empty()
            && title.is_empty()
            && we_file.is_empty()
            && unsupported_reason.is_empty()
        {
            None
        } else {
            Some(WallpaperProject {
                project_type,
                preview_path: non_empty(preview_path),
                workshop_id: non_empty(workshop_id),
                title: non_empty(title),
                we_file: non_empty(we_file),
                backend: Some(backend.as_str().to_string()),
                unsupported_reason: non_empty(unsupported_reason),
            })
        };
        cache.insert(
            canon,
            WallpaperEntry {
                path: Utf8PathBuf::from(&path),
                file_type,
                ext,
                backend,
                size: size as u64,
                mtime: mtime as u64,
                resolution,
                project,
            },
        );
    }
    cache
}
