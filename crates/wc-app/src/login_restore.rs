//! Keep `restore_on_login` wired to a real session-start command.
//!
//! Enabling the setting installs an XDG autostart entry that runs
//! `wallpaper-console-rust restore-at-login`. Disabling removes it.
use std::fs;
use std::path::{Path, PathBuf};

use wc_core::error::WcError;

const AUTOSTART_FILE: &str = "wallpaper-console-restore-at-login.desktop";

fn autostart_path() -> Result<PathBuf, WcError> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .ok_or_else(|| WcError::Other("HOME is not set; cannot manage login restore".into()))?;
    Ok(config.join("autostart").join(AUTOSTART_FILE))
}

fn resolve_cli_executable() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            for candidate in [
                parent.join("wallpaper-console-rust"),
                parent.join("../../bin/wallpaper-console-rust"),
            ] {
                if candidate.is_file() {
                    return candidate;
                }
            }
        }
    }
    let local = std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join(".local/bin/wallpaper-console-rust"));
    if let Some(path) = local.filter(|path| path.is_file()) {
        return path;
    }
    PathBuf::from("wallpaper-console-rust")
}

fn desktop_entry(executable: &Path) -> String {
    let exec = executable.display().to_string().replace('\\', "\\\\");
    format!(
        "\
[Desktop Entry]
Type=Application
Name=Wallpaper Console Login Restore
Comment=Restore saved wallpapers when the desktop session starts
Exec=\"{exec}\" restore-at-login
X-GNOME-Autostart-enabled=true
"
    )
}

/// Install or remove the XDG autostart entry for `restore-at-login`.
pub fn sync_login_restore_autostart(enabled: bool) -> Result<(), WcError> {
    let path = autostart_path()?;
    if !enabled {
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(WcError::Io(error)),
        }
    } else {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(WcError::Io)?;
        }
        fs::write(&path, desktop_entry(&resolve_cli_executable())).map_err(WcError::Io)
    }
}

/// After a successful config write for `restore_on_login`, keep autostart in sync.
pub fn sync_login_restore_autostart_for_value(value: &str) -> Result<(), WcError> {
    sync_login_restore_autostart(value == "on")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn enabling_writes_autostart_and_disabling_removes_it() {
        let _guard = env_lock().lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        std::env::set_var("XDG_CONFIG_HOME", &config);
        let path = config.join("autostart").join(AUTOSTART_FILE);

        sync_login_restore_autostart(true).unwrap();
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("restore-at-login"));
        assert!(content.contains("Exec="));

        sync_login_restore_autostart(false).unwrap();
        assert!(!path.exists());
        std::env::remove_var("XDG_CONFIG_HOME");
    }
}
