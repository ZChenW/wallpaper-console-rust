//! Keep `restore_on_login` wired to a real session-start command.
//!
//! Enabling the setting installs an XDG autostart entry that runs
//! `wallpaper-console-rust restore-at-login`. Disabling removes it.
use fs2::FileExt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use wc_core::behavior_setting::{BehaviorSettingsPatch, BehaviorSettingsSnapshot};
use wc_core::error::WcError;
use wc_storage::StorageApi;

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
    // Exec is desktop-entry syntax, not a shell command. Escape both layers.
    let exec = executable
        .display()
        .to_string()
        .replace('\\', "\\\\\\\\")
        .replace('"', "\\\\\"")
        .replace('`', "\\\\`")
        .replace('$', "\\\\$")
        .replace('%', "%%");
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
    let _lock = lock_at(&path)?;
    sync_at(&path, enabled)
}

fn sync_at(path: &Path, enabled: bool) -> Result<(), WcError> {
    if !enabled {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(WcError::Io(error)),
        }
    } else {
        let contents = desktop_entry(&resolve_cli_executable());
        if fs::read(path).ok().as_deref() == Some(contents.as_bytes()) {
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(WcError::Io)?;
        }
        write_entry(path, contents.as_bytes())
    }
}

fn lock_at(path: &Path) -> Result<fs::File, WcError> {
    let config = path
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| WcError::Other("invalid autostart path".into()))?;
    fs::create_dir_all(config)?;
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(config.join(".wallpaper-console-login-restore.lock"))?;
    lock.lock_exclusive()?;
    Ok(lock)
}

fn write_entry(path: &Path, contents: &[u8]) -> Result<(), WcError> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let temporary = path.with_extension(format!(
        "tmp.{}.{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| {
        file.write_all(contents)?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(WcError::Io)
}

pub fn update_behavior_settings(
    storage: &StorageApi,
    revision: &str,
    patch: &BehaviorSettingsPatch,
) -> Result<BehaviorSettingsSnapshot, WcError> {
    if patch.restore_on_login.is_none() {
        return storage.update_behavior_settings(revision, patch);
    }
    update_at(storage, revision, patch, &autostart_path()?)
}

fn update_at(
    storage: &StorageApi,
    revision: &str,
    patch: &BehaviorSettingsPatch,
    path: &Path,
) -> Result<BehaviorSettingsSnapshot, WcError> {
    let _lock = lock_at(path)?;
    let before = storage.behavior_settings()?;
    if before.revision != revision {
        return Err(WcError::ConfigRevisionChanged {
            expected: revision.into(),
            observed: before.revision,
        });
    }
    let previous_entry = match fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    // Always reconcile, including same-value retries. A failed filesystem update
    // leaves the entire settings patch and revision unchanged.
    sync_at(path, before.settings.apply_patch(patch).restore_on_login)?;
    match storage.update_behavior_settings(revision, patch) {
        Ok(snapshot) => Ok(snapshot),
        Err(error) => {
            let rollback = match previous_entry {
                Some(bytes) => write_entry(path, &bytes),
                None => sync_at(path, false),
            };
            if let Err(rollback) = rollback {
                return Err(WcError::Other(format!("{error}; login autostart rollback failed: {rollback}; retry saving restore_on_login")));
            }
            Err(error)
        }
    }
}

/// CLI and compatibility GUI writes share the typed, revision-checked path.
pub fn config_set(storage: &StorageApi, key: &str, value: &str) -> Result<(), WcError> {
    if key != "restore_on_login" {
        return storage.config_set(key, value);
    }
    wc_core::config::validate_config_entry(key, value)?;
    let enabled = wc_core::config_normalizer::normalize_config_value(key, value) == "on";
    let revision = storage.behavior_settings()?.revision;
    update_behavior_settings(
        storage,
        &revision,
        &BehaviorSettingsPatch {
            restore_on_login: Some(enabled),
            ..Default::default()
        },
    )
    .map(|_| ())
}

/// Repair older enabled installs and interrupted saves on application startup.
/// Never run restore here: registration is distinct from wallpaper playback.
pub fn reconcile(storage: &StorageApi) -> Result<(), WcError> {
    reconcile_at(storage, &autostart_path()?)
}

fn reconcile_at(storage: &StorageApi, path: &Path) -> Result<(), WcError> {
    let _lock = lock_at(path)?;
    sync_at(path, storage.behavior_settings()?.settings.restore_on_login)
}

/// After a successful config write for `restore_on_login`, keep autostart in sync.
pub fn sync_login_restore_autostart_for_value(value: &str) -> Result<(), WcError> {
    sync_login_restore_autostart(value == "on")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autostart_failure_does_not_commit_settings_and_retry_repairs_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = StorageApi::try_new(wc_core::ConfigDir {
            path: tmp.path().join("wc"),
        })
        .unwrap();
        let parent = tmp.path().join("autostart");
        fs::write(&parent, "not a directory").unwrap();
        let path = parent.join(AUTOSTART_FILE);
        let before = storage.behavior_settings().unwrap();
        let patch = BehaviorSettingsPatch {
            restore_on_login: Some(true),
            awww_transition_fps: Some(30),
            ..Default::default()
        };
        assert!(update_at(&storage, &before.revision, &patch, &path).is_err());
        assert_eq!(storage.behavior_settings().unwrap(), before);
        fs::remove_file(parent).unwrap();
        let after = update_at(&storage, &before.revision, &patch, &path).unwrap();
        assert!(path.exists());
        fs::remove_file(&path).unwrap();
        update_at(&storage, &after.revision, &patch, &path).unwrap();
        assert!(
            path.exists(),
            "same-value retry must repair a missing entry"
        );
    }

    #[test]
    fn enabling_writes_autostart_and_disabling_removes_it() {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        let path = config.join("autostart").join(AUTOSTART_FILE);

        sync_at(&path, true).unwrap();
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("restore-at-login"));
        assert!(content.contains("Exec="));

        sync_at(&path, false).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn startup_repairs_existing_enabled_config_without_changing_preferences() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = StorageApi::try_new(wc_core::ConfigDir {
            path: tmp.path().join("wc"),
        })
        .unwrap();
        storage.config_set("restore_on_login", "on").unwrap();
        let before = storage.behavior_settings().unwrap();
        let path = tmp.path().join("autostart").join(AUTOSTART_FILE);
        reconcile_at(&storage, &path).unwrap();
        assert!(path.exists());
        assert_eq!(storage.behavior_settings().unwrap(), before);
        storage.config_set("restore_on_login", "off").unwrap();
        reconcile_at(&storage, &path).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn database_failure_restores_previous_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = StorageApi::try_new(wc_core::ConfigDir {
            path: tmp.path().join("wc"),
        })
        .unwrap();
        let path = tmp.path().join("autostart").join(AUTOSTART_FILE);
        fs::create_dir(path.parent().unwrap()).unwrap();
        fs::write(&path, "old entry").unwrap();
        let before = storage.behavior_settings().unwrap();
        let conn = wc_storage::sqlite::open_runtime_connection(&storage.cd).unwrap();
        conn.execute_batch("CREATE TRIGGER reject_config BEFORE INSERT ON config BEGIN SELECT RAISE(FAIL, 'injected'); END;").unwrap();
        let patch = BehaviorSettingsPatch {
            restore_on_login: Some(true),
            ..Default::default()
        };
        assert!(update_at(&storage, &before.revision, &patch, &path).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "old entry");
        assert_eq!(storage.behavior_settings().unwrap(), before);
    }

    #[test]
    fn revision_conflict_never_changes_autostart() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = StorageApi::try_new(wc_core::ConfigDir {
            path: tmp.path().join("wc"),
        })
        .unwrap();
        let path = tmp.path().join("autostart").join(AUTOSTART_FILE);
        let patch = BehaviorSettingsPatch {
            restore_on_login: Some(true),
            ..Default::default()
        };
        assert!(matches!(
            update_at(&storage, "stale", &patch, &path),
            Err(WcError::ConfigRevisionChanged { .. })
        ));
        assert!(!path.exists());
    }

    #[test]
    fn failed_disable_restores_enabled_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = StorageApi::try_new(wc_core::ConfigDir {
            path: tmp.path().join("wc"),
        })
        .unwrap();
        storage.config_set("restore_on_login", "on").unwrap();
        let path = tmp.path().join("autostart").join(AUTOSTART_FILE);
        reconcile_at(&storage, &path).unwrap();
        let before = storage.behavior_settings().unwrap();
        let contents = fs::read(&path).unwrap();
        let conn = wc_storage::sqlite::open_runtime_connection(&storage.cd).unwrap();
        conn.execute_batch("CREATE TRIGGER reject_config BEFORE INSERT ON config BEGIN SELECT RAISE(FAIL, 'injected'); END;").unwrap();
        let patch = BehaviorSettingsPatch {
            restore_on_login: Some(false),
            ..Default::default()
        };
        assert!(update_at(&storage, &before.revision, &patch, &path).is_err());
        assert_eq!(fs::read(&path).unwrap(), contents);
        assert_eq!(storage.behavior_settings().unwrap(), before);
    }
}
