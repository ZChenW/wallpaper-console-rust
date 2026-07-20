//! wc-config — filesystem and environment IO for wallpaper-console config.
//!
//! Pure path types and defaults live in `wc-core::config`. This crate owns the
//! real IO: resolving the config directory, initializing runtime files, and
//! reading/writing the flat `key=value` config file.

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use fs2::FileExt;
use wc_core::config::{default_config_keys, default_config_pairs, ConfigDir};
use wc_core::error::WcError;

/// Resolve the wallpaper-console config directory from the environment.
pub fn resolve_config_dir() -> Result<PathBuf, WcError> {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return Ok(PathBuf::from(xdg).join("wallpaper-console"));
        }
    }
    let home = std::env::var("HOME").map_err(|_| WcError::HomeNotSet)?;
    Ok(Path::new(&home).join(".config").join("wallpaper-console"))
}

/// Runtime files that must exist.
const RUNTIME_FILES: &[&str] = &[
    "sources",
    "config",
    "current",
    "last_backend",
    "favorites",
    "history",
];

/// Initialize the config directory: create it and populate missing runtime files
/// and config defaults. Never overwrites existing values.
pub fn init_config_dir(config_dir: &Path) -> Result<(), WcError> {
    fs::create_dir_all(config_dir).map_err(WcError::Io)?;

    // Ensure runtime files exist
    for f in RUNTIME_FILES {
        let path = config_dir.join(f);
        if !path.exists() {
            fs::write(&path, "").map_err(WcError::Io)?;
        }
    }

    // Ensure library.tsv exists
    let lib_path = config_dir.join("library.tsv");
    if !lib_path.exists() {
        fs::write(&lib_path, "").map_err(WcError::Io)?;
    }

    // Populate defaults for missing config keys
    let config_path = config_dir.join("config");
    let existing = parse_config_file(&config_path)?;
    let mut to_add = Vec::new();
    for (key, val) in default_config_pairs() {
        if !existing.contains_key(*key) {
            to_add.push(format!("{}={}", key, val));
        }
    }
    if !to_add.is_empty() {
        let mut content = fs::read_to_string(&config_path).unwrap_or_default();
        for line in &to_add {
            content.push_str(line);
            content.push('\n');
        }
        fs::write(&config_path, content).map_err(WcError::Io)?;
    }

    Ok(())
}

/// Parse a flat `key=value` config file into a HashMap.
/// Lines starting with `#` are treated as comments.
/// Empty lines are ignored.
pub fn parse_config_file(path: &Path) -> Result<HashMap<String, String>, WcError> {
    let mut map = HashMap::new();
    if !path.exists() {
        return Ok(map);
    }
    let content = fs::read_to_string(path).map_err(WcError::Io)?;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(eq_pos) = line.find('=') {
            let key = line[..eq_pos].to_string();
            let value = line[eq_pos + 1..].to_string();
            map.insert(key, value);
        }
    }
    Ok(map)
}

static ATOMIC_CONFIG_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn atomic_config_temp_path(config_path: &Path) -> PathBuf {
    let sequence = ATOMIC_CONFIG_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let mut temp_file_name = OsString::from(".");
    temp_file_name.push(
        config_path
            .file_name()
            .unwrap_or_else(|| OsStr::new("config")),
    );
    temp_file_name.push(format!(".tmp.{}.{}", std::process::id(), sequence));
    config_path.with_file_name(temp_file_name)
}

fn config_lock_path(config_dir: &Path) -> PathBuf {
    config_dir.join(".config.lock")
}

fn acquire_config_lock(config_dir: &Path) -> Result<File, WcError> {
    let lock_path = config_lock_path(config_dir);
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(WcError::Io)?;
    lock.lock_exclusive().map_err(WcError::Io)?;
    Ok(lock)
}

/// Set a config value in the flat config file (atomic write).
pub fn write_config_value(config_dir: &Path, key: &str, value: &str) -> Result<(), WcError> {
    if value.contains('\n') || value.contains('\r') {
        return Err(WcError::Other(format!(
            "config value for {key:?} must be a single line (found newline characters)"
        )));
    }
    let _lock = acquire_config_lock(config_dir)?;
    let config_path = config_dir.join("config");
    let mut map = if config_path.exists() {
        parse_config_file(&config_path)?
    } else {
        HashMap::new()
    };
    let value = if key == "storage_backend" {
        "sqlite"
    } else {
        value
    };
    map.insert(key.to_string(), value.to_string());

    let content = serialize_config_map(&map);
    // Atomic: write to temp, fsync, then rename
    let tmp = atomic_config_temp_path(&config_path);
    let write_result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp)
            .map_err(WcError::Io)?;
        file.write_all(content.as_bytes()).map_err(WcError::Io)?;
        file.sync_all().map_err(WcError::Io)?;
        drop(file);
        fs::rename(&tmp, &config_path).map_err(WcError::Io)
    })();
    if let Err(error) = write_result {
        let _ = fs::remove_file(&tmp);
        return Err(error);
    }
    Ok(())
}

fn serialize_config_map(map: &HashMap<String, String>) -> String {
    let mut content = String::new();
    let mut emitted = std::collections::HashSet::new();

    for key in default_config_keys() {
        if let Some(value) = map.get(key) {
            content.push_str(key);
            content.push('=');
            content.push_str(value);
            content.push('\n');
            emitted.insert(key.to_string());
        }
    }

    let mut unknown: Vec<&String> = map.keys().filter(|key| !emitted.contains(*key)).collect();
    unknown.sort();
    for key in unknown {
        if let Some(value) = map.get(key) {
            content.push_str(key);
            content.push('=');
            content.push_str(value);
            content.push('\n');
        }
    }

    content
}

/// Read a single config value. Returns `default` if key not found.
pub fn read_config_value(config_dir: &Path, key: &str, default: &str) -> String {
    let path = config_dir.join("config");
    let value = parse_config_file(&path)
        .ok()
        .and_then(|m| m.get(key).cloned())
        .unwrap_or_else(|| default.to_string());

    if key == "storage_backend" {
        "sqlite".to_string()
    } else {
        value
    }
}

/// IO helpers for [`ConfigDir`] (resolve from env, initialize on disk).
///
/// Kept as an extension trait so call sites can keep `ConfigDir::new()` /
/// `cd.init()` after the IO moved out of `wc-core`.
pub trait ConfigDirExt: Sized {
    fn new() -> Result<Self, WcError>;
    fn init(&self) -> Result<(), WcError>;
}

impl ConfigDirExt for ConfigDir {
    fn new() -> Result<Self, WcError> {
        let path = resolve_config_dir()?;
        Ok(ConfigDir { path })
    }

    fn init(&self) -> Result<(), WcError> {
        init_config_dir(&self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wc_core::config::default_config;

    #[test]
    fn write_config_value_is_deterministic() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = tmp.path().join("config");
        fs::write(
            &cfg,
            "z_custom=last\nimage_backend=awww\ngif_backend=awww\n",
        )
        .unwrap();

        write_config_value(tmp.path(), "mpvpaper_output", "DP-1").unwrap();
        let first = fs::read_to_string(&cfg).unwrap();
        write_config_value(tmp.path(), "mpvpaper_output", "DP-1").unwrap();
        let second = fs::read_to_string(&cfg).unwrap();

        assert_eq!(first, second);
        assert!(first.contains("gif_backend=awww\nimage_backend=awww\n"));
        assert!(first.ends_with("z_custom=last\n"));
    }

    #[test]
    fn atomic_config_temp_paths_are_unique() {
        let config_path = Path::new("/tmp/config");

        let first = atomic_config_temp_path(config_path);
        let second = atomic_config_temp_path(config_path);

        assert_ne!(first, second);
        assert_eq!(first.parent(), config_path.parent());
        assert_eq!(second.parent(), config_path.parent());

        let expected_prefix = format!(".config.tmp.{}.", std::process::id());
        assert!(first
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with(&expected_prefix));
        assert!(second
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with(&expected_prefix));
    }

    #[test]
    fn init_config_dir_appends_missing_defaults_in_registry_order() {
        let tmp = tempfile::tempdir().unwrap();
        init_config_dir(tmp.path()).unwrap();
        let content = fs::read_to_string(tmp.path().join("config")).unwrap();
        let first_three: Vec<&str> = content.lines().take(3).collect();
        assert_eq!(
            first_three,
            vec![
                "gif_backend=awww",
                "image_backend=awww",
                "video_backend=mpvpaper"
            ]
        );
    }

    #[test]
    fn storage_backend_reads_are_sqlite_only() {
        let tmp = tempfile::tempdir().unwrap();
        init_config_dir(tmp.path()).unwrap();
        std::fs::write(tmp.path().join("config"), "storage_backend=hybrid\n").unwrap();

        assert_eq!(
            read_config_value(tmp.path(), "storage_backend", "file"),
            "sqlite"
        );
    }

    #[test]
    fn storage_backend_writes_are_normalized_to_sqlite() {
        let tmp = tempfile::tempdir().unwrap();
        init_config_dir(tmp.path()).unwrap();

        write_config_value(tmp.path(), "storage_backend", "file").unwrap();

        let content = std::fs::read_to_string(tmp.path().join("config")).unwrap();
        assert!(content.contains("storage_backend=sqlite\n"));
        assert!(!content.contains("storage_backend=file\n"));
    }

    #[test]
    fn test_config_parse_basic() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config");
        std::fs::write(&config_path, "key1=val1\nkey2=val with spaces\n# comment\n").unwrap();
        let map = parse_config_file(&config_path).unwrap();
        assert_eq!(map.get("key1").unwrap(), "val1");
        assert_eq!(map.get("key2").unwrap(), "val with spaces");
        assert!(!map.contains_key("# comment"));
    }

    #[test]
    fn default_mpvpaper_options_includes_panscan() {
        let defaults = default_config();
        assert_eq!(
            defaults.get("mpvpaper_options").unwrap(),
            "--loop-file=inf --panscan=1.0"
        );
    }

    #[test]
    fn write_config_value_rejects_multiline_values() {
        let tmp = tempfile::tempdir().unwrap();
        init_config_dir(tmp.path()).unwrap();

        let error =
            write_config_value(tmp.path(), "lwe_last_stderr", "line one\nline two").unwrap_err();

        assert!(
            error
                .to_string()
                .contains("must be a single line"),
            "unexpected error: {error}"
        );
    }
}
