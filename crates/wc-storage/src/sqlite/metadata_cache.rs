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
        Err(_) => return cache,
    };
    ensure_wallpaper_metadata_columns(&conn).ok();
    let mut stmt = match conn.prepare(
        "SELECT path, type, ext, backend, size, mtime, resolution,
                project_type, preview_path, workshop_id, title, we_file, unsupported_reason
         FROM wallpapers",
    ) {
        Ok(s) => s,
        Err(_) => return cache,
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

/// Paths in `prior_cache` that were not seen during a rescan (canonical path keys).
pub fn removed_from_prior_cache(
    prior_cache: &HashMap<String, WallpaperEntry>,
    seen_canonical_paths: &std::collections::HashSet<String>,
) -> (usize, Vec<String>) {
    let mut removed = 0usize;
    let mut workshop_ids = Vec::new();
    for (canon_path, entry) in prior_cache {
        if seen_canonical_paths.contains(canon_path) {
            continue;
        }
        removed += 1;
        if let Some(wid) = entry
            .project
            .as_ref()
            .and_then(|p| p.workshop_id.as_deref())
            .filter(|wid| !wid.is_empty())
        {
            workshop_ids.push(wid.to_string());
        }
    }
    workshop_ids.sort();
    workshop_ids.dedup();
    (removed, workshop_ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;
    use wc_core::types::{Backend, FileType, WallpaperProject};

    #[test]
    fn removed_from_prior_cache_counts_missing_paths_and_workshop_ids() {
        let mut prior = HashMap::new();
        prior.insert(
            "/steamapps/workshop/content/431960/3589454154".into(),
            WallpaperEntry {
                path: Utf8PathBuf::from("/steamapps/workshop/content/431960/3589454154"),
                file_type: FileType::WeScene,
                ext: "scene".into(),
                backend: Backend::LinuxWallpaperEngine,
                size: 1,
                mtime: 1,
                resolution: "WE".into(),
                project: Some(WallpaperProject {
                    project_type: "we_scene".into(),
                    preview_path: None,
                    workshop_id: Some("3589454154".into()),
                    title: None,
                    we_file: None,
                    backend: None,
                    unsupported_reason: None,
                }),
            },
        );
        prior.insert(
            "/walls/old.jpg".into(),
            WallpaperEntry {
                path: Utf8PathBuf::from("/walls/old.jpg"),
                file_type: FileType::Image,
                ext: "jpg".into(),
                backend: Backend::Awww,
                size: 1,
                mtime: 1,
                resolution: "1x1".into(),
                project: None,
            },
        );
        let seen = std::collections::HashSet::from(["/walls/keep.jpg".into()]);
        let (removed, workshop_ids) = removed_from_prior_cache(&prior, &seen);
        assert_eq!(removed, 2);
        assert_eq!(workshop_ids, vec!["3589454154".to_string()]);
    }
}
