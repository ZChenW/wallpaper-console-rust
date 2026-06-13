mod common;

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use common::*;
use rusqlite::Connection;
use wc_core::types::{Backend, FileType, WallpaperEntry, WallpaperProject};

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

#[tauri::command]
pub async fn status() -> Result<StatusDto, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let s = storage()?;
        Ok(StatusDto {
            config_dir: s.cd.path.to_string_lossy().to_string(),
            current: s
                .current_read()
                .map_err(|e| e.to_string())?
                .unwrap_or_default(),
            last_backend: s
                .last_backend_read()
                .map_err(|e| e.to_string())?
                .unwrap_or_default(),
            source_count: s.sources_list().map_err(|e| e.to_string())?.len(),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn linux_wallpaperengine_status() -> Result<BackendStatusDto, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let s = storage()?;
        let st = wc_backend::linux_wallpaperengine::status(
            &wc_backend::linux_wallpaperengine::LinuxWallpaperEngineConfig::from_storage(&s),
        );
        Ok(BackendStatusDto {
            available: st.available,
            path: st.path,
            message: st.message,
            detail: st.detail,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn web_wallpaper_status() -> Result<BackendStatusDto, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let s = storage()?;
        let st = wc_backend::web_wallpaper::status(
            &wc_backend::web_wallpaper::WebWallpaperConfig::from_storage(&s),
        );
        Ok(BackendStatusDto {
            available: st.available,
            path: st.path,
            message: st.message,
            detail: st.detail,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn web_renderer_status() -> Result<BackendStatusDto, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let s = storage()?;
        let st = wc_backend::web_renderer::status(
            &wc_backend::web_renderer::WebRendererConfig::from_storage(&s),
        );
        Ok(BackendStatusDto {
            available: st.available,
            path: st.path,
            message: st.message,
            detail: st.detail,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn apply(path: String) -> CommandResult {
    tauri::async_runtime::spawn_blocking(move || match storage() {
        Ok(s) => {
            let service = wc_app::AppService::from_config_dir(wc_core::ConfigDir {
                path: s.cd.path.clone(),
            });
            match service.apply(&path) {
                Ok(target) => {
                    if target.file_type == FileType::WeScene {
                        wc_storage::we_compat::clear_failure(&target.resolved_path).ok();
                    }
                    ok(format!("Applied: {}", target.resolved_path))
                }
                Err(err) => CommandResult {
                    success: false,
                    stdout: String::new(),
                    stderr: err.message.clone(),
                    exit_code: 1,
                    error: Some(CommandErrorDto {
                        kind: err.code,
                        message: err.message,
                        detail: err.detail,
                        recoverable: err.recoverable,
                        suggestion: err.suggestion,
                    }),
                },
            }
        }
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn open_web_preview(path: String) -> CommandResult {
    tauri::async_runtime::spawn_blocking(move || match storage() {
        Ok(s) => match wc_backend::web_wallpaper::preflight(&path, &s) {
            Ok(p) => match wc_backend::web_wallpaper::apply_preflighted(&s, &p) {
                Ok(()) => ok("Opened Web wallpaper preview."),
                Err(e) => fail(e.to_string()),
            },
            Err(e) => fail(e.to_string()),
        },
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn stop() -> CommandResult {
    tauri::async_runtime::spawn_blocking(|| match storage() {
        Ok(s) => match wc_backend::stop_all_backends(Some(&s)) {
            Ok(()) => ok("Stopped wallpaper backends."),
            Err(e) => fail(e.to_string()),
        },
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn restore() -> CommandResult {
    tauri::async_runtime::spawn_blocking(|| match storage() {
        Ok(s) => match wc_backend::restore(&s) {
            Ok(()) => ok("Restored wallpaper."),
            Err(e) => fail(e.to_string()),
        },
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn we_clear_backend_error(path: String) -> CommandResult {
    tauri::async_runtime::spawn_blocking(move || {
        match wc_storage::we_compat::clear_failure(&path) {
            Ok(()) => ok("Cleared backend error."),
            Err(e) => fail(e.to_string()),
        }
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn sources_list() -> Result<Vec<SourceDto>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let s = storage()?;
        let sources = s.sources_list().map_err(|e| e.to_string())?;
        Ok(sources
            .into_iter()
            .map(|path| SourceDto {
                exists: std::path::Path::new(&path).exists(),
                is_we: wc_scan::is_wallpaper_engine_source(&path),
                label: source_label(&path),
                path,
            })
            .collect())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn source_add(path: String) -> CommandResult {
    tauri::async_runtime::spawn_blocking(move || match storage() {
        Ok(s) => match s.sources_add(&path) {
            Ok(true) => ok("Source added."),
            Ok(false) => ok("Source already exists."),
            Err(e) => fail(e.to_string()),
        },
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn source_remove(path: String) -> CommandResult {
    tauri::async_runtime::spawn_blocking(move || match storage() {
        Ok(s) => match s.sources_remove(&path) {
            Ok(true) => ok("Source removed."),
            Ok(false) => ok("Source was not configured."),
            Err(e) => fail(e.to_string()),
        },
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn validate_sources() -> CommandResult {
    tauri::async_runtime::spawn_blocking(|| match storage() {
        Ok(s) => match s.sources_list() {
            Ok(src) => {
                let missing = src
                    .iter()
                    .filter(|p| !std::path::Path::new(p).exists())
                    .count();
                if missing == 0 {
                    ok("All sources are valid.")
                } else {
                    fail(format!("{} source(s) are missing.", missing))
                }
            }
            Err(e) => fail(e.to_string()),
        },
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn remove_missing_sources() -> CommandResult {
    tauri::async_runtime::spawn_blocking(|| match storage() {
        Ok(s) => match s.sources_list() {
            Ok(src) => {
                let mut removed = 0;
                for path in src {
                    if !std::path::Path::new(&path).exists()
                        && s.sources_remove(&path).unwrap_or(false)
                    {
                        removed += 1;
                    }
                }
                ok(format!("Removed {} missing source(s).", removed))
            }
            Err(e) => fail(e.to_string()),
        },
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn scan_steam_workshop() -> CommandResult {
    rescan().await
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
        "chromium-web" => Backend::ChromiumWeb,
        "webkit-layer-shell" => Backend::WebKitLayerShell,
        _ => Backend::Unsupported,
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
        "name" => entries.sort_by(|a, b| a.filename().cmp(b.filename())),
        "size" => entries.sort_by(|a, b| b.size.cmp(&a.size).then(a.path.cmp(&b.path))),
        _ => entries.sort_by(|a, b| b.mtime.cmp(&a.mtime).then(a.path.cmp(&b.path))),
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

#[tauri::command]
pub async fn thumbnail_for(path: String) -> Result<ThumbnailDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let s = storage()?;
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

#[tauri::command]
pub async fn open_path(path: String) -> CommandResult {
    tauri::async_runtime::spawn_blocking(move || {
        match std::process::Command::new("xdg-open").arg(&path).spawn() {
            Ok(_) => ok("Opened path."),
            Err(e) => fail(e.to_string()),
        }
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn reveal_in_file_manager(path: String) -> CommandResult {
    open_path(
        std::path::Path::new(&path)
            .parent()
            .unwrap_or_else(|| std::path::Path::new(&path))
            .to_string_lossy()
            .to_string(),
    )
    .await
}

#[tauri::command]
pub async fn browse_directory(app: tauri::AppHandle) -> Result<String, String> {
    let _ = app;
    Err("Directory picker is not available in this build.".into())
}

#[tauri::command]
pub async fn config_get(key: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let s = storage()?;
        Ok(s.config_get(&key, ""))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn config_set(key: String, value: String) -> CommandResult {
    tauri::async_runtime::spawn_blocking(move || match storage() {
        Ok(s) => match s.config_set(&key, &value) {
            Ok(()) => ok(format!("{} = {}", key, value)),
            Err(e) => fail(e.to_string()),
        },
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn migrate_to_sqlite() -> CommandResult {
    tauri::async_runtime::spawn_blocking(|| match storage() {
        Ok(s) => match wc_storage::sqlite::migrate_to_sqlite(&s.cd) {
            Ok(()) => ok("Migrated to SQLite."),
            Err(e) => fail(e.to_string()),
        },
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn sqlite_verify() -> CommandResult {
    tauri::async_runtime::spawn_blocking(|| match storage() {
        Ok(s) => match wc_storage::sqlite::verify(&s.cd) {
            Ok(wc_storage::sqlite::VerifyResult::Ok) => ok("VERIFY OK"),
            Ok(wc_storage::sqlite::VerifyResult::OkWithWarnings(warnings)) => {
                ok(format!("VERIFY OK WITH WARNINGS\n{}", warnings.join("\n")))
            }
            Ok(wc_storage::sqlite::VerifyResult::Failed(errors)) => fail(format!(
                "VERIFY FAILED: {} mismatch(es) found: {}",
                errors.len(),
                errors.join(", ")
            )),
            Err(e) => fail(e.to_string()),
        },
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn sqlite_resync() -> CommandResult {
    tauri::async_runtime::spawn_blocking(|| match storage() {
        Ok(s) => match wc_storage::sqlite::resync(&s.cd) {
            Ok(()) => ok("Resync complete."),
            Err(e) => fail(e.to_string()),
        },
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn sqlite_backup() -> CommandResult {
    tauri::async_runtime::spawn_blocking(|| match storage() {
        Ok(s) => match wc_storage::sqlite::backup(&s.cd) {
            Ok(path) => ok(path),
            Err(e) => fail(e.to_string()),
        },
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn sqlite_restore(path: String) -> CommandResult {
    tauri::async_runtime::spawn_blocking(move || match storage() {
        Ok(s) => match wc_storage::sqlite::restore(&s.cd, &PathBuf::from(path)) {
            Ok(()) => ok("Restore complete."),
            Err(e) => fail(e.to_string()),
        },
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn sqlite_export_flat() -> CommandResult {
    tauri::async_runtime::spawn_blocking(|| match storage() {
        Ok(s) => match wc_storage::sqlite::export_flat(&s.cd) {
            Ok(()) => ok("Export complete."),
            Err(e) => fail(e.to_string()),
        },
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn export_diagnostics() -> CommandResult {
    tauri::async_runtime::spawn_blocking(|| match storage() {
        Ok(s) => {
            let dir = s.cd.path.join("diagnostics");
            if let Err(e) = std::fs::create_dir_all(&dir) {
                return fail(e.to_string());
            }
            let path = dir.join(format!(
                "wallpaper-console-diagnostics-{}.txt",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
            ));
            let current = s.current_read().unwrap_or_default().unwrap_or_default();
            let content = format!(
                "wallpaper-console diagnostics\nconfig_dir={}\ncurrent={}\nsources={}\n",
                s.cd.path
                    .file_name()
                    .map(|n| n.to_string_lossy())
                    .unwrap_or_default(),
                std::path::Path::new(&current)
                    .file_name()
                    .map(|n| n.to_string_lossy())
                    .unwrap_or_default(),
                s.sources_list().unwrap_or_default().len()
            );
            match std::fs::write(&path, content) {
                Ok(()) => ok(path.to_string_lossy().to_string()),
                Err(e) => fail(e.to_string()),
            }
        }
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}
