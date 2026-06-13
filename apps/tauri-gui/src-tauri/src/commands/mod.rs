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
        let config =
            wc_backend::linux_wallpaperengine::LinuxWallpaperEngineConfig::from_storage(&s);
        let st = wc_backend::linux_wallpaperengine::status(&config);
        let mut detail = st.detail;
        if config.target_mode == "auto" {
            let wayland = std::env::var("WAYLAND_DISPLAY").is_ok()
                || std::env::var("XDG_SESSION_TYPE").map(|v| v == "wayland").unwrap_or(false);
            if wayland {
                let warning = "⚠ Wayland detected: recommend setting target_mode=screen-root and target=<your output name> for stable scene rendering.";
                detail = Some(match detail {
                    Some(d) => format!("{}\n{}", d, warning),
                    None => warning.to_string(),
                });
            }
        }
        Ok(BackendStatusDto {
            available: st.available,
            path: st.path,
            message: st.message,
            detail,
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
pub async fn we_debug_info() -> Result<WeDebugInfoDto, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let s = storage()?;
        let log_path = s.cd.path.join("linux-wallpaperengine-last.log");
        Ok(WeDebugInfoDto {
            last_command_line: s.config_get("lwe_last_command_line", ""),
            last_target_config: s.config_get("lwe_last_target_config", ""),
            last_stderr: s.config_get("lwe_last_stderr", ""),
            last_exit_status: s.config_get("lwe_last_exit_status", ""),
            log_path: log_path.to_string_lossy().to_string(),
        })
    })
    .await
    .map_err(|e| e.to_string())?
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

/// Resolve a path for opening: directories are opened directly, files reveal their parent,
/// except WE project directories (containing project.json) which open directly.
pub(crate) fn open_location_target(path: &str) -> Result<std::path::PathBuf, String> {
    let p = std::path::Path::new(path);
    if !p.exists() {
        return Err(format!("Path does not exist: {}", path));
    }
    if p.is_dir() {
        return Ok(p.to_path_buf());
    }
    if p.is_file() {
        return Ok(p.parent().unwrap_or(p).to_path_buf());
    }
    Err(format!("Not a regular file or directory: {}", path))
}

fn terminal_spawn_command(name: &str, inner: &[String]) -> Option<(String, Vec<String>)> {
    let args: Vec<String> = match name {
        "kitty" => {
            let mut a = vec!["--".to_string()];
            a.extend_from_slice(inner);
            a
        }
        "alacritty" => {
            let mut a = vec!["-e".to_string()];
            a.extend_from_slice(inner);
            a
        }
        "foot" => inner.to_vec(),
        "wezterm" | "wezterm-204" => {
            let mut a = vec!["start".to_string(), "--".to_string()];
            a.extend_from_slice(inner);
            a
        }
        n if n.starts_with("wezterm") => {
            let mut a = vec!["start".to_string(), "--".to_string()];
            a.extend_from_slice(inner);
            a
        }
        "gnome-terminal" => {
            let mut a = vec!["--".to_string()];
            a.extend_from_slice(inner);
            a
        }
        "konsole" => {
            let mut a = vec!["-e".to_string()];
            a.extend_from_slice(inner);
            a
        }
        _ => return None,
    };
    Some((name.to_string(), args))
}

/// Return candidate terminal executable names for the current environment.
/// $TERMINAL is tried first, then well-known terminals, deduplicated.
fn terminal_candidates(term_var: Option<&str>) -> Vec<&str> {
    let mut seen = std::collections::HashSet::new();
    let mut list = Vec::new();
    let fallback = [
        "kitty",
        "alacritty",
        "foot",
        "wezterm",
        "gnome-terminal",
        "konsole",
    ];
    if let Some(term) = term_var {
        let exe = term.split_whitespace().next().unwrap_or("");
        if !exe.is_empty() && seen.insert(exe) {
            list.push(exe);
        }
    }
    for &f in &fallback {
        if seen.insert(f) {
            list.push(f);
        }
    }
    list
}

/// Split a custom command string, replacing `{path}` with target or appending it.
fn custom_command_parts(custom_cmd: &str, target: &str) -> Result<Vec<String>, String> {
    let trimmed = custom_cmd.trim();
    if trimmed.is_empty() {
        return Err("Custom command is empty.".into());
    }
    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    let mut has_placeholder = false;
    let mut result = Vec::new();
    for p in &parts {
        if *p == "{path}" {
            has_placeholder = true;
            result.push(target.to_string());
        } else {
            result.push(p.to_string());
        }
    }
    if !has_placeholder {
        result.push(target.to_string());
    }
    Ok(result)
}

/// Return candidate executable names for a given file manager config value.
fn file_manager_candidates(file_mgr: &str) -> Vec<&str> {
    match file_mgr {
        "auto" => vec!["nautilus", "dolphin", "thunar", "nemo", "pcmanfm"],
        "custom" | "" => vec![],
        other => vec![other],
    }
}

fn open_with_file_manager(
    target: &std::path::Path,
    file_mgr: &str,
    custom_cmd: &str,
) -> Result<String, String> {
    let target_str = target.to_string_lossy().to_string();

    if file_mgr == "custom" {
        let parts = custom_command_parts(custom_cmd, &target_str)?;
        let prog = parts[0].clone();
        let _status = std::process::Command::new(&prog)
            .args(&parts[1..])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| format!("Failed to launch {}: {}", prog, e))?;
        return Ok(format!("Opened with custom: {}", target_str));
    }

    let candidates = file_manager_candidates(file_mgr);
    if candidates.is_empty() {
        return Err(
            "No file manager configured. Choose one in Settings or set a custom command.".into(),
        );
    }
    let candidate_names = candidates.join(", ");
    for c in &candidates {
        let status = std::process::Command::new(c)
            .arg(&target_str)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        match status {
            Ok(_) => return Ok(format!("Opened with {}: {}", c, target_str)),
            Err(_) => continue,
        }
    }
    Err(format!(
        "No file manager found. Tried: {}. Install one or set a custom command in Settings.",
        candidate_names
    ))
}

fn terminal_file_manager_command(
    tui_mgr: &str,
    custom_cmd: &str,
    target: &str,
) -> Result<Vec<String>, String> {
    match tui_mgr {
        "yazi" => Ok(vec!["yazi".to_string(), target.to_string()]),
        "custom" => custom_command_parts(custom_cmd, target),
        _ => Ok(vec!["yazi".to_string(), target.to_string()]),
    }
}

fn try_terminal_spawn_candidates(
    candidates: &[&str],
    tui_cmd: &[String],
    tui_label: &str,
    target_str: &str,
    spawn_fn: &mut dyn FnMut(&str, &[String]) -> bool,
) -> Result<String, String> {
    let mut attempted = Vec::new();

    for c in candidates {
        let Some((prog, args)) = terminal_spawn_command(c, tui_cmd) else {
            continue;
        };
        attempted.push(format!("{} {:?}", prog, args));
        if spawn_fn(&prog, &args) {
            return Ok(format!(
                "Opened with {} in {}: {}",
                tui_label, c, target_str
            ));
        }
    }
    Err(format!(
        "No terminal emulator could be launched. Tried: {}. Install a terminal emulator or set $TERMINAL.",
        attempted.join(", ")
    ))
}

fn open_terminal_file_manager(
    target: &std::path::Path,
    tui_mgr: &str,
    custom_cmd: &str,
) -> Result<String, String> {
    let target_str = target.to_string_lossy().to_string();
    let term = std::env::var("TERMINAL").ok();
    let terminal_candidates = terminal_candidates(term.as_deref());
    let tui_cmd = terminal_file_manager_command(tui_mgr, custom_cmd, &target_str)?;

    try_terminal_spawn_candidates(
        &terminal_candidates,
        &tui_cmd,
        tui_mgr,
        &target_str,
        &mut |prog: &str, args: &[String]| {
            std::process::Command::new(prog)
                .args(args)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .is_ok()
        },
    )
}

#[tauri::command]
pub async fn open_project_location(path: String, mode: Option<String>) -> CommandResult {
    tauri::async_runtime::spawn_blocking(move || {
        let target = match open_location_target(&path) {
            Ok(t) => t,
            Err(e) => return fail(e),
        };
        let s = match storage() {
            Ok(s) => s,
            Err(e) => return fail(e),
        };
        let mode = mode.unwrap_or_else(|| s.config_get("open_project_location_mode", "ask"));
        if mode == "ask" {
            return fail(
                "Open location mode is ask-on-first-use. \
                 Choose File Manager or Terminal File Manager in Settings first.",
            );
        }
        if mode == "terminal" {
            let tui_mgr = s.config_get("gui_terminal_file_manager", "yazi");
            let custom = s.config_get("gui_terminal_file_manager_custom", "");
            match open_terminal_file_manager(&target, &tui_mgr, &custom) {
                Ok(msg) => ok(msg),
                Err(e) => fail(e),
            }
        } else {
            let file_mgr = s.config_get("gui_file_manager", "auto");
            let custom = s.config_get("gui_file_manager_custom", "");
            match open_with_file_manager(&target, &file_mgr, &custom) {
                Ok(msg) => ok(msg),
                Err(e) => fail(e),
            }
        }
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn open_path(path: String) -> CommandResult {
    tauri::async_runtime::spawn_blocking(move || {
        let p = std::path::Path::new(&path);
        // If it's a directory, open it directly; otherwise reveal parent.
        let target = if p.is_dir() {
            p.to_path_buf()
        } else {
            p.parent().unwrap_or(p).to_path_buf()
        };
        match std::process::Command::new("xdg-open")
            .arg(target.to_string_lossy().as_ref())
            .spawn()
        {
            Ok(_) => ok("Opened path."),
            Err(e) => fail(e.to_string()),
        }
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn reveal_in_file_manager(path: String) -> CommandResult {
    tauri::async_runtime::spawn_blocking(move || {
        let p = std::path::Path::new(&path);
        let target = if p.is_dir() {
            p.to_path_buf()
        } else {
            p.parent().unwrap_or(p).to_path_buf()
        };
        let s = match storage() {
            Ok(s) => s,
            Err(e) => return fail(e),
        };
        let file_mgr = s.config_get("gui_file_manager", "auto");
        let custom = s.config_get("gui_file_manager_custom", "");
        match open_with_file_manager(&target, &file_mgr, &custom) {
            Ok(msg) => ok(msg),
            Err(e) => fail(e),
        }
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
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
        use wc_core::types::{Backend, FileType, WallpaperEntry};
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

    #[test]
    fn open_location_target_dir_returns_self() {
        let tmp = tempfile::tempdir().unwrap();
        let result = super::open_location_target(&tmp.path().to_string_lossy()).unwrap();
        assert_eq!(result, tmp.path());
    }

    #[test]
    fn open_location_target_file_returns_parent() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("test.txt");
        std::fs::write(&f, b"hello").unwrap();
        let result = super::open_location_target(&f.to_string_lossy()).unwrap();
        assert_eq!(result, tmp.path());
    }

    #[test]
    fn open_location_target_regular_wallpaper_file_returns_containing_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("wallpapers");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("wall.png");
        std::fs::write(&file, b"png").unwrap();

        let result = super::open_location_target(&file.to_string_lossy()).unwrap();
        assert_eq!(result, dir);
    }

    #[test]
    fn file_manager_auto_candidates_excludes_xdg_open_and_terminals() {
        let candidates = super::file_manager_candidates("auto");
        assert!(candidates.contains(&"nautilus"));
        assert!(candidates.contains(&"dolphin"));
        assert!(candidates.contains(&"thunar"));
        assert!(candidates.contains(&"nemo"));
        assert!(candidates.contains(&"pcmanfm"));
        assert!(!candidates.contains(&"xdg-open"));
        assert!(!candidates.contains(&"yazi"));
        assert!(!candidates.contains(&"kitty"));
        assert!(!candidates.contains(&"alacritty"));
        assert!(!candidates.contains(&"foot"));
        assert!(!candidates.contains(&"konsole"));
        assert!(!candidates.contains(&"gnome-terminal"));
        assert_eq!(candidates.len(), 5);
    }

    #[test]
    fn file_manager_specific_returns_single_candidate() {
        let candidates = super::file_manager_candidates("nautilus");
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0], "nautilus");
    }

    #[test]
    fn file_manager_custom_returns_empty() {
        let candidates = super::file_manager_candidates("custom");
        assert!(candidates.is_empty());
    }

    #[test]
    fn custom_command_parts_appends_target() {
        let parts = super::custom_command_parts("nautilus", "/tmp/walls").unwrap();
        assert_eq!(parts, vec!["nautilus", "/tmp/walls"]);
    }

    #[test]
    fn custom_command_parts_replaces_path_placeholder() {
        let parts = super::custom_command_parts("nautilus {path}", "/tmp/walls").unwrap();
        assert_eq!(parts, vec!["nautilus", "/tmp/walls"]);
    }

    #[test]
    fn custom_command_parts_empty_errors() {
        assert!(super::custom_command_parts("", "/tmp/walls").is_err());
        assert!(super::custom_command_parts("   ", "/tmp/walls").is_err());
    }

    #[test]
    fn terminal_candidates_keeps_env_and_fallbacks() {
        let candidates = super::terminal_candidates(Some("ghostty --foo"));
        assert_eq!(candidates[0], "ghostty");
        assert!(candidates.contains(&"kitty"));
        assert!(candidates.contains(&"alacritty"));
        assert!(candidates.contains(&"konsole"));
        assert_eq!(candidates.len(), 7); // ghostty + 6 fallbacks
    }

    #[test]
    fn terminal_candidates_deduplicates_env() {
        let candidates = super::terminal_candidates(Some("kitty"));
        let kitty_count = candidates.iter().filter(|&&c| c == "kitty").count();
        assert_eq!(kitty_count, 1);
        assert_eq!(candidates.len(), 6);
    }

    #[test]
    fn terminal_spawn_command_kitty() {
        let inner: Vec<String> = vec!["yazi".into(), "/tmp/walls".into()];
        let (prog, args) = super::terminal_spawn_command("kitty", &inner).unwrap();
        assert_eq!(prog, "kitty");
        assert_eq!(args, vec!["--", "yazi", "/tmp/walls"]);
    }

    #[test]
    fn terminal_spawn_command_alacritty() {
        let inner: Vec<String> = vec!["yazi".into(), "/tmp/walls".into()];
        let (_prog, args) = super::terminal_spawn_command("alacritty", &inner).unwrap();
        assert_eq!(args, vec!["-e", "yazi", "/tmp/walls"]);
    }

    #[test]
    fn terminal_spawn_command_foot() {
        let inner: Vec<String> = vec!["yazi".into(), "/tmp/walls".into()];
        let (_prog, args) = super::terminal_spawn_command("foot", &inner).unwrap();
        assert_eq!(args, vec!["yazi", "/tmp/walls"]);
    }

    #[test]
    fn terminal_spawn_command_wezterm() {
        let inner: Vec<String> = vec!["yazi".into(), "/tmp/walls".into()];
        let (_prog, args) = super::terminal_spawn_command("wezterm", &inner).unwrap();
        assert_eq!(args, vec!["start", "--", "yazi", "/tmp/walls"]);
    }

    #[test]
    fn terminal_spawn_command_gnome_terminal() {
        let inner: Vec<String> = vec!["yazi".into(), "/tmp/walls".into()];
        let (_prog, args) = super::terminal_spawn_command("gnome-terminal", &inner).unwrap();
        assert_eq!(args, vec!["--", "yazi", "/tmp/walls"]);
    }

    #[test]
    fn terminal_spawn_command_konsole() {
        let inner: Vec<String> = vec!["yazi".into(), "/tmp/walls".into()];
        let (_prog, args) = super::terminal_spawn_command("konsole", &inner).unwrap();
        assert_eq!(args, vec!["-e", "yazi", "/tmp/walls"]);
    }

    #[test]
    fn terminal_spawn_command_unknown_returns_none() {
        let inner: Vec<String> = vec!["yazi".into()];
        assert!(super::terminal_spawn_command("unknown", &inner).is_none());
    }

    #[test]
    fn terminal_file_manager_yazi_includes_target() {
        let cmd = super::terminal_file_manager_command("yazi", "", "/tmp/walls").unwrap();
        assert_eq!(cmd, vec!["yazi", "/tmp/walls"]);
    }

    #[test]
    fn open_with_custom_cmd_empty_errors() {
        let target = std::path::Path::new("/tmp/test");
        let err = super::open_with_file_manager(target, "custom", "");
        assert!(err.is_err());
        assert!(err.unwrap_err().contains("empty"));
    }

    #[test]
    fn try_terminal_spawn_first_fails_second_succeeds() {
        let candidates = vec!["alacritty", "foot"];
        let tui_cmd: Vec<String> = vec!["yazi".into(), "/tmp".into()];
        let mut tries: Vec<String> = Vec::new();
        let result = super::try_terminal_spawn_candidates(
            &candidates,
            &tui_cmd,
            "yazi",
            "/tmp",
            &mut |prog: &str, _: &[String]| {
                tries.push(prog.to_string());
                prog != "alacritty"
            },
        );
        assert!(result.is_ok());
        assert_eq!(tries, vec!["alacritty", "foot"]);
    }

    #[test]
    fn try_terminal_spawn_all_fail_reports_attempted() {
        let candidates = vec!["alacritty", "kitty"];
        let tui_cmd: Vec<String> = vec!["yazi".into(), "/tmp".into()];
        let result = super::try_terminal_spawn_candidates(
            &candidates,
            &tui_cmd,
            "yazi",
            "/tmp",
            &mut |_, _| false,
        );
        let err = result.unwrap_err();
        assert!(err.contains("Tried:"));
        assert!(err.contains("alacritty"));
        assert!(err.contains("kitty"));
    }

    #[test]
    fn try_terminal_spawn_skips_unknown_and_continues() {
        // unknown is not recognised by terminal_spawn_command so gets skipped.
        // kitty is recognised and spawn_fn returns true (success).
        let candidates = vec!["unknown", "kitty"];
        let tui_cmd: Vec<String> = vec!["yazi".into(), "/tmp".into()];
        let mut tries: Vec<String> = Vec::new();
        let result = super::try_terminal_spawn_candidates(
            &candidates,
            &tui_cmd,
            "yazi",
            "/tmp",
            &mut |prog: &str, _: &[String]| {
                tries.push(prog.to_string());
                true
            },
        );
        assert!(result.is_ok());
        // unknown skipped, kitty tried and succeeded
        assert_eq!(tries, vec!["kitty"]);
    }
}
