//! wc-backend — wallpaper backend process management.

use std::process::Command;
use wc_core::config::ConfigDir;
use wc_core::error::WcError;
use wc_core::types::Backend;
use wc_storage::flat;

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
pub fn stop_backends_for_target(cd: &ConfigDir, target_backend: Backend) -> Result<(), WcError> {
    let last = flat::last_backend_read(cd)?.unwrap_or_default();
    if target_backend == Backend::Mpvpaper {
        return stop_all_backends();
    }
    // image→image: keep awww daemon, only kill mpvpaper
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
pub fn apply_wallpaper(cd: &ConfigDir, path: &str, backend: Backend) -> Result<(), WcError> {
    stop_backends_for_target(cd, backend)?;

    match backend {
        Backend::Awww => {
            ensure_awww_daemon()?;
            let status = Command::new("awww")
                .arg("img")
                .arg(path)
                .status()
                .map_err(|e| WcError::Other(format!("awww failed: {}", e)))?;
            if !status.success() {
                return Err(WcError::Other("awww failed to apply wallpaper".into()));
            }
        }
        Backend::Mpvpaper => {
            let status = Command::new("setsid")
                .args(["-f", "mpvpaper", "--fork", "--", path])
                .status()
                .map_err(|e| WcError::Other(format!("mpvpaper failed: {}", e)))?;
            if !status.success() {
                return Err(WcError::Other("mpvpaper failed to apply wallpaper".into()));
            }
        }
    }

    // Write state only after successful apply
    flat::current_write(cd, path)?;
    flat::last_backend_write(cd, backend.as_str())?;
    flat::history_add(cd, path, 100)?;

    Ok(())
}

/// Restore the last wallpaper.
pub fn restore(cd: &ConfigDir) -> Result<(), WcError> {
    let current = flat::current_read(cd)?
        .ok_or_else(|| WcError::Other("no previous wallpaper to restore".into()))?;
    let p = std::path::Path::new(&current);
    if !p.is_file() {
        return Err(WcError::WallpaperMissing(p.to_path_buf()));
    }
    let ext = wc_core::formats::get_extension(&current)
        .ok_or_else(|| WcError::UnsupportedFileType(current.clone()))?;
    let (_ftype, backend) = wc_core::formats::classify_extension(&ext)
        .ok_or_else(|| WcError::UnsupportedFileType(current.clone()))?;
    apply_wallpaper(cd, &current, backend)
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
