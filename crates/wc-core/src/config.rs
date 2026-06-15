use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::WcError;

const DEFAULT_CONFIG_PAIRS: &[(&str, &str)] = &[
    ("gif_backend", "awww"),
    ("image_backend", "awww"),
    ("video_backend", "mpvpaper"),
    ("mpvpaper_options", "--loop-file=inf --panscan=1.0"),
    ("mpvpaper_output", "*"),
    ("awww_transition_type", "fade"),
    ("awww_transition_duration", "1"),
    ("awww_resize", "crop"),
    ("wallpaper_transition_fps", "60"),
    ("linux_wallpaperengine_enabled", "on"),
    ("linux_wallpaperengine_path", "auto"),
    ("linux_wallpaperengine_scaling", "default"),
    ("linux_wallpaperengine_fps", "60"),
    ("linux_wallpaperengine_muted", "off"),
    ("linux_wallpaperengine_volume", "100"),
    ("linux_wallpaperengine_assets_dir", "auto"),
    ("linux_wallpaperengine_target_mode", "auto"),
    ("linux_wallpaperengine_target", ""),
    ("min_wallpaper_width", "1280"),
    ("min_wallpaper_height", "720"),
    ("preview_metadata", "compact"),
    ("gui_thumbnail_mode", "cache"),
    ("gui_thumbnail_cleanup_days", "30"),
    ("gui_thumbnail_failure_ttl_secs", "900"),
    ("gui_debug_logs", "off"),
    ("storage_backend", "sqlite"),
    ("open_project_location_mode", "ask"),
    ("gui_file_manager", "auto"),
    ("gui_file_manager_custom", ""),
    ("gui_terminal_file_manager", "yazi"),
    ("gui_terminal_file_manager_custom", ""),
];

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
    DEFAULT_CONFIG_PAIRS
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect()
}

/// Default config keys in the order used when writing flat config files.
pub fn default_config_keys() -> Vec<&'static str> {
    DEFAULT_CONFIG_PAIRS.iter().map(|(key, _)| *key).collect()
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
    for (key, val) in DEFAULT_CONFIG_PAIRS {
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

/// Set a config value in the flat config file (atomic write).
pub fn write_config_value(config_dir: &Path, key: &str, value: &str) -> Result<(), WcError> {
    let config_path = config_dir.join("config");
    let mut map = if config_path.exists() {
        parse_config_file(&config_path)?
    } else {
        HashMap::new()
    };
    map.insert(key.to_string(), value.to_string());

    let content = serialize_config_map(&map);
    // Atomic: write to temp, then rename
    let tmp = config_path.with_extension("tmp");
    fs::write(&tmp, content).map_err(WcError::Io)?;
    fs::rename(&tmp, &config_path).map_err(WcError::Io)?;
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

/// Pure normalization: map any image_backend value to a valid backend name.
/// "mpvpaper" => "mpvpaper", "awww" | "swww" => "awww", anything else => "awww".
pub fn normalize_image_backend(raw: &str) -> &'static str {
    match raw {
        "mpvpaper" => "mpvpaper",
        _ => "awww",
    }
}

/// A handle for the wallpaper-console config directory.
pub struct ConfigDir {
    pub path: PathBuf,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_image_backend_known() {
        assert_eq!(normalize_image_backend("awww"), "awww");
        assert_eq!(normalize_image_backend("mpvpaper"), "mpvpaper");
    }

    #[test]
    fn normalize_image_backend_legacy_swww() {
        assert_eq!(normalize_image_backend("swww"), "awww");
    }

    #[test]
    fn normalize_image_backend_unknown_fallback() {
        assert_eq!(normalize_image_backend("bad"), "awww");
        assert_eq!(normalize_image_backend(""), "awww");
        assert_eq!(normalize_image_backend("unknown"), "awww");
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
    fn default_config_keys_are_unique() {
        let keys = default_config_keys();
        let unique = keys.iter().collect::<std::collections::HashSet<_>>();
        assert_eq!(keys.len(), unique.len());
        assert_eq!(keys.first().copied(), Some("gif_backend"));
        assert_eq!(
            keys.last().copied(),
            Some("gui_terminal_file_manager_custom")
        );
    }

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
