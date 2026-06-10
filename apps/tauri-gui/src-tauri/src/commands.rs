use std::path::{Path, PathBuf};
use std::process::Command;

use rusqlite::params;
use serde::{Deserialize, Serialize};
use wc_core::config::ConfigDir;
use wc_core::formats;
use wc_core::types::{Backend, FileType};
use wc_storage::StorageApi;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandResult {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusDto {
    pub config_dir: String,
    pub current: String,
    pub last_backend: String,
    pub source_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WallpaperDto {
    pub path: String,
    #[serde(rename = "type")]
    pub file_type: String,
    pub ext: String,
    pub backend: String,
    pub size: i64,
    pub mtime: i64,
    pub resolution: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LibraryPageDto {
    pub total: usize,
    pub items: Vec<WallpaperDto>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LibraryCountDto {
    pub total: usize,
    pub images: usize,
    pub gifs: usize,
    pub videos: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryDto {
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceDto {
    pub path: String,
    pub exists: bool,
    #[serde(rename = "isWE")]
    pub is_we: bool,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbnailDto {
    pub path: String,
    pub thumbnail: Option<String>,
    pub cache_hit: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ThumbnailCacheDto {
    pub dir: String,
    pub size: String,
    pub entries: usize,
}

fn ok(stdout: impl Into<String>) -> CommandResult {
    CommandResult {
        success: true,
        stdout: stdout.into(),
        stderr: String::new(),
        exit_code: 0,
    }
}

fn fail(err: impl ToString) -> CommandResult {
    CommandResult {
        success: false,
        stdout: String::new(),
        stderr: err.to_string(),
        exit_code: 1,
    }
}

fn storage() -> Result<StorageApi, String> {
    let cd = ConfigDir::new().map_err(|e| e.to_string())?;
    cd.init().map_err(|e| e.to_string())?;
    Ok(StorageApi::new(cd))
}

fn backend_for_type(storage: &StorageApi, file_type: FileType) -> Backend {
    match file_type {
        FileType::Image => match storage.config_get("image_backend", "awww").as_str() {
            "mpvpaper" => Backend::Mpvpaper,
            _ => Backend::Awww,
        },
        FileType::Gif => match storage.config_get("gif_backend", "awww").as_str() {
            "mpvpaper" => Backend::Mpvpaper,
            _ => Backend::Awww,
        },
        FileType::Video => match storage.config_get("video_backend", "mpvpaper").as_str() {
            "awww" => Backend::Awww,
            _ => Backend::Mpvpaper,
        },
    }
}

#[tauri::command]
pub fn status() -> Result<StatusDto, String> {
    let s = storage()?;
    Ok(StatusDto {
        config_dir: s.cd.path.to_string_lossy().to_string(),
        current: s
            .current_read()
            .map_err(|e| e.to_string())?
            .unwrap_or_else(|| "(none)".into()),
        last_backend: s
            .last_backend_read()
            .map_err(|e| e.to_string())?
            .unwrap_or_else(|| "(none)".into()),
        source_count: s.sources_list().map_err(|e| e.to_string())?.len(),
    })
}

#[tauri::command]
pub fn apply(path: String) -> CommandResult {
    let result = (|| -> Result<String, String> {
        let s = storage()?;
        if !Path::new(&path).is_file() {
            return Err(format!("not a regular file: {}", path));
        }
        let ext =
            formats::get_extension(&path).ok_or_else(|| format!("unsupported file: {}", path))?;
        let (file_type, _) = formats::classify_extension(&ext)
            .ok_or_else(|| format!("unsupported file: {}", path))?;
        let backend = backend_for_type(&s, file_type);
        wc_backend::apply_wallpaper(&s, &path, backend).map_err(|e| e.to_string())?;
        Ok(format!("Applied: {}", path))
    })();
    match result {
        Ok(message) => ok(message),
        Err(err) => fail(err),
    }
}

#[tauri::command]
pub fn stop() -> CommandResult {
    match wc_backend::stop_all_backends() {
        Ok(()) => ok("All wallpaper backends stopped."),
        Err(err) => fail(err),
    }
}

#[tauri::command]
pub fn restore() -> CommandResult {
    let result = (|| -> Result<(), String> {
        let s = storage()?;
        wc_backend::restore(&s).map_err(|e| e.to_string())
    })();
    match result {
        Ok(()) => ok("Wallpaper restored."),
        Err(err) => fail(err),
    }
}

#[tauri::command]
pub fn config_get(key: String) -> Result<String, String> {
    let s = storage()?;
    Ok(s.config_get(&key, ""))
}

#[tauri::command]
pub fn config_set(key: String, value: String) -> CommandResult {
    match storage().and_then(|s| s.config_set(&key, &value).map_err(|e| e.to_string())) {
        Ok(()) => ok(format!("{} = {}", key, value)),
        Err(err) => fail(err),
    }
}

#[tauri::command]
pub fn sources_list() -> Result<Vec<SourceDto>, String> {
    let s = storage()?;
    let sources = s.sources_list().map_err(|e| e.to_string())?;
    Ok(sources
        .into_iter()
        .map(|path| {
            let exists = Path::new(&path).is_dir();
            SourceDto {
                label: source_label(&path),
                is_we: path.contains("/steamapps/workshop/content/431960"),
                path,
                exists,
            }
        })
        .collect())
}

#[tauri::command]
pub fn source_add(path: String) -> CommandResult {
    let result = storage().and_then(|s| {
        let canonical = std::fs::canonicalize(&path)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or(path);
        s.sources_add(&canonical)
            .map(|_| format!("Added source: {}", canonical))
            .map_err(|e| e.to_string())
    });
    match result {
        Ok(message) => ok(message),
        Err(err) => fail(err),
    }
}

#[tauri::command]
pub fn source_remove(path: String) -> CommandResult {
    let result = storage().and_then(|s| {
        s.sources_remove(&path)
            .map_err(|e| e.to_string())
            .and_then(|removed| {
                if removed {
                    Ok(format!("Removed source: {}", path))
                } else {
                    Err(format!("source not found: {}", path))
                }
            })
    });
    match result {
        Ok(message) => ok(message),
        Err(err) => fail(err),
    }
}

#[tauri::command]
pub fn validate_sources() -> CommandResult {
    match sources_list() {
        Ok(sources) => {
            let mut out = String::new();
            for src in sources {
                out.push_str(if src.exists { "OK\t" } else { "MISSING\t" });
                out.push_str(&src.path);
                out.push('\n');
            }
            ok(out)
        }
        Err(err) => fail(err),
    }
}

#[tauri::command]
pub fn remove_missing_sources() -> CommandResult {
    let result = storage().and_then(|s| {
        let mut removed = 0usize;
        for src in s.sources_list().map_err(|e| e.to_string())? {
            if !Path::new(&src).is_dir() && s.sources_remove(&src).map_err(|e| e.to_string())? {
                removed += 1;
            }
        }
        Ok(format!("Removed {} missing sources.", removed))
    });
    match result {
        Ok(message) => ok(message),
        Err(err) => fail(err),
    }
}

#[tauri::command]
pub fn scan_steam_workshop() -> CommandResult {
    let result = storage().and_then(|s| {
        let home = std::env::var("HOME").unwrap_or_default();
        let bases = [
            format!("{}/.local/share/Steam", home),
            format!("{}/.steam/steam", home),
            format!(
                "{}/.var/app/com.valvesoftware.Steam/.local/share/Steam",
                home
            ),
            format!("{}/.var/app/com.valvesoftware.Steam/.steam/steam", home),
        ];
        let mut added = 0usize;
        let mut seen = std::collections::HashSet::new();
        for base in bases {
            let root = Path::new(&base).join("steamapps/workshop/content/431960");
            if !root.is_dir() {
                continue;
            }
            for entry in std::fs::read_dir(root).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
                    let canonical = std::fs::canonicalize(entry.path())
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|_| entry.path().to_string_lossy().to_string());
                    if seen.insert(canonical.clone())
                        && s.sources_add(&canonical).map_err(|e| e.to_string())?
                    {
                        added += 1;
                    }
                }
            }
        }
        Ok(format!("Added {} Steam Workshop sources.", added))
    });
    match result {
        Ok(message) => ok(message),
        Err(err) => fail(err),
    }
}

#[tauri::command]
pub fn favorites_list() -> Result<Vec<String>, String> {
    storage()?.favorites_list().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn favorite_add(path: String) -> CommandResult {
    match storage().and_then(|s| s.favorites_add(&path).map_err(|e| e.to_string())) {
        Ok(_) => ok(format!("Added favorite: {}", path)),
        Err(err) => fail(err),
    }
}

#[tauri::command]
pub fn favorite_remove(path: String) -> CommandResult {
    match storage().and_then(|s| s.favorites_remove(&path).map_err(|e| e.to_string())) {
        Ok(()) => ok(format!("Removed favorite: {}", path)),
        Err(err) => fail(err),
    }
}

#[tauri::command]
pub fn history_list() -> Result<Vec<HistoryDto>, String> {
    Ok(storage()?
        .history_list()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|path| HistoryDto { path })
        .collect())
}

#[tauri::command]
pub fn history_clear() -> CommandResult {
    match storage().and_then(|s| s.history_clear().map_err(|e| e.to_string())) {
        Ok(()) => ok("History cleared."),
        Err(err) => fail(err),
    }
}

#[tauri::command]
pub fn library_count() -> Result<LibraryCountDto, String> {
    let s = storage()?;
    let entries = library_entries_from_tsv(&s)?;
    let mut counts = LibraryCountDto {
        total: 0,
        images: 0,
        gifs: 0,
        videos: 0,
    };
    for entry in entries {
        counts.total += 1;
        match entry.file_type.as_str() {
            "image" => counts.images += 1,
            "gif" => counts.gifs += 1,
            "video" => counts.videos += 1,
            _ => {}
        }
    }
    Ok(counts)
}

#[tauri::command]
pub fn library_list(source: String) -> Result<Vec<WallpaperDto>, String> {
    Ok(library_page(
        source,
        "all".into(),
        "name".into(),
        String::new(),
        0,
        usize::MAX,
    )?
    .items)
}

#[tauri::command]
pub fn library_page(
    source: String,
    filter: String,
    sort: String,
    search: String,
    offset: usize,
    limit: usize,
) -> Result<LibraryPageDto, String> {
    let s = storage()?;
    match source.as_str() {
        "sqlite" => library_page_sqlite(&s, &filter, &sort, &search, offset, limit),
        "tsv" => library_page_tsv(&s, &filter, &sort, &search, offset, limit),
        other => Err(format!("unknown library source: {}", other)),
    }
}

#[tauri::command]
pub fn rescan() -> CommandResult {
    let result = storage().and_then(|s| {
        let sources = s.sources_list().map_err(|e| e.to_string())?;
        wc_storage::sqlite::library_clear(&s.cd).ok();
        let files = wc_scan::scan_wallpapers(&sources);
        let mut rows = String::new();
        let mut count = 0usize;
        for path in files {
            if let Some(entry) = wc_scan::make_entry(&path) {
                wc_storage::sqlite::library_insert(
                    &s.cd,
                    &path,
                    entry.file_type.as_str(),
                    &entry.ext,
                    entry.backend.as_str(),
                    entry.size,
                    entry.mtime,
                    &entry.resolution,
                )
                .ok();
                rows.push_str(&format!(
                    "{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                    entry.file_type.as_str(),
                    entry.ext,
                    entry.backend.as_str(),
                    entry.size,
                    entry.mtime,
                    entry.resolution,
                    entry.path
                ));
                count += 1;
            }
        }
        std::fs::write(s.cd.library_tsv_path(), rows).map_err(|e| e.to_string())?;
        Ok(format!("Library rescanned: {} entries.", count))
    });
    match result {
        Ok(message) => ok(message),
        Err(err) => fail(err),
    }
}

#[tauri::command]
pub fn migrate_to_sqlite() -> CommandResult {
    match storage()
        .and_then(|s| wc_storage::sqlite::migrate_to_sqlite(&s.cd).map_err(|e| e.to_string()))
    {
        Ok(()) => ok("Migrated to SQLite."),
        Err(err) => fail(err),
    }
}

#[tauri::command]
pub fn sqlite_verify() -> CommandResult {
    match storage().and_then(|s| wc_storage::sqlite::verify(&s.cd).map_err(|e| e.to_string())) {
        Ok(()) => ok("VERIFY OK"),
        Err(err) => fail(err),
    }
}

#[tauri::command]
pub fn sqlite_resync() -> CommandResult {
    match storage().and_then(|s| wc_storage::sqlite::resync(&s.cd).map_err(|e| e.to_string())) {
        Ok(()) => ok("Resync complete."),
        Err(err) => fail(err),
    }
}

#[tauri::command]
pub fn sqlite_backup() -> CommandResult {
    match storage().and_then(|s| wc_storage::sqlite::backup(&s.cd).map_err(|e| e.to_string())) {
        Ok(path) => ok(path),
        Err(err) => fail(err),
    }
}

#[tauri::command]
pub fn sqlite_restore(path: String) -> CommandResult {
    match storage().and_then(|s| {
        wc_storage::sqlite::restore(&s.cd, &PathBuf::from(path)).map_err(|e| e.to_string())
    }) {
        Ok(()) => ok("Restore complete."),
        Err(err) => fail(err),
    }
}

#[tauri::command]
pub fn sqlite_export_flat() -> CommandResult {
    match storage().and_then(|s| wc_storage::sqlite::export_flat(&s.cd).map_err(|e| e.to_string()))
    {
        Ok(()) => ok("Export complete."),
        Err(err) => fail(err),
    }
}

#[tauri::command]
pub fn thumbnail_for(path: String) -> Result<ThumbnailDto, String> {
    let s = storage()?;
    let mut dto = ThumbnailDto {
        path: path.clone(),
        thumbnail: None,
        cache_hit: false,
    };
    let meta = match std::fs::metadata(&path) {
        Ok(meta) => meta,
        Err(_) => return Ok(dto),
    };
    let real = std::fs::canonicalize(&path).unwrap_or_else(|_| PathBuf::from(&path));
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let key = format!("{}:{}:{}", real.to_string_lossy(), mtime, meta.len());
    let hash = format!("{:x}", md5_hash(key.as_bytes()));
    let cache_dir = s.cd.gui_thumbnail_cache_dir();
    std::fs::create_dir_all(&cache_dir).map_err(|e| e.to_string())?;
    let thumb = cache_dir.join(format!("{}.webp", hash));
    if thumb.exists() {
        dto.thumbnail = Some(thumb.to_string_lossy().to_string());
        dto.cache_hit = true;
        return Ok(dto);
    }
    if generate_thumbnail(&path, &thumb).is_ok() {
        dto.thumbnail = Some(thumb.to_string_lossy().to_string());
    }
    Ok(dto)
}

#[tauri::command]
pub fn thumbnail_cache_status() -> Result<ThumbnailCacheDto, String> {
    let s = storage()?;
    let dir = s.cd.gui_thumbnail_cache_dir();
    let mut entries = 0usize;
    let mut size = 0u64;
    if let Ok(read_dir) = std::fs::read_dir(&dir) {
        for entry in read_dir.flatten() {
            entries += 1;
            size += entry.metadata().map(|m| m.len()).unwrap_or(0);
        }
    }
    Ok(ThumbnailCacheDto {
        dir: dir.to_string_lossy().to_string(),
        size: format_bytes(size),
        entries,
    })
}

#[tauri::command]
pub fn thumbnail_cache_clear() -> CommandResult {
    match storage().and_then(|s| {
        let dir = s.cd.gui_thumbnail_cache_dir();
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())
    }) {
        Ok(()) => ok("Thumbnail cache cleared."),
        Err(err) => fail(err),
    }
}

#[tauri::command]
pub fn open_path(path: String) -> CommandResult {
    run_external("xdg-open", &[path])
}

#[tauri::command]
pub fn reveal_in_file_manager(path: String) -> CommandResult {
    let reveal = if Path::new(&path).is_dir() {
        path
    } else {
        Path::new(&path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or(path)
    };
    run_external("xdg-open", &[reveal])
}

#[tauri::command]
pub fn browse_directory() -> Result<String, String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let candidates: Vec<(&str, Vec<String>)> = vec![
        (
            "zenity",
            vec![
                "--file-selection".into(),
                "--directory".into(),
                "--title=Select Wallpaper Directory".into(),
            ],
        ),
        ("kdialog", vec!["--getexistingdirectory".into(), home]),
        (
            "yad",
            vec![
                "--file-selection".into(),
                "--directory".into(),
                "--title=Select Wallpaper Directory".into(),
            ],
        ),
    ];
    for (program, args) in candidates {
        if Command::new(program).arg("--help").output().is_err() {
            continue;
        }
        let output = Command::new(program)
            .args(args)
            .output()
            .map_err(|e| e.to_string())?;
        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
        }
    }
    Err("no directory picker available (install zenity, kdialog, or yad)".into())
}

fn library_page_sqlite(
    s: &StorageApi,
    filter: &str,
    sort: &str,
    search: &str,
    offset: usize,
    limit: usize,
) -> Result<LibraryPageDto, String> {
    let db = s.cd.db_path();
    if !db.exists() {
        return Err("wallpapers.db not found. Run migrate-to-sqlite and rescan first.".into());
    }
    let conn = rusqlite::Connection::open(db).map_err(|e| e.to_string())?;
    let filter = validate_filter(filter)?;
    let order_by = match validate_sort(sort)? {
        "newest" => "mtime DESC, path ASC",
        "largest" => "size DESC, path ASC",
        "name" => "path ASC",
        _ => unreachable!(),
    };
    let where_clause =
        "WHERE (?1 = 'all' OR type = ?1) AND (?2 = '' OR lower(path) LIKE '%' || lower(?2) || '%')";
    let total: i64 = conn
        .query_row(
            &format!("SELECT COUNT(*) FROM wallpapers {}", where_clause),
            params![filter, search],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    let sql = format!(
        "SELECT path, type, ext, backend, size, mtime, resolution FROM wallpapers {} ORDER BY {} LIMIT ?3 OFFSET ?4",
        where_clause, order_by
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let items = stmt
        .query_map(
            params![filter, search, limit as i64, offset as i64],
            |row| {
                Ok(WallpaperDto {
                    path: row.get(0)?,
                    file_type: row.get(1)?,
                    ext: row.get(2)?,
                    backend: row.get(3)?,
                    size: row.get(4)?,
                    mtime: row.get(5)?,
                    resolution: row.get(6)?,
                })
            },
        )
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(LibraryPageDto {
        total: total as usize,
        items,
    })
}

fn library_page_tsv(
    s: &StorageApi,
    filter: &str,
    sort: &str,
    search: &str,
    offset: usize,
    limit: usize,
) -> Result<LibraryPageDto, String> {
    let filter = validate_filter(filter)?;
    let sort = validate_sort(sort)?;
    let search = search.to_lowercase();
    let mut entries: Vec<WallpaperDto> = library_entries_from_tsv(s)?
        .into_iter()
        .filter(|entry| filter == "all" || entry.file_type == filter)
        .filter(|entry| search.is_empty() || entry.path.to_lowercase().contains(&search))
        .collect();
    match sort {
        "newest" => entries.sort_by(|a, b| b.mtime.cmp(&a.mtime).then_with(|| a.path.cmp(&b.path))),
        "largest" => entries.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.path.cmp(&b.path))),
        "name" => entries.sort_by(|a, b| a.path.cmp(&b.path)),
        _ => unreachable!(),
    }
    let total = entries.len();
    let items = entries.into_iter().skip(offset).take(limit).collect();
    Ok(LibraryPageDto { total, items })
}

fn library_entries_from_tsv(s: &StorageApi) -> Result<Vec<WallpaperDto>, String> {
    let content = std::fs::read_to_string(s.cd.library_tsv_path()).unwrap_or_default();
    let mut entries = Vec::new();
    for line in content.lines().filter(|line| !line.trim().is_empty()) {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 7 {
            continue;
        }
        entries.push(WallpaperDto {
            file_type: parts[0].to_string(),
            ext: parts[1].to_string(),
            backend: parts[2].to_string(),
            size: parts[3].parse().unwrap_or(0),
            mtime: parts[4].parse().unwrap_or(0),
            resolution: parts[5].to_string(),
            path: parts[6].to_string(),
        });
    }
    Ok(entries)
}

fn validate_filter(filter: &str) -> Result<&str, String> {
    match filter {
        "all" | "image" | "gif" | "video" => Ok(filter),
        other => Err(format!("unknown library filter: {}", other)),
    }
}

fn validate_sort(sort: &str) -> Result<&str, String> {
    match sort {
        "newest" | "largest" | "name" => Ok(sort),
        other => Err(format!("unknown library sort: {}", other)),
    }
}

fn source_label(path: &str) -> String {
    if let Some(index) = path.find("/431960/") {
        return format!("Steam Workshop: {}", &path[index + "/431960/".len()..]);
    }
    Path::new(path)
        .file_name()
        .map(|v| v.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

fn generate_thumbnail(src: &str, dst: &Path) -> Result<(), String> {
    let ext = Path::new(src)
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if matches!(ext.as_str(), "mp4" | "webm" | "mkv" | "mov") {
        let status = Command::new("ffmpeg")
            .args(["-y", "-ss", "1", "-i", src, "-frames:v", "1", "-q:v", "3"])
            .arg(dst)
            .status()
            .map_err(|e| e.to_string())?;
        if status.success() {
            return Ok(());
        }
        return Err("ffmpeg thumbnail generation failed".into());
    }
    for program in ["magick", "convert"] {
        let status = Command::new(program)
            .arg(src)
            .args(["-resize", "400x", "-quality", "80", "-auto-orient"])
            .arg(dst)
            .status();
        if matches!(status, Ok(status) if status.success()) {
            return Ok(());
        }
    }
    Err("no thumbnail generator available".into())
}

fn md5_hash(bytes: &[u8]) -> md5::Digest {
    md5::compute(bytes)
}

fn format_bytes(bytes: u64) -> String {
    match bytes {
        b if b >= 1 << 30 => format!("{:.1} GB", b as f64 / (1 << 30) as f64),
        b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / (1 << 20) as f64),
        b if b >= 1 << 10 => format!("{:.1} KB", b as f64 / (1 << 10) as f64),
        b => format!("{} B", b),
    }
}

fn run_external(program: &str, args: &[String]) -> CommandResult {
    match Command::new(program).args(args).output() {
        Ok(out) if out.status.success() => ok(String::from_utf8_lossy(&out.stdout).to_string()),
        Ok(out) => CommandResult {
            success: false,
            stdout: String::from_utf8_lossy(&out.stdout).to_string(),
            stderr: String::from_utf8_lossy(&out.stderr).to_string(),
            exit_code: out.status.code().unwrap_or(1),
        },
        Err(err) => fail(err),
    }
}
