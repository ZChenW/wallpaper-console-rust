//! wc-backend — wallpaper backend process management.

use std::process::{Command, Stdio};
use wc_core::error::WcError;
use wc_core::types::Backend;
use wc_storage::StorageApi;

pub mod linux_wallpaperengine;
pub mod process_control;

/// Stop all wallpaper backends via pkill.
pub fn stop_all_backends(s: Option<&StorageApi>) -> Result<(), WcError> {
    let user = whoami();
    linux_wallpaperengine::stop(s);
    let _ = Command::new("pkill")
        .args(["-u", &user, "-f", r"(^|/)mpvpaper\b"])
        .status();
    let _ = Command::new("pkill")
        .args(["-u", &user, "-f", r"(^|/)awww\b"])
        .status();
    // Fallback cleanup: kill residual scene renderer processes that may not have been
    // recorded in config (e.g. setsid forked and parent PID was recorded, or a crash
    // left the process behind).
    let _ = Command::new("pkill")
        .args(["-u", &user, "-f", r"(^|/)linux-wallpaperengine\b"])
        .status();
    Ok(())
}

/// Backend name constant used for LWE state tracking.
pub const LWE_BACKEND_NAME: &str = "linux-wallpaperengine";

/// Stop only non-LWE wallpaper backends (mpvpaper, awww).
/// Used during scene-to-scene handoff to avoid exposing the static background.
pub fn stop_non_lwe_backends(_s: &StorageApi) {
    let user = whoami();
    let _ = Command::new("pkill")
        .args(["-u", &user, "-f", r"(^|/)mpvpaper\b"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = Command::new("pkill")
        .args(["-u", &user, "-f", r"(^|/)awww\b"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Smart-stop: kill only what's needed for the target backend.
pub fn stop_backends_for_target(s: &StorageApi, target_backend: Backend) -> Result<(), WcError> {
    let last = s.last_backend_read()?.unwrap_or_default();
    if target_backend == Backend::Mpvpaper || target_backend == Backend::LinuxWallpaperEngine {
        return stop_all_backends(Some(s));
    }
    if last == "awww" {
        let user = whoami();
        let _ = Command::new("pkill")
            .args(["-u", &user, "-f", r"(^|/)mpvpaper\b"])
            .status();
    } else {
        stop_all_backends(Some(s))?;
    }
    Ok(())
}

/// Apply a wallpaper via the appropriate backend process.
/// State is written ONLY after successful backend execution.
pub fn apply_wallpaper(s: &StorageApi, path: &str, backend: Backend) -> Result<(), WcError> {
    // Safety: verify file exists before attempting backend
    let p = std::path::Path::new(path);
    if backend == Backend::LinuxWallpaperEngine {
        let project = linux_wallpaperengine::project_from_path(path)?;
        return linux_wallpaperengine::apply(s, project);
    }
    if backend == Backend::Unsupported {
        return Err(WcError::UnsupportedFileType(path.to_string()));
    }
    if !p.is_file() {
        return Err(WcError::NotRegularFile(p.to_path_buf()));
    }

    stop_backends_for_target(s, backend)?;

    match backend {
        Backend::Awww => {
            ensure_awww_daemon()?;
            let transition = s.config_get("awww_transition_type", "fade");
            let duration = s.config_get("awww_transition_duration", "1");
            let resize = s.config_get("awww_resize", "crop");
            let status = Command::new("awww")
                .arg("img")
                .arg("--transition-type")
                .arg(&transition)
                .arg("--transition-duration")
                .arg(&duration)
                .arg("--resize")
                .arg(&resize)
                .arg("--filter")
                .arg("Lanczos3")
                .arg(path)
                .status()
                .map_err(|e| WcError::Other(format!("awww failed: {}", e)))?;
            if !status.success() {
                return Err(WcError::Other("awww failed to apply wallpaper".into()));
            }
        }
        Backend::Mpvpaper => {
            let opts = s.config_get("mpvpaper_options", "no-audio --loop-file=inf");
            let output = s.config_get("mpvpaper_output", "*");
            let status = Command::new("setsid")
                .args(["-f", "mpvpaper", "--fork", "-o", &opts, &output, "--", path])
                .status()
                .map_err(|e| WcError::Other(format!("mpvpaper failed: {}", e)))?;
            if !status.success() {
                return Err(WcError::Other("mpvpaper failed to apply wallpaper".into()));
            }
        }
        Backend::LinuxWallpaperEngine | Backend::Unsupported => unreachable!(),
    }

    // Write state only after successful apply
    s.current_write(path)?;
    s.last_backend_write(backend.as_str())?;
    s.history_add(path, backend.as_str())?;

    Ok(())
}

/// Restore the last wallpaper.
pub fn restore(s: &StorageApi) -> Result<(), WcError> {
    let current = s
        .current_read()?
        .ok_or_else(|| WcError::Other("no previous wallpaper to restore".into()))?;
    let p = std::path::Path::new(&current);
    if !p.is_file() && !p.is_dir() {
        return Err(WcError::WallpaperMissing(p.to_path_buf()));
    }
    let entry = wc_scan::make_entry(&current)
        .ok_or_else(|| WcError::UnsupportedFileType(current.clone()))?;
    // Route through config
    let backend = match entry.file_type {
        wc_core::types::FileType::Image => match s.config_get("image_backend", "awww").as_str() {
            "mpvpaper" => Backend::Mpvpaper,
            _ => Backend::Awww,
        },
        wc_core::types::FileType::Gif => match s.config_get("gif_backend", "awww").as_str() {
            "mpvpaper" => Backend::Mpvpaper,
            _ => Backend::Awww,
        },
        wc_core::types::FileType::Video => {
            match s.config_get("video_backend", "mpvpaper").as_str() {
                "awww" => Backend::Awww,
                _ => Backend::Mpvpaper,
            }
        }
        wc_core::types::FileType::WeScene => Backend::LinuxWallpaperEngine,
        wc_core::types::FileType::WeWeb => Backend::Unsupported,
        wc_core::types::FileType::WeApplication => Backend::Unsupported,
    };
    apply_wallpaper(s, &current, backend)
}

fn ensure_awww_daemon() -> Result<(), WcError> {
    let user = whoami();
    let pgrep = Command::new("pgrep")
        .args(["-u", &user, "-x", "awww-daemon"])
        .status()
        .unwrap_or_else(|_| std::process::ExitStatus::default());
    if pgrep.success() {
        return Ok(());
    }
    let _ = Command::new("setsid").args(["-f", "awww-daemon"]).status();
    std::thread::sleep(std::time::Duration::from_millis(200));
    Ok(())
}

fn whoami() -> String {
    std::env::var("USER").unwrap_or_else(|_| "unknown".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wc_core::config::ConfigDir;

    fn temp_storage() -> (tempfile::TempDir, StorageApi) {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        wc_core::config::write_config_value(&cd.path, "storage_backend", "sqlite").unwrap();
        let s = StorageApi::new(cd);
        (tmp, s)
    }

    #[test]
    fn restore_we_web_rejects_as_unsupported() {
        let (tmp, s) = temp_storage();

        let project = tmp
            .path()
            .join("steamapps/workshop/content/431960/3650880224");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("index.html"), b"<html></html>").unwrap();
        std::fs::write(
            project.join("project.json"),
            r#"{"type":"web","file":"index.html","title":"Test Web"}"#,
        )
        .unwrap();

        // Simulate a previous session having written a WE Web project as current.
        s.current_write(&project.to_string_lossy()).unwrap();
        s.last_backend_write("unsupported").unwrap();
        s.history_add(&project.to_string_lossy(), "unsupported")
            .unwrap();

        let history_before = s.history_list().unwrap().len();

        let err = restore(&s).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("unsupported") || msg.contains("Unsupported"),
            "error should explain that WE Web restore is unsupported, got: {}",
            msg
        );

        // Old state should remain — restore doesn't clear on error.
        assert_eq!(
            s.current_read().unwrap().as_deref(),
            Some(project.to_string_lossy().as_ref())
        );
        assert_eq!(
            s.last_backend_read().unwrap().as_deref(),
            Some("unsupported")
        );
        // No new history entry added by the failed restore.
        assert_eq!(
            s.history_list().unwrap().len(),
            history_before,
            "failed restore should not add history"
        );
    }

    #[test]
    fn apply_wallpaper_rejects_unsupported_backend() {
        let (_tmp, s) = temp_storage();

        let img = _tmp.path().join("test.png");
        std::fs::write(&img, b"").unwrap();

        let err = apply_wallpaper(&s, &img.to_string_lossy().to_string(), Backend::Unsupported)
            .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("unsupported") || msg.contains("Unsupported"),
            "apply_wallpaper should reject Unsupported backend, got: {}",
            msg
        );
    }
}
