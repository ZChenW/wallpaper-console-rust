use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::WcError;

/// Resolve the wallpaper-console config directory.
pub fn resolve_config_dir() -> Result<PathBuf, WcError> {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return Ok(PathBuf::from(xdg).join("wallpaper-console"));
        }
    }
    let home = std::env::var("HOME").map_err(|_| WcError::HomeNotSet)?;
    Ok(Path::new(&home).join(".config").join("wallpaper-console"))
}

/// Default config key-value pairs (populated on first run).
pub fn default_config() -> HashMap<String, String> {
    let mut defaults = HashMap::new();
    defaults.insert("gif_backend".into(), "awww".into());
    defaults.insert("image_backend".into(), "awww".into());
    defaults.insert("video_backend".into(), "mpvpaper".into());
    defaults.insert("mpvpaper_options".into(), "no-audio --loop-file=inf".into());
    defaults.insert("mpvpaper_output".into(), "*".into());
    defaults.insert("awww_transition_type".into(), "fade".into());
    defaults.insert("awww_transition_duration".into(), "1".into());
    defaults.insert("awww_resize".into(), "crop".into());
    defaults.insert("min_wallpaper_width".into(), "1280".into());
    defaults.insert("min_wallpaper_height".into(), "720".into());
    defaults.insert("preview_metadata".into(), "compact".into());
    defaults.insert("gui_thumbnail_mode".into(), "cache".into());
    defaults.insert("storage_backend".into(), "file".into());
    defaults.insert("gui_library_source".into(), "tsv".into());
    defaults
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
    let defaults = default_config();
    let mut to_add = Vec::new();
    for (key, val) in &defaults {
        if !existing.contains_key(key.as_str()) {
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

/// Set a config value in the flat config file (atomic write).
pub fn write_config_value(config_dir: &Path, key: &str, value: &str) -> Result<(), WcError> {
    let config_path = config_dir.join("config");
    let mut map = if config_path.exists() {
        parse_config_file(&config_path)?
    } else {
        HashMap::new()
    };
    map.insert(key.to_string(), value.to_string());

    let mut content = String::new();
    for (k, v) in &map {
        content.push_str(&format!("{}={}\n", k, v));
    }
    // Atomic: write to temp, then rename
    let tmp = config_path.with_extension("tmp");
    fs::write(&tmp, content).map_err(WcError::Io)?;
    fs::rename(&tmp, &config_path).map_err(WcError::Io)?;
    Ok(())
}

/// Read a single config value. Returns `default` if key not found.
pub fn read_config_value(config_dir: &Path, key: &str, default: &str) -> String {
    if key == "storage_backend" {
        // Bootstrap-safe: always read from flat config
        let path = config_dir.join("config");
        parse_config_file(&path)
            .ok()
            .and_then(|m| m.get(key).cloned())
            .unwrap_or_else(|| default.to_string())
    } else {
        let path = config_dir.join("config");
        parse_config_file(&path)
            .ok()
            .and_then(|m| m.get(key).cloned())
            .unwrap_or_else(|| default.to_string())
    }
}

/// A handle for the wallpaper-console config directory.
pub struct ConfigDir {
    pub path: PathBuf,
}

impl ConfigDir {
    pub fn new() -> Result<Self, WcError> {
        let path = resolve_config_dir()?;
        Ok(ConfigDir { path })
    }

    pub fn init(&self) -> Result<(), WcError> {
        init_config_dir(&self.path)
    }

    pub fn config_path(&self) -> PathBuf {
        self.path.join("config")
    }

    pub fn sources_path(&self) -> PathBuf {
        self.path.join("sources")
    }

    pub fn favorites_path(&self) -> PathBuf {
        self.path.join("favorites")
    }

    pub fn history_path(&self) -> PathBuf {
        self.path.join("history")
    }

    pub fn current_path(&self) -> PathBuf {
        self.path.join("current")
    }

    pub fn last_backend_path(&self) -> PathBuf {
        self.path.join("last_backend")
    }

    pub fn library_tsv_path(&self) -> PathBuf {
        self.path.join("library.tsv")
    }

    pub fn db_path(&self) -> PathBuf {
        self.path.join("wallpapers.db")
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.path.join("cache")
    }

    pub fn preview_cache_dir(&self) -> PathBuf {
        self.path.join("cache").join("previews")
    }

    pub fn gui_thumbnail_cache_dir(&self) -> PathBuf {
        self.path.join("cache").join("gui-thumbnails")
    }
}
