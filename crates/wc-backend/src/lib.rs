//! wc-backend — wallpaper backend process management.

use std::process::Command;
use wc_core::error::WcError;
use wc_core::types::Backend;
use wc_storage::StorageApi;

/// Stop all wallpaper backends via pkill.
pub fn stop_all_backends() -> Result<(), WcError> {
    let user = whoami();
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
    if target_backend == Backend::Mpvpaper {
        return stop_all_backends();
    }
    if last == "awww" {
        let user = whoami();
        let _ = Command::new("pkill")
            .args(["-u", &user, "-x", "mpvpaper"])
            .status();
    } else {
        stop_all_backends()?;
    }
    Ok(())
}

/// Apply a wallpaper via the appropriate backend process.
/// State is written ONLY after successful backend execution.
pub fn apply_wallpaper(s: &StorageApi, path: &str, backend: Backend) -> Result<(), WcError> {
    // Safety: verify file exists before attempting backend
    let p = std::path::Path::new(path);
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
    if !p.is_file() {
        return Err(WcError::WallpaperMissing(p.to_path_buf()));
    }
    let ext = wc_core::formats::get_extension(&current)
        .ok_or_else(|| WcError::UnsupportedFileType(current.clone()))?;
    let (_ftype, _default) = wc_core::formats::classify_extension(&ext)
        .ok_or_else(|| WcError::UnsupportedFileType(current.clone()))?;
    // Route through config
    let backend = match _ftype {
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
