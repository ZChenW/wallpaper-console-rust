/// Runtime config normalizer for all keys that flow into external command parameters.
/// This is the final defence — even if frontend or legacy config contains invalid values,
/// the backend must never pass unsupported arguments to external renderers.

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
        Ok(v) if v.is_finite() && v >= 0.0 && v <= 60.0 => trimmed.to_string(),
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
        assert_eq!(
            normalize_config_value("wallpaper_transition_fps", "999"),
            "240"
        );
        assert_eq!(
            normalize_config_value("awww_transition_duration", "-1"),
            "1"
        );
        assert_eq!(
            normalize_config_value("linux_wallpaperengine_target_mode", "window"),
            "auto"
        );
        assert_eq!(
            normalize_config_value("linux_wallpaperengine_scaling", "bad"),
            "default"
        );
        assert_eq!(normalize_config_value("storage_backend", "file"), "sqlite");
        assert_eq!(
            normalize_config_value("storage_backend", "hybrid"),
            "sqlite"
        );
    }
}
