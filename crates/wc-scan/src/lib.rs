//! wc-scan — recursive wallpaper scanning, library index building.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use camino::Utf8PathBuf;
use walkdir::WalkDir;
use wc_core::formats;
use wc_core::types::WallpaperEntry;

const WE_MARKER: &str = "/steamapps/workshop/content/431960";

/// Scan all sources for wallpaper files.
///
/// For Wallpaper Engine workshop-root sources, iterates project subdirectories
/// one level deep, reads project.json to pick the `file` field (skipping
/// scene / web / application types), and falls back to a recursive scan only
/// when no project.json exists.  This matches the Bash scanner behaviour.
pub fn scan_wallpapers(sources: &[String]) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut files: Vec<String> = Vec::new();

    for source in sources {
        let src_path = Path::new(source);
        if !src_path.is_dir() {
            continue;
        }

        if is_wallpaper_engine_source(source) {
            scan_we_workshop_root(src_path, &mut seen, &mut files);
            continue;
        }

        // ── Normal source directory ─────────────────────────────────
        scan_dir_recursive(src_path, &mut seen, &mut files);
    }
    files
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

        // Try to read the project.json for this project.
        let has_proj = project_dir.join("project.json").exists();
        let we_file = read_we_project_json(&project_dir);

        if let Some(ref wp) = we_file {
            // project.json gave us a usable file.
            let p = std::path::Path::new(wp);
            let c = canonicalize_str(&std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()));
            if seen.insert(c.clone()) {
                files.push(c);
            }
            continue;
        }

        if has_proj {
            // project.json exists but was unreadable or an unsupported type
            // (scene / web / application).  Skip the whole project.
            continue;
        }

        // No project.json at all — fall back to recursive scan.
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

fn canonicalize_str(p: &std::path::Path) -> String {
    p.to_string_lossy().to_string()
}

pub fn is_wallpaper_engine_source(path: &str) -> bool {
    path.contains(WE_MARKER)
}

/// Read a Wallpaper Engine project.json from `project_dir`.
///
/// Returns `Some(full_path)` for image/video projects that have a valid
/// `file` field pointing to an existing file with a supported extension.
/// Returns `None` for scene / web / application types, missing project.json,
/// unreadable JSON, or when the referenced file doesn't exist.
pub fn read_we_project_json(project_dir: &Path) -> Option<String> {
    let proj_path = project_dir.join("project.json");
    if !proj_path.exists() {
        return None;
    }
    let content = fs::read_to_string(&proj_path).ok()?;
    let proj: serde_json::Value = serde_json::from_str(&content).ok()?;

    let proj_type = proj.get("type").and_then(|v| v.as_str()).unwrap_or("");
    // scene / web / application types are not wallpapers
    if matches!(proj_type, "scene" | "web" | "application") {
        return None;
    }

    let file = proj.get("file").and_then(|v| v.as_str())?;
    // scene.json / scene.pkg / *.html / *.htm are not displayable
    let file_lower = file.to_lowercase();
    if file_lower == "scene.json"
        || file_lower == "scene.pkg"
        || file_lower.ends_with(".html")
        || file_lower.ends_with(".htm")
    {
        return None;
    }

    let full = project_dir.join(file);
    if !full.is_file() {
        return None;
    }
    let ext = formats::get_extension(&full.to_string_lossy())?;
    if !formats::is_supported_extension(&ext) {
        return None;
    }
    Some(full.to_string_lossy().to_string())
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

#[cfg(test)]
mod tests {
    use super::*;

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
        // Simulate a workshop root with two project dirs: one scene, one image.
        let root = tempfile::tempdir().unwrap();
        let root_path = format!(
            "{}/steamapps/workshop/content/431960",
            root.path().display()
        );
        std::fs::create_dir_all(&root_path).unwrap();

        // Scene project
        let scene_dir = format!("{}/111", root_path);
        std::fs::create_dir_all(&scene_dir).unwrap();
        std::fs::write(
            format!("{}/project.json", scene_dir),
            r#"{"type":"scene","file":"scene.json"}"#,
        )
        .unwrap();

        // Image project
        let img_dir = format!("{}/222", root_path);
        std::fs::create_dir_all(&img_dir).unwrap();
        let img = format!("{}/bg.png", img_dir);
        std::fs::write(&img, b"").unwrap();
        std::fs::write(
            format!("{}/project.json", img_dir),
            format!(r#"{{"type":"image","file":"bg.png"}}"#),
        )
        .unwrap();

        // No-project dir (fallback scan)
        let fallback_dir = format!("{}/333", root_path);
        std::fs::create_dir_all(&fallback_dir).unwrap();
        std::fs::write(format!("{}/pic.jpg", fallback_dir), b"").unwrap();

        let sources = vec![root_path];
        let result = scan_wallpapers(&sources);

        let img_canon = std::fs::canonicalize(&img)
            .unwrap()
            .to_string_lossy()
            .to_string();
        let jpg_canon = std::fs::canonicalize(format!("{}/pic.jpg", fallback_dir))
            .unwrap()
            .to_string_lossy()
            .to_string();

        // Scene project must NOT be in results.
        let scene_assets: Vec<&String> = result.iter().filter(|p| p.contains("111")).collect();
        assert!(
            scene_assets.is_empty(),
            "scene project should not produce any entries, got: {:?}",
            scene_assets
        );

        // Image project's bg.png must be in results.
        assert!(
            result.contains(&img_canon),
            "image project's bg.png should be in results"
        );

        // Fallback dir's pic.jpg (no project.json) must be in results.
        assert!(
            result.contains(&jpg_canon),
            "no-project dir should be scanned recursively, got: {:?}",
            result
        );
    }
}
