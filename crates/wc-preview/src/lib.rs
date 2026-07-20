//! wc-preview — fzf preview rendering (kitty icat, chafa, ffmpeg thumbnails, metadata).
//!
//! Matches the Bash preview.sh behaviour:
//!   1. Compact metadata line (filename | type | resolution | size | backend)
//!   2. Image: kitty icat → chafa fallback → text metadata
//!   3. Video: ffmpegthumbnailer/ffmpeg thumb → render → text metadata
//!   4. Cached video thumbnails in cache/previews/
//!   5. Respects preview_metadata config: compact (default), visual, full

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};

use wc_core::config::ConfigDir;

/// Render the fzf preview for a file.  This is the top-level entry point
/// called by the `__preview__` subcommand.
pub fn render_preview(cd: &ConfigDir, file: &str) {
    let path = Path::new(file);
    if !path.is_file() {
        println!("(no preview)");
        return;
    }

    let ext = wc_core::formats::get_extension(file).unwrap_or_default();

    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "webp" | "bmp" | "gif" => preview_image(cd, file, &ext),
        "mp4" | "webm" | "mkv" | "mov" => preview_video(cd, file, &ext),
        _ => println!(
            "Unsupported: {}",
            path.file_name()
                .map(|n| n.to_string_lossy())
                .unwrap_or_default()
        ),
    }
}

// ── Metadata helpers ───────────────────────────────────────────────────────

/// One-line compact metadata: `filename | type | res | size | backend`
fn preview_compact_metadata(cd: &ConfigDir, file: &str, ext: &str) {
    let ftype = wc_core::formats::file_type_for_ext_str(ext);
    let backend = config_backend_for_ext(cd, ext);
    let size = human_size(file);
    let res = detect_resolution(file);
    let name = Path::new(file)
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    println!("{} | {} | {} | {} | {}", name, ftype, res, size, backend);
}

/// Full metadata block for fallback / full mode.
fn preview_text_metadata(cd: &ConfigDir, file: &str, ext: &str) {
    let ftype = wc_core::formats::file_type_for_ext_str(ext);
    let backend = config_backend_for_ext(cd, ext);
    let size = human_size(file);
    let res = detect_resolution(file);
    let name = Path::new(file)
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    println!();
    println!("───");
    println!("File:       {}", name);
    println!("Type:       {} ({})", ext, ftype);
    println!("Backend:    {}", backend);
    println!("Resolution: {}", res);
    println!("Size:       {}", size);
    println!("Path:       {}", file);
}

// ── Image rendering ─────────────────────────────────────────────────────────

/// Try to render an image in the terminal.  Falls back: kitty icat → chafa.
/// `avail_lines`: 0 = use full preview height; >0 = reserve that many lines.
fn preview_render_image(file: &str, avail_lines: u32) -> bool {
    let cols = std::env::var("FZF_PREVIEW_COLUMNS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(80u32);
    let total_lines = std::env::var("FZF_PREVIEW_LINES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(24u32);

    let (size_spec, place_spec) = if avail_lines > 0 {
        (
            format!("{}x{}", cols, avail_lines),
            format!("{}x{}@0x1", cols, avail_lines),
        )
    } else {
        (
            format!("{}x{}", cols, total_lines),
            format!("{}x{}@0x0", cols, total_lines),
        )
    };

    // 1) kitty icat (Wayland-native)
    if std::env::var("KITTY_WINDOW_ID").is_ok() && command_exists("kitty") {
        let result = Command::new("kitty")
            .args([
                "+kitten",
                "icat",
                "--clear",
                "--transfer-mode=file",
                "--stdin=no",
                "--place",
                &place_spec,
                file,
            ])
            .status();
        if result.map(|s| s.success()).unwrap_or(false) {
            return true;
        }
    }

    // 2) chafa (universal fallback)
    if command_exists("chafa") {
        let result = Command::new("chafa")
            .args(["--size", &size_spec, file])
            .status();
        if result.map(|s| s.success()).unwrap_or(false) {
            return true;
        }
    }

    false
}

// ── Video thumbnail generation ──────────────────────────────────────────────

/// Generate or retrieve a cached video thumbnail.  Returns the path on success.
fn video_thumbnail(cd: &ConfigDir, file: &str) -> Option<PathBuf> {
    let cache_dir = cd.preview_cache_dir();
    std::fs::create_dir_all(&cache_dir).ok()?;

    let mtime = std::fs::metadata(file)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let hash = format!(
        "{}.jpg",
        stable_hash_hex(&format!("{}:{}", file, mtime))
    );
    let thumb = cache_dir.join(&hash);

    if thumb.exists() {
        return Some(thumb);
    }

    // Try ffmpegthumbnailer first, then ffmpeg
    if command_exists("ffmpegthumbnailer") {
        let ok = Command::new("ffmpegthumbnailer")
            .args(["-i", file, "-o"])
            .arg(thumb.to_string_lossy().as_ref())
            .args(["-s", "0", "-q", "8"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ok && thumb.exists() {
            return Some(thumb);
        }
    }

    if command_exists("ffmpeg") {
        let ok = Command::new("ffmpeg")
            .args(["-ss", "1", "-i", file, "-frames:v", "1", "-q:v", "2", "-y"])
            .arg(thumb.to_string_lossy().as_ref())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ok && thumb.exists() {
            return Some(thumb);
        }
    }

    None
}

// ── Per-type preview dispatchers ────────────────────────────────────────────

fn preview_image(cd: &ConfigDir, file: &str, ext: &str) {
    let mode = config_preview_metadata(cd);
    let total_lines = std::env::var("FZF_PREVIEW_LINES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(24u32);

    match mode.as_str() {
        "visual" => {
            if !preview_render_image(file, 0) {
                preview_text_metadata(cd, file, ext);
            }
        }
        "compact" => {
            preview_compact_metadata(cd, file, ext);
            let avail = if total_lines > 1 { total_lines - 1 } else { 1 };
            if !preview_render_image(file, avail) {
                preview_text_metadata(cd, file, ext);
            }
        }
        "full" => {
            preview_render_image(file, 0);
            preview_text_metadata(cd, file, ext);
        }
        _ => {
            preview_render_image(file, 0);
            preview_text_metadata(cd, file, ext);
        }
    }
}

fn preview_video(cd: &ConfigDir, file: &str, ext: &str) {
    let mode = config_preview_metadata(cd);
    let total_lines = std::env::var("FZF_PREVIEW_LINES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(24u32);

    match mode.as_str() {
        "visual" => {
            if let Some(ref thumb) = video_thumbnail(cd, file) {
                let thumb_s = thumb.to_string_lossy();
                if preview_render_image(&thumb_s, 0) {
                    return;
                }
            }
            preview_text_metadata(cd, file, ext);
        }
        "compact" => {
            preview_compact_metadata(cd, file, ext);
            let avail = if total_lines > 1 { total_lines - 1 } else { 1 };
            if let Some(ref thumb) = video_thumbnail(cd, file) {
                let thumb_s = thumb.to_string_lossy();
                if !preview_render_image(&thumb_s, avail) {
                    preview_text_metadata(cd, file, ext);
                }
            } else {
                preview_text_metadata(cd, file, ext);
            }
        }
        "full" => {
            if let Some(ref thumb) = video_thumbnail(cd, file) {
                let thumb_s = thumb.to_string_lossy();
                preview_render_image(&thumb_s, 0);
            }
            preview_text_metadata(cd, file, ext);
            // Duration via ffprobe
            if command_exists("ffprobe") {
                if let Ok(out) = Command::new("ffprobe")
                    .args([
                        "-v",
                        "quiet",
                        "-show_entries",
                        "format=duration",
                        "-of",
                        "csv=p=0",
                    ])
                    .arg("--")
                    .arg(file)
                    .output()
                {
                    let dur = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    if !dur.is_empty() {
                        if let Ok(d) = dur.parse::<f64>() {
                            println!("Duration:  {:.1}s", d);
                        }
                    }
                }
            }
        }
        _ => {
            preview_text_metadata(cd, file, ext);
        }
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn config_preview_metadata(cd: &ConfigDir) -> String {
    wc_config::read_config_value(&cd.path, "preview_metadata", "compact")
}

fn config_backend_for_ext(cd: &ConfigDir, ext: &str) -> String {
    match ext {
        "png" | "jpg" | "jpeg" | "webp" | "bmp" => {
            wc_config::read_config_value(&cd.path, "image_backend", "awww")
        }
        "gif" => wc_config::read_config_value(&cd.path, "gif_backend", "awww"),
        "mp4" | "webm" | "mkv" | "mov" => {
            wc_config::read_config_value(&cd.path, "video_backend", "mpvpaper")
        }
        _ => "?".to_string(),
    }
}

fn human_size(file: &str) -> String {
    let size = std::fs::metadata(file).map(|m| m.len()).unwrap_or(0);
    if size >= 1_073_741_824 {
        format!("{:.1}G", size as f64 / 1_073_741_824.0)
    } else if size >= 1_048_576 {
        format!("{:.1}M", size as f64 / 1_048_576.0)
    } else if size >= 1024 {
        format!("{:.1}K", size as f64 / 1024.0)
    } else {
        format!("{}B", size)
    }
}

fn detect_resolution(file: &str) -> String {
    // Try identify (ImageMagick) first
    if let Ok(out) = Command::new("identify")
        .arg("-format")
        .arg("%wx%h")
        .arg("--")
        .arg(format!("{file}[0]"))
        .output()
    {
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !s.is_empty() && s.contains('x') {
            return s;
        }
    }
    // Try ffprobe for video
    if let Ok(out) = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height",
            "-of",
            "csv=p=0",
        ])
        .arg("--")
        .arg(file)
        .output()
    {
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let parts: Vec<&str> = s.split(',').collect();
        if parts.len() >= 2 {
            return format!("{}x{}", parts[0], parts[1]);
        }
    }
    "?x?".to_string()
}

fn command_exists(cmd: &str) -> bool {
    static CACHE: OnceLock<Mutex<HashMap<String, bool>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));

    if let Ok(guard) = cache.lock() {
        if let Some(hit) = guard.get(cmd) {
            return *hit;
        }
    }

    let exists = which::which(cmd).is_ok();

    if let Ok(mut guard) = cache.lock() {
        guard.insert(cmd.to_string(), exists);
    }
    exists
}

/// Stable 64-bit FNV-1a hash (deterministic across runs and platforms).
fn stable_hash_hex(input: &str) -> String {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;
    let mut hash = FNV_OFFSET_BASIS;
    for byte in input.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{hash:016x}")
}

// ── GUI Thumbnail v3 ───────────────────────────────────────────────────────
//
// Generates 400px-wide thumbnails with:
//   - Single-frame output for animated image sources
//   - Multi-point frame selection for videos (avoids black/title frames)
//   - Atomic writes (.tmp → rename)
//   - v3- cache key prefix (invalidates animated v2 thumbnails)

pub const DEFAULT_FAILURE_TTL_SECS: u64 = 15 * 60;
const FAILURE_CACHE_DIR_NAME: &str = ".failures";

/// Cache key for a GUI thumbnail (v3 — incompatible with animated v2 entries).
pub fn gui_thumb_cache_key_v3(path: &str, mtime: u64, size: u64) -> String {
    let real = std::fs::canonicalize(path)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| path.to_string());
    let raw = format!("v3-{}:{}:{}", real, mtime, size);
    format!("{}.webp", stable_hash_hex(&raw))
}

/// Generate a GUI thumbnail. Returns (path, was_cached) or failure reason.
pub fn generate_gui_thumbnail(
    cache_dir: &Path,
    path: &str,
    mtime: u64,
    size: u64,
) -> Result<(PathBuf, bool), ThumbnailFailure> {
    std::fs::create_dir_all(cache_dir).map_err(|_| ThumbnailFailure::CacheWriteFailed)?;

    let key = gui_thumb_cache_key_v3(path, mtime, size);
    let dst = cache_dir.join(&key);

    // Already cached — return immediately.
    if dst.exists() {
        return Ok((dst, true));
    }

    let ext = wc_core::formats::get_extension(path)
        .unwrap_or_default()
        .to_lowercase();
    if !wc_core::formats::is_supported_extension(&ext) {
        return Err(ThumbnailFailure::Unsupported);
    }

    // Atomic write: generate to .tmp.webp so ffmpeg detects the output format.
    let tmp = cache_dir.join(format!(".{}.tmp.webp", key));
    let _ = std::fs::remove_file(&tmp);

    let ok = if matches!(ext.as_str(), "mp4" | "webm" | "mkv" | "mov") {
        generate_video_thumbnail_v2(path, &tmp)
    } else {
        generate_image_thumbnail(path, &tmp)
    };

    if ok {
        let _ = std::fs::rename(&tmp, &dst);
        if dst.exists() {
            return Ok((dst, false));
        }
        // rename succeeded but dst doesn't exist → cache write failed
        return Err(ThumbnailFailure::CacheWriteFailed);
    }
    let _ = std::fs::remove_file(&tmp);
    Err(ThumbnailFailure::ProbeFailed)
}

/// Result of an attempted thumbnail generation.
pub struct ThumbnailResult {
    pub path: String,
    pub thumbnail: Option<String>,
    pub cache_hit: bool,
    pub error: Option<String>,
    pub failure_reason: Option<ThumbnailFailure>,
}

/// Typed classification of why thumbnail generation did not produce an image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThumbnailFailure {
    Unsupported,
    ProbeFailed,
    CacheWriteFailed,
    MissingFile,
}

impl ThumbnailFailure {
    pub fn as_str(&self) -> &'static str {
        match self {
            ThumbnailFailure::Unsupported => "unsupported",
            ThumbnailFailure::ProbeFailed => "probe_failed",
            ThumbnailFailure::CacheWriteFailed => "cache_write_failed",
            ThumbnailFailure::MissingFile => "missing_file",
        }
    }

    fn from_str(value: &str) -> Option<Self> {
        match value {
            "unsupported" => Some(ThumbnailFailure::Unsupported),
            "probe_failed" => Some(ThumbnailFailure::ProbeFailed),
            "cache_write_failed" => Some(ThumbnailFailure::CacheWriteFailed),
            "missing_file" => Some(ThumbnailFailure::MissingFile),
            _ => None,
        }
    }
}

/// Metadata about the thumbnail cache directory.
pub struct ThumbnailCacheInfo {
    pub entries: usize,
    pub total_bytes: u64,
    pub oldest_mtime: Option<u64>,
    pub newest_mtime: Option<u64>,
    pub failure_entries: usize,
}

/// Collect metadata about every file in the thumbnail cache directory.
pub fn thumbnail_cache_info(cache_dir: &Path) -> ThumbnailCacheInfo {
    let mut entries = 0usize;
    let mut total_bytes = 0u64;
    let mut oldest: Option<u64> = None;
    let mut newest: Option<u64> = None;

    if let Ok(read_dir) = std::fs::read_dir(cache_dir) {
        for entry in read_dir.flatten() {
            let Ok(meta) = entry.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            entries += 1;
            total_bytes += meta.len();
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            match oldest {
                None => oldest = Some(mtime),
                Some(t) if mtime < t => oldest = Some(mtime),
                _ => {}
            }
            match newest {
                None => newest = Some(mtime),
                Some(t) if mtime > t => newest = Some(mtime),
                _ => {}
            }
        }
    }

    ThumbnailCacheInfo {
        entries,
        total_bytes,
        oldest_mtime: oldest,
        newest_mtime: newest,
        failure_entries: count_failure_markers(cache_dir),
    }
}

/// Clear thumbnail cache entries older than `max_age_days`. Returns count removed.
pub fn thumbnail_cache_cleanup_old(cache_dir: &Path, max_age_days: u64) -> u64 {
    let cutoff = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .saturating_sub(max_age_days * 86400);
    let mut removed = 0u64;
    if let Ok(read_dir) = std::fs::read_dir(cache_dir) {
        for entry in read_dir.flatten() {
            let Ok(meta) = entry.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            if mtime < cutoff && std::fs::remove_file(entry.path()).is_ok() {
                removed += 1;
            }
        }
    }
    let failures = failure_cache_dir(cache_dir);
    if let Ok(read_dir) = std::fs::read_dir(failures) {
        for entry in read_dir.flatten() {
            let Ok(meta) = entry.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            if mtime < cutoff && std::fs::remove_file(entry.path()).is_ok() {
                removed += 1;
            }
        }
    }
    removed
}

/// Clear all files in the thumbnail cache directory. Returns count removed.
pub fn thumbnail_cache_cleanup_all(cache_dir: &Path) -> u64 {
    let mut removed = 0u64;
    if let Ok(read_dir) = std::fs::read_dir(cache_dir) {
        for entry in read_dir.flatten() {
            if entry.file_type().map(|t| t.is_file()).unwrap_or(false)
                && std::fs::remove_file(entry.path()).is_ok()
            {
                removed += 1;
            }
        }
    }
    let failures = failure_cache_dir(cache_dir);
    if let Ok(read_dir) = std::fs::read_dir(&failures) {
        for entry in read_dir.flatten() {
            if entry.file_type().map(|t| t.is_file()).unwrap_or(false)
                && std::fs::remove_file(entry.path()).is_ok()
            {
                removed += 1;
            }
        }
    }
    removed
}

const TMP_CLEANUP_INTERVAL_SECS: u64 = 3_600;

#[derive(Default)]
struct TmpCleanupSchedule {
    last_sweeps: Mutex<HashMap<PathBuf, u64>>,
}

impl TmpCleanupSchedule {
    fn should_run(&self, cache_dir: &Path, now: u64, interval_secs: u64) -> bool {
        let mut last_sweeps = self
            .last_sweeps
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if last_sweeps.get(cache_dir).is_some_and(|last_sweep| {
            now.checked_sub(*last_sweep)
                .is_some_and(|elapsed| elapsed < interval_secs)
        }) {
            return false;
        }

        last_sweeps.insert(cache_dir.to_path_buf(), now);
        true
    }
}

static TMP_CLEANUP_SCHEDULE: OnceLock<TmpCleanupSchedule> = OnceLock::new();

fn maybe_cleanup_stale_tmp_thumbnails(cache_dir: &Path, now: u64) -> u64 {
    let schedule = TMP_CLEANUP_SCHEDULE.get_or_init(TmpCleanupSchedule::default);
    if schedule.should_run(cache_dir, now, TMP_CLEANUP_INTERVAL_SECS) {
        cleanup_stale_tmp_thumbnails(cache_dir, TMP_CLEANUP_INTERVAL_SECS)
    } else {
        0
    }
}

/// Full thumbnail-for API suitable for Tauri commands.
pub fn thumbnail_for(cache_dir: &Path, path: &str) -> ThumbnailResult {
    thumbnail_for_with_failure_ttl(cache_dir, path, DEFAULT_FAILURE_TTL_SECS)
}

pub fn thumbnail_for_with_failure_ttl(
    cache_dir: &Path,
    path: &str,
    failure_ttl_secs: u64,
) -> ThumbnailResult {
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) => {
            return ThumbnailResult {
                path: path.to_string(),
                thumbnail: None,
                cache_hit: false,
                error: Some(format!("cannot stat file: {}", e)),
                failure_reason: Some(ThumbnailFailure::MissingFile),
            }
        }
    };
    let size = meta.len();
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let _ = maybe_cleanup_stale_tmp_thumbnails(cache_dir, current_epoch_secs());
    let key = gui_thumb_cache_key_v3(path, mtime, size);
    let marker = failure_marker_path(cache_dir, &key);
    if let Some(failure) = read_failure_marker(&marker) {
        return ThumbnailResult {
            path: path.to_string(),
            thumbnail: None,
            cache_hit: false,
            error: Some(format!(
                "thumbnail generation skipped: cached {}",
                failure.as_str()
            )),
            failure_reason: Some(failure),
        };
    }

    match generate_gui_thumbnail(cache_dir, path, mtime, size) {
        Ok((thumb, cached)) => ThumbnailResult {
            path: path.to_string(),
            thumbnail: Some(thumb.to_string_lossy().to_string()),
            cache_hit: cached,
            error: None,
            failure_reason: None,
        },
        Err(failure) => {
            let expires_at = current_epoch_secs().saturating_add(failure_ttl_secs);
            let _ = write_failure_marker(&marker, failure, expires_at);
            ThumbnailResult {
                path: path.to_string(),
                thumbnail: None,
                cache_hit: false,
                error: Some(format!("thumbnail generation failed: {:?}", failure)),
                failure_reason: Some(failure),
            }
        }
    }
}

fn current_epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn failure_cache_dir(cache_dir: &Path) -> PathBuf {
    cache_dir.join(FAILURE_CACHE_DIR_NAME)
}

fn failure_marker_path(cache_dir: &Path, key: &str) -> PathBuf {
    failure_cache_dir(cache_dir).join(format!("{}.fail", key))
}

fn count_failure_markers(cache_dir: &Path) -> usize {
    std::fs::read_dir(failure_cache_dir(cache_dir))
        .map(|read_dir| {
            read_dir
                .flatten()
                .filter(|entry| entry.file_type().map(|t| t.is_file()).unwrap_or(false))
                .count()
        })
        .unwrap_or(0)
}

fn read_failure_marker(marker: &Path) -> Option<ThumbnailFailure> {
    let content = std::fs::read_to_string(marker).ok()?;
    let mut lines = content.lines();
    if lines.next()? != "v1" {
        let _ = std::fs::remove_file(marker);
        return None;
    }
    let failure = ThumbnailFailure::from_str(lines.next()?)?;
    let expires_at = lines.next()?.parse::<u64>().ok()?;
    if expires_at < current_epoch_secs() {
        let _ = std::fs::remove_file(marker);
        return None;
    }
    Some(failure)
}

fn write_failure_marker(
    marker: &Path,
    failure: ThumbnailFailure,
    expires_at: u64,
) -> std::io::Result<()> {
    if let Some(parent) = marker.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(
        marker,
        format!("v1\n{}\n{}\n", failure.as_str(), expires_at),
    )
}

// ── Internal generators ────────────────────────────────────────────────────

pub fn cleanup_stale_tmp_thumbnails(cache_dir: &Path, max_age_secs: u64) -> u64 {
    let now = current_epoch_secs();
    let mut removed = 0u64;
    let Ok(read_dir) = std::fs::read_dir(cache_dir) else {
        return 0;
    };
    for entry in read_dir.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.starts_with('.') || !name.ends_with(".tmp.webp") {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if now.saturating_sub(mtime) >= max_age_secs && std::fs::remove_file(path).is_ok() {
            removed += 1;
        }
    }
    removed
}

fn generate_image_thumbnail_rust(src: &str, dst: &Path) -> bool {
    let Ok(reader) = image::ImageReader::open(src) else {
        return false;
    };
    let Ok(reader) = reader.with_guessed_format() else {
        return false;
    };
    let Ok(img) = reader.decode() else {
        return false;
    };
    let width = img.width();
    let height = img.height();
    if width == 0 || height == 0 {
        return false;
    }
    let target_width = 400u32;
    let target_height = ((height as f64) * (target_width as f64 / width as f64))
        .round()
        .max(1.0) as u32;
    let resized = img.resize(
        target_width,
        target_height,
        image::imageops::FilterType::Lanczos3,
    );
    resized
        .save_with_format(dst, image::ImageFormat::WebP)
        .is_ok()
        && dst.exists()
}

fn imagemagick_first_frame_source(src: &str) -> String {
    format!("{src}[0]")
}

fn generate_image_thumbnail(src: &str, dst: &Path) -> bool {
    if generate_image_thumbnail_rust(src, dst) {
        return true;
    }
    let first_frame_source = imagemagick_first_frame_source(src);
    for program in &["magick", "convert"] {
        if !command_exists(program) {
            continue;
        }
        let ok = Command::new(program)
            .arg(&first_frame_source)
            .args(["-resize", "400x", "-quality", "80", "-auto-orient"])
            .arg(dst)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ok {
            return true;
        }
    }
    false
}

/// Generate a video thumbnail using multi-point frame selection.
fn generate_video_thumbnail_v2(src: &str, dst: &Path) -> bool {
    if !command_exists("ffmpeg") {
        return false;
    }

    // Get duration.
    let duration = get_video_duration(src).unwrap_or(0.0);
    if duration <= 0.0 {
        return false;
    }

    // Candidate timestamps: prefer 25%, then 50%. Stop at the first usable frame.
    let candidates: Vec<f64> = vec![duration * 0.25, duration * 0.50];

    // Try ffmpegthumbnailer first (fast, good defaults).
    if command_exists("ffmpegthumbnailer") {
        for &ts in &candidates {
            let tmp = dst.with_extension("tmp.jpg");
            let _ = std::fs::remove_file(&tmp);
            let ok = Command::new("ffmpegthumbnailer")
                .args(["-i", src, "-o"])
                .arg(&tmp)
                .args(["-t", &format!("{}", ts as u64), "-s", "400", "-q", "8"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if ok && tmp.exists() {
                // Convert to 400px webp.
                let _ = std::fs::remove_file(dst);
                let ok = Command::new("ffmpeg")
                    .args([
                        "-hide_banner",
                        "-loglevel",
                        "error",
                        "-nostats",
                        "-y",
                        "-i",
                        tmp.to_str().unwrap_or(""),
                        "-vf",
                        "scale=400:-1:flags=lanczos",
                        "-quality",
                        "80",
                        dst.to_str().unwrap_or(""),
                    ])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false);
                let _ = std::fs::remove_file(&tmp);
                if ok && dst.exists() && frame_has_content(dst) {
                    return true;
                }
                let _ = std::fs::remove_file(dst);
            }
            let _ = std::fs::remove_file(&tmp);
        }
    }

    // Fallback: ffmpeg multi-point with scale filter.
    for &ts in &candidates {
        let _ = std::fs::remove_file(dst);
        let ok = Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-nostats",
                "-y",
                "-ss",
                &format!("{:.1}", ts),
                "-i",
                src,
                "-frames:v",
                "1",
                "-vf",
                "scale=400:-1:flags=lanczos",
                "-quality",
                "80",
                dst.to_str().unwrap_or(""),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ok && dst.exists() && frame_has_content(dst) {
            return true;
        }
        let _ = std::fs::remove_file(dst);
    }

    // Last resort: just pick the middle frame regardless of content.
    let ts = duration * 0.5;
    let _ = std::fs::remove_file(dst);
    let ok = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostats",
            "-y",
            "-ss",
            &format!("{:.1}", ts),
            "-i",
            src,
            "-frames:v",
            "1",
            "-vf",
            "scale=400:-1:flags=lanczos",
            "-quality",
            "80",
            dst.to_str().unwrap_or(""),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    ok && dst.exists()
}

fn get_video_duration(path: &str) -> Option<f64> {
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "quiet",
            "-show_entries",
            "format=duration",
            "-of",
            "csv=p=0",
        ])
        .arg("--")
        .arg(path)
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    s.parse().ok()
}

/// Quick check: is this frame too dark, too bright, or too flat?
/// Uses in-memory mean/stddev over decoded pixels (0..1 range), matching the
/// former ImageMagick `identify` fx thresholds without spawning a process.
fn frame_has_content(path: &Path) -> bool {
    let Ok(img) = image::open(path) else {
        return true; // can't check, accept it
    };
    let rgb = img.to_rgb8();
    let pixels = rgb.as_raw();
    if pixels.is_empty() {
        return true;
    }
    let mut sum = 0.0_f64;
    let mut sum_sq = 0.0_f64;
    let samples = pixels.len() as f64;
    for &channel in pixels {
        let v = f64::from(channel) / 255.0;
        sum += v;
        sum_sq += v * v;
    }
    let mean = sum / samples;
    let variance = (sum_sq / samples) - (mean * mean);
    let stddev = variance.max(0.0).sqrt();
    // Reject very dark (< 0.06), very bright (> 0.94), or flat (< 0.015 stddev).
    mean > 0.06 && mean < 0.94 && stddev > 0.015
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_key_is_deterministic() {
        let key1 = gui_thumb_cache_key_v3("/path/to/file.jpg", 1700000000, 12345);
        let key2 = gui_thumb_cache_key_v3("/path/to/file.jpg", 1700000000, 12345);
        assert_eq!(key1, key2, "same inputs must produce same cache key");
    }

    #[test]
    fn cache_key_changes_with_mtime() {
        let key1 = gui_thumb_cache_key_v3("/path/to/file.jpg", 100, 12345);
        let key2 = gui_thumb_cache_key_v3("/path/to/file.jpg", 200, 12345);
        assert_ne!(key1, key2, "different mtime must produce different key");
    }

    #[test]
    fn cache_key_changes_with_size() {
        let key1 = gui_thumb_cache_key_v3("/path/to/file.jpg", 100, 100);
        let key2 = gui_thumb_cache_key_v3("/path/to/file.jpg", 100, 200);
        assert_ne!(key1, key2, "different size must produce different key");
    }

    #[test]
    fn cache_key_changes_with_path() {
        let key1 = gui_thumb_cache_key_v3("/a.jpg", 100, 100);
        let key2 = gui_thumb_cache_key_v3("/b.jpg", 100, 100);
        assert_ne!(key1, key2, "different path must produce different key");
    }

    #[test]
    fn thumbnail_for_missing_file_returns_missing_file_failure() {
        let tmp = tempfile::tempdir().unwrap();
        let result = thumbnail_for(tmp.path(), "/nonexistent/file.jpg");
        assert_eq!(result.failure_reason, Some(ThumbnailFailure::MissingFile));
        assert!(result.thumbnail.is_none());
        assert!(!result.cache_hit);
    }

    #[test]
    fn cache_info_returns_zero_for_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let info = thumbnail_cache_info(tmp.path());
        assert_eq!(info.entries, 0);
        assert_eq!(info.total_bytes, 0);
        assert!(info.oldest_mtime.is_none());
    }

    #[test]
    fn cleanup_old_removes_no_files_in_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let removed = thumbnail_cache_cleanup_old(tmp.path(), 30);
        assert_eq!(removed, 0);
    }

    #[test]
    fn thumbnail_failure_cache_records_failed_generation() {
        let cache = tempfile::tempdir().unwrap();
        let media_dir = tempfile::tempdir().unwrap();
        let bad_image = media_dir.path().join("bad.jpg");
        std::fs::write(&bad_image, b"not an image").unwrap();

        let result = thumbnail_for_with_failure_ttl(
            cache.path(),
            bad_image.to_str().unwrap(),
            DEFAULT_FAILURE_TTL_SECS,
        );

        assert!(result.thumbnail.is_none());
        assert!(result.failure_reason.is_some());
        assert_eq!(thumbnail_cache_info(cache.path()).failure_entries, 1);
    }

    #[test]
    fn thumbnail_failure_cache_uses_persisted_reason_until_ttl() {
        let cache = tempfile::tempdir().unwrap();
        let media_dir = tempfile::tempdir().unwrap();
        let bad_image = media_dir.path().join("bad.jpg");
        std::fs::write(&bad_image, b"not an image").unwrap();
        let meta = std::fs::metadata(&bad_image).unwrap();
        let size = meta.len();
        let mtime = meta
            .modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let key = gui_thumb_cache_key_v3(bad_image.to_str().unwrap(), mtime, size);
        let marker = failure_marker_path(cache.path(), &key);
        std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
        write_failure_marker(
            &marker,
            ThumbnailFailure::Unsupported,
            current_epoch_secs() + 3600,
        )
        .unwrap();

        let result = thumbnail_for_with_failure_ttl(
            cache.path(),
            bad_image.to_str().unwrap(),
            DEFAULT_FAILURE_TTL_SECS,
        );

        assert_eq!(result.failure_reason, Some(ThumbnailFailure::Unsupported));
    }

    #[test]
    fn thumbnail_failure_cache_expires_and_replaces_old_marker() {
        let cache = tempfile::tempdir().unwrap();
        let media_dir = tempfile::tempdir().unwrap();
        let bad_image = media_dir.path().join("bad.jpg");
        std::fs::write(&bad_image, b"not an image").unwrap();
        let meta = std::fs::metadata(&bad_image).unwrap();
        let size = meta.len();
        let mtime = meta
            .modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let key = gui_thumb_cache_key_v3(bad_image.to_str().unwrap(), mtime, size);
        let marker = failure_marker_path(cache.path(), &key);
        std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
        write_failure_marker(
            &marker,
            ThumbnailFailure::Unsupported,
            current_epoch_secs() - 1,
        )
        .unwrap();

        let result = thumbnail_for_with_failure_ttl(
            cache.path(),
            bad_image.to_str().unwrap(),
            DEFAULT_FAILURE_TTL_SECS,
        );

        assert_ne!(result.failure_reason, Some(ThumbnailFailure::Unsupported));
        assert_eq!(thumbnail_cache_info(cache.path()).failure_entries, 1);
    }

    #[test]
    fn cleanup_stale_tmp_thumbnails_removes_only_old_tmp_files() {
        let cache = tempfile::tempdir().unwrap();
        let old_tmp = cache.path().join(".old.tmp.webp");
        let fresh_tmp = cache.path().join(".fresh.tmp.webp");
        let normal = cache.path().join("normal.webp");
        std::fs::write(&old_tmp, b"x").unwrap();
        std::fs::write(&fresh_tmp, b"x").unwrap();
        std::fs::write(&normal, b"x").unwrap();

        let old_time = filetime::FileTime::from_unix_time((current_epoch_secs() - 7200) as i64, 0);
        filetime::set_file_mtime(&old_tmp, old_time).unwrap();

        let removed = cleanup_stale_tmp_thumbnails(cache.path(), 3600);
        assert_eq!(removed, 1);
        assert!(!old_tmp.exists());
        assert!(fresh_tmp.exists());
        assert!(normal.exists());
    }

    #[test]
    fn tmp_cleanup_schedule_throttles_each_directory_independently() {
        let schedule = TmpCleanupSchedule::default();

        assert!(schedule.should_run(Path::new("/cache"), 1_000, 3_600));
        assert!(!schedule.should_run(Path::new("/cache"), 1_001, 3_600));
        assert!(schedule.should_run(Path::new("/cache"), 4_600, 3_600));
        assert!(schedule.should_run(Path::new("/other"), 1_001, 3_600));
    }

    #[test]
    fn tmp_cleanup_schedule_reopens_window_after_clock_rollback() {
        let schedule = TmpCleanupSchedule::default();

        assert!(schedule.should_run(Path::new("/cache"), 5_000, 3_600));
        assert!(schedule.should_run(Path::new("/cache"), 1_000, 3_600));
    }

    #[test]
    fn tmp_cleanup_schedule_allows_one_concurrent_sweep() {
        const THREADS: usize = 16;

        let schedule = TmpCleanupSchedule::default();
        let barrier = std::sync::Barrier::new(THREADS);
        let winners = std::sync::atomic::AtomicUsize::new(0);

        std::thread::scope(|scope| {
            for _ in 0..THREADS {
                let schedule = &schedule;
                let barrier = &barrier;
                let winners = &winners;
                scope.spawn(move || {
                    barrier.wait();
                    if schedule.should_run(Path::new("/cache"), 1_000, 3_600) {
                        winners.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    }
                });
            }
        });

        assert_eq!(winners.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn tmp_cleanup_schedule_recovers_from_poison() {
        let schedule = TmpCleanupSchedule::default();
        let poisoned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = schedule.last_sweeps.lock().unwrap();
            panic!("poison cleanup schedule for test");
        }));

        assert!(poisoned.is_err());
        assert!(schedule.should_run(Path::new("/cache"), 1_000, 3_600));
    }

    #[test]
    fn thumbnail_for_static_image_generates_and_hits_cache() {
        let cache = tempfile::tempdir().unwrap();
        let media = tempfile::tempdir().unwrap();
        let image_path = media.path().join("image.png");
        let img = image::RgbImage::from_pixel(16, 9, image::Rgb([200, 20, 40]));
        img.save(&image_path).unwrap();

        let first = thumbnail_for(cache.path(), image_path.to_str().unwrap());
        assert!(
            first.thumbnail.is_some(),
            "first call should generate thumbnail"
        );
        assert!(!first.cache_hit);
        assert!(std::path::Path::new(first.thumbnail.as_ref().unwrap()).exists());

        let second = thumbnail_for(cache.path(), image_path.to_str().unwrap());
        assert!(
            second.thumbnail.is_some(),
            "second call should return cached thumbnail"
        );
        assert!(second.cache_hit);
    }

    #[test]
    fn animated_gif_generates_single_frame_gui_thumbnail() {
        let cache = tempfile::tempdir().unwrap();
        let media = tempfile::tempdir().unwrap();
        let source = media.path().join("animated.gif");
        let file = std::fs::File::create(&source).unwrap();
        let mut encoder = image::codecs::gif::GifEncoder::new(file);
        let red = image::Frame::new(image::RgbaImage::from_pixel(
            8,
            8,
            image::Rgba([255, 0, 0, 255]),
        ));
        let blue = image::Frame::new(image::RgbaImage::from_pixel(
            8,
            8,
            image::Rgba([0, 0, 255, 255]),
        ));
        encoder.encode_frames([red, blue]).unwrap();

        let result = thumbnail_for(cache.path(), source.to_str().unwrap());
        let bytes = std::fs::read(result.thumbnail.expect("thumbnail")).unwrap();
        assert!(!bytes
            .windows(4)
            .any(|chunk| chunk == b"ANIM" || chunk == b"ANMF"));
    }

    #[test]
    fn imagemagick_fallback_selects_only_the_first_frame() {
        assert_eq!(
            imagemagick_first_frame_source("/tmp/a.gif"),
            "/tmp/a.gif[0]",
        );
    }

    #[test]
    fn thumbnail_for_misnamed_jpeg_uses_magic_byte_format() {
        let cache = tempfile::tempdir().unwrap();
        let media = tempfile::tempdir().unwrap();
        let image_path = media.path().join("wrong-ext.png");
        let img = image::RgbImage::from_pixel(16, 9, image::Rgb([200, 20, 40]));
        img.save_with_format(&image_path, image::ImageFormat::Jpeg)
            .unwrap();

        let result = thumbnail_for(cache.path(), image_path.to_str().unwrap());
        assert!(
            result.thumbnail.is_some(),
            "misnamed JPEG should thumbnail via format sniffing"
        );
        assert!(!result.cache_hit);
    }
}
