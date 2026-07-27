//! Typed persisted behavior settings shared by CLI and GUI adapters.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

pub const DEFAULT_MPVPAPER_OPTIONS: &str = "--loop-file=inf --panscan=1.0";

pub const BEHAVIOR_SETTING_KEYS: &[&str] = &[
    "image_backend",
    "gif_backend",
    "video_backend",
    "mpvpaper_options",
    "awww_resize",
    "awww_transition_type",
    "awww_transition_duration",
    "wallpaper_transition_fps",
    "linux_wallpaperengine_scaling",
    "linux_wallpaperengine_fps",
    "linux_wallpaperengine_muted",
    "linux_wallpaperengine_volume",
    "restore_on_login",
    "open_project_location_mode",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageRenderer {
    Awww,
    Mpvpaper,
    Swaybg,
    Feh,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GifRenderer {
    Awww,
    Mpvpaper,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VideoRenderer {
    Mpvpaper,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WallpaperFillMode {
    Crop,
    Fit,
    Stretch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AwwwTransitionType {
    Simple,
    Fade,
    Left,
    Right,
    Top,
    Bottom,
    Wipe,
    Grow,
    Center,
    Outer,
    Random,
    Wave,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LweScalingMode {
    Default,
    Fill,
    Fit,
    Stretch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenProjectLocationMode {
    FileManager,
    Terminal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BehaviorSettings {
    pub image_backend: ImageRenderer,
    pub gif_backend: GifRenderer,
    pub video_backend: VideoRenderer,
    pub mpvpaper_options: String,
    pub fill_mode: WallpaperFillMode,
    pub awww_transition_type: AwwwTransitionType,
    pub awww_transition_duration: f64,
    pub awww_transition_fps: u16,
    pub lwe_scaling: LweScalingMode,
    pub lwe_fps: u16,
    pub lwe_muted: bool,
    pub lwe_volume: u8,
    pub restore_on_login: bool,
    pub open_project_location_mode: OpenProjectLocationMode,
}

impl Default for BehaviorSettings {
    fn default() -> Self {
        Self {
            image_backend: ImageRenderer::Awww,
            gif_backend: GifRenderer::Awww,
            video_backend: VideoRenderer::Mpvpaper,
            mpvpaper_options: DEFAULT_MPVPAPER_OPTIONS.into(),
            fill_mode: WallpaperFillMode::Crop,
            awww_transition_type: AwwwTransitionType::Fade,
            awww_transition_duration: 1.0,
            awww_transition_fps: 60,
            lwe_scaling: LweScalingMode::Default,
            lwe_fps: 60,
            lwe_muted: false,
            lwe_volume: 100,
            restore_on_login: false,
            open_project_location_mode: OpenProjectLocationMode::FileManager,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BehaviorSettingsPatch {
    pub image_backend: Option<ImageRenderer>,
    pub gif_backend: Option<GifRenderer>,
    pub video_backend: Option<VideoRenderer>,
    pub mpvpaper_options: Option<String>,
    pub fill_mode: Option<WallpaperFillMode>,
    pub awww_transition_type: Option<AwwwTransitionType>,
    pub awww_transition_duration: Option<f64>,
    pub awww_transition_fps: Option<u16>,
    pub lwe_scaling: Option<LweScalingMode>,
    pub lwe_fps: Option<u16>,
    pub lwe_muted: Option<bool>,
    pub lwe_volume: Option<u8>,
    pub restore_on_login: Option<bool>,
    pub open_project_location_mode: Option<OpenProjectLocationMode>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BehaviorSettingsSnapshot {
    pub settings: BehaviorSettings,
    pub revision: String,
}

impl BehaviorSettings {
    pub fn from_config(values: &HashMap<String, String>) -> Self {
        let value = |key: &str| values.get(key).map(String::as_str).unwrap_or_default();
        Self {
            image_backend: match value("image_backend") {
                "mpvpaper" => ImageRenderer::Mpvpaper,
                "swaybg" => ImageRenderer::Swaybg,
                "feh" => ImageRenderer::Feh,
                _ => ImageRenderer::Awww,
            },
            gif_backend: if value("gif_backend") == "mpvpaper" {
                GifRenderer::Mpvpaper
            } else {
                GifRenderer::Awww
            },
            video_backend: VideoRenderer::Mpvpaper,
            mpvpaper_options: values
                .get("mpvpaper_options")
                .cloned()
                .unwrap_or_else(|| DEFAULT_MPVPAPER_OPTIONS.into()),
            fill_mode: match value("awww_resize") {
                "fit" => WallpaperFillMode::Fit,
                "stretch" => WallpaperFillMode::Stretch,
                _ => WallpaperFillMode::Crop,
            },
            awww_transition_type: match value("awww_transition_type") {
                "simple" | "none" => AwwwTransitionType::Simple,
                "left" | "slide" => AwwwTransitionType::Left,
                "right" => AwwwTransitionType::Right,
                "top" => AwwwTransitionType::Top,
                "bottom" => AwwwTransitionType::Bottom,
                "wipe" => AwwwTransitionType::Wipe,
                "grow" => AwwwTransitionType::Grow,
                "center" => AwwwTransitionType::Center,
                "outer" => AwwwTransitionType::Outer,
                "random" => AwwwTransitionType::Random,
                "wave" => AwwwTransitionType::Wave,
                _ => AwwwTransitionType::Fade,
            },
            awww_transition_duration: bounded_f64(
                value("awww_transition_duration"),
                0.0,
                60.0,
                1.0,
            ),
            awww_transition_fps: bounded_u16(value("wallpaper_transition_fps"), 1, 240, 60),
            lwe_scaling: match value("linux_wallpaperengine_scaling") {
                "fill" => LweScalingMode::Fill,
                "fit" => LweScalingMode::Fit,
                "stretch" => LweScalingMode::Stretch,
                _ => LweScalingMode::Default,
            },
            lwe_fps: bounded_u16(value("linux_wallpaperengine_fps"), 1, 240, 60),
            lwe_muted: value("linux_wallpaperengine_muted") == "on",
            lwe_volume: bounded_u16(value("linux_wallpaperengine_volume"), 0, 100, 100) as u8,
            restore_on_login: value("restore_on_login") == "on",
            open_project_location_mode: if value("open_project_location_mode") == "terminal" {
                OpenProjectLocationMode::Terminal
            } else {
                OpenProjectLocationMode::FileManager
            },
        }
    }

    pub fn apply_patch(&self, patch: &BehaviorSettingsPatch) -> Self {
        let mut next = self.clone();
        macro_rules! replace {
            ($field:ident) => {
                if let Some(value) = patch.$field {
                    next.$field = value;
                }
            };
        }
        replace!(image_backend);
        replace!(gif_backend);
        replace!(video_backend);
        if let Some(value) = &patch.mpvpaper_options {
            next.mpvpaper_options = value.clone();
        }
        replace!(fill_mode);
        replace!(awww_transition_type);
        if let Some(value) = patch.awww_transition_duration {
            next.awww_transition_duration = if value.is_finite() {
                value.clamp(0.0, 60.0)
            } else {
                1.0
            };
        }
        if let Some(value) = patch.awww_transition_fps {
            next.awww_transition_fps = value.clamp(1, 240);
        }
        replace!(lwe_scaling);
        if let Some(value) = patch.lwe_fps {
            next.lwe_fps = value.clamp(1, 240);
        }
        replace!(lwe_muted);
        if let Some(value) = patch.lwe_volume {
            next.lwe_volume = value.min(100);
        }
        replace!(restore_on_login);
        replace!(open_project_location_mode);
        next
    }

    pub fn config_entries(&self) -> Vec<(&'static str, String)> {
        vec![
            ("image_backend", enum_value(self.image_backend)),
            ("gif_backend", enum_value(self.gif_backend)),
            ("video_backend", "mpvpaper".into()),
            ("mpvpaper_options", self.mpvpaper_options.clone()),
            ("awww_resize", enum_value(self.fill_mode)),
            (
                "awww_transition_type",
                enum_value(self.awww_transition_type),
            ),
            (
                "awww_transition_duration",
                compact_number(self.awww_transition_duration),
            ),
            (
                "wallpaper_transition_fps",
                self.awww_transition_fps.to_string(),
            ),
            (
                "linux_wallpaperengine_scaling",
                enum_value(self.lwe_scaling),
            ),
            ("linux_wallpaperengine_fps", self.lwe_fps.to_string()),
            ("linux_wallpaperengine_muted", on_off(self.lwe_muted).into()),
            ("linux_wallpaperengine_volume", self.lwe_volume.to_string()),
            ("restore_on_login", on_off(self.restore_on_login).into()),
            (
                "open_project_location_mode",
                enum_value(self.open_project_location_mode),
            ),
        ]
    }

    pub fn snapshot(self) -> BehaviorSettingsSnapshot {
        let revision = behavior_settings_revision(&self);
        BehaviorSettingsSnapshot {
            settings: self,
            revision,
        }
    }
}

pub fn behavior_settings_revision(settings: &BehaviorSettings) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for (key, value) in settings.config_entries() {
        for byte in key
            .bytes()
            .chain(std::iter::once(0))
            .chain(value.bytes())
            .chain(std::iter::once(0xff))
        {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    format!("{hash:016x}")
}

fn enum_value<T: Serialize>(value: T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .expect("behavior setting enums serialize as strings")
}

fn on_off(value: bool) -> &'static str {
    if value {
        "on"
    } else {
        "off"
    }
}

fn bounded_f64(raw: &str, minimum: f64, maximum: f64, fallback: f64) -> f64 {
    raw.trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && (*value >= minimum) && (*value <= maximum))
        .unwrap_or(fallback)
}

fn bounded_u16(raw: &str, minimum: u16, maximum: u16, fallback: u16) -> u16 {
    raw.trim()
        .parse::<i64>()
        .ok()
        .map(|value| value.clamp(i64::from(minimum), i64::from(maximum)) as u16)
        .unwrap_or(fallback)
}

fn compact_number(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_config_normalizes_to_one_typed_snapshot() {
        let values = HashMap::from([
            ("image_backend".into(), "bad".into()),
            ("gif_backend".into(), "mpvpaper".into()),
            ("video_backend".into(), "awww".into()),
            ("awww_resize".into(), "fit".into()),
            ("awww_transition_type".into(), "slide".into()),
            ("awww_transition_duration".into(), "nan".into()),
            ("wallpaper_transition_fps".into(), "999".into()),
            ("linux_wallpaperengine_scaling".into(), "bad".into()),
            ("linux_wallpaperengine_fps".into(), "0".into()),
            ("linux_wallpaperengine_muted".into(), "on".into()),
            ("linux_wallpaperengine_volume".into(), "-4".into()),
            ("restore_on_login".into(), "on".into()),
            ("open_project_location_mode".into(), "bad".into()),
        ]);

        let settings = BehaviorSettings::from_config(&values);

        assert_eq!(settings.image_backend, ImageRenderer::Awww);
        assert_eq!(settings.gif_backend, GifRenderer::Mpvpaper);
        assert_eq!(settings.video_backend, VideoRenderer::Mpvpaper);
        assert_eq!(settings.fill_mode, WallpaperFillMode::Fit);
        assert_eq!(settings.awww_transition_type, AwwwTransitionType::Left);
        assert_eq!(settings.awww_transition_duration, 1.0);
        assert_eq!(settings.awww_transition_fps, 240);
        assert_eq!(settings.lwe_scaling, LweScalingMode::Default);
        assert_eq!(settings.lwe_fps, 1);
        assert!(settings.lwe_muted);
        assert_eq!(settings.lwe_volume, 0);
        assert!(settings.restore_on_login);
        assert_eq!(
            settings.open_project_location_mode,
            OpenProjectLocationMode::FileManager
        );
    }

    #[test]
    fn patch_clamps_values_and_changes_revision_deterministically() {
        let current = BehaviorSettings::default();
        let revision = behavior_settings_revision(&current);
        let next = current.apply_patch(&BehaviorSettingsPatch {
            awww_transition_fps: Some(500),
            lwe_volume: Some(200),
            open_project_location_mode: Some(OpenProjectLocationMode::Terminal),
            ..BehaviorSettingsPatch::default()
        });

        assert_eq!(next.awww_transition_fps, 240);
        assert_eq!(next.lwe_volume, 100);
        assert_eq!(
            next.open_project_location_mode,
            OpenProjectLocationMode::Terminal
        );
        assert_ne!(behavior_settings_revision(&next), revision);
        assert_eq!(
            behavior_settings_revision(&next),
            behavior_settings_revision(&next)
        );
    }

    #[test]
    fn config_entries_are_complete_and_canonical() {
        let entries = BehaviorSettings::default().config_entries();
        assert_eq!(entries.len(), BEHAVIOR_SETTING_KEYS.len());
        assert_eq!(
            entries.iter().map(|(key, _)| *key).collect::<Vec<_>>(),
            BEHAVIOR_SETTING_KEYS
        );
        assert!(entries.contains(&("awww_transition_duration", "1".into())));
        assert!(entries.contains(&("restore_on_login", "off".into())));
    }

    #[test]
    fn swaybg_image_renderer_round_trips_through_config() {
        let settings = BehaviorSettings::from_config(&HashMap::from([(
            "image_backend".into(),
            "swaybg".into(),
        )]));

        assert_eq!(settings.image_backend, ImageRenderer::Swaybg);
        assert!(settings
            .config_entries()
            .contains(&("image_backend", "swaybg".into())));
    }

    #[test]
    fn feh_image_renderer_round_trips_through_config() {
        let settings =
            BehaviorSettings::from_config(&HashMap::from([("image_backend".into(), "feh".into())]));

        assert_eq!(settings.image_backend, ImageRenderer::Feh);
        assert!(settings
            .config_entries()
            .contains(&("image_backend", "feh".into())));
    }
}
