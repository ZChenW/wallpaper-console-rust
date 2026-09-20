//! Durable renderer intent. Runtime process/socket identity never belongs here.
use crate::{behavior_setting::*, error::WcError, types::Backend};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "backend", rename_all = "kebab-case", deny_unknown_fields)]
pub enum RenderOptions {
    Awww {
        resize: WallpaperFillMode,
        transition: AwwwTransitionType,
        duration: f64,
        fps: u16,
    },
    Mpvpaper {
        options: String,
    },
    Swaybg {
        resize: WallpaperFillMode,
    },
    Feh {
        resize: WallpaperFillMode,
    },
    LinuxWallpaperengine {
        scaling: LweScalingMode,
        fps: u16,
        muted: bool,
        volume: u8,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Presentation {
    Original,
    Preview,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderRecipe {
    pub schema_version: u32,
    pub source: String,
    pub media_path: String,
    pub presentation: Presentation,
    pub options: RenderOptions,
}
impl RenderOptions {
    pub fn backend(&self) -> Backend {
        match self {
            Self::Awww { .. } => Backend::Awww,
            Self::Mpvpaper { .. } => Backend::Mpvpaper,
            Self::Swaybg { .. } => Backend::Swaybg,
            Self::Feh { .. } => Backend::Feh,
            Self::LinuxWallpaperengine { .. } => Backend::LinuxWallpaperEngine,
        }
    }
    pub fn capture(backend: Backend, values: &HashMap<String, String>) -> Result<Self, WcError> {
        let s = BehaviorSettings::from_config(values);
        Ok(match backend {
            Backend::Awww => Self::Awww {
                resize: s.fill_mode,
                transition: s.awww_transition_type,
                duration: s.awww_transition_duration,
                fps: s.awww_transition_fps,
            },
            Backend::Mpvpaper => Self::Mpvpaper {
                options: normalize_mpvpaper_options(&s.mpvpaper_options).into(),
            },
            Backend::Swaybg => Self::Swaybg {
                resize: s.fill_mode,
            },
            Backend::Feh => Self::Feh {
                resize: s.fill_mode,
            },
            Backend::LinuxWallpaperEngine => Self::LinuxWallpaperengine {
                scaling: s.lwe_scaling,
                fps: s.lwe_fps,
                muted: s.lwe_muted,
                volume: s.lwe_volume,
            },
            _ => return Err(WcError::Other("unsupported recipe backend".into())),
        })
    }
    /// Only rendering keys are overlaid; paths, outputs and process control stay live.
    pub fn config_entries(&self) -> HashMap<String, String> {
        let mut s = BehaviorSettings::default();
        let keys: &[&str] = match self {
            Self::Awww {
                resize,
                transition,
                duration,
                fps,
            } => {
                s.fill_mode = *resize;
                s.awww_transition_type = *transition;
                s.awww_transition_duration = *duration;
                s.awww_transition_fps = *fps;
                &[
                    "awww_resize",
                    "awww_transition_type",
                    "awww_transition_duration",
                    "wallpaper_transition_fps",
                ]
            }
            Self::Mpvpaper { options } => {
                s.mpvpaper_options = options.clone();
                &["mpvpaper_options"]
            }
            Self::Swaybg { resize } | Self::Feh { resize } => {
                s.fill_mode = *resize;
                &["awww_resize"]
            }
            Self::LinuxWallpaperengine {
                scaling,
                fps,
                muted,
                volume,
            } => {
                s.lwe_scaling = *scaling;
                s.lwe_fps = *fps;
                s.lwe_muted = *muted;
                s.lwe_volume = *volume;
                &[
                    "linux_wallpaperengine_scaling",
                    "linux_wallpaperengine_fps",
                    "linux_wallpaperengine_muted",
                    "linux_wallpaperengine_volume",
                ]
            }
        };
        s.config_entries()
            .into_iter()
            .filter(|(key, _)| keys.contains(key))
            .map(|(key, value)| (key.to_owned(), value))
            .collect()
    }
}
impl RenderRecipe {
    pub fn validate(&self) -> Result<(), WcError> {
        if self.schema_version != 1
            || self.source.trim().is_empty()
            || self.media_path.trim().is_empty()
        {
            return Err(WcError::Other(
                "unsupported or invalid render recipe; repair the saved assignment".into(),
            ));
        }
        if RenderOptions::capture(self.options.backend(), &self.options.config_entries())?
            != self.options
        {
            return Err(WcError::Other("invalid render recipe parameters".into()));
        }
        if let RenderOptions::Mpvpaper { options } = &self.options {
            // mpv receives this string through -o. WC owns IPC and process lifetime.
            for token in options.split_whitespace() {
                let key = token
                    .trim_start_matches('-')
                    .split('=')
                    .next()
                    .unwrap_or("");
                let key = key.strip_prefix("no-").unwrap_or(key);
                if matches!(
                    key,
                    "input-ipc-server"
                        | "input-ipc-client"
                        | "wid"
                        | "idle"
                        | "keep-open"
                        | "terminal"
                        | "input-terminal"
                        | "config"
                        | "include"
                        | "scripts"
                        | "script"
                ) {
                    return Err(WcError::Other(format!(
                        "mpv option {key} conflicts with WC process control"
                    )));
                }
            }
        }
        Ok(())
    }
}

/// Historic defaults are normalized before persistence, not again on Restore.
pub fn normalize_mpvpaper_options(raw: &str) -> &str {
    match raw.trim() {
        "no-audio --loop-file=inf" | "--loop-file=inf" => "--loop-file=inf --panscan=1.0",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mpv_control_flags_and_boolean_aliases_are_rejected() {
        for value in [
            "--idle=no",
            "--no-idle",
            "--no-keep-open",
            "--no-terminal",
            "--input-ipc-server=/tmp/other",
        ] {
            let recipe = RenderRecipe {
                schema_version: 1,
                source: "/a.mp4".into(),
                media_path: "/a.mp4".into(),
                presentation: Presentation::Original,
                options: RenderOptions::Mpvpaper {
                    options: value.into(),
                },
            };
            assert!(recipe.validate().is_err(), "{value}");
        }
        let options = RenderOptions::capture(
            Backend::Mpvpaper,
            &HashMap::from([("mpvpaper_options".into(), "--no-audio --volume=30".into())]),
        )
        .unwrap();
        RenderRecipe {
            schema_version: 1,
            source: "/a.mp4".into(),
            media_path: "/a.mp4".into(),
            presentation: Presentation::Original,
            options,
        }
        .validate()
        .unwrap();
    }
}
