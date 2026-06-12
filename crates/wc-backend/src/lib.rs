//! wc-backend — wallpaper backend process management.

use std::process::Command;
use wc_core::error::WcError;
use wc_core::types::Backend;
use wc_storage::StorageApi;

pub mod linux_wallpaperengine;
pub mod web_wallpaper;

/// Stop all wallpaper backends via pkill.
pub fn stop_all_backends(s: Option<&StorageApi>) -> Result<(), WcError> {
    let user = whoami();
    linux_wallpaperengine::stop(s);
    web_wallpaper::stop(s);
    let _ = Command::new("pkill")
        .args(["-u", &user, "-x", "mpvpaper"])
        .status();
    let _ = Command::new("pkill")
        .args(["-u", &user, "-x", "awww"])
        .status();
    Ok(())
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
            .args(["-u", &user, "-x", "mpvpaper"])
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
    if backend == Backend::ChromiumWeb {
        let p = web_wallpaper::preflight(path, s)?;
        stop_all_backends(Some(s))?;
        web_wallpaper::apply_preflighted(s, &p)?;
        // Write state after successful backend execution.
        s.current_write(path)?;
        s.last_backend_write(backend.as_str())?;
        s.history_add(path, backend.as_str())?;
        return Ok(());
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
        Backend::LinuxWallpaperEngine | Backend::Unsupported | Backend::ChromiumWeb => {
            unreachable!()
        }
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
        wc_core::types::FileType::WeWeb => {
            return Err(WcError::Other(
                "Web wallpapers cannot be restored. Web wallpaper support requires a native layer-shell renderer and is not yet available. Use the experimental Chromium preview from the Library context menu instead.".into(),
            ));
        }
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
