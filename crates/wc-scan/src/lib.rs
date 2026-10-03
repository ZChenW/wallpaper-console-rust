//! Recursive wallpaper scanning and indexed project metadata.

pub mod source_scan;
pub use source_scan::*;
mod steam_discovery;
pub use steam_discovery::{discover_steam_workshop_roots, steam_workshop_root_candidates};
mod project;
mod resolution;
mod walk;
pub(crate) use project::make_we_project_entry_from_info;
pub use project::{
    classify_entry, make_entry, make_entry_cached, read_we_project_info, read_we_project_json,
    safe_join, we_project_info_from_json, workshop_id_from_path, EntryClassification,
    WeProjectInfo,
};
pub use walk::{
    dedupe_sources, is_wallpaper_engine_source, normalize_source_path, scan_wallpapers,
    scan_wallpapers_with_callback, visit_wallpapers_with_callback, ScanControl, ScanEvent,
    ScanVisitControl,
};

#[cfg(test)]
use camino::Utf8PathBuf;
#[cfg(test)]
use std::{collections::HashSet, path::Path};
#[cfg(test)]
use steam_discovery::{
    discover_steam_workshop_roots_with_xdg_data_home, STEAM_LIBRARY_FOLDERS_SIZE_CAP,
};
#[cfg(test)]
use walk::{we_source_kind, we_workshop_root, WeKind, CANCEL_CHECK_INTERVAL};
#[cfg(test)]
use wc_core::types::{Backend, FileType, WallpaperEntry};

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
fn cached_dimensions_do_not_preserve_obsolete_static_animation_classification() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("animated.png");
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.extend_from_slice(&8u32.to_be_bytes());
    bytes.extend_from_slice(b"acTL");
    bytes.extend_from_slice(&[0; 12]);
    std::fs::write(&path, bytes).unwrap();
    let mut prior = make_entry(path.to_str().unwrap()).unwrap();
    prior.file_type = wc_core::types::FileType::Image;
    prior.backend = wc_core::types::Backend::Awww;
    prior.resolution = "32x32".into();
    let cache = std::collections::HashMap::from([(
        path.canonicalize().unwrap().to_string_lossy().into_owned(),
        prior,
    )]);
    let (entry, reused) = make_entry_cached(path.to_str().unwrap(), &cache);
    let entry = entry.unwrap();
    assert!(reused);
    assert_eq!(entry.file_type, wc_core::types::FileType::Gif);
    assert_eq!(entry.backend, wc_core::types::Backend::Mpvpaper);
    assert_eq!(entry.resolution, "32x32");
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
