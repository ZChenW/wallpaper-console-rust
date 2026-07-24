//! wc-preview — fzf preview rendering (kitty icat, chafa, ffmpeg thumbnails, metadata).
//!
//! Matches the Bash preview.sh behaviour:
//!   1. Compact metadata line (filename | type | resolution | size | backend)
//!   2. Image: kitty icat → chafa fallback → text metadata
//!   3. Video: ffmpegthumbnailer/ffmpeg thumb → render → text metadata
//!   4. Cached video thumbnails in cache/previews/
//!   5. Respects preview_metadata config: compact (default), visual, full

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use wc_core::config::ConfigDir;

const THUMBNAIL_COMMAND_TIMEOUT: Duration = Duration::from_secs(8);
const METADATA_COMMAND_TIMEOUT: Duration = Duration::from_secs(2);
const COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(10);
const COMMAND_DRAIN_GRACE: Duration = Duration::from_millis(200);
const COMMAND_OUTPUT_CAP: usize = 32 * 1024;
const MAX_IMAGE_DIMENSION: u32 = 32_768;
const MAX_IMAGE_PIXELS: u64 = 64 * 1024 * 1024;
const MAX_IMAGE_DECODE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_THUMBNAIL_WIDTH: u32 = 400;
const MAX_THUMBNAIL_HEIGHT: u32 = 400;
const MAX_THUMBNAIL_PIXELS: u64 = MAX_THUMBNAIL_WIDTH as u64 * MAX_THUMBNAIL_HEIGHT as u64;
const FFMPEG_THUMBNAIL_SCALE_FILTER: &str =
    "scale='min(400,iw)':'min(400,ih)':force_original_aspect_ratio=decrease:flags=lanczos";

#[derive(Debug, PartialEq, Eq)]
struct DeadlineCommandOutput {
    success: bool,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DeadlineCommandError {
    Spawn(String),
    Wait(String),
    TimedOut,
}

struct DrainCapture {
    bytes: Arc<Mutex<Vec<u8>>>,
    finished: mpsc::Receiver<()>,
}

impl DrainCapture {
    fn snapshot_after(self, deadline: Instant) -> Vec<u8> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if !remaining.is_zero() {
            let _ = self.finished.recv_timeout(remaining);
        }
        self.bytes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

/// Execute a thumbnail helper within a hard deadline.
///
/// The child gets a fresh process group. Output is continuously drained into
/// bounded buffers, then the group is killed and the direct child reaped on
/// timeout. Drainers are observed only for a fixed grace period: if a detached
/// descendant escaped the process group while retaining a pipe, the caller
/// returns instead of joining that reader forever.
fn run_command_with_deadline(
    command: &mut Command,
    timeout: Duration,
) -> Result<DeadlineCommandOutput, DeadlineCommandError> {
    #[cfg(unix)]
    use std::os::unix::process::CommandExt;

    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    command.process_group(0);

    let mut child = command
        .spawn()
        .map_err(|error| DeadlineCommandError::Spawn(error.to_string()))?;
    let child_pid = child.id();
    let stdout = child
        .stdout
        .take()
        .map(spawn_bounded_drainer)
        .expect("piped stdout must be available");
    let stderr = child
        .stderr
        .take()
        .map(spawn_bounded_drainer)
        .expect("piped stderr must be available");
    let started = Instant::now();

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                kill_process_group(child_pid);
                let drain_deadline = Instant::now() + COMMAND_DRAIN_GRACE;
                return Ok(DeadlineCommandOutput {
                    success: status.success(),
                    stdout: stdout.snapshot_after(drain_deadline),
                    stderr: stderr.snapshot_after(drain_deadline),
                });
            }
            Ok(None) if started.elapsed() >= timeout => {
                terminate_and_reap(&mut child, child_pid);
                let drain_deadline = Instant::now() + COMMAND_DRAIN_GRACE;
                let _ = stdout.snapshot_after(drain_deadline);
                let _ = stderr.snapshot_after(drain_deadline);
                return Err(DeadlineCommandError::TimedOut);
            }
            Ok(None) => std::thread::sleep(
                COMMAND_POLL_INTERVAL.min(timeout.saturating_sub(started.elapsed())),
            ),
            Err(error) => {
                terminate_and_reap(&mut child, child_pid);
                let drain_deadline = Instant::now() + COMMAND_DRAIN_GRACE;
                let _ = stdout.snapshot_after(drain_deadline);
                let _ = stderr.snapshot_after(drain_deadline);
                return Err(DeadlineCommandError::Wait(error.to_string()));
            }
        }
    }
}

fn spawn_bounded_drainer(mut stream: impl Read + Send + 'static) -> DrainCapture {
    let bytes = Arc::new(Mutex::new(Vec::with_capacity(COMMAND_OUTPUT_CAP)));
    let writer = bytes.clone();
    let (finished_tx, finished) = mpsc::channel();
    std::thread::spawn(move || {
        let mut chunk = [0_u8; 4096];
        loop {
            match stream.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    let mut captured = writer
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    let remaining = COMMAND_OUTPUT_CAP.saturating_sub(captured.len());
                    captured.extend_from_slice(&chunk[..read.min(remaining)]);
                }
            }
        }
        let _ = finished_tx.send(());
    });
    DrainCapture { bytes, finished }
}

fn terminate_and_reap(child: &mut Child, child_pid: u32) {
    kill_process_group(child_pid);
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(unix)]
fn kill_process_group(pid: u32) {
    let Ok(pid) = i32::try_from(pid) else {
        return;
    };
    // SAFETY: `run_command_with_deadline` creates a new process group whose
    // group id is the direct child pid. A negative pid targets only that group.
    unsafe {
        libc::kill(-pid, libc::SIGKILL);
    }
}

#[cfg(not(unix))]
fn kill_process_group(_pid: u32) {}

fn command_succeeded(command: &mut Command, timeout: Duration, label: &str) -> bool {
    match run_command_with_deadline(command, timeout) {
        Ok(output) if output.success => true,
        Ok(output) => {
            let detail = String::from_utf8_lossy(&output.stderr);
            eprintln!("{label} failed: {}", detail.trim());
            false
        }
        Err(error) => {
            eprintln!("{label} failed: {error:?}");
            false
        }
    }
}

fn command_output(
    command: &mut Command,
    timeout: Duration,
    label: &str,
) -> Option<DeadlineCommandOutput> {
    match run_command_with_deadline(command, timeout) {
        Ok(output) if output.success => Some(output),
        Ok(output) => {
            let detail = String::from_utf8_lossy(&output.stderr);
            eprintln!("{label} failed: {}", detail.trim());
            None
        }
        Err(error) => {
            eprintln!("{label} failed: {error:?}");
            None
        }
    }
}

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

    let hash = format!("{}.jpg", stable_hash_hex(&format!("{}:{}", file, mtime)));
    let thumb = cache_dir.join(&hash);

    if thumb.exists() {
        return Some(thumb);
    }

    // Try ffmpegthumbnailer first, then ffmpeg
    if command_exists("ffmpegthumbnailer") {
        let mut command = Command::new("ffmpegthumbnailer");
        command
            .args(["-i", file, "-o"])
            .arg(thumb.to_string_lossy().as_ref())
            .args(["-s", "0", "-q", "8"]);
        let ok = command_succeeded(
            &mut command,
            THUMBNAIL_COMMAND_TIMEOUT,
            "ffmpegthumbnailer preview",
        );
        if ok && thumb.exists() {
            return Some(thumb);
        }
    }

    if command_exists("ffmpeg") {
        let mut command = Command::new("ffmpeg");
        command
            .args(["-ss", "1", "-i", file, "-frames:v", "1", "-q:v", "2", "-y"])
            .arg(thumb.to_string_lossy().as_ref());
        let ok = command_succeeded(&mut command, THUMBNAIL_COMMAND_TIMEOUT, "ffmpeg preview");
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
                let mut command = Command::new("ffprobe");
                command
                    .args([
                        "-v",
                        "quiet",
                        "-show_entries",
                        "format=duration",
                        "-of",
                        "csv=p=0",
                    ])
                    .arg("--")
                    .arg(file);
                if let Some(out) =
                    command_output(&mut command, METADATA_COMMAND_TIMEOUT, "ffprobe duration")
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
    let mut identify = Command::new("identify");
    identify
        .arg("-format")
        .arg("%wx%h")
        .arg("--")
        .arg(format!("{file}[0]"));
    if let Some(out) = command_output(
        &mut identify,
        METADATA_COMMAND_TIMEOUT,
        "ImageMagick resolution probe",
    ) {
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !s.is_empty() && s.contains('x') {
            return s;
        }
    }
    // Try ffprobe for video
    let mut ffprobe = Command::new("ffprobe");
    ffprobe
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
        .arg(file);
    if let Some(out) = command_output(
        &mut ffprobe,
        METADATA_COMMAND_TIMEOUT,
        "ffprobe resolution probe",
    ) {
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

    // Each producer owns a unique temporary file. This avoids same-key calls
    // deleting or partially overwriting one another before atomic publication.
    let tmp = reserve_unique_thumbnail_temp(cache_dir, &key)?;

    let generated = if matches!(ext.as_str(), "mp4" | "webm" | "mkv" | "mov") {
        generate_video_thumbnail_v2(path, &tmp)
            .then_some(())
            .ok_or(ThumbnailFailure::ProbeFailed)
    } else {
        generate_image_thumbnail(path, &tmp)
    };

    if let Err(failure) = generated {
        let _ = std::fs::remove_file(&tmp);
        return Err(failure);
    }
    if validate_generated_thumbnail(&tmp).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return Err(ThumbnailFailure::ProbeFailed);
    }

    match std::fs::rename(&tmp, &dst) {
        Ok(()) => Ok((dst, false)),
        Err(_) if dst.exists() && validate_generated_thumbnail(&dst).is_ok() => {
            let _ = std::fs::remove_file(&tmp);
            Ok((dst, true))
        }
        Err(_) => {
            let _ = std::fs::remove_file(&tmp);
            Err(ThumbnailFailure::CacheWriteFailed)
        }
    }
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
    ImageTooLarge,
    CacheWriteFailed,
    MissingFile,
}

impl ThumbnailFailure {
    pub fn as_str(&self) -> &'static str {
        match self {
            ThumbnailFailure::Unsupported => "unsupported",
            ThumbnailFailure::ProbeFailed => "probe_failed",
            ThumbnailFailure::ImageTooLarge => "image_too_large",
            ThumbnailFailure::CacheWriteFailed => "cache_write_failed",
            ThumbnailFailure::MissingFile => "missing_file",
        }
    }

    fn from_str(value: &str) -> Option<Self> {
        match value {
            "unsupported" => Some(ThumbnailFailure::Unsupported),
            "probe_failed" => Some(ThumbnailFailure::ProbeFailed),
            "image_too_large" => Some(ThumbnailFailure::ImageTooLarge),
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
                error: Some(format!("thumbnail generation failed: {}", failure.as_str())),
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

fn reserve_unique_thumbnail_temp(cache_dir: &Path, key: &str) -> Result<PathBuf, ThumbnailFailure> {
    static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

    for _ in 0..128 {
        let sequence = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let path = cache_dir.join(format!(
            ".{key}.{}.{}.tmp.webp",
            std::process::id(),
            sequence
        ));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(_) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(ThumbnailFailure::CacheWriteFailed),
        }
    }

    Err(ThumbnailFailure::CacheWriteFailed)
}

fn validate_generated_thumbnail(path: &Path) -> Result<(), ThumbnailFailure> {
    let reader = image::ImageReader::open(path).map_err(|_| ThumbnailFailure::ProbeFailed)?;
    let mut reader = reader
        .with_guessed_format()
        .map_err(|_| ThumbnailFailure::ProbeFailed)?;
    if reader.format() != Some(image::ImageFormat::WebP) {
        return Err(ThumbnailFailure::ProbeFailed);
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_THUMBNAIL_WIDTH);
    limits.max_image_height = Some(MAX_THUMBNAIL_HEIGHT);
    limits.max_alloc = Some(MAX_THUMBNAIL_PIXELS.saturating_mul(4));
    reader.limits(limits);
    let image = reader.decode().map_err(|_| ThumbnailFailure::ProbeFailed)?;
    validate_thumbnail_dimensions(image.width(), image.height())
}

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

fn validate_image_dimensions(width: u32, height: u32) -> Result<(), ThumbnailFailure> {
    let pixels = u64::from(width).saturating_mul(u64::from(height));
    if width == 0
        || height == 0
        || width > MAX_IMAGE_DIMENSION
        || height > MAX_IMAGE_DIMENSION
        || pixels > MAX_IMAGE_PIXELS
    {
        return Err(ThumbnailFailure::ImageTooLarge);
    }
    Ok(())
}

fn validate_thumbnail_dimensions(width: u32, height: u32) -> Result<(), ThumbnailFailure> {
    let pixels = u64::from(width).saturating_mul(u64::from(height));
    if width == 0
        || height == 0
        || width > MAX_THUMBNAIL_WIDTH
        || height > MAX_THUMBNAIL_HEIGHT
        || pixels > MAX_THUMBNAIL_PIXELS
    {
        return Err(ThumbnailFailure::ProbeFailed);
    }
    Ok(())
}

fn thumbnail_resize_bounds(width: u32, height: u32) -> (u32, u32) {
    (
        width.min(MAX_THUMBNAIL_WIDTH),
        height.min(MAX_THUMBNAIL_HEIGHT),
    )
}

fn generate_image_thumbnail_rust(src: &str, dst: &Path) -> Result<bool, ThumbnailFailure> {
    let Ok(reader) = image::ImageReader::open(src) else {
        return Ok(false);
    };
    let Ok(reader) = reader.with_guessed_format() else {
        return Ok(false);
    };
    let Ok((width, height)) = reader.into_dimensions() else {
        return Ok(false);
    };
    validate_image_dimensions(width, height)?;

    let Ok(reader) = image::ImageReader::open(src) else {
        return Ok(false);
    };
    let Ok(mut reader) = reader.with_guessed_format() else {
        return Ok(false);
    };
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_DIMENSION);
    limits.max_image_height = Some(MAX_IMAGE_DIMENSION);
    limits.max_alloc = Some(MAX_IMAGE_DECODE_BYTES);
    reader.limits(limits);
    let Ok(img) = reader.decode() else {
        return Ok(false);
    };
    let width = img.width();
    let height = img.height();
    validate_image_dimensions(width, height)?;
    let (target_width, target_height) = thumbnail_resize_bounds(width, height);
    let resized = img.resize(
        target_width,
        target_height,
        image::imageops::FilterType::Lanczos3,
    );
    validate_thumbnail_dimensions(resized.width(), resized.height())?;
    Ok(resized
        .save_with_format(dst, image::ImageFormat::WebP)
        .is_ok()
        && dst.exists())
}

fn imagemagick_first_frame_source(src: &str) -> String {
    format!("{src}[0]")
}

fn generate_image_thumbnail(src: &str, dst: &Path) -> Result<(), ThumbnailFailure> {
    if generate_image_thumbnail_rust(src, dst)? {
        return Ok(());
    }
    let first_frame_source = imagemagick_first_frame_source(src);
    for program in &["magick", "convert"] {
        if !command_exists(program) {
            continue;
        }
        let mut command = Command::new(program);
        command
            .arg(&first_frame_source)
            .args(["-resize", "400x400>", "-quality", "80", "-auto-orient"])
            .arg(dst);
        if command_succeeded(
            &mut command,
            THUMBNAIL_COMMAND_TIMEOUT,
            "ImageMagick thumbnail",
        ) {
            return Ok(());
        }
    }
    Err(ThumbnailFailure::ProbeFailed)
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
            let mut thumbnailer = Command::new("ffmpegthumbnailer");
            thumbnailer.args(["-i", src, "-o"]).arg(&tmp).args([
                "-t",
                &format!("{}", ts as u64),
                "-s",
                "0",
                "-q",
                "8",
            ]);
            let ok = command_succeeded(
                &mut thumbnailer,
                THUMBNAIL_COMMAND_TIMEOUT,
                "ffmpegthumbnailer GUI thumbnail",
            );
            if ok && tmp.exists() {
                // Convert to 400px webp.
                let _ = std::fs::remove_file(dst);
                let mut ffmpeg = Command::new("ffmpeg");
                ffmpeg.args([
                    "-hide_banner",
                    "-loglevel",
                    "error",
                    "-nostats",
                    "-y",
                    "-i",
                    tmp.to_str().unwrap_or(""),
                    "-vf",
                    FFMPEG_THUMBNAIL_SCALE_FILTER,
                    "-quality",
                    "80",
                    dst.to_str().unwrap_or(""),
                ]);
                let ok = command_succeeded(
                    &mut ffmpeg,
                    THUMBNAIL_COMMAND_TIMEOUT,
                    "ffmpeg GUI thumbnail conversion",
                );
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
        let mut ffmpeg = Command::new("ffmpeg");
        ffmpeg.args([
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
            FFMPEG_THUMBNAIL_SCALE_FILTER,
            "-quality",
            "80",
            dst.to_str().unwrap_or(""),
        ]);
        let ok = command_succeeded(
            &mut ffmpeg,
            THUMBNAIL_COMMAND_TIMEOUT,
            "ffmpeg GUI thumbnail",
        );
        if ok && dst.exists() && frame_has_content(dst) {
            return true;
        }
        let _ = std::fs::remove_file(dst);
    }

    // Last resort: just pick the middle frame regardless of content.
    let ts = duration * 0.5;
    let _ = std::fs::remove_file(dst);
    let mut ffmpeg = Command::new("ffmpeg");
    ffmpeg.args([
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
        FFMPEG_THUMBNAIL_SCALE_FILTER,
        "-quality",
        "80",
        dst.to_str().unwrap_or(""),
    ]);
    let ok = command_succeeded(
        &mut ffmpeg,
        THUMBNAIL_COMMAND_TIMEOUT,
        "ffmpeg GUI fallback thumbnail",
    );
    ok && dst.exists()
}

fn get_video_duration(path: &str) -> Option<f64> {
    let mut ffprobe = Command::new("ffprobe");
    ffprobe
        .args([
            "-v",
            "quiet",
            "-show_entries",
            "format=duration",
            "-of",
            "csv=p=0",
        ])
        .arg("--")
        .arg(path);
    let out = command_output(&mut ffprobe, METADATA_COMMAND_TIMEOUT, "ffprobe duration")?;
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

    #[cfg(target_os = "linux")]
    #[test]
    fn deadline_runner_times_out_kills_group_and_reaps_child() {
        let temp = tempfile::tempdir().unwrap();
        let pid_file = temp.path().join("child.pid");
        let mut command = Command::new("/bin/sh");
        command
            .arg("-c")
            .arg("echo $$ > \"$1\"; sleep 30")
            .arg("deadline-test")
            .arg(&pid_file);

        let started = Instant::now();
        let result = run_command_with_deadline(&mut command, Duration::from_millis(100));

        assert_eq!(result, Err(DeadlineCommandError::TimedOut));
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "timeout path exceeded its bounded drain grace"
        );
        let pid = std::fs::read_to_string(pid_file)
            .unwrap()
            .trim()
            .parse::<u32>()
            .unwrap();
        assert!(
            !Path::new(&format!("/proc/{pid}")).exists(),
            "timed-out direct child was not reaped"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn deadline_runner_does_not_wait_for_detached_pipe_holder() {
        if which::which("setsid").is_err() {
            return;
        }

        let temp = tempfile::tempdir().unwrap();
        let pid_file = temp.path().join("detached.pid");
        let mut command = Command::new("/bin/sh");
        command
            .arg("-c")
            .arg("setsid sh -c 'echo $$ > \"$1\"; sleep 30' detached \"$1\" & exit 0")
            .arg("deadline-test")
            .arg(&pid_file);

        let started = Instant::now();
        let result = run_command_with_deadline(&mut command, Duration::from_secs(2)).unwrap();

        assert!(result.success);
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "escaped descendant kept the drain path blocked"
        );

        let mut detached_pid = None;
        for _ in 0..50 {
            detached_pid = std::fs::read_to_string(&pid_file)
                .ok()
                .and_then(|value| value.trim().parse::<i32>().ok());
            if detached_pid.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let detached_pid = detached_pid.expect("detached descendant should report its pid");
        // SAFETY: the test-created process is a session/process-group leader.
        unsafe {
            libc::kill(-detached_pid, libc::SIGKILL);
        }
    }

    #[cfg(unix)]
    #[test]
    fn deadline_runner_continuously_drains_and_bounds_large_output() {
        let mut command = Command::new("/bin/sh");
        command
            .arg("-c")
            .arg("head -c 1048576 /dev/zero; head -c 1048576 /dev/zero >&2");

        let output = run_command_with_deadline(&mut command, Duration::from_secs(2)).unwrap();

        assert!(output.success);
        assert_eq!(output.stdout.len(), COMMAND_OUTPUT_CAP);
        assert_eq!(output.stderr.len(), COMMAND_OUTPUT_CAP);
    }

    #[test]
    fn image_dimension_limits_cover_each_axis_and_total_pixels() {
        assert_eq!(
            validate_image_dimensions(MAX_IMAGE_DIMENSION + 1, 1),
            Err(ThumbnailFailure::ImageTooLarge)
        );
        assert_eq!(
            validate_image_dimensions(1, MAX_IMAGE_DIMENSION + 1),
            Err(ThumbnailFailure::ImageTooLarge)
        );
        assert_eq!(
            validate_image_dimensions(8_193, 8_193),
            Err(ThumbnailFailure::ImageTooLarge)
        );
        assert_eq!(validate_image_dimensions(8_192, 8_192), Ok(()));
    }

    #[test]
    fn thumbnail_resize_bounds_never_upscale_or_exceed_output_limits() {
        assert_eq!(thumbnail_resize_bounds(16, 9), (16, 9));
        assert_eq!(
            thumbnail_resize_bounds(1, MAX_IMAGE_DIMENSION),
            (1, MAX_THUMBNAIL_HEIGHT)
        );
        assert_eq!(thumbnail_resize_bounds(800, 200), (400, 200));
    }

    #[test]
    fn one_by_max_height_image_generates_a_bounded_thumbnail() {
        let cache = tempfile::tempdir().unwrap();
        let media = tempfile::tempdir().unwrap();
        let image_path = media.path().join("tall.png");
        image::RgbImage::from_pixel(1, MAX_IMAGE_DIMENSION, image::Rgb([120, 80, 40]))
            .save(&image_path)
            .unwrap();

        let result = thumbnail_for(cache.path(), image_path.to_str().unwrap());
        let thumbnail = result.thumbnail.expect("bounded thumbnail");
        let decoded = image::open(thumbnail).unwrap();

        assert_eq!((decoded.width(), decoded.height()), (1, 400));
    }

    #[test]
    fn oversized_image_header_is_rejected_and_failure_is_cached() {
        let cache = tempfile::tempdir().unwrap();
        let media = tempfile::tempdir().unwrap();
        let image_path = media.path().join("oversized.bmp");
        image::RgbImage::from_pixel(1, 1, image::Rgb([1, 2, 3]))
            .save_with_format(&image_path, image::ImageFormat::Bmp)
            .unwrap();
        let mut bytes = std::fs::read(&image_path).unwrap();
        bytes[18..22].copy_from_slice(&(MAX_IMAGE_DIMENSION + 1).to_le_bytes());
        std::fs::write(&image_path, bytes).unwrap();

        let first = thumbnail_for_with_failure_ttl(
            cache.path(),
            image_path.to_str().unwrap(),
            DEFAULT_FAILURE_TTL_SECS,
        );

        assert_eq!(first.failure_reason, Some(ThumbnailFailure::ImageTooLarge));
        assert_eq!(
            first.error.as_deref(),
            Some("thumbnail generation failed: image_too_large")
        );
        assert_eq!(thumbnail_cache_info(cache.path()).failure_entries, 1);

        let second = thumbnail_for_with_failure_ttl(
            cache.path(),
            image_path.to_str().unwrap(),
            DEFAULT_FAILURE_TTL_SECS,
        );
        assert_eq!(second.failure_reason, Some(ThumbnailFailure::ImageTooLarge));
        assert_eq!(
            second.error.as_deref(),
            Some("thumbnail generation skipped: cached image_too_large")
        );
    }

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
        let thumbnail = std::path::Path::new(first.thumbnail.as_ref().unwrap());
        assert!(thumbnail.exists());
        let decoded = image::open(thumbnail).unwrap();
        assert_eq!(
            (decoded.width(), decoded.height()),
            (16, 9),
            "small inputs must not be upscaled"
        );

        let second = thumbnail_for(cache.path(), image_path.to_str().unwrap());
        assert!(
            second.thumbnail.is_some(),
            "second call should return cached thumbnail"
        );
        assert!(second.cache_hit);
    }

    #[test]
    fn concurrent_same_key_generation_publishes_one_valid_thumbnail() {
        let cache = tempfile::tempdir().unwrap();
        let media = tempfile::tempdir().unwrap();
        let image_path = media.path().join("concurrent.png");
        image::RgbImage::from_pixel(800, 800, image::Rgb([20, 120, 220]))
            .save(&image_path)
            .unwrap();
        let cache_path = Arc::new(cache.path().to_path_buf());
        let image_path = Arc::new(image_path);
        let barrier = Arc::new(std::sync::Barrier::new(8));

        let handles = (0..8)
            .map(|_| {
                let cache_path = Arc::clone(&cache_path);
                let image_path = Arc::clone(&image_path);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    thumbnail_for(&cache_path, image_path.to_str().unwrap())
                })
            })
            .collect::<Vec<_>>();
        let results = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>();

        let published = results[0].thumbnail.as_ref().expect("thumbnail");
        assert!(results
            .iter()
            .all(|result| result.thumbnail.as_deref() == Some(published.as_str())));
        validate_generated_thumbnail(Path::new(published)).unwrap();

        let root_files = std::fs::read_dir(cache.path())
            .unwrap()
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            root_files
                .iter()
                .filter(|name| !name.starts_with('.') && name.ends_with(".webp"))
                .count(),
            1
        );
        assert!(
            root_files
                .iter()
                .all(|name| !name.starts_with('.') || !name.ends_with(".tmp.webp")),
            "all producer-owned temporary files must be removed"
        );
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
