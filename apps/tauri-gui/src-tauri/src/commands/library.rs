use std::sync::Mutex;
use std::sync::OnceLock;

use rusqlite::Connection;
use wc_core::types::{Backend, FileType, WallpaperEntry, WallpaperProject};

use super::common::{
    dto_from_entry, fail, ok, storage, CommandResult, LibraryCountDto, LibraryPageDto,
    LibrarySourceStatusDto, ScanProgressDto, WallpaperDto,
};

static SCAN_STATE: OnceLock<Mutex<ScanProgressDto>> = OnceLock::new();

fn scan_state() -> &'static Mutex<ScanProgressDto> {
    SCAN_STATE.get_or_init(|| {
        Mutex::new(ScanProgressDto {
            running: false,
            stage: "idle".into(),
            scanned: 0,
            total_hint: None,
            reused_metadata: 0,
            probed_metadata: 0,
            inserted_sqlite: 0,
            current_path: None,
            cancel_requested: false,
            error: None,
        })
    })
}

fn set_scan(stage: &str, running: bool) {
    if let Ok(mut state) = scan_state().lock() {
        state.stage = stage.into();
        state.running = running;
        if running {
            state.error = None;
            state.cancel_requested = false;
        }
    }
}

fn read_sqlite_entries(s: &wc_storage::StorageApi) -> Result<Vec<WallpaperEntry>, String> {
    wc_storage::sqlite::ensure_sqlite_db(&s.cd);
    let conn = Connection::open(s.cd.db_path()).map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT path, type, ext, backend, size, mtime, resolution,
                    project_type, preview_path, workshop_id, title, we_file, unsupported_reason
             FROM wallpapers",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            let path: String = row.get(0)?;
            let ftype: String = row.get(1)?;
            let backend: String = row.get(3)?;
            let project_type: String = row.get(7)?;
            let project = if project_type.is_empty() {
                None
            } else {
                Some(WallpaperProject {
                    project_type,
                    preview_path: non_empty(row.get::<_, String>(8)?),
                    workshop_id: non_empty(row.get::<_, String>(9)?),
                    title: non_empty(row.get::<_, String>(10)?),
                    we_file: non_empty(row.get::<_, String>(11)?),
                    backend: Some(backend.clone()),
                    unsupported_reason: non_empty(row.get::<_, String>(12)?),
                })
            };
            Ok(WallpaperEntry {
                path: path.into(),
                file_type: parse_file_type(&ftype),
                ext: row.get(2)?,
                backend: parse_backend(&backend),
                size: row.get::<_, i64>(4)? as u64,
                mtime: row.get::<_, i64>(5)? as u64,
                resolution: row.get(6)?,
                project,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

fn non_empty(s: String) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn parse_file_type(s: &str) -> FileType {
    match s {
        "image" => FileType::Image,
        "gif" => FileType::Gif,
        "video" => FileType::Video,
        "we_scene" => FileType::WeScene,
        "we_web" => FileType::WeWeb,
        _ => FileType::WeApplication,
    }
}

fn parse_backend(s: &str) -> Backend {
    match s {
        "awww" => Backend::Awww,
        "mpvpaper" => Backend::Mpvpaper,
        "linux-wallpaperengine" => Backend::LinuxWallpaperEngine,
        _ => Backend::Unsupported,
    }
}

fn applyability_rank(entry: &WallpaperEntry) -> u8 {
    match entry.file_type {
        FileType::WeWeb => 1,
        FileType::WeApplication => 2,
        _ => 0,
    }
}

fn sort_filter_page(
    mut entries: Vec<WallpaperEntry>,
    filter: &str,
    sort: &str,
    search: &str,
    offset: usize,
    limit: usize,
) -> LibraryPageDto {
    let query = search.trim().to_lowercase();
    entries.retain(|e| {
        let type_ok = match filter {
            "images" | "image" => e.file_type == FileType::Image,
            "gifs" | "gif" => e.file_type == FileType::Gif,
            "videos" | "video" => e.file_type == FileType::Video,
            "we" => matches!(e.file_type, FileType::WeScene | FileType::WeWeb),
            "we_scene" => e.file_type == FileType::WeScene,
            "we_web" => e.file_type == FileType::WeWeb,
            "unsupported" => e.file_type == FileType::WeApplication,
            _ => true,
        };
        let search_ok = query.is_empty()
            || e.path.to_string().to_lowercase().contains(&query)
            || e.project
                .as_ref()
                .and_then(|p| p.title.as_ref())
                .map(|t| t.to_lowercase().contains(&query))
                .unwrap_or(false);
        type_ok && search_ok
    });
    match sort {
        "name" => entries.sort_by(|a, b| {
            applyability_rank(a)
                .cmp(&applyability_rank(b))
                .then(a.filename().cmp(b.filename()))
        }),
        "size" => entries.sort_by(|a, b| {
            applyability_rank(a)
                .cmp(&applyability_rank(b))
                .then(b.size.cmp(&a.size))
                .then(a.path.cmp(&b.path))
        }),
        _ => entries.sort_by(|a, b| {
            applyability_rank(a)
                .cmp(&applyability_rank(b))
                .then(b.mtime.cmp(&a.mtime))
                .then(a.path.cmp(&b.path))
        }),
    }
    let total = entries.len();
    let items = entries
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(dto_from_entry)
        .collect();
    LibraryPageDto { total, items }
}

#[tauri::command]
pub async fn library_count() -> Result<LibraryCountDto, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let s = storage()?;
        let entries = read_sqlite_entries(&s)?;
        Ok(LibraryCountDto {
            total: entries.len(),
            images: entries
                .iter()
                .filter(|e| e.file_type == FileType::Image)
                .count(),
            gifs: entries
                .iter()
                .filter(|e| e.file_type == FileType::Gif)
                .count(),
            videos: entries
                .iter()
                .filter(|e| e.file_type == FileType::Video)
                .count(),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn library_list(_source: String) -> Result<Vec<WallpaperDto>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let s = storage()?;
        Ok(read_sqlite_entries(&s)?
            .into_iter()
            .map(dto_from_entry)
            .collect())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn library_page(
    source: String,
    filter: String,
    sort: String,
    search: String,
    offset: usize,
    limit: usize,
) -> Result<LibraryPageDto, String> {
    let _ = source;
    library_page_gui(filter, sort, search, offset, limit).await
}

#[tauri::command]
pub async fn library_page_gui(
    filter: String,
    sort: String,
    search: String,
    offset: usize,
    limit: usize,
) -> Result<LibraryPageDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let s = storage()?;
        Ok(sort_filter_page(
            read_sqlite_entries(&s)?,
            &filter,
            &sort,
            &search,
            offset,
            limit,
        ))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn favorites_list() -> Result<Vec<WallpaperDto>, String> {
    favorites_page(0, usize::MAX).await.map(|p| p.items)
}

#[tauri::command]
pub async fn favorites_page(offset: usize, limit: usize) -> Result<LibraryPageDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let s = storage()?;
        let favs = s.favorites_list().map_err(|e| e.to_string())?;
        let entries = read_sqlite_entries(&s)?
            .into_iter()
            .filter(|e| favs.iter().any(|p| p == e.path.as_str()))
            .collect();
        Ok(sort_filter_page(entries, "all", "mtime", "", offset, limit))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn favorite_add(path: String) -> CommandResult {
    tauri::async_runtime::spawn_blocking(move || match storage() {
        Ok(s) => match s.favorites_add(&path) {
            Ok(_) => ok("Added favorite."),
            Err(e) => fail(e.to_string()),
        },
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn favorite_remove(path: String) -> CommandResult {
    tauri::async_runtime::spawn_blocking(move || match storage() {
        Ok(s) => match s.favorites_remove(&path) {
            Ok(_) => ok("Removed favorite."),
            Err(e) => fail(e.to_string()),
        },
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn history_list() -> Result<Vec<WallpaperDto>, String> {
    history_page(0, usize::MAX).await.map(|p| p.items)
}

#[tauri::command]
pub async fn history_page(offset: usize, limit: usize) -> Result<LibraryPageDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let s = storage()?;
        let hist = s.history_list().map_err(|e| e.to_string())?;
        let all = read_sqlite_entries(&s)?;
        let mut entries = Vec::new();
        for path in hist {
            if let Some(entry) = all.iter().find(|e| e.path.as_str() == path).cloned() {
                entries.push(entry);
            }
        }
        let total = entries.len();
        let items = entries
            .into_iter()
            .skip(offset)
            .take(limit)
            .map(dto_from_entry)
            .collect();
        Ok(LibraryPageDto { total, items })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn history_clear() -> CommandResult {
    tauri::async_runtime::spawn_blocking(|| match storage() {
        Ok(s) => match s.history_clear() {
            Ok(_) => ok("History cleared."),
            Err(e) => fail(e.to_string()),
        },
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn rescan() -> CommandResult {
    tauri::async_runtime::spawn_blocking(|| {
        set_scan("scanning", true);
        let result = (|| {
            let s = storage()?;
            let sources = s.sources_list().map_err(|e| e.to_string())?;
            let paths = wc_scan::scan_wallpapers(&sources);
            if let Ok(mut state) = scan_state().lock() {
                state.total_hint = Some(paths.len());
            }
            let mut entries = Vec::new();
            for (idx, path) in paths.iter().enumerate() {
                if let Ok(mut state) = scan_state().lock() {
                    if state.cancel_requested {
                        return Err("scan cancelled".to_string());
                    }
                    state.scanned = idx + 1;
                    state.current_path = Some(path.clone());
                }
                if let Some(entry) = wc_scan::make_entry(path) {
                    entries.push(entry);
                }
            }
            let inserted =
                wc_storage::sqlite::library_replace_entries_batch_atomic(&s.cd, &entries)
                    .map_err(|e| e.to_string())?;
            if let Ok(mut state) = scan_state().lock() {
                state.inserted_sqlite = inserted;
            }
            Ok(format!("Scan complete. {} wallpaper(s) indexed.", inserted))
        })();
        match result {
            Ok(msg) => {
                set_scan("idle", false);
                ok(msg)
            }
            Err(err) => {
                if let Ok(mut state) = scan_state().lock() {
                    state.running = false;
                    state.error = Some(err.clone());
                }
                fail(err)
            }
        }
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn scan_progress() -> Result<ScanProgressDto, String> {
    Ok(scan_state().lock().map_err(|e| e.to_string())?.clone())
}

#[tauri::command]
pub async fn scan_cancel() -> CommandResult {
    if let Ok(mut state) = scan_state().lock() {
        state.cancel_requested = true;
    }
    ok("Cancel requested.")
}

#[tauri::command]
pub async fn library_source_status() -> Result<LibrarySourceStatusDto, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let s = storage()?;
        let sqlite_rows = wc_storage::sqlite::library_count(&s.cd).unwrap_or(0);
        let tsv_rows = std::fs::read_to_string(s.cd.library_tsv_path())
            .map(|c| c.lines().filter(|l| !l.trim().is_empty()).count())
            .unwrap_or(0);
        Ok(LibrarySourceStatusDto {
            configured: "sqlite".into(),
            effective: "sqlite".into(),
            sqlite_ready: s.cd.db_path().exists(),
            sqlite_rows,
            tsv_rows,
            stale: false,
            message: "SQLite library database is active.".into(),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;

    fn entry_stub(path: &str, ft: FileType) -> WallpaperEntry {
        WallpaperEntry {
            path: Utf8PathBuf::from(path),
            file_type: ft,
            ext: "html".to_string(),
            backend: match ft {
                FileType::WeScene => Backend::LinuxWallpaperEngine,
                FileType::Image => Backend::Awww,
                FileType::Gif => Backend::Awww,
                FileType::Video => Backend::Mpvpaper,
                FileType::WeWeb | FileType::WeApplication => Backend::Unsupported,
            },
            size: 1024,
            mtime: 1000,
            resolution: String::new(),
            project: None,
        }
    }

    #[test]
    fn filter_we_scene_shows_only_scene() {
        let entries = vec![
            entry_stub("/a/scene", FileType::WeScene),
            entry_stub("/b/web", FileType::WeWeb),
            entry_stub("/c/img.png", FileType::Image),
        ];
        let page = sort_filter_page(entries, "we_scene", "mtime", "", 0, 10);
        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].path, "/a/scene");
    }

    #[test]
    fn filter_we_web_shows_only_web() {
        let entries = vec![
            entry_stub("/a/scene", FileType::WeScene),
            entry_stub("/b/web", FileType::WeWeb),
            entry_stub("/c/img.png", FileType::Image),
        ];
        let page = sort_filter_page(entries, "we_web", "mtime", "", 0, 10);
        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].path, "/b/web");
    }

    #[test]
    fn filter_unsupported_shows_only_app() {
        let entries = vec![
            entry_stub("/a/app", FileType::WeApplication),
            entry_stub("/b/scene", FileType::WeScene),
            entry_stub("/c/img.png", FileType::Image),
        ];
        let page = sort_filter_page(entries, "unsupported", "mtime", "", 0, 10);
        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].path, "/a/app");
    }

    #[test]
    fn filter_all_shows_everything() {
        let entries = vec![
            entry_stub("/a/scene", FileType::WeScene),
            entry_stub("/b/web", FileType::WeWeb),
            entry_stub("/c/img.png", FileType::Image),
        ];
        let page = sort_filter_page(entries, "all", "mtime", "", 0, 10);
        assert_eq!(page.total, 3);
    }

    #[test]
    fn filter_we_backward_compat_matches_scene_and_web() {
        let entries = vec![
            entry_stub("/a/scene", FileType::WeScene),
            entry_stub("/b/web", FileType::WeWeb),
            entry_stub("/c/img.png", FileType::Image),
        ];
        let page = sort_filter_page(entries, "we", "mtime", "", 0, 10);
        assert_eq!(page.total, 2);
    }

    #[test]
    fn applyability_sort_we_web_and_unsupported_after_normal() {
        let make = |path: &str, ft: FileType, mtime: u64| WallpaperEntry {
            path: path.into(),
            file_type: ft,
            ext: "jpg".into(),
            backend: Backend::Awww,
            size: 100,
            mtime,
            resolution: "1920x1080".into(),
            project: None,
        };
        let entries = vec![
            make("d.web", FileType::WeWeb, 300),
            make("a.jpg", FileType::Image, 200),
            make("e.app", FileType::WeApplication, 400),
            make("b.gif", FileType::Gif, 100),
        ];
        let result = sort_filter_page(entries, "all", "mtime", "", 0, 10);
        let types: Vec<String> = result.items.iter().map(|i| i.file_type.clone()).collect();
        let normal_idx: Vec<usize> = types
            .iter()
            .enumerate()
            .filter(|(_, t)| *t == "image" || *t == "gif")
            .map(|(i, _)| i)
            .collect();
        let we_web_idx = types.iter().position(|t| t == "we_web").unwrap();
        let unsup_idx = types.iter().position(|t| t == "unsupported").unwrap();
        assert!(
            normal_idx.iter().all(|&i| i < we_web_idx),
            "normal before we_web"
        );
        assert!(
            normal_idx.iter().all(|&i| i < unsup_idx),
            "normal before unsupported"
        );
    }
}
