//! wc-preview — fzf preview rendering (kitty icat, chafa, ffmpeg thumbnails, metadata).
//!
//! Matches the Bash preview.sh behaviour:
//!   1. Compact metadata line (filename | type | resolution | size | backend)
//!   2. Image: kitty icat → chafa fallback → text metadata
//!   3. Video: ffmpegthumbnailer/ffmpeg thumb → render → text metadata
//!   4. Cached video thumbnails in cache/previews/
//!   5. Respects preview_metadata config: compact (default), visual, full

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;

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

    let mut hasher = DefaultHasher::new();
    format!("{}:{}", file, mtime).hash(&mut hasher);
    let hash = format!("{:x}.jpg", hasher.finish());
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
                        file,
                    ])
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
    wc_core::config::read_config_value(&cd.path, "preview_metadata", "compact")
}

fn config_backend_for_ext(cd: &ConfigDir, ext: &str) -> String {
    match ext {
        "png" | "jpg" | "jpeg" | "webp" | "bmp" => {
            wc_core::config::read_config_value(&cd.path, "image_backend", "awww")
        }
        "gif" => wc_core::config::read_config_value(&cd.path, "gif_backend", "awww"),
        "mp4" | "webm" | "mkv" | "mov" => {
            wc_core::config::read_config_value(&cd.path, "video_backend", "mpvpaper")
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
        .args(["-format", "%wx%h", file, "[0]"])
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
            file,
        ])
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
    Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// ── Retained from original (thumbnail path helpers) ────────────────────────

use wc_core::types::WallpaperEntry;

/// Get the cached video thumbnail path for an entry.
pub fn video_thumbnail_path(cd: &ConfigDir, entry: &WallpaperEntry) -> Option<String> {
    let key = video_thumb_cache_key(entry);
    let thumb = cd.preview_cache_dir().join(&key);
    if thumb.exists() {
        Some(thumb.to_string_lossy().to_string())
    } else {
        None
    }
}

/// Get the GUI thumbnail path (400px webp).
pub fn gui_thumbnail_path(cd: &ConfigDir, entry: &WallpaperEntry) -> Option<String> {
    let key = gui_thumb_cache_key(entry);
    let thumb = cd.gui_thumbnail_cache_dir().join(&key);
    if thumb.exists() {
        Some(thumb.to_string_lossy().to_string())
    } else {
        None
    }
}

fn video_thumb_cache_key(entry: &WallpaperEntry) -> String {
    let raw = format!("{}:{}", entry.path, entry.mtime);
    let mut hasher = DefaultHasher::new();
    raw.hash(&mut hasher);
    format!("{:x}.jpg", hasher.finish())
}

fn gui_thumb_cache_key(entry: &WallpaperEntry) -> String {
    let real = std::fs::canonicalize(entry.path.as_str())
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| entry.path.to_string());
    let raw = format!("{}:{}:{}", real, entry.mtime, entry.size);
    let mut hasher = DefaultHasher::new();
    raw.hash(&mut hasher);
    format!("{:x}.webp", hasher.finish())
}

// ── GUI Thumbnail v2 ───────────────────────────────────────────────────────
//
// Generates 400px-wide thumbnails with:
//   - Multi-point frame selection for videos (avoids black/title frames)
//   - Atomic writes (.tmp → rename)
//   - v2- cache key prefix (invalidates old bad thumbnails)

/// Cache key for a GUI thumbnail (v2 — incompatible with v1 keys).
pub fn gui_thumb_cache_key_v2(path: &str, mtime: u64, size: u64) -> String {
    let real = std::fs::canonicalize(path)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| path.to_string());
    let raw = format!("v2-{}:{}:{}", real, mtime, size);
    let mut hasher = DefaultHasher::new();
    raw.hash(&mut hasher);
    format!("{:x}.webp", hasher.finish())
}

/// Generate a GUI thumbnail into `cache/gui-thumbnails/`, returning the path.
/// Returns `None` if no generator is available.
pub fn generate_gui_thumbnail(
    cache_dir: &Path,
    path: &str,
    mtime: u64,
    size: u64,
) -> Option<PathBuf> {
    std::fs::create_dir_all(cache_dir).ok()?;

    let key = gui_thumb_cache_key_v2(path, mtime, size);
    let dst = cache_dir.join(&key);

    // Already cached — return immediately.
    if dst.exists() {
        return Some(dst);
    }

    let ext = wc_core::formats::get_extension(path)
        .unwrap_or_default()
        .to_lowercase();

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
            return Some(dst);
        }
    }
    let _ = std::fs::remove_file(&tmp);
    None
}

/// Result of an attempted thumbnail generation.
pub struct ThumbnailResult {
    pub path: String,
    pub thumbnail: Option<String>,
    pub cache_hit: bool,
    pub error: Option<String>,
}

/// Full thumbnail-for API suitable for Tauri/Wails commands.
pub fn thumbnail_for(cache_dir: &Path, path: &str) -> ThumbnailResult {
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) => {
            return ThumbnailResult {
                path: path.to_string(),
                thumbnail: None,
                cache_hit: false,
                error: Some(format!("cannot stat file: {}", e)),
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

    match generate_gui_thumbnail(cache_dir, path, mtime, size) {
        Some(thumb) => {
            let hit = true; // checked existence inside generate_gui_thumbnail
            ThumbnailResult {
                path: path.to_string(),
                thumbnail: Some(thumb.to_string_lossy().to_string()),
                cache_hit: hit,
                error: None,
            }
        }
        None => ThumbnailResult {
            path: path.to_string(),
            thumbnail: None,
            cache_hit: false,
            error: Some("no thumbnail generator available (install ffmpeg + imagemagick)".into()),
        },
    }
}

// ── Internal generators ────────────────────────────────────────────────────

fn generate_image_thumbnail(src: &str, dst: &Path) -> bool {
    for program in &["magick", "convert"] {
        if !command_exists(program) {
            continue;
        }
        let ok = Command::new(program)
            .arg(src)
            .args(["-resize", "400x", "-quality", "80", "-auto-orient"])
            .arg(dst)
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

    // Candidate timestamps: 25%, 50%, 10%, 5s, 75%.
    let candidates: Vec<f64> = vec![
        duration * 0.25,
        duration * 0.50,
        duration * 0.10,
        5.0_f64.min(duration * 0.8),
        duration * 0.75,
    ];

    // Try ffmpegthumbnailer first (fast, good defaults).
    if command_exists("ffmpegthumbnailer") {
        for &ts in &candidates {
            let tmp = dst.with_extension("tmp.jpg");
            let _ = std::fs::remove_file(&tmp);
            let ok = Command::new("ffmpegthumbnailer")
                .args(["-i", src, "-o"])
                .arg(&tmp)
                .args(["-t", &format!("{}", ts as u64), "-s", "400", "-q", "8"])
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if ok && tmp.exists() {
                // Convert to 400px webp.
                let _ = std::fs::remove_file(dst);
                let ok = Command::new("ffmpeg")
                    .args([
                        "-y",
                        "-i",
                        tmp.to_str().unwrap_or(""),
                        "-vf",
                        "scale=400:-1:flags=lanczos",
                        "-quality",
                        "80",
                        dst.to_str().unwrap_or(""),
                    ])
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
            path,
        ])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    s.parse().ok()
}

/// Quick check: is this frame too dark, too bright, or too flat?
/// Uses identify fx:mean and fx:standard_deviation (0..1 range).
fn frame_has_content(path: &Path) -> bool {
    if !command_exists("identify") {
        return true; // can't check, accept it
    }
    let out = Command::new("identify")
        .args([
            "-format",
            "%[fx:mean] %[fx:standard_deviation]",
            path.to_str().unwrap_or(""),
        ])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            let s = String::from_utf8_lossy(&o.stdout).to_string();
            let parts: Vec<f64> = s
                .split_whitespace()
                .filter_map(|v| v.parse().ok())
                .collect();
            let mean = parts.first().copied().unwrap_or(0.5);
            let stddev = parts.get(1).copied().unwrap_or(0.1);
            // Reject very dark (< 0.06), very bright (> 0.94), or flat (< 0.015 stddev).
            mean > 0.06 && mean < 0.94 && stddev > 0.015
        }
        _ => true, // can't check, accept it
    }
}
