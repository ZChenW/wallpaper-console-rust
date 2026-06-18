//! Runtime config normalizer for all keys that flow into external command parameters.
//! This is the final defence — even if frontend or legacy config contains invalid values,
//! the backend must never pass unsupported arguments to external renderers.

fn clamp_i32_string(raw: &str, min: i32, max: i32, fallback: i32) -> String {
    raw.trim()
        .parse::<i32>()
        .ok()
        .map(|v| v.clamp(min, max))
        .unwrap_or(fallback)
        .to_string()
}

fn on_off(value: &str, fallback: &str) -> String {
    match value {
        "on" | "off" => value.to_string(),
        _ => fallback.to_string(),
    }
}

pub fn normalize_awww_transition_duration(raw: &str) -> String {
    let trimmed = raw.trim();
    match trimmed.parse::<f32>() {
        Ok(v) if v.is_finite() && (0.0..=60.0).contains(&v) => trimmed.to_string(),
        _ => "1".to_string(),
    }
}

pub fn normalize_awww_transition_fps(raw: &str) -> String {
    clamp_i32_string(raw, 1, 240, 60)
}

pub fn normalize_lwe_scaling(raw: &str) -> &'static str {
    match raw.trim() {
        "default" => "default",
        "fill" => "fill",
        "fit" => "fit",
        "stretch" => "stretch",
        _ => "default",
    }
}

pub fn normalize_lwe_target_mode(raw: &str) -> &'static str {
    match raw.trim() {
        "auto" => "auto",
        "screen-root" => "screen-root",
        "screen-span" => "screen-span",
        _ => "auto",
    }
}

pub fn normalize_config_value(key: &str, value: &str) -> String {
    match key {
        "storage_backend" => "sqlite".to_string(),

        "awww_transition_duration" => normalize_awww_transition_duration(value),

        "wallpaper_transition_fps" => normalize_awww_transition_fps(value),

        "linux_wallpaperengine_fps" => clamp_i32_string(value, 1, 240, 60),

        "linux_wallpaperengine_volume" => clamp_i32_string(value, 0, 100, 100),

        "linux_wallpaperengine_enabled" => on_off(value, "on"),

        "linux_wallpaperengine_muted" => on_off(value, "off"),

        "linux_wallpaperengine_scaling" => normalize_lwe_scaling(value).to_string(),

        "linux_wallpaperengine_target_mode" => normalize_lwe_target_mode(value).to_string(),

        _ => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_transition_fps() {
        assert_eq!(normalize_awww_transition_fps("0"), "1");
        assert_eq!(normalize_awww_transition_fps("999"), "240");
        assert_eq!(normalize_awww_transition_fps("bad"), "60");
        assert_eq!(normalize_awww_transition_fps("60"), "60");
    }

    #[test]
    fn normalizes_transition_duration() {
        assert_eq!(normalize_awww_transition_duration("0"), "0");
        assert_eq!(normalize_awww_transition_duration("1.5"), "1.5");
        assert_eq!(normalize_awww_transition_duration("-1"), "1");
        assert_eq!(normalize_awww_transition_duration("nan"), "1");
        assert_eq!(normalize_awww_transition_duration("999"), "1");
    }

    #[test]
    fn normalizes_lwe_enums() {
        assert_eq!(normalize_lwe_scaling("fill"), "fill");
        assert_eq!(normalize_lwe_scaling("bad"), "default");
        assert_eq!(normalize_lwe_target_mode("screen-root"), "screen-root");
        assert_eq!(normalize_lwe_target_mode("window"), "auto");
    }

    #[test]
    fn normalize_config_value_runtime_keys() {
        // storage_backend: only sqlite is valid; file/hybrid are legacy and forced to sqlite.
        assert_eq!(
            normalize_config_value("storage_backend", "sqlite"),
            "sqlite"
        );
        assert_eq!(normalize_config_value("storage_backend", "file"), "sqlite");
        assert_eq!(
            normalize_config_value("storage_backend", "hybrid"),
            "sqlite"
        );
        assert_eq!(
            normalize_config_value("storage_backend", "garbage"),
            "sqlite"
        );

        // awww_transition_duration: finite 0..=60 passes through, else fallback "1".
        assert_eq!(
            normalize_config_value("awww_transition_duration", "1.5"),
            "1.5"
        );
        assert_eq!(normalize_config_value("awww_transition_duration", "0"), "0");
        assert_eq!(
            normalize_config_value("awww_transition_duration", "60"),
            "60"
        );
        assert_eq!(
            normalize_config_value("awww_transition_duration", "-1"),
            "1"
        );
        assert_eq!(
            normalize_config_value("awww_transition_duration", "999"),
            "1"
        );
        assert_eq!(
            normalize_config_value("awww_transition_duration", "nan"),
            "1"
        );
        assert_eq!(
            normalize_config_value("awww_transition_duration", "bad"),
            "1"
        );

        // wallpaper_transition_fps: 1..=240 passes through, else fallback 60.
        assert_eq!(
            normalize_config_value("wallpaper_transition_fps", "60"),
            "60"
        );
        assert_eq!(normalize_config_value("wallpaper_transition_fps", "1"), "1");
        assert_eq!(
            normalize_config_value("wallpaper_transition_fps", "240"),
            "240"
        );
        assert_eq!(normalize_config_value("wallpaper_transition_fps", "0"), "1");
        assert_eq!(
            normalize_config_value("wallpaper_transition_fps", "999"),
            "240"
        );
        assert_eq!(
            normalize_config_value("wallpaper_transition_fps", "bad"),
            "60"
        );

        // linux_wallpaperengine_fps: 1..=240 passes through, else fallback 60.
        assert_eq!(
            normalize_config_value("linux_wallpaperengine_fps", "120"),
            "120"
        );
        assert_eq!(
            normalize_config_value("linux_wallpaperengine_fps", "1"),
            "1"
        );
        assert_eq!(
            normalize_config_value("linux_wallpaperengine_fps", "240"),
            "240"
        );
        assert_eq!(
            normalize_config_value("linux_wallpaperengine_fps", "0"),
            "1"
        );
        assert_eq!(
            normalize_config_value("linux_wallpaperengine_fps", "999"),
            "240"
        );
        assert_eq!(
            normalize_config_value("linux_wallpaperengine_fps", "bad"),
            "60"
        );

        // linux_wallpaperengine_volume: 0..=100 passes through, else fallback 100.
        assert_eq!(
            normalize_config_value("linux_wallpaperengine_volume", "50"),
            "50"
        );
        assert_eq!(
            normalize_config_value("linux_wallpaperengine_volume", "0"),
            "0"
        );
        assert_eq!(
            normalize_config_value("linux_wallpaperengine_volume", "100"),
            "100"
        );
        assert_eq!(
            normalize_config_value("linux_wallpaperengine_volume", "-5"),
            "0"
        );
        assert_eq!(
            normalize_config_value("linux_wallpaperengine_volume", "999"),
            "100"
        );
        assert_eq!(
            normalize_config_value("linux_wallpaperengine_volume", "bad"),
            "100"
        );

        // linux_wallpaperengine_enabled: on/off pass through, legacy/invalid -> "on".
        assert_eq!(
            normalize_config_value("linux_wallpaperengine_enabled", "on"),
            "on"
        );
        assert_eq!(
            normalize_config_value("linux_wallpaperengine_enabled", "off"),
            "off"
        );
        assert_eq!(
            normalize_config_value("linux_wallpaperengine_enabled", "true"),
            "on"
        );
        assert_eq!(
            normalize_config_value("linux_wallpaperengine_enabled", "bad"),
            "on"
        );

        // linux_wallpaperengine_muted: on/off pass through, legacy/invalid -> "off".
        assert_eq!(
            normalize_config_value("linux_wallpaperengine_muted", "on"),
            "on"
        );
        assert_eq!(
            normalize_config_value("linux_wallpaperengine_muted", "off"),
            "off"
        );
        assert_eq!(
            normalize_config_value("linux_wallpaperengine_muted", "false"),
            "off"
        );
        assert_eq!(
            normalize_config_value("linux_wallpaperengine_muted", "bad"),
            "off"
        );

        // linux_wallpaperengine_scaling: default/fill/fit/stretch pass through, else "default".
        assert_eq!(
            normalize_config_value("linux_wallpaperengine_scaling", "default"),
            "default"
        );
        assert_eq!(
            normalize_config_value("linux_wallpaperengine_scaling", "fill"),
            "fill"
        );
        assert_eq!(
            normalize_config_value("linux_wallpaperengine_scaling", "fit"),
            "fit"
        );
        assert_eq!(
            normalize_config_value("linux_wallpaperengine_scaling", "stretch"),
            "stretch"
        );
        assert_eq!(
            normalize_config_value("linux_wallpaperengine_scaling", "bad"),
            "default"
        );

        // linux_wallpaperengine_target_mode: auto/screen-root/screen-span pass through;
        // legacy "window" and invalid values -> "auto".
        assert_eq!(
            normalize_config_value("linux_wallpaperengine_target_mode", "auto"),
            "auto"
        );
        assert_eq!(
            normalize_config_value("linux_wallpaperengine_target_mode", "screen-root"),
            "screen-root"
        );
        assert_eq!(
            normalize_config_value("linux_wallpaperengine_target_mode", "screen-span"),
            "screen-span"
        );
        assert_eq!(
            normalize_config_value("linux_wallpaperengine_target_mode", "window"),
            "auto"
        );
        assert_eq!(
            normalize_config_value("linux_wallpaperengine_target_mode", "bad"),
            "auto"
        );
    }

    #[test]
    fn normalize_config_value_passes_gui_only_keys_through() {
        // GUI-only keys must pass through the `_ => value.to_string()` arm unchanged,
        // so the Rust normalizer never rewrites frontend-specific settings.
        assert_eq!(
            normalize_config_value("gui_theme", "obsidian_warm"),
            "obsidian_warm"
        );
        assert_eq!(
            normalize_config_value("gui_file_manager", "nautilus"),
            "nautilus"
        );
        assert_eq!(
            normalize_config_value("gui_thumbnail_cleanup_days", "30"),
            "30"
        );
        assert_eq!(
            normalize_config_value("gui_terminal_file_manager", "custom"),
            "custom"
        );
        assert_eq!(
            normalize_config_value("open_project_location_mode", "terminal"),
            "terminal"
        );
    }
}
