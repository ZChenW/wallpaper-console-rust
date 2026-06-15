use camino::Utf8PathBuf;
use wc_core::types::{Backend, FileType, WallpaperEntry, WallpaperProject};

pub fn wallpaper_entry_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<WallpaperEntry> {
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

    let file_type = match ftype_s.as_str() {
        "image" => FileType::Image,
        "gif" => FileType::Gif,
        "video" => FileType::Video,
        "we_scene" => FileType::WeScene,
        "we_web" => FileType::WeWeb,
        _ => FileType::WeApplication,
    };
    let backend = match backend_s.as_str() {
        "awww" | "swww" => Backend::Awww,
        "mpvpaper" => Backend::Mpvpaper,
        "linux-wallpaperengine" => Backend::LinuxWallpaperEngine,
        "chromium-web" | "webkit-layer-shell" => Backend::Unsupported,
        _ => Backend::Unsupported,
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

    Ok(WallpaperEntry {
        path: Utf8PathBuf::from(path),
        file_type,
        ext,
        backend,
        size: size.max(0) as u64,
        mtime: mtime.max(0) as u64,
        resolution,
        project,
    })
}

pub(crate) fn non_empty(value: String) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}
