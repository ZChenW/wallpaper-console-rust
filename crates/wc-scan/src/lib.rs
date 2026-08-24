//! wc-scan — recursive wallpaper scanning, library index building.

pub mod source_scan;
pub use source_scan::*;
mod steam_discovery;
pub use steam_discovery::{discover_steam_workshop_roots, steam_workshop_root_candidates};

use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use camino::Utf8PathBuf;
use walkdir::WalkDir;
use wc_core::formats;
use wc_core::types::{Backend, FileType, WallpaperEntry, WallpaperProject};

const CANCEL_CHECK_INTERVAL: usize = 500;
const RESOLUTION_PROBE_TIMEOUT: Duration = Duration::from_secs(2);
const RESOLUTION_PROBE_OUTPUT_CAP: usize = 32 * 1024;
const RESOLUTION_PROBE_POLL_INTERVAL: Duration = Duration::from_millis(10);

#[cfg(test)]
use steam_discovery::{
    discover_steam_workshop_roots_with_xdg_data_home, STEAM_LIBRARY_FOLDERS_SIZE_CAP,
};

const WE_MARKER: &str = "/steamapps/workshop/content/431960";
/// Flatpak Steam installs workshop content under a different prefix.
const FLATPAK_WE_MARKER: &str =
    "/.var/app/com.valvesoftware.Steam/data/Steam/steamapps/workshop/content/431960";

/// Deduplicate sources by canonical path before scanning.
pub fn dedupe_sources(sources: &[String]) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut result: Vec<String> = Vec::new();
    for src in sources {
        let p = Path::new(src);
        if !p.is_dir() {
            continue;
        }
        let canon = std::fs::canonicalize(p)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| src.clone());
        if seen.insert(canon.clone()) {
            result.push(canon);
        }
    }
    result
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanEvent {
    SourceStarted { source: String },
    CandidateFound { path: String, count: usize },
    WalkProgress { entries_visited: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanControl {
    Continue,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanVisitControl {
    Continue,
    Cancel,
}

enum CandidateSink<'a> {
    Collect(&'a mut Vec<String>),
    Stream,
}

impl CandidateSink<'_> {
    fn push_if_collecting(&mut self, path: String) {
        if let CandidateSink::Collect(files) = self {
            files.push(path);
        }
    }
}

/// Scan all sources for wallpaper files with a callback for progress/cancellation.
pub fn scan_wallpapers_with_callback<F>(sources: &[String], mut on_event: F) -> Vec<String>
where
    F: FnMut(ScanEvent) -> ScanControl,
{
    let mut files: Vec<String> = Vec::new();
    let mut sink = CandidateSink::Collect(&mut files);
    let mut noop = |_| ScanVisitControl::Continue;
    scan_wallpapers_with_visitor(sources, &mut sink, &mut 0usize, &mut on_event, &mut noop);
    files
}

/// Legacy compatibility wrapper — scans without cancellation support.
pub fn scan_wallpapers(sources: &[String]) -> Vec<String> {
    scan_wallpapers_with_callback(sources, |_| ScanControl::Continue)
}

/// Stream wallpaper paths through a visitor callback. Paths are yielded
/// as they are discovered — no full Vec is accumulated in this call.
/// Returns true if the visitor cancelled.
pub fn visit_wallpapers_with_callback<F, V>(
    sources: &[String],
    mut on_event: F,
    mut on_candidate: V,
) -> bool
where
    F: FnMut(ScanEvent) -> ScanControl,
    V: FnMut(String) -> ScanVisitControl,
{
    let mut cancelled = false;
    let mut sink = CandidateSink::Stream;
    let mut wrapper = |path| {
        if matches!(on_candidate(path), ScanVisitControl::Cancel) {
            cancelled = true;
            ScanVisitControl::Cancel
        } else {
            ScanVisitControl::Continue
        }
    };
    scan_wallpapers_with_visitor(sources, &mut sink, &mut 0usize, &mut on_event, &mut wrapper);
    cancelled
}

fn scan_wallpapers_with_visitor<F, V>(
    sources: &[String],
    sink: &mut CandidateSink<'_>,
    count: &mut usize,
    on_event: &mut F,
    on_candidate: &mut V,
) where
    F: FnMut(ScanEvent) -> ScanControl,
    V: FnMut(String) -> ScanVisitControl,
{
    let deduped = dedupe_sources(sources);
    let mut seen: HashSet<String> = HashSet::new();

    for source in &deduped {
        if matches!(
            on_event(ScanEvent::SourceStarted {
                source: source.clone()
            }),
            ScanControl::Cancel
        ) {
            break;
        }
        let src_path = Path::new(source);
        if !src_path.is_dir() {
            continue;
        }
        let cancelled = match we_source_kind(source) {
            WeKind::WorkshopRoot => scan_we_workshop_root_with_callback(
                src_path,
                &mut seen,
                sink,
                count,
                on_event,
                on_candidate,
            ),
            WeKind::ProjectDir => scan_we_project_dir_with_callback(
                src_path,
                &mut seen,
                sink,
                count,
                on_event,
                on_candidate,
            ),
            WeKind::Normal => scan_dir_recursive_with_callback(
                src_path,
                &mut seen,
                sink,
                count,
                on_event,
                on_candidate,
            ),
        };
        if cancelled {
            break;
        }
    }
}

/// Classify a WE source path.
#[derive(Debug, PartialEq)]
enum WeKind {
    /// .../431960 — iterate subdirectories as projects.
    WorkshopRoot,
    /// .../431960/<project_id> — read this project's project.json.
    ProjectDir,
    /// Not a WE path at all.
    Normal,
}

fn find_we_marker(path: &str) -> Option<(usize, &'static str)> {
    for marker in [WE_MARKER, FLATPAK_WE_MARKER] {
        for (position, _) in path.match_indices(marker) {
            let end = position + marker.len();
            if end == path.len() || path.as_bytes().get(end) == Some(&b'/') {
                return Some((position, marker));
            }
        }
    }
    None
}

fn we_source_kind(path: &str) -> WeKind {
    let Some((pos, marker)) = find_we_marker(path) else {
        return WeKind::Normal;
    };
    let after = path[pos + marker.len()..].trim_start_matches('/');
    if !after.is_empty() {
        let first_seg = after.split('/').next().unwrap_or("");
        if first_seg.chars().all(|c| c.is_ascii_digit()) {
            return WeKind::ProjectDir;
        }
    }
    WeKind::WorkshopRoot
}

/// Scan a Wallpaper Engine workshop root with cancellation support.
fn scan_we_workshop_root_with_callback<F, V>(
    root: &Path,
    seen: &mut HashSet<String>,
    sink: &mut CandidateSink<'_>,
    count: &mut usize,
    on_event: &mut F,
    on_candidate: &mut V,
) -> bool
where
    F: FnMut(ScanEvent) -> ScanControl,
    V: FnMut(String) -> ScanVisitControl,
{
    let entries = match fs::read_dir(root) {
        Ok(e) => e,
        Err(_) => return false,
    };

    let mut visited = 0usize;

    for entry in entries.filter_map(|e| e.ok()) {
        visited += 1;
        if visited.is_multiple_of(CANCEL_CHECK_INTERVAL)
            && matches!(
                on_event(ScanEvent::WalkProgress {
                    entries_visited: visited
                }),
                ScanControl::Cancel
            )
        {
            return true;
        }

        let ftype = entry.file_type().ok();
        if !ftype.map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let project_dir = entry.path();

        let has_proj = project_dir.join("project.json").exists();
        let we_file = indexed_we_project_path(&project_dir);

        if let Some(ref wp) = we_file {
            let p = Path::new(wp);
            let c = canonicalize_str(p);
            if seen.insert(c.clone()) {
                sink.push_if_collecting(c.clone());
                *count += 1;
                if matches!(
                    on_event(ScanEvent::CandidateFound {
                        path: c.clone(),
                        count: *count
                    }),
                    ScanControl::Cancel
                ) {
                    return true;
                }
                if matches!(on_candidate(c), ScanVisitControl::Cancel) {
                    return true;
                }
            }
            continue;
        }

        if has_proj {
            continue;
        }

        let cancelled = scan_dir_recursive_with_callback(
            &project_dir,
            seen,
            sink,
            count,
            on_event,
            on_candidate,
        );
        if cancelled {
            return true;
        }
    }
    false
}

/// Scan a single WE project directory with cancellation support.
fn scan_we_project_dir_with_callback<F, V>(
    project_dir: &Path,
    seen: &mut HashSet<String>,
    sink: &mut CandidateSink<'_>,
    count: &mut usize,
    on_event: &mut F,
    on_candidate: &mut V,
) -> bool
where
    F: FnMut(ScanEvent) -> ScanControl,
    V: FnMut(String) -> ScanVisitControl,
{
    if let Some(wp) = indexed_we_project_path(project_dir) {
        let p = Path::new(&wp);
        let c = canonicalize_str(p);
        if seen.insert(c.clone()) {
            sink.push_if_collecting(c.clone());
            *count += 1;
            if matches!(
                on_event(ScanEvent::CandidateFound {
                    path: c.clone(),
                    count: *count
                }),
                ScanControl::Cancel
            ) {
                return true;
            }
            if matches!(on_candidate(c), ScanVisitControl::Cancel) {
                return true;
            }
        }
    }
    false
}

/// Recursively scan a directory for supported wallpaper files with cancellation support.
fn scan_dir_recursive_with_callback<F, V>(
    dir: &Path,
    seen: &mut HashSet<String>,
    sink: &mut CandidateSink<'_>,
    count: &mut usize,
    on_event: &mut F,
    on_candidate: &mut V,
) -> bool
where
    F: FnMut(ScanEvent) -> ScanControl,
    V: FnMut(String) -> ScanVisitControl,
{
    let mut visited = 0usize;

    for entry in WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        visited += 1;
        if visited.is_multiple_of(CANCEL_CHECK_INTERVAL)
            && matches!(
                on_event(ScanEvent::WalkProgress {
                    entries_visited: visited
                }),
                ScanControl::Cancel
            )
        {
            return true;
        }

        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if formats::is_preview_filename(name) {
            continue;
        }
        if let Some(ext) = formats::get_extension(&path.to_string_lossy()) {
            if formats::is_supported_extension(&ext) {
                let canonical = canonicalize_str(path);
                if seen.insert(canonical.clone()) {
                    sink.push_if_collecting(canonical.clone());
                    *count += 1;
                    if matches!(
                        on_event(ScanEvent::CandidateFound {
                            path: canonical.clone(),
                            count: *count
                        }),
                        ScanControl::Cancel
                    ) {
                        return true;
                    }
                    if matches!(on_candidate(canonical), ScanVisitControl::Cancel) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn canonicalize_str(p: &Path) -> String {
    std::fs::canonicalize(p)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| p.to_string_lossy().to_string())
}

pub fn normalize_source_path(path: &str) -> String {
    if let Some(we_root) = we_workshop_root(path) {
        return canonicalize_str(Path::new(&we_root));
    }
    canonicalize_str(Path::new(path))
}

fn we_workshop_root(path: &str) -> Option<String> {
    let (pos, marker) = find_we_marker(path)?;
    let after = path[pos + marker.len()..].trim_start_matches('/');
    let first_seg = after.split('/').next().unwrap_or("");
    if !first_seg.is_empty() && first_seg.chars().all(|c| c.is_ascii_digit()) {
        return Some(path[..pos + marker.len()].to_string());
    }
    None
}

pub fn is_wallpaper_engine_source(path: &str) -> bool {
    find_we_marker(path).is_some()
}

/// Parsed Wallpaper Engine project.json metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeProjectInfo {
    pub project_dir: String,
    pub project_type: String,
    pub file: Option<String>,
    pub preview_path: Option<String>,
    pub workshop_id: Option<String>,
    pub title: Option<String>,
    pub entry_type: FileType,
    pub backend: Backend,
    pub unsupported_reason: Option<String>,
}

impl WeProjectInfo {
    fn project_entry_path(&self) -> String {
        self.project_dir.clone()
    }

    fn file_entry_path(&self) -> Option<String> {
        let file = self.file.as_ref()?;
        let root = Path::new(&self.project_dir);
        let full = safe_join(root, file).ok()?;
        if full.is_file() {
            Some(full.to_string_lossy().to_string())
        } else {
            None
        }
    }

    fn wallpaper_project(&self) -> WallpaperProject {
        WallpaperProject {
            project_type: self.entry_type.as_str().to_string(),
            preview_path: self.preview_path.clone(),
            workshop_id: self.workshop_id.clone(),
            title: self.title.clone(),
            we_file: self.file.clone(),
            backend: Some(self.backend.as_str().to_string()),
            unsupported_reason: self.unsupported_reason.clone(),
        }
    }
}

/// Read and classify a Wallpaper Engine project.json from `project_dir`.
pub fn read_we_project_info(project_dir: &Path) -> Option<WeProjectInfo> {
    let proj_path = project_dir.join("project.json");
    if !proj_path.exists() {
        return None;
    }
    let content = fs::read_to_string(&proj_path).ok()?;
    let proj: serde_json::Value = serde_json::from_str(&content).ok()?;
    we_project_info_from_json(project_dir, &proj)
}

/// Classify an already-parsed Wallpaper Engine `project.json` value.
pub fn we_project_info_from_json(
    project_dir: &Path,
    proj: &serde_json::Value,
) -> Option<WeProjectInfo> {
    let raw_type = proj.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let normalized_type = raw_type.trim().to_lowercase();
    let file = proj
        .get("file")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let preview_path = proj
        .get("preview")
        .and_then(|v| v.as_str())
        .and_then(|preview| safe_join(project_dir, preview).ok())
        .filter(|preview| preview.is_file())
        .map(|preview| preview.to_string_lossy().to_string());
    let title = proj
        .get("title")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let workshop_id = workshop_id_from_path(project_dir);
    let (entry_type, backend, unsupported_reason) = match normalized_type.as_str() {
        "scene" => (FileType::WeScene, Backend::LinuxWallpaperEngine, None),
        "web" => (
            FileType::WeWeb,
            Backend::Unsupported,
            Some("Wallpaper Engine Web projects are indexed for browsing only and cannot be applied by this app.".to_string()),
        ),
        "application" => (
            FileType::WeApplication,
            Backend::Unsupported,
            Some("Wallpaper Engine application projects are not supported.".to_string()),
        ),
        "image" | "gif" | "video" => {
            let file = file.as_deref()?;
            let ext = formats::get_extension(file)?;
            let (ftype, backend) = formats::classify_extension(&ext)?;
            (ftype, backend, None)
        }
        other => (
            FileType::WeApplication,
            Backend::Unsupported,
            Some(format!(
                "Unsupported Wallpaper Engine project type: {}",
                other
            )),
        ),
    };

    Some(WeProjectInfo {
        project_dir: project_dir.to_string_lossy().to_string(),
        project_type: normalized_type,
        file,
        preview_path,
        workshop_id,
        title,
        entry_type,
        backend,
        unsupported_reason,
    })
}

/// Read a Wallpaper Engine project.json from `project_dir` and return
/// the indexed file path (real media file for image/video/gif, or
/// None for scene/web/application which are handled at the project level).
pub fn read_we_project_json(project_dir: &Path) -> Option<String> {
    let info = read_we_project_info(project_dir)?;
    match info.entry_type {
        FileType::Image | FileType::Gif | FileType::Video => info.file_entry_path(),
        _ => None,
    }
}

fn indexed_we_project_path(project_dir: &Path) -> Option<String> {
    let info = read_we_project_info(project_dir)?;
    match info.entry_type {
        FileType::WeScene | FileType::WeWeb | FileType::WeApplication => {
            Some(info.project_entry_path())
        }
        FileType::Image | FileType::Gif | FileType::Video => info.file_entry_path(),
    }
}

/// Safely resolve a relative file path under `root`, rejecting traversal,
/// absolute paths, and symlink escapes.
pub fn safe_join(root: &Path, file: &str) -> Result<std::path::PathBuf, String> {
    let file_path = Path::new(file);
    for comp in file_path.components() {
        match comp {
            std::path::Component::ParentDir => {
                return Err("path traversal rejected".to_string());
            }
            std::path::Component::RootDir | std::path::Component::Prefix(_) => {
                return Err("absolute path rejected".to_string());
            }
            _ => {}
        }
    }
    let joined = root.join(file_path);
    let root_canon = root
        .canonicalize()
        .map_err(|e| format!("cannot canonicalize root: {}", e))?;
    let joined_canon = joined
        .canonicalize()
        .map_err(|e| format!("cannot canonicalize candidate: {}", e))?;
    if !joined_canon.starts_with(&root_canon) {
        return Err("symlink escape rejected".to_string());
    }
    Ok(joined_canon)
}

pub fn workshop_id_from_path(project_dir: &Path) -> Option<String> {
    let path_str = project_dir.to_string_lossy();
    let (pos, marker) = find_we_marker(&path_str)?;
    let after = path_str[pos + marker.len()..].trim_start_matches('/');
    let first_seg = after.split('/').next()?;
    if !first_seg.is_empty() && first_seg.chars().all(|c| c.is_ascii_digit()) {
        Some(first_seg.to_string())
    } else {
        None
    }
}

/// If a regular file lies inside a WE project directory and matches the
/// file field of project.json (type=image/gif/video), return its WallpaperProject
/// metadata so title / preview_path / workshop_id / we_file are preserved.
fn try_we_project_metadata(file_path: &Path) -> Option<WallpaperProject> {
    let parent = file_path.parent()?;
    if !is_wallpaper_engine_source(parent.to_string_lossy().as_ref()) {
        return None;
    }
    let info = read_we_project_info(parent)?;
    match info.entry_type {
        FileType::Image | FileType::Gif | FileType::Video => {
            let we_file = info.file.as_ref()?;
            if file_path.file_name().and_then(|n| n.to_str()) == Some(we_file.as_str()) {
                Some(info.wallpaper_project())
            } else {
                None
            }
        }
        _ => None,
    }
}

fn make_we_project_entry(project_dir: &Path) -> Option<WallpaperEntry> {
    make_we_project_entry_cached(project_dir, &std::collections::HashMap::new()).0
}

/// Build a Wallpaper Engine project entry while reusing only expensive media
/// resolution metadata. Project metadata always comes from the latest
/// project.json parse.
pub(crate) fn make_we_project_entry_cached(
    project_dir: &Path,
    cache: &std::collections::HashMap<String, WallpaperEntry>,
) -> (Option<WallpaperEntry>, bool) {
    let canonical_project = match std::fs::canonicalize(project_dir) {
        Ok(path) => path,
        Err(_) => return (None, false),
    };
    let info = match read_we_project_info(&canonical_project) {
        Some(info) => info,
        None => return (None, false),
    };
    make_we_project_entry_from_info(canonical_project, info, cache)
}

/// Like [`make_we_project_entry_cached`], but reuses an already-parsed
/// [`WeProjectInfo`] so callers that validated `project.json` need not reread it.
pub(crate) fn make_we_project_entry_from_info(
    canonical_project: PathBuf,
    info: WeProjectInfo,
    cache: &std::collections::HashMap<String, WallpaperEntry>,
) -> (Option<WallpaperEntry>, bool) {
    if matches!(
        info.entry_type,
        FileType::Image | FileType::Gif | FileType::Video
    ) {
        let Some(file) = info.file.as_ref() else {
            return (None, false);
        };
        let media_path = match safe_join(&canonical_project, file)
            .ok()
            .and_then(|path| std::fs::canonicalize(path).ok())
        {
            Some(path) => path,
            None => return (None, false),
        };
        if !media_path.is_file() {
            return (None, false);
        }
        let Some(ext) = formats::get_extension(file) else {
            return (None, false);
        };
        let meta = match fs::metadata(&media_path) {
            Ok(meta) => meta,
            Err(_) => return (None, false),
        };
        let size = meta.len();
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let path = media_path.to_string_lossy().to_string();
        let (resolution, reused) = cache
            .get(&path)
            .filter(|prior| prior.size == size && prior.mtime == mtime)
            .map(|prior| (prior.resolution.clone(), true))
            .unwrap_or_else(|| (detect_resolution(&path, info.entry_type), false));
        return (
            Some(WallpaperEntry {
                path: Utf8PathBuf::from(path),
                file_type: info.entry_type,
                ext,
                backend: info.backend,
                size,
                mtime,
                resolution,
                project: Some(info.wallpaper_project()),
            }),
            reused,
        );
    }

    let project_json = canonical_project.join("project.json");
    let meta = fs::metadata(&project_json).or_else(|_| fs::metadata(&canonical_project));
    let Ok(meta) = meta else {
        return (None, false);
    };
    let size = project_entry_size_hint(&canonical_project, info.preview_path.as_deref());
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let ext = match info.entry_type {
        FileType::WeScene => "scene",
        FileType::WeWeb => "web",
        FileType::WeApplication => "application",
        FileType::Image | FileType::Gif | FileType::Video => unreachable!(),
    }
    .to_string();

    (
        Some(WallpaperEntry {
            path: Utf8PathBuf::from(info.project_entry_path()),
            file_type: info.entry_type,
            ext,
            backend: info.backend,
            size,
            mtime,
            resolution: "WE".to_string(),
            project: Some(info.wallpaper_project()),
        }),
        false,
    )
}

fn project_entry_size_hint(project_dir: &Path, preview_path: Option<&str>) -> u64 {
    let mut size = fs::metadata(project_dir.join("project.json"))
        .map(|m| m.len())
        .unwrap_or(0);
    if let Some(preview) = preview_path {
        size += fs::metadata(preview).map(|m| m.len()).unwrap_or(0);
    }
    size
}

pub fn make_entry(path: &str) -> Option<WallpaperEntry> {
    let p = Path::new(path);
    if p.is_dir() {
        return make_we_project_entry(p);
    }
    if !p.is_file() {
        return None;
    }
    let ext = formats::get_extension(path)?;
    let (ftype, backend) = formats::classify_extension(&ext)?;
    let meta = fs::metadata(path).ok()?;
    let size = meta.len();
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let resolution = detect_resolution(path, ftype);
    Some(WallpaperEntry {
        path: Utf8PathBuf::from(path),
        file_type: ftype,
        ext,
        backend,
        size,
        mtime,
        resolution,
        project: try_we_project_metadata(p),
    })
}

/// Detect resolution using the right tool per file type:
///   - image/gif → identify (ImageMagick)
///   - video    → ffprobe (never runs identify on video)
fn detect_resolution(path: &str, ftype: wc_core::types::FileType) -> String {
    match ftype {
        wc_core::types::FileType::Image | wc_core::types::FileType::Gif => {
            let first_frame = format!("{path}[0]");
            let args = ["-format", "%wx%h", "--", first_frame.as_str()];
            if let Some(output) = resolution_probe("identify", &args, path) {
                let s = String::from_utf8_lossy(&output).to_string();
                if !s.is_empty() && s.contains('x') {
                    return s;
                }
            }
        }
        wc_core::types::FileType::Video => {
            let args = [
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=width,height",
                "-of",
                "csv=p=0",
                path,
            ];
            if let Some(output) = resolution_probe("ffprobe", &args, path) {
                let s = String::from_utf8_lossy(&output).trim().to_string();
                let parts: Vec<&str> = s.split(',').collect();
                if parts.len() >= 2 {
                    return format!("{}x{}", parts[0], parts[1]);
                }
            }
        }
        wc_core::types::FileType::WeScene
        | wc_core::types::FileType::WeWeb
        | wc_core::types::FileType::WeApplication => {}
    }
    "?x?".to_string()
}

fn resolution_probe(program: &str, args: &[&str], path: &str) -> Option<Vec<u8>> {
    match run_command_with_deadline(program, args, RESOLUTION_PROBE_TIMEOUT) {
        Ok(output) if output.success => Some(output.stdout),
        Ok(output) => {
            let detail = String::from_utf8_lossy(&output.stderr);
            let detail = if detail.trim().is_empty() {
                String::new()
            } else {
                format!(": {}", detail.trim())
            };
            report_resolution_probe_issue(
                program,
                "unsuccessful",
                format!(
                    "could not inspect {path:?}: command exited unsuccessfully{detail}; \
                     further {program} exit failures will be suppressed"
                ),
            );
            None
        }
        Err(DeadlineCommandError::TimedOut) => {
            report_resolution_probe_issue(
                program,
                "timeout",
                format!(
                    "timed out after {} ms while inspecting {path:?}; verify the file and the \
                     installed {program} executable; further {program} timeouts will be suppressed",
                    RESOLUTION_PROBE_TIMEOUT.as_millis()
                ),
            );
            None
        }
        Err(DeadlineCommandError::Spawn(message)) => {
            report_resolution_probe_issue(
                program,
                "spawn",
                format!(
                    "could not start {program} while inspecting {path:?}: {message}; install \
                     {program} or configure a working executable; further spawn failures will be \
                     suppressed"
                ),
            );
            None
        }
        Err(DeadlineCommandError::Wait(message)) => {
            report_resolution_probe_issue(
                program,
                "wait",
                format!(
                    "{program} failed while inspecting {path:?}: {message}; verify the file and \
                     retry; further {program} wait failures will be suppressed"
                ),
            );
            None
        }
    }
}

fn report_resolution_probe_issue(program: &str, kind: &str, message: String) {
    static REPORTED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    if should_report_resolution_probe_issue(REPORTED.get_or_init(Mutex::default), program, kind) {
        eprintln!("wc-scan: {message}");
    }
}

fn should_report_resolution_probe_issue(
    reported: &Mutex<HashSet<String>>,
    program: &str,
    kind: &str,
) -> bool {
    reported
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(format!("{program}:{kind}"))
}

#[derive(Debug)]
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

/// Execute a metadata probe within a hard deadline.
///
/// Probes run in their own process group so a timeout also terminates helper
/// descendants. Stdout/stderr are continuously drained into bounded buffers,
/// preventing both pipe deadlocks and unbounded diagnostic memory.
fn run_command_with_deadline(
    program: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<DeadlineCommandOutput, DeadlineCommandError> {
    #[cfg(unix)]
    use std::os::unix::process::CommandExt;

    let mut command = Command::new(program);
    command
        .args(args)
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
        .map(spawn_bounded_output_drainer)
        .expect("piped stdout must be available");
    let stderr = child
        .stderr
        .take()
        .map(spawn_bounded_output_drainer)
        .expect("piped stderr must be available");
    let started = Instant::now();

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                kill_spawned_process_group(child_pid);
                std::thread::sleep(RESOLUTION_PROBE_POLL_INTERVAL);
                return Ok(DeadlineCommandOutput {
                    success: status.success(),
                    stdout: snapshot_drainer(&stdout),
                    stderr: snapshot_drainer(&stderr),
                });
            }
            Ok(None) if started.elapsed() >= timeout => {
                kill_spawned_process_group(child_pid);
                let _ = child.kill();
                let _ = child.wait();
                return Err(DeadlineCommandError::TimedOut);
            }
            Ok(None) => {
                std::thread::sleep(
                    RESOLUTION_PROBE_POLL_INTERVAL.min(timeout.saturating_sub(started.elapsed())),
                );
            }
            Err(error) => {
                kill_spawned_process_group(child_pid);
                let _ = child.kill();
                let _ = child.wait();
                return Err(DeadlineCommandError::Wait(error.to_string()));
            }
        }
    }
}

fn spawn_bounded_output_drainer(mut stream: impl Read + Send + 'static) -> Arc<Mutex<Vec<u8>>> {
    let captured = Arc::new(Mutex::new(Vec::with_capacity(RESOLUTION_PROBE_OUTPUT_CAP)));
    let writer = Arc::clone(&captured);
    std::thread::spawn(move || {
        let mut chunk = [0_u8; 4096];
        loop {
            match stream.read(&mut chunk) {
                Ok(0) | Err(_) => return,
                Ok(read) => {
                    let mut captured = writer.lock().unwrap_or_else(|error| error.into_inner());
                    let remaining = RESOLUTION_PROBE_OUTPUT_CAP.saturating_sub(captured.len());
                    captured.extend_from_slice(&chunk[..read.min(remaining)]);
                }
            }
        }
    });
    captured
}

fn snapshot_drainer(captured: &Arc<Mutex<Vec<u8>>>) -> Vec<u8> {
    captured
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

#[cfg(unix)]
fn kill_spawned_process_group(pid: u32) {
    let Ok(pid) = i32::try_from(pid) else {
        return;
    };
    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }
    // SAFETY: `run_command_with_deadline` creates a fresh process group whose
    // ID is the direct child PID. A negative PID targets only that group.
    unsafe {
        kill(-pid, 9);
    }
}

#[cfg(not(unix))]
fn kill_spawned_process_group(_pid: u32) {}

/// Build an entry for a wallpaper file, reusing prior metadata when the file's
/// size and mtime haven't changed.  Returns (entry, reused).
pub fn make_entry_cached(
    path: &str,
    cache: &std::collections::HashMap<String, WallpaperEntry>,
) -> (Option<WallpaperEntry>, bool) {
    let p = Path::new(path);
    let meta = match fs::metadata(p) {
        Ok(m) => m,
        Err(_) => return (None, false),
    };
    if meta.is_dir() {
        return make_we_project_entry_cached(p, cache);
    }
    if !meta.is_file() {
        return (None, false);
    }
    let ext = match formats::get_extension(path) {
        Some(e) => e,
        None => return (None, false),
    };
    let (ftype, backend) = match formats::classify_extension(&ext) {
        Some(fb) => fb,
        None => return (None, false),
    };
    let size = meta.len();
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // Check prior cache — same canonical path + same size + same mtime = reuse.
    let canon = std::fs::canonicalize(p)
        .map(|cp| cp.to_string_lossy().to_string())
        .unwrap_or_else(|_| path.to_string());
    if let Some(prior) = cache.get(&canon) {
        if prior.size == size && prior.mtime == mtime {
            return (
                Some(WallpaperEntry {
                    path: Utf8PathBuf::from(path),
                    resolution: prior.resolution.clone(),
                    file_type: prior.file_type,
                    ext: prior.ext.clone(),
                    backend: prior.backend,
                    size,
                    mtime,
                    project: prior.project.clone(),
                }),
                true, // reused
            );
        }
    }

    // Probe resolution.
    let resolution = detect_resolution(path, ftype);
    (
        Some(WallpaperEntry {
            path: Utf8PathBuf::from(path),
            file_type: ftype,
            ext,
            backend,
            size,
            mtime,
            resolution,
            project: try_we_project_metadata(p),
        }),
        false, // probed
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn we_source_kind_workshop_root() {
        assert_eq!(
            we_source_kind("/home/user/.steam/steam/steamapps/workshop/content/431960"),
            WeKind::WorkshopRoot
        );
        assert_eq!(
            we_source_kind("/home/user/.steam/steam/steamapps/workshop/content/431960/"),
            WeKind::WorkshopRoot
        );
    }

    #[test]
    fn we_source_kind_project_dir() {
        assert_eq!(
            we_source_kind(
                "/home/user/.local/share/Steam/steamapps/workshop/content/431960/123456"
            ),
            WeKind::ProjectDir
        );
    }

    #[test]
    fn we_source_kind_normal() {
        assert_eq!(we_source_kind("/home/user/Pictures"), WeKind::Normal);
    }

    #[test]
    fn steam_workshop_root_candidates_cover_native_and_flatpak_paths() {
        let home = Path::new("/home/user");
        let candidates = steam_workshop_root_candidates(home);
        let as_text: Vec<String> = candidates
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        assert!(as_text
            .iter()
            .any(|p| p.ends_with(".local/share/Steam/steamapps/workshop/content/431960")));
        assert!(as_text
            .iter()
            .any(|p| p.ends_with(".steam/steam/steamapps/workshop/content/431960")));
        assert!(as_text
            .iter()
            .any(|p| p.ends_with(".steam/root/steamapps/workshop/content/431960")));
        assert!(as_text.iter().any(|p| p.ends_with(
            ".var/app/com.valvesoftware.Steam/data/Steam/steamapps/workshop/content/431960"
        )));
    }

    #[test]
    fn discover_steam_workshop_roots_covers_xdg_and_legacy_install_roots() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let xdg_data_home = tmp.path().join("xdg-data");
        let xdg_workshop = xdg_data_home.join("Steam/steamapps/workshop/content/431960");
        let legacy_workshop = home.join("Steam/steamapps/workshop/content/431960");
        std::fs::create_dir_all(&xdg_workshop).unwrap();
        std::fs::create_dir_all(&legacy_workshop).unwrap();

        let roots = discover_steam_workshop_roots_with_xdg_data_home(&home, Some(&xdg_data_home));

        assert_eq!(
            roots.into_iter().collect::<HashSet<_>>(),
            HashSet::from([
                std::fs::canonicalize(xdg_workshop).unwrap(),
                std::fs::canonicalize(legacy_workshop).unwrap(),
            ])
        );
    }

    #[test]
    fn discover_steam_workshop_roots_deduplicates_symlinked_roots() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let native = home
            .join(".local/share/Steam")
            .join("steamapps/workshop/content/431960");
        std::fs::create_dir_all(&native).unwrap();

        let alias_parent = home.join(".steam");
        std::fs::create_dir_all(&alias_parent).unwrap();
        std::os::unix::fs::symlink(home.join(".local/share/Steam"), alias_parent.join("steam"))
            .unwrap();

        let roots = discover_steam_workshop_roots(home);
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0], std::fs::canonicalize(native).unwrap());
    }

    #[test]
    fn discover_steam_workshop_roots_includes_configured_steam_libraries() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let steam_root = home.join(".local/share/Steam");
        let external_library = tmp.path().join("games/SteamLibrary");
        let workshop = external_library.join("steamapps/workshop/content/431960");
        std::fs::create_dir_all(steam_root.join("steamapps")).unwrap();
        std::fs::create_dir_all(&workshop).unwrap();
        std::fs::write(
            steam_root.join("steamapps/libraryfolders.vdf"),
            format!(
                r#""libraryfolders"
{{
    "0"
    {{
        "path" "{}"
        "apps"
        {{
            "431960" "1"
        }}
    }}
}}"#,
                external_library.to_string_lossy()
            ),
        )
        .unwrap();

        let roots = discover_steam_workshop_roots(&home);

        assert_eq!(roots, vec![std::fs::canonicalize(workshop).unwrap()]);
    }

    #[test]
    fn discover_steam_workshop_roots_reads_the_client_config_copy() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let steam_root = home.join(".local/share/Steam");
        let external_library = tmp.path().join("external/SteamLibrary");
        let workshop = external_library.join("steamapps/workshop/content/431960");
        std::fs::create_dir_all(steam_root.join("config")).unwrap();
        std::fs::create_dir_all(&workshop).unwrap();
        std::fs::write(
            steam_root.join("config/libraryfolders.vdf"),
            format!(
                r#""libraryfolders"
{{
    "1" "{}"
}}"#,
                external_library.to_string_lossy()
            ),
        )
        .unwrap();

        let roots = discover_steam_workshop_roots_with_xdg_data_home(&home, None);

        assert_eq!(roots, vec![std::fs::canonicalize(workshop).unwrap()]);
    }

    #[test]
    fn discover_steam_workshop_roots_ignores_oversized_library_configuration() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let steam_root = home.join(".local/share/Steam");
        let external_library = tmp.path().join("external/SteamLibrary");
        let workshop = external_library.join("steamapps/workshop/content/431960");
        std::fs::create_dir_all(steam_root.join("config")).unwrap();
        std::fs::create_dir_all(&workshop).unwrap();
        let mut config = format!(
            r#""libraryfolders"
{{
    "1" "{}"
}}"#,
            external_library.to_string_lossy()
        );
        config.push_str(&" ".repeat(STEAM_LIBRARY_FOLDERS_SIZE_CAP));
        std::fs::write(steam_root.join("config/libraryfolders.vdf"), config).unwrap();

        let roots = discover_steam_workshop_roots_with_xdg_data_home(&home, None);

        assert!(roots.is_empty());
    }

    #[test]
    fn discover_steam_workshop_roots_reads_flatpak_external_library() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let steam_root = home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam");
        let external_library = tmp.path().join("mounted/SteamLibrary");
        let workshop = external_library.join("steamapps/workshop/content/431960");
        std::fs::create_dir_all(steam_root.join("config")).unwrap();
        std::fs::create_dir_all(&workshop).unwrap();
        std::fs::write(
            steam_root.join("config/libraryfolders.vdf"),
            format!(
                r#""libraryfolders"
{{
    "1" {{ "path" "{}" }}
}}"#,
                external_library.to_string_lossy()
            ),
        )
        .unwrap();

        let roots = discover_steam_workshop_roots_with_xdg_data_home(&home, None);

        assert_eq!(roots, vec![std::fs::canonicalize(workshop).unwrap()]);
    }

    #[test]
    fn malformed_library_configuration_does_not_hide_fixed_root() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let steam_root = home.join(".local/share/Steam");
        let workshop = steam_root.join("steamapps/workshop/content/431960");
        std::fs::create_dir_all(steam_root.join("config")).unwrap();
        std::fs::create_dir_all(&workshop).unwrap();
        std::fs::write(
            steam_root.join("config/libraryfolders.vdf"),
            r#""libraryfolders" { "1" { "path" "unterminated }"#,
        )
        .unwrap();

        let roots = discover_steam_workshop_roots_with_xdg_data_home(&home, None);

        assert_eq!(roots, vec![std::fs::canonicalize(workshop).unwrap()]);
    }

    #[test]
    fn discover_steam_workshop_roots_ignores_relative_vdf_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let steam_root = home.join(".local/share/Steam");
        let relative_library = home.join("relative-library");
        std::fs::create_dir_all(steam_root.join("config")).unwrap();
        std::fs::create_dir_all(relative_library.join("steamapps/workshop/content/431960"))
            .unwrap();
        std::fs::write(
            steam_root.join("config/libraryfolders.vdf"),
            r#""libraryfolders" { "1" "relative-library" }"#,
        )
        .unwrap();

        let roots = discover_steam_workshop_roots_with_xdg_data_home(&home, None);

        assert!(roots.is_empty());
    }

    #[test]
    fn discover_steam_workshop_roots_empty_when_no_known_dir_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let roots = discover_steam_workshop_roots(tmp.path());
        assert!(roots.is_empty());
    }

    #[test]
    fn source_dedupe_removes_symlink_duplicates() {
        // Create a dir and a symlink pointing to it — they should dedupe to one.
        let root = tempfile::tempdir().unwrap();
        let real = root.path().join("walls");
        std::fs::create_dir_all(&real).unwrap();
        let link = root.path().join("walls-link");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let sources = vec![
            real.to_string_lossy().to_string(),
            link.to_string_lossy().to_string(),
        ];
        let deduped = dedupe_sources(&sources);
        assert_eq!(
            deduped.len(),
            1,
            "duplicate sources should be deduped: {:?}",
            deduped
        );
    }

    #[test]
    fn we_project_dir_reads_project_json() {
        let root = tempfile::tempdir().unwrap();
        let proj_dir = root.path().join("431960").join("821372791");
        std::fs::create_dir_all(&proj_dir).unwrap();
        let img = proj_dir.join("bg.mp4");
        std::fs::write(&img, b"").unwrap();
        std::fs::write(
            proj_dir.join("project.json"),
            r#"{"type":"video","file":"bg.mp4"}"#,
        )
        .unwrap();

        let source = proj_dir.to_string_lossy().to_string();
        let result = scan_wallpapers(&[source]);
        assert!(
            result.iter().any(|p| p.contains("bg.mp4")),
            "project dir should be read via project.json: {:?}",
            result
        );
    }

    #[test]
    fn we_marker_detection() {
        assert!(is_wallpaper_engine_source(
            "/home/user/.steam/steam/steamapps/workshop/content/431960"
        ));
        assert!(is_wallpaper_engine_source(
            "/home/user/.steam/steam/steamapps/workshop/content/431960/123456"
        ));
        assert!(!is_wallpaper_engine_source("/home/user/Pictures"));
        assert!(!is_wallpaper_engine_source(
            "/home/user/steamapps/workshop/content/4319600"
        ));
        assert!(!is_wallpaper_engine_source(
            "/home/user/steamapps/workshop/content/431960-backup"
        ));
    }

    #[test]
    fn we_project_json_scene_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let proj = serde_json::json!({
            "type": "scene",
            "file": "scene.json"
        });
        std::fs::write(dir.path().join("project.json"), proj.to_string()).unwrap();
        assert_eq!(read_we_project_json(dir.path()), None);
    }

    #[test]
    fn we_project_json_web_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let proj = serde_json::json!({
            "type": "web",
            "file": "index.html"
        });
        std::fs::write(dir.path().join("project.json"), proj.to_string()).unwrap();
        assert_eq!(read_we_project_json(dir.path()), None);
    }

    #[test]
    fn we_project_info_web_is_unsupported_case_insensitive() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("index.html"), "<html></html>").unwrap();
        let proj = serde_json::json!({
            "type": "Web",
            "file": "index.html",
            "preview": "preview.gif",
            "title": "Web Project"
        });
        std::fs::write(dir.path().join("preview.gif"), b"gif").unwrap();
        std::fs::write(dir.path().join("project.json"), proj.to_string()).unwrap();

        let info = read_we_project_info(dir.path()).expect("web project should parse");
        assert_eq!(info.entry_type, FileType::WeWeb);
        assert_eq!(info.backend, Backend::Unsupported);
        assert!(info
            .unsupported_reason
            .as_deref()
            .unwrap_or("")
            .contains("browsing only"));

        let entry = make_entry(&dir.path().to_string_lossy()).expect("web project should index");
        assert_eq!(entry.file_type, FileType::WeWeb);
        assert_eq!(entry.backend, Backend::Unsupported);
        assert_eq!(
            entry.project.and_then(|p| p.title),
            Some("Web Project".to_string())
        );
    }

    #[test]
    fn we_project_json_image_is_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let img = dir.path().join("wallpaper.png");
        std::fs::write(&img, b"").unwrap();
        let proj = serde_json::json!({
            "type": "image",
            "file": "wallpaper.png"
        });
        std::fs::write(dir.path().join("project.json"), proj.to_string()).unwrap();
        let result = read_we_project_json(dir.path());
        assert!(result.is_some(), "image project should be accepted");
        assert_eq!(result.unwrap(), img.to_string_lossy().to_string());
    }

    #[test]
    fn scan_wallpapers_with_callback_can_cancel_after_first_candidate() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("walls");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.jpg"), b"a").unwrap();
        std::fs::write(dir.join("b.jpg"), b"b").unwrap();
        let source = dir.to_string_lossy().to_string();
        let mut seen_candidates = 0usize;

        let files = scan_wallpapers_with_callback(&[source], |event| {
            if matches!(event, ScanEvent::CandidateFound { .. }) {
                seen_candidates += 1;
                return ScanControl::Cancel;
            }
            ScanControl::Continue
        });

        assert_eq!(seen_candidates, 1);
        assert_eq!(files.len(), 1);
    }

    #[test]
    fn we_workshop_root_can_cancel_during_top_level_walk_without_candidates() {
        let tmp = tempfile::tempdir().unwrap();
        let root_path = format!("{}/steamapps/workshop/content/431960", tmp.path().display());
        std::fs::create_dir_all(&root_path).unwrap();

        // Create empty subdirs without project.json — these fall through
        // to recursive walk and will not emit CandidateFound (no files inside).
        for i in 0..CANCEL_CHECK_INTERVAL + 10 {
            let sub = format!("{}/{}", root_path, i);
            std::fs::create_dir_all(&sub).unwrap();
        }

        let mut walk_progress_count = 0usize;

        let files = scan_wallpapers_with_callback(&[root_path], |event| {
            if matches!(event, ScanEvent::WalkProgress { .. }) {
                walk_progress_count += 1;
                return ScanControl::Cancel;
            }
            ScanControl::Continue
        });

        assert!(
            walk_progress_count >= 1,
            "WalkProgress should have fired at least once"
        );
        assert!(
            files.is_empty(),
            "cancel should stop before producing candidates"
        );
    }

    #[test]
    fn we_workshop_root_scans_subdirs() {
        let root = tempfile::tempdir().unwrap();
        let root_path = format!(
            "{}/steamapps/workshop/content/431960",
            root.path().display()
        );
        std::fs::create_dir_all(&root_path).unwrap();

        let scene_dir = format!("{}/111", root_path);
        std::fs::create_dir_all(&scene_dir).unwrap();
        std::fs::write(
            format!("{}/project.json", scene_dir),
            r#"{"type":"scene","file":"scene.json"}"#,
        )
        .unwrap();

        let img_dir = format!("{}/222", root_path);
        std::fs::create_dir_all(&img_dir).unwrap();
        let img = format!("{}/bg.png", img_dir);
        std::fs::write(&img, b"").unwrap();
        std::fs::write(
            format!("{}/project.json", img_dir),
            r#"{"type":"image","file":"bg.png"}"#,
        )
        .unwrap();

        let fallback_dir = format!("{}/333", root_path);
        std::fs::create_dir_all(&fallback_dir).unwrap();
        std::fs::write(format!("{}/pic.jpg", fallback_dir), b"").unwrap();

        let sources = vec![root_path];
        let result = scan_wallpapers(&sources);

        let img_canon = std::fs::canonicalize(&img)
            .unwrap()
            .to_string_lossy()
            .to_string();
        let scene_canon = std::fs::canonicalize(&scene_dir)
            .unwrap()
            .to_string_lossy()
            .to_string();
        let jpg_canon = std::fs::canonicalize(format!("{}/pic.jpg", fallback_dir))
            .unwrap()
            .to_string_lossy()
            .to_string();

        assert!(
            result.contains(&scene_canon),
            "scene project should appear once: {:?}",
            result
        );
        assert!(
            !result
                .iter()
                .any(|p| p.ends_with("scene.json") || p.ends_with("project.json")),
            "scene internals should not appear: {:?}",
            result
        );
        assert!(
            result.contains(&img_canon),
            "image bg.png should be in results"
        );
        assert!(
            result.contains(&jpg_canon),
            "fallback pic.jpg should be in results"
        );
    }
}

#[test]
fn we_video_project_make_entry_returns_media_path_with_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let bg = dir.path().join("bg.mp4");
    std::fs::write(&bg, b"").unwrap();
    std::fs::write(dir.path().join("preview.gif"), b"").unwrap();
    let proj = serde_json::json!({
        "type": "video",
        "file": "bg.mp4",
        "preview": "preview.gif",
        "title": "My Video Wallpaper"
    });
    std::fs::write(dir.path().join("project.json"), proj.to_string()).unwrap();
    let entry = make_entry(dir.path().to_string_lossy().as_ref());
    assert!(entry.is_some());
    let entry = entry.unwrap();
    assert!(
        entry.path.to_string().ends_with("bg.mp4"),
        "path should point to the media file, got: {}",
        entry.path
    );
    assert_eq!(entry.file_type, FileType::Video);
    assert_eq!(entry.backend, Backend::Mpvpaper);
    let proj_meta = entry
        .project
        .as_ref()
        .expect("should have project metadata");
    assert_eq!(proj_meta.title.as_deref(), Some("My Video Wallpaper"));
    assert!(
        proj_meta
            .preview_path
            .as_ref()
            .is_some_and(|p| p.ends_with("preview.gif")),
        "preview_path should point to preview.gif"
    );
    assert!(proj_meta.we_file.as_deref() == Some("bg.mp4"));
}

#[test]
fn we_video_file_make_entry_detects_project_parent() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir
        .path()
        .join("steamapps/workshop/content/431960/2924684771");
    std::fs::create_dir_all(&marker).unwrap();
    let bg = marker.join("bg.mp4");
    std::fs::write(&bg, b"").unwrap();
    let proj = serde_json::json!({
        "type": "video",
        "file": "bg.mp4",
        "title": "Workshop Video"
    });
    std::fs::write(marker.join("project.json"), proj.to_string()).unwrap();

    let canon = std::fs::canonicalize(&bg)
        .unwrap()
        .to_string_lossy()
        .to_string();
    let entry = make_entry(&canon);
    assert!(entry.is_some());
    let entry = entry.unwrap();
    assert_eq!(entry.file_type, FileType::Video);
    assert_eq!(entry.backend, Backend::Mpvpaper);
    let proj_meta = entry
        .project
        .as_ref()
        .expect("file inside WE project dir should have project metadata");
    assert_eq!(proj_meta.title.as_deref(), Some("Workshop Video"));
    assert!(proj_meta.we_file.as_deref() == Some("bg.mp4"));
    assert_eq!(proj_meta.workshop_id.as_deref(), Some("2924684771"));
}

#[test]
fn missing_we_video_file_returns_none() {
    let dir = tempfile::tempdir().unwrap();
    let proj = serde_json::json!({
        "type": "video",
        "file": "nonexistent.mp4"
    });
    std::fs::write(dir.path().join("project.json"), proj.to_string()).unwrap();
    let entry = make_entry(dir.path().to_string_lossy().as_ref());
    assert!(
        entry.is_none(),
        "missing WE media file should not produce an entry"
    );
}

#[test]
fn safe_join_rejects_traversal() {
    let dir = tempfile::tempdir().unwrap();
    assert!(safe_join(dir.path(), "../evil").is_err());
    assert!(safe_join(dir.path(), "foo/../../bar").is_err());
}

#[test]
fn safe_join_rejects_absolute_path() {
    let dir = tempfile::tempdir().unwrap();
    assert!(safe_join(dir.path(), "/etc/passwd").is_err());
}

#[test]
fn we_preview_path_is_canonical_and_confined_to_the_project() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    std::fs::create_dir_all(project.join("assets")).unwrap();
    let preview = project.join("assets").join("preview.jpg");
    std::fs::write(&preview, b"preview").unwrap();

    let info = we_project_info_from_json(
        &project,
        &serde_json::json!({
            "type": "scene",
            "preview": "./assets/preview.jpg"
        }),
    )
    .unwrap();

    assert_eq!(
        info.preview_path.as_deref(),
        Some(preview.canonicalize().unwrap().to_string_lossy().as_ref())
    );
}

#[test]
fn we_preview_path_rejects_absolute_and_traversing_paths() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    let outside = dir.path().join("outside.jpg");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(&outside, b"outside").unwrap();

    for preview in [
        outside.to_string_lossy().into_owned(),
        "../outside.jpg".to_string(),
    ] {
        let info = we_project_info_from_json(
            &project,
            &serde_json::json!({
                "type": "scene",
                "preview": preview
            }),
        )
        .unwrap();
        assert_eq!(info.preview_path, None);
    }
}

#[cfg(unix)]
#[test]
fn we_preview_path_rejects_symlink_escape() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    let outside = dir.path().join("outside.jpg");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(&outside, b"outside").unwrap();
    symlink(&outside, project.join("preview.jpg")).unwrap();

    let info = we_project_info_from_json(
        &project,
        &serde_json::json!({
            "type": "scene",
            "preview": "preview.jpg"
        }),
    )
    .unwrap();

    assert_eq!(info.preview_path, None);
}

#[test]
fn cached_entry_reuses_prior_metadata() {
    use std::collections::HashMap;

    let dir = tempfile::tempdir().unwrap();
    let img = dir.path().join("test.png");
    std::fs::write(&img, b"test data").unwrap();

    // Build a prior cache entry for the same file.
    let canon = std::fs::canonicalize(&img)
        .unwrap()
        .to_string_lossy()
        .to_string();
    let prior = WallpaperEntry {
        path: Utf8PathBuf::from(img.to_string_lossy().to_string()),
        file_type: wc_core::types::FileType::Image,
        ext: "png".to_string(),
        backend: wc_core::types::Backend::Awww,
        size: img.metadata().unwrap().len(),
        mtime: img
            .metadata()
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        resolution: "1920x1080".to_string(), // known prior value
        project: None,
    };
    let mut cache = HashMap::new();
    cache.insert(canon, prior);

    let (entry, reused) = make_entry_cached(img.to_string_lossy().as_ref(), &cache);
    assert!(entry.is_some());
    assert!(reused, "unchanged file should reuse metadata");
    assert_eq!(
        entry.unwrap().resolution,
        "1920x1080",
        "resolution should come from cache"
    );
}

#[test]
fn normalize_source_path_we_project_collapses_to_root() {
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("steamapps/workshop/content/431960");
    std::fs::create_dir_all(&marker).unwrap();
    let project = marker.join("123456");
    std::fs::create_dir_all(&project).unwrap();

    let normalized = normalize_source_path(&project.to_string_lossy());
    assert_eq!(
        normalized,
        std::fs::canonicalize(&marker)
            .unwrap()
            .to_string_lossy()
            .to_string(),
        "project dir should collapse to canonical workshop root"
    );
}

#[test]
fn normalize_source_path_we_root_is_canonicalized() {
    let root = tempfile::tempdir().unwrap();
    let steam_we = root.path().join("steamapps/workshop/content/431960");
    std::fs::create_dir_all(&steam_we).unwrap();
    let flatpak_we = root
        .path()
        .join(".var/app/com.valvesoftware.Steam/data/Steam/steamapps/workshop/content/431960");
    std::fs::create_dir_all(&flatpak_we).unwrap();
    let project = steam_we.join("123456");
    std::fs::create_dir_all(&project).unwrap();

    let steam_proj = project.to_string_lossy();
    let flatpak_mirror = format!("{}/123456", flatpak_we.display());

    let ns = normalize_source_path(&steam_proj);
    let nf = normalize_source_path(&flatpak_mirror);

    // Both should collapse to their respective canonical workshop roots
    assert_eq!(
        ns,
        std::fs::canonicalize(&steam_we)
            .unwrap()
            .to_string_lossy()
            .to_string(),
        "Steam project should collapse to canonical workshop root"
    );
    assert_eq!(
        nf,
        std::fs::canonicalize(&flatpak_we)
            .unwrap()
            .to_string_lossy()
            .to_string(),
        "Flatpak project should collapse to canonical workshop root"
    );
    // If the projects are the same physical directory (symlink scenario), the
    // canonicalized roots would be the same
    assert_ne!(ns, nf, "Steam and Flatpak roots are distinct directories");
}

#[test]
fn normalize_source_path_non_we_is_canonicalized() {
    let root = tempfile::tempdir().unwrap();
    let real = root.path().join("walls");
    std::fs::create_dir_all(&real).unwrap();
    let link = root.path().join("walls-link");
    std::os::unix::fs::symlink(&real, &link).unwrap();

    let normalized = normalize_source_path(&link.to_string_lossy());
    assert_eq!(
        normalized,
        std::fs::canonicalize(&real)
            .unwrap()
            .to_string_lossy()
            .to_string(),
        "non-WE path should be canonicalized"
    );
}

#[test]
fn we_workshop_root_returns_none_for_non_we() {
    assert_eq!(we_workshop_root("/home/user/Pictures"), None);
}

#[test]
fn we_workshop_root_returns_none_for_workshop_root() {
    assert_eq!(we_workshop_root("/steamapps/workshop/content/431960"), None);
    assert_eq!(
        we_workshop_root("/steamapps/workshop/content/431960/"),
        None
    );
}

#[test]
fn cached_entry_probes_when_size_changed() {
    use std::collections::HashMap;

    let dir = tempfile::tempdir().unwrap();
    let img = dir.path().join("changed.png");
    std::fs::write(&img, b"new content").unwrap();

    let canon = std::fs::canonicalize(&img)
        .unwrap()
        .to_string_lossy()
        .to_string();
    // Prior has a different size.
    let prior = WallpaperEntry {
        path: Utf8PathBuf::from(img.to_string_lossy().to_string()),
        file_type: wc_core::types::FileType::Image,
        ext: "png".to_string(),
        backend: wc_core::types::Backend::Awww,
        size: 999, // different from actual
        mtime: img
            .metadata()
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        resolution: "old".to_string(),
        project: None,
    };
    let mut cache = HashMap::new();
    cache.insert(canon, prior);

    let (entry, reused) = make_entry_cached(img.to_string_lossy().as_ref(), &cache);
    assert!(entry.is_some());
    assert!(!reused, "changed file must be re-probed");
}

#[test]
fn visit_wallpapers_streams_without_collecting_all_candidates() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("walls");
    std::fs::create_dir_all(&source).unwrap();

    for i in 0..10 {
        std::fs::write(source.join(format!("wall-{i}.jpg")), b"jpg").unwrap();
    }

    let mut visited = Vec::new();
    let cancelled = visit_wallpapers_with_callback(
        &[source.to_string_lossy().to_string()],
        |_| ScanControl::Continue,
        |path| {
            visited.push(path);
            if visited.len() == 3 {
                ScanVisitControl::Cancel
            } else {
                ScanVisitControl::Continue
            }
        },
    );

    assert!(cancelled);
    assert_eq!(visited.len(), 3);
}

#[test]
fn resolution_probe_runner_returns_small_successful_output() {
    let output = run_command_with_deadline(
        "/bin/sh",
        &["-c", "printf 1920x1080"],
        Duration::from_secs(1),
    )
    .unwrap();

    assert!(output.success);
    assert_eq!(output.stdout, b"1920x1080");
    assert!(output.stderr.is_empty());
}

#[test]
fn resolution_probe_issues_are_rate_limited_per_tool_and_reason() {
    let reported = Mutex::new(HashSet::new());

    assert!(should_report_resolution_probe_issue(
        &reported, "identify", "spawn"
    ));
    assert!(!should_report_resolution_probe_issue(
        &reported, "identify", "spawn"
    ));
    assert!(should_report_resolution_probe_issue(
        &reported, "identify", "timeout"
    ));
    assert!(should_report_resolution_probe_issue(
        &reported, "ffprobe", "spawn"
    ));
}

#[cfg(unix)]
#[test]
fn resolution_probe_timeout_kills_and_reaps_its_process_group() {
    let temp = tempfile::tempdir().unwrap();
    let pid_file = temp.path().join("probe.pid");
    let pid_file_arg = pid_file.to_string_lossy().to_string();
    let started = Instant::now();

    let error = run_command_with_deadline(
        "/bin/sh",
        &[
            "-c",
            "echo $$ > \"$1\"; sleep 30",
            "resolution-probe",
            &pid_file_arg,
        ],
        Duration::from_millis(300),
    )
    .unwrap_err();

    assert_eq!(error, DeadlineCommandError::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(2));
    let pid = std::fs::read_to_string(pid_file).expect("probe must publish its PID");
    assert!(
        !Path::new("/proc").join(pid.trim()).exists(),
        "timed-out probe child must be synchronously reaped"
    );
}

#[cfg(unix)]
#[test]
fn resolution_probe_does_not_join_an_escaped_pipe_holder() {
    let temp = tempfile::tempdir().unwrap();
    let pid_file = temp.path().join("escaped.pid");
    let script = format!(
        "setsid sh -c 'echo $$ > \"{}\"; sleep 30' & exit 0",
        pid_file.display()
    );
    let started = Instant::now();

    let output =
        run_command_with_deadline("/bin/sh", &["-c", &script], Duration::from_secs(1)).unwrap();

    assert!(output.success);
    assert!(started.elapsed() < Duration::from_secs(2));
    if let Ok(raw) = std::fs::read_to_string(pid_file) {
        if let Ok(pid) = raw.trim().parse::<i32>() {
            unsafe extern "C" {
                fn kill(pid: i32, signal: i32) -> i32;
            }
            // SAFETY: test cleanup targets the PID written by its child.
            unsafe {
                kill(pid, 9);
            }
        }
    }
}
