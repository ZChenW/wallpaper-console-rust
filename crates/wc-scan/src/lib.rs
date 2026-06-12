//! wc-scan — recursive wallpaper scanning, library index building.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use camino::Utf8PathBuf;
use walkdir::WalkDir;
use wc_core::formats;
use wc_core::types::{Backend, FileType, WallpaperEntry, WallpaperProject};

const WE_MARKER: &str = "/steamapps/workshop/content/431960";

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

/// Scan all sources for wallpaper files.
///
/// Sources are deduplicated by canonical path first.
///
/// Wallpaper Engine source detection:
///   - Workshop root (.../431960) → iterate one level of project subdirs,
///     read each project.json, skip scene/web/application.
///   - Single project dir (.../431960/<id> with project.json) → read its
///     project.json directly.
///   - Other dirs → recursive walkdir scan.
pub fn scan_wallpapers(sources: &[String]) -> Vec<String> {
    let deduped = dedupe_sources(sources);
    let mut seen: HashSet<String> = HashSet::new();
    let mut files: Vec<String> = Vec::new();

    for source in &deduped {
        let src_path = Path::new(source);
        if !src_path.is_dir() {
            continue;
        }

        match we_source_kind(source) {
            WeKind::WorkshopRoot => {
                scan_we_workshop_root(src_path, &mut seen, &mut files);
            }
            WeKind::ProjectDir => {
                // Single WE project — read its project.json directly.
                if let Some(wp) = indexed_we_project_path(src_path) {
                    let p = Path::new(&wp);
                    let c = canonicalize_str(
                        &std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()),
                    );
                    if seen.insert(c.clone()) {
                        files.push(c);
                    }
                }
            }
            WeKind::Normal => {
                scan_dir_recursive(src_path, &mut seen, &mut files);
            }
        }
    }
    files
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

fn we_source_kind(path: &str) -> WeKind {
    if !path.contains(WE_MARKER) {
        return WeKind::Normal;
    }
    // Find the position of the marker in the path.
    if let Some(pos) = path.find(WE_MARKER) {
        let after = &path[pos + WE_MARKER.len()..];
        let after = after.trim_start_matches('/');
        // If there's a numeric project ID (and possibly trailing slash), it's a project dir.
        if !after.is_empty() {
            // Check if it looks like a Steam workshop ID (all digits).
            let first_seg = after.split('/').next().unwrap_or("");
            if first_seg.chars().all(|c| c.is_ascii_digit()) {
                return WeKind::ProjectDir;
            }
        }
        // Otherwise it's the workshop root itself.
        return WeKind::WorkshopRoot;
    }
    WeKind::Normal
}

/// Scan a Wallpaper Engine workshop root: iterate one level of project
/// directories, read each project.json, and decide what to include.
fn scan_we_workshop_root(root: &Path, seen: &mut HashSet<String>, files: &mut Vec<String>) {
    let entries = match fs::read_dir(root) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.filter_map(|e| e.ok()) {
        let ftype = entry.file_type().ok();
        if !ftype.map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let project_dir = entry.path();

        let has_proj = project_dir.join("project.json").exists();
        let we_file = indexed_we_project_path(&project_dir);

        if let Some(ref wp) = we_file {
            let p = Path::new(wp);
            let c = canonicalize_str(&std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()));
            if seen.insert(c.clone()) {
                files.push(c);
            }
            continue;
        }

        if has_proj {
            // project.json exists but unreadable or unsupported type → skip.
            continue;
        }

        // No project.json — fall back to recursive scan.
        scan_dir_recursive(&project_dir, seen, files);
    }
}

/// Recursively scan a directory for supported wallpaper files.
fn scan_dir_recursive(dir: &Path, seen: &mut HashSet<String>, files: &mut Vec<String>) {
    for entry in WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
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
                let canonical = canonicalize_str(
                    &std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()),
                );
                if seen.insert(canonical.clone()) {
                    files.push(canonical);
                }
            }
        }
    }
}

fn canonicalize_str(p: &Path) -> String {
    p.to_string_lossy().to_string()
}

pub fn is_wallpaper_engine_source(path: &str) -> bool {
    path.contains(WE_MARKER)
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

    let raw_type = proj.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let normalized_type = raw_type.trim().to_lowercase();
    let file = proj
        .get("file")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let preview_path = proj
        .get("preview")
        .and_then(|v| v.as_str())
        .map(|preview| project_dir.join(preview).to_string_lossy().to_string())
        .filter(|preview| Path::new(preview).is_file());
    let title = proj
        .get("title")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let workshop_id = workshop_id_from_path(project_dir);
    let (entry_type, backend, unsupported_reason) = match normalized_type.as_str() {
        "scene" => (FileType::WeScene, Backend::LinuxWallpaperEngine, None),
        "web" => (FileType::WeWeb, Backend::ChromiumWeb, None),
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
    Ok(joined)
}

pub fn workshop_id_from_path(project_dir: &Path) -> Option<String> {
    let path_str = project_dir.to_string_lossy();
    let pos = path_str.find("/431960/")?;
    let after = &path_str[pos + 7..];
    let after = after.trim_start_matches('/');
    let first_seg = after.split('/').next()?;
    if first_seg.chars().all(|c| c.is_ascii_digit()) {
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
    let info = read_we_project_info(project_dir)?;

    if matches!(
        info.entry_type,
        FileType::Image | FileType::Gif | FileType::Video
    ) {
        let file = info.file.as_ref()?;
        let media_path = safe_join(project_dir, file).ok()?;
        if !media_path.is_file() {
            return None;
        }
        let ext = formats::get_extension(file)?;
        let meta = fs::metadata(&media_path).ok()?;
        let size = meta.len();
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let path = media_path.to_string_lossy().to_string();
        let resolution = detect_resolution(&path, info.entry_type);
        return Some(WallpaperEntry {
            path: Utf8PathBuf::from(path),
            file_type: info.entry_type,
            ext,
            backend: info.backend,
            size,
            mtime,
            resolution,
            project: Some(info.wallpaper_project()),
        });
    }

    let project_json = project_dir.join("project.json");
    let meta = fs::metadata(&project_json)
        .or_else(|_| fs::metadata(project_dir))
        .ok()?;
    let size = project_entry_size_hint(project_dir, info.preview_path.as_deref());
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

    Some(WallpaperEntry {
        path: Utf8PathBuf::from(info.project_entry_path()),
        file_type: info.entry_type,
        ext,
        backend: info.backend,
        size,
        mtime,
        resolution: "WE".to_string(),
        project: Some(info.wallpaper_project()),
    })
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
            if let Ok(output) = std::process::Command::new("identify")
                .args(["-format", "%wx%h", path, "[0]"])
                .output()
            {
                let s = String::from_utf8_lossy(&output.stdout).to_string();
                if !s.is_empty() && s.contains('x') {
                    return s;
                }
            }
        }
        wc_core::types::FileType::Video => {
            if let Ok(output) = std::process::Command::new("ffprobe")
                .args([
                    "-v",
                    "error",
                    "-select_streams",
                    "v:0",
                    "-show_entries",
                    "stream=width,height",
                    "-of",
                    "csv=p=0",
                    path,
                ])
                .output()
            {
                let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
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

/// Load prior metadata from a library TSV file into a HashMap keyed by canonical path.
pub fn prior_metadata_cache(tsv_path: &Path) -> std::collections::HashMap<String, WallpaperEntry> {
    let mut cache = std::collections::HashMap::new();
    let content = match fs::read_to_string(tsv_path) {
        Ok(c) => c,
        Err(_) => return cache,
    };
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 7 {
            continue;
        }
        let path = parts[6];
        let canon = std::fs::canonicalize(path)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| path.to_string());
        let ftype = match parts[0] {
            "gif" => wc_core::types::FileType::Gif,
            "video" => wc_core::types::FileType::Video,
            _ => wc_core::types::FileType::Image,
        };
        let ext = parts[1].to_string();
        let backend = match parts[2] {
            "mpvpaper" => wc_core::types::Backend::Mpvpaper,
            _ => wc_core::types::Backend::Awww,
        };
        let size: u64 = parts[3].parse().unwrap_or(0);
        let mtime: u64 = parts[4].parse().unwrap_or(0);
        let resolution = parts[5].to_string();
        cache.insert(
            canon,
            WallpaperEntry {
                path: Utf8PathBuf::from(path),
                file_type: ftype,
                ext,
                backend,
                size,
                mtime,
                resolution,
                project: None,
            },
        );
    }
    cache
}

/// Build an entry for a wallpaper file, reusing prior metadata when the file's
/// size and mtime haven't changed.  Returns (entry, reused).
pub fn make_entry_cached(
    path: &str,
    cache: &std::collections::HashMap<String, WallpaperEntry>,
) -> (Option<WallpaperEntry>, bool) {
    let p = Path::new(path);
    if p.is_dir() {
        return (make_we_project_entry(p), false);
    }
    if !p.is_file() {
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
    let meta = match fs::metadata(path) {
        Ok(m) => m,
        Err(_) => return (None, false),
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

pub fn passes_resolution_filter(resolution: &str, min_width: u32, min_height: u32) -> bool {
    if min_width == 0 && min_height == 0 {
        return true;
    }
    if resolution == "?x?" {
        return true;
    }
    let parts: Vec<&str> = resolution.split('x').collect();
    if parts.len() != 2 {
        return true;
    }
    let w: u32 = parts[0].parse().unwrap_or(0);
    let h: u32 = parts[1].parse().unwrap_or(0);
    w >= min_width && h >= min_height
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
        assert!(!is_wallpaper_engine_source("/home/user/Pictures"));
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
            format!(r#"{{"type":"image","file":"bg.png"}}"#),
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
    let entry = make_entry(&dir.path().to_string_lossy().to_string());
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
            .map_or(false, |p| p.ends_with("preview.gif")),
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
    let entry = make_entry(&dir.path().to_string_lossy().to_string());
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

    let (entry, reused) = make_entry_cached(&img.to_string_lossy().to_string(), &cache);
    assert!(entry.is_some());
    assert!(reused, "unchanged file should reuse metadata");
    assert_eq!(
        entry.unwrap().resolution,
        "1920x1080",
        "resolution should come from cache"
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

    let (entry, reused) = make_entry_cached(&img.to_string_lossy().to_string(), &cache);
    assert!(entry.is_some());
    assert!(!reused, "changed file must be re-probed");
}
