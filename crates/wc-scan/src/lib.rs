//! wc-scan — recursive wallpaper scanning, library index building.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use camino::Utf8PathBuf;
use walkdir::WalkDir;
use wc_core::formats;
use wc_core::types::WallpaperEntry;

const WE_MARKER: &str = "/steamapps/workshop/content/431960";

pub fn scan_wallpapers(sources: &[String]) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut files: Vec<String> = Vec::new();
    let mut we_processed: HashSet<String> = HashSet::new();

    for source in sources {
        let src_path = Path::new(source);
        if !src_path.is_dir() {
            continue;
        }

        if is_wallpaper_engine_source(source) {
            let canon = std::fs::canonicalize(src_path)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| source.clone());
            if we_processed.contains(&canon) {
                continue;
            }
            we_processed.insert(canon);

            if let Some(wp) = read_we_project_json(src_path) {
                let c = std::fs::canonicalize(&wp)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or(wp.clone());
                if seen.insert(c.clone()) {
                    files.push(c);
                }
                continue;
            }
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

pub fn is_wallpaper_engine_source(path: &str) -> bool {
    path.contains(WE_MARKER)
}

pub fn read_we_project_json(project_dir: &Path) -> Option<String> {
    let proj_path = project_dir.join("project.json");
    if !proj_path.exists() {
        return None;
    }
    let content = fs::read_to_string(&proj_path).ok()?;
    let proj: serde_json::Value = serde_json::from_str(&content).ok()?;
    let proj_type = proj.get("type").and_then(|v| v.as_str()).unwrap_or("");
    if matches!(proj_type, "scene" | "web" | "application") {
        return None;
    }
    proj.get("file")
        .and_then(|v| v.as_str())
        .map(|f| project_dir.join(f).to_string_lossy().to_string())
}

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
    let resolution = detect_resolution(path);
    Some(WallpaperEntry {
        path: Utf8PathBuf::from(path),
        file_type: ftype,
        ext,
        backend,
        size,
        mtime,
        resolution,
    })
}

fn detect_resolution(path: &str) -> String {
    if let Ok(output) = std::process::Command::new("identify")
        .args(["-format", "%wx%h", path, "[0]"])
        .output()
    {
        let s = String::from_utf8_lossy(&output.stdout).to_string();
        if !s.is_empty() && s.contains('x') {
            return s;
        }
    }
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
        if s.contains(',') {
            let parts: Vec<&str> = s.split(',').collect();
            if parts.len() >= 2 {
                return format!("{}x{}", parts[0], parts[1]);
            }
        }
    }
    "?x?".to_string()
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
