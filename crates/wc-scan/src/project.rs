//! Project parsing and indexed media entries, including reusable dimension metadata.
use crate::resolution::detect_resolution;
use crate::walk::{find_we_marker, is_wallpaper_engine_source};
use camino::Utf8PathBuf;
use std::{
    fs,
    path::{Path, PathBuf},
};
use wc_core::{
    formats,
    types::{Backend, FileType, WallpaperEntry, WallpaperProject},
};

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
            let (ftype, backend) = formats::classify_media_path(Path::new(file))?;
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

pub(super) fn indexed_we_project_path(project_dir: &Path) -> Option<String> {
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

/// Routing metadata without subprocesses or dimension probes. Indexing callers
/// should keep using `make_entry` for size, resolution and project metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryClassification {
    pub path: Utf8PathBuf,
    pub file_type: FileType,
    pub ext: String,
    pub backend: Backend,
}

pub fn classify_entry(path: &str) -> Option<EntryClassification> {
    let p = Path::new(path);
    if p.is_dir() {
        let canonical = p.canonicalize().ok()?;
        let info = read_we_project_info(&canonical)?;
        let (path, ext) = match info.entry_type {
            FileType::Image | FileType::Gif | FileType::Video => {
                let file = info.file.as_ref()?;
                let media = safe_join(&canonical, file).ok()?.canonicalize().ok()?;
                if !media.is_file() {
                    return None;
                }
                fs::metadata(&media).ok()?;
                (
                    media.to_string_lossy().to_string(),
                    formats::get_extension(file)?,
                )
            }
            FileType::WeScene | FileType::WeWeb | FileType::WeApplication => {
                fs::metadata(canonical.join("project.json"))
                    .or_else(|_| fs::metadata(&canonical))
                    .ok()?;
                let ext = match info.entry_type {
                    FileType::WeScene => "scene",
                    FileType::WeWeb => "web",
                    _ => "application",
                };
                (info.project_entry_path(), ext.into())
            }
        };
        return Some(EntryClassification {
            path: path.into(),
            file_type: info.entry_type,
            ext,
            backend: info.backend,
        });
    }
    if !p.is_file() {
        return None;
    }
    let ext = formats::get_extension(path)?;
    let (file_type, backend) = formats::classify_media_path(p)?;
    fs::metadata(p).ok()?;
    Some(EntryClassification {
        path: path.into(),
        file_type,
        ext,
        backend,
    })
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
    let (ftype, backend) = formats::classify_media_path(p)?;
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
    let (ftype, backend) = match formats::classify_media_path(p) {
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
                    // Container classification can improve between versions even
                    // when the file and its expensive dimension probe are unchanged.
                    file_type: ftype,
                    ext,
                    backend,
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
