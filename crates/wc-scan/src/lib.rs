//! wc-scan — recursive wallpaper scanning, library index building.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use camino::Utf8PathBuf;
use walkdir::WalkDir;
use wc_core::formats;
use wc_core::types::WallpaperEntry;

/// WE (Wallpaper Engine) workshop marker path component.
const WE_MARKER: &str = "/steamapps/workshop/content/431960";

/// Scan all configured sources and return wallpaper file paths (deduplicated).
pub fn scan_wallpapers(sources: &[String]) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut files: Vec<String> = Vec::new();

    for source in sources {
        let src_path = Path::new(source);
        if !src_path.is_dir() {
            continue;
        }
        for entry in WalkDir::new(source)
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
                    let canonical = std::fs::canonicalize(path)
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|_| path.to_string_lossy().to_string());
                    if seen.insert(canonical.clone()) {
                        files.push(canonical);
                    }
                }
            }
        }
    }
    files
}

/// Check if a path is a Wallpaper Engine workshop source.
pub fn is_wallpaper_engine_source(path: &str) -> bool {
    path.contains(WE_MARKER)
}

/// Read a Wallpaper Engine project.json file and extract the wallpaper file path.
/// Returns None if the project type is unsupported (scene, web, application).
pub fn read_we_project_json(project_dir: &Path) -> Option<String> {
    let proj_path = project_dir.join("project.json");
    if !proj_path.exists() {
        return None;
    }
    let content = fs::read_to_string(&proj_path).ok()?;
    let proj: serde_json::Value = serde_json::from_str(&content).ok()?;

    // Skip unsupported types
    let proj_type = proj.get("type").and_then(|v| v.as_str()).unwrap_or("");
    if matches!(proj_type, "scene" | "web" | "application") {
        return None;
    }

    // Use the "file" field if present
    proj.get("file")
        .and_then(|v| v.as_str())
        .map(|f| project_dir.join(f).to_string_lossy().to_string())
}

/// Build a WallpaperEntry from a file path via stat and classification.
pub fn make_entry(path: &str) -> Option<WallpaperEntry> {
    let p = Path::new(path);
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

    Some(WallpaperEntry {
        path: Utf8PathBuf::from(path),
        file_type: ftype,
        ext,
        backend,
        size,
        mtime,
        resolution: "?x?".into(),
    })
}

/// Resolution filter check against configured minimums.
pub fn passes_resolution_filter(_path: &str, min_width: u32, min_height: u32) -> bool {
    if min_width == 0 && min_height == 0 {
        return true;
    }
    // In the Rust MVP, resolution detection is deferred.
    // Unknown resolution → allowed (matches Bash behavior).
    true
}
