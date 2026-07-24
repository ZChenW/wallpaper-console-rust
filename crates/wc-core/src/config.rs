use std::collections::HashMap;
use std::path::PathBuf;

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
    ("restore_on_login", "off"),
    ("linux_wallpaperengine_enabled", "on"),
    ("linux_wallpaperengine_path", "auto"),
    ("linux_wallpaperengine_scaling", "default"),
    ("linux_wallpaperengine_fps", "60"),
    ("linux_wallpaperengine_muted", "off"),
    ("linux_wallpaperengine_volume", "100"),
    ("linux_wallpaperengine_target_mode", "auto"),
    ("linux_wallpaperengine_target", ""),
    ("preview_metadata", "compact"),
    ("gui_thumbnail_mode", "cache"),
    ("gui_thumbnail_cleanup_days", "30"),
    ("gui_thumbnail_failure_ttl_secs", "900"),
    ("gui_debug_logs", "off"),
    ("gui_theme", "light"),
    ("storage_backend", "sqlite"),
    ("open_project_location_mode", "ask"),
    ("gui_file_manager", "auto"),
    ("gui_file_manager_custom", ""),
    ("gui_terminal_file_manager", "yazi"),
    ("gui_terminal_file_manager_custom", ""),
    ("post_apply_enabled", "off"),
    ("post_apply_command", "matugen image \"$still\""),
    ("post_apply_timeout_secs", "30"),
];

/// Default config key-value pairs (populated on first run).
pub fn default_config_pairs() -> &'static [(&'static str, &'static str)] {
    DEFAULT_CONFIG_PAIRS
}

/// Default config key-value pairs as an owned map (test/helper convenience).
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

/// A handle for the wallpaper-console config directory.
///
/// Pure path derivation only. Resolving from the environment and creating
/// on-disk files live in `wc-config`.
pub struct ConfigDir {
    pub path: PathBuf,
}

impl ConfigDir {
    /// Construct from an already-resolved path (tests and explicit paths).
    pub fn from_path(path: PathBuf) -> Self {
        ConfigDir { path }
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

    pub fn theme_stills_cache_dir(&self) -> PathBuf {
        self.path.join("cache").join("theme-stills")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_mpvpaper_options_includes_panscan() {
        let defaults = default_config();
        assert_eq!(
            defaults.get("mpvpaper_options").unwrap(),
            "--loop-file=inf --panscan=1.0"
        );
    }

    #[test]
    fn login_restore_is_opt_in_by_default() {
        let defaults = default_config();
        assert_eq!(
            defaults.get("restore_on_login").map(String::as_str),
            Some("off")
        );
    }

    #[test]
    fn default_config_keys_are_unique() {
        let keys = default_config_keys();
        let unique = keys.iter().collect::<std::collections::HashSet<_>>();
        assert_eq!(keys.len(), unique.len());
        assert_eq!(keys.first().copied(), Some("gif_backend"));
        assert_eq!(keys.last().copied(), Some("post_apply_timeout_secs"));
    }

    #[test]
    fn default_config_includes_gui_theme() {
        let defaults = default_config();
        assert_eq!(defaults.get("gui_theme").map(|s| s.as_str()), Some("light"));
    }

    #[test]
    fn path_helpers_join_expected_names() {
        let cd = ConfigDir::from_path(PathBuf::from("/tmp/wc-test"));
        assert_eq!(cd.config_path(), PathBuf::from("/tmp/wc-test/config"));
        assert_eq!(cd.db_path(), PathBuf::from("/tmp/wc-test/wallpapers.db"));
        assert_eq!(
            cd.gui_thumbnail_cache_dir(),
            PathBuf::from("/tmp/wc-test/cache/gui-thumbnails")
        );
        assert_eq!(
            cd.theme_stills_cache_dir(),
            PathBuf::from("/tmp/wc-test/cache/theme-stills")
        );
    }

    #[test]
    fn default_config_includes_post_apply_keys() {
        let defaults = default_config();
        assert_eq!(
            defaults.get("post_apply_enabled").map(String::as_str),
            Some("off")
        );
        assert_eq!(
            defaults.get("post_apply_command").map(String::as_str),
            Some("matugen image \"$still\"")
        );
        assert_eq!(
            defaults.get("post_apply_timeout_secs").map(String::as_str),
            Some("30")
        );
    }
}
