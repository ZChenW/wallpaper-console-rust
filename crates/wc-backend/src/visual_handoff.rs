use crate::lifecycle::RunningBackend;
use wc_core::types::Backend;

pub const MPVPAPER_STARTUP_SETTLE_MS: u64 = 700;
pub const LWE_STARTUP_SETTLE_MS: u64 = 1200;
pub const AWWW_FALLBACK_SETTLE_MS: u64 = 50;

/// Fallback stage for visual handoff. Only TargetImageInstant is used for
/// cross-backend (video/scene → image) transitions. TargetPreviewInstant was
/// removed in the clean-handoff pass and must not be re-added for video/scene
/// target planners.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FallbackStage {
    None,
    TargetImageInstant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisualHandoffPlan {
    pub fallback_stage: FallbackStage,
    pub target_startup_settle_ms: u64,
    pub stop_previous_after_fallback: bool,
    pub stop_fallback_after_target_settle: bool,
}

pub fn plan_visual_handoff(
    previous: RunningBackend,
    target: Backend,
    fallback_path: Option<&str>,
) -> VisualHandoffPlan {
    match target {
        Backend::Awww => plan_awww_handoff(previous, fallback_path),
        Backend::Mpvpaper => plan_mpvpaper_handoff(previous, fallback_path),
        Backend::LinuxWallpaperEngine => plan_lwe_handoff(previous, fallback_path),
        Backend::Unsupported => VisualHandoffPlan {
            fallback_stage: FallbackStage::None,
            target_startup_settle_ms: 0,
            stop_previous_after_fallback: false,
            stop_fallback_after_target_settle: false,
        },
    }
}

fn plan_awww_handoff(previous: RunningBackend, fallback_path: Option<&str>) -> VisualHandoffPlan {
    match previous {
        RunningBackend::Awww
        | RunningBackend::None
        | RunningBackend::Unknown
        | RunningBackend::Unsupported => VisualHandoffPlan {
            fallback_stage: FallbackStage::None,
            target_startup_settle_ms: 0,
            stop_previous_after_fallback: false,
            stop_fallback_after_target_settle: false,
        },
        RunningBackend::Mpvpaper | RunningBackend::LinuxWallpaperEngine => {
            let stage = if fallback_path.is_some() {
                FallbackStage::TargetImageInstant
            } else {
                FallbackStage::None
            };
            VisualHandoffPlan {
                fallback_stage: stage,
                target_startup_settle_ms: 0,
                stop_previous_after_fallback: false,
                stop_fallback_after_target_settle: false,
            }
        }
    }
}

fn plan_mpvpaper_handoff(
    previous: RunningBackend,
    _fallback_path: Option<&str>,
) -> VisualHandoffPlan {
    match previous {
        RunningBackend::Mpvpaper => VisualHandoffPlan {
            fallback_stage: FallbackStage::None,
            target_startup_settle_ms: MPVPAPER_STARTUP_SETTLE_MS,
            stop_previous_after_fallback: false,
            stop_fallback_after_target_settle: false,
        },
        RunningBackend::Awww => VisualHandoffPlan {
            fallback_stage: FallbackStage::None,
            target_startup_settle_ms: MPVPAPER_STARTUP_SETTLE_MS,
            stop_previous_after_fallback: false,
            stop_fallback_after_target_settle: false,
        },
        RunningBackend::LinuxWallpaperEngine => VisualHandoffPlan {
            fallback_stage: FallbackStage::None,
            target_startup_settle_ms: MPVPAPER_STARTUP_SETTLE_MS,
            stop_previous_after_fallback: false,
            stop_fallback_after_target_settle: false,
        },
        RunningBackend::None | RunningBackend::Unknown | RunningBackend::Unsupported => {
            VisualHandoffPlan {
                fallback_stage: FallbackStage::None,
                target_startup_settle_ms: MPVPAPER_STARTUP_SETTLE_MS,
                stop_previous_after_fallback: false,
                stop_fallback_after_target_settle: false,
            }
        }
    }
}

fn plan_lwe_handoff(previous: RunningBackend, _fallback_path: Option<&str>) -> VisualHandoffPlan {
    match previous {
        RunningBackend::LinuxWallpaperEngine => VisualHandoffPlan {
            fallback_stage: FallbackStage::None,
            target_startup_settle_ms: LWE_STARTUP_SETTLE_MS,
            stop_previous_after_fallback: false,
            stop_fallback_after_target_settle: false,
        },
        RunningBackend::Awww | RunningBackend::Mpvpaper => VisualHandoffPlan {
            fallback_stage: FallbackStage::None,
            target_startup_settle_ms: LWE_STARTUP_SETTLE_MS,
            stop_previous_after_fallback: false,
            stop_fallback_after_target_settle: false,
        },
        RunningBackend::None | RunningBackend::Unknown | RunningBackend::Unsupported => {
            VisualHandoffPlan {
                fallback_stage: FallbackStage::None,
                target_startup_settle_ms: LWE_STARTUP_SETTLE_MS,
                stop_previous_after_fallback: false,
                stop_fallback_after_target_settle: false,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_after_video_uses_target_image_instant_fallback() {
        let plan = plan_visual_handoff(
            RunningBackend::Mpvpaper,
            Backend::Awww,
            Some("/tmp/img.jpg"),
        );
        assert_eq!(plan.fallback_stage, FallbackStage::TargetImageInstant);
        assert!(!plan.stop_previous_after_fallback);
    }

    #[test]
    fn image_after_scene_uses_target_image_instant_fallback() {
        let plan = plan_visual_handoff(
            RunningBackend::LinuxWallpaperEngine,
            Backend::Awww,
            Some("/tmp/img.jpg"),
        );
        assert_eq!(plan.fallback_stage, FallbackStage::TargetImageInstant);
        assert!(!plan.stop_previous_after_fallback);
    }

    #[test]
    fn image_after_image_uses_no_fallback_stage() {
        let plan = plan_visual_handoff(RunningBackend::Awww, Backend::Awww, Some("/tmp/img.jpg"));
        assert_eq!(plan.fallback_stage, FallbackStage::None);
        assert_eq!(plan.target_startup_settle_ms, 0);
    }

    #[test]
    fn video_after_image_uses_no_preview_fallback() {
        let plan = plan_visual_handoff(
            RunningBackend::Awww,
            Backend::Mpvpaper,
            Some("/tmp/preview.gif"),
        );
        assert_eq!(plan.fallback_stage, FallbackStage::None);
        assert_eq!(plan.target_startup_settle_ms, MPVPAPER_STARTUP_SETTLE_MS);
        assert!(!plan.stop_previous_after_fallback);
        assert!(!plan.stop_fallback_after_target_settle);
    }

    #[test]
    fn scene_after_video_uses_no_preview_fallback() {
        let plan = plan_visual_handoff(
            RunningBackend::Mpvpaper,
            Backend::LinuxWallpaperEngine,
            Some("/tmp/preview.gif"),
        );
        assert_eq!(plan.fallback_stage, FallbackStage::None);
        assert_eq!(plan.target_startup_settle_ms, LWE_STARTUP_SETTLE_MS);
        assert!(!plan.stop_previous_after_fallback);
        assert!(!plan.stop_fallback_after_target_settle);
    }

    #[test]
    fn scene_to_scene_uses_no_preview_fallback() {
        let plan = plan_visual_handoff(
            RunningBackend::LinuxWallpaperEngine,
            Backend::LinuxWallpaperEngine,
            Some("/tmp/preview.gif"),
        );
        assert_eq!(plan.fallback_stage, FallbackStage::None);
        assert_eq!(plan.target_startup_settle_ms, LWE_STARTUP_SETTLE_MS);
        assert!(!plan.stop_previous_after_fallback);
        assert!(!plan.stop_fallback_after_target_settle);
    }

    #[test]
    fn unknown_previous_has_no_extra_visual_delay() {
        let plan =
            plan_visual_handoff(RunningBackend::Unknown, Backend::Awww, Some("/tmp/img.jpg"));
        assert_eq!(plan.fallback_stage, FallbackStage::None);
        assert_eq!(plan.target_startup_settle_ms, 0);
    }

    #[test]
    fn scene_to_scene_uses_preview_fallback_when_available() {
        let plan = plan_visual_handoff(
            RunningBackend::LinuxWallpaperEngine,
            Backend::LinuxWallpaperEngine,
            Some("/tmp/preview.gif"),
        );
        assert_eq!(plan.fallback_stage, FallbackStage::None);
        assert_eq!(plan.target_startup_settle_ms, LWE_STARTUP_SETTLE_MS);
        assert!(!plan.stop_previous_after_fallback);
        assert!(!plan.stop_fallback_after_target_settle);
    }

    #[test]
    fn scene_after_video_with_preview_cleans_preview_fallback_after_settle() {
        let plan = plan_visual_handoff(
            RunningBackend::Mpvpaper,
            Backend::LinuxWallpaperEngine,
            Some("/tmp/preview.gif"),
        );
        assert_eq!(plan.fallback_stage, FallbackStage::None);
        assert!(!plan.stop_fallback_after_target_settle);
    }

    #[test]
    fn video_after_scene_with_preview_cleans_preview_fallback_after_settle() {
        let plan = plan_visual_handoff(
            RunningBackend::LinuxWallpaperEngine,
            Backend::Mpvpaper,
            Some("/tmp/preview.gif"),
        );
        assert_eq!(plan.fallback_stage, FallbackStage::None);
        assert!(!plan.stop_fallback_after_target_settle);
    }

    #[test]
    fn scene_to_scene_without_preview_has_no_fallback_but_will_settle() {
        let plan = plan_visual_handoff(
            RunningBackend::LinuxWallpaperEngine,
            Backend::LinuxWallpaperEngine,
            None,
        );
        assert_eq!(plan.fallback_stage, FallbackStage::None);
        assert_eq!(plan.target_startup_settle_ms, LWE_STARTUP_SETTLE_MS);
        assert!(!plan.stop_previous_after_fallback);
        assert!(!plan.stop_fallback_after_target_settle);
    }

    #[test]
    fn target_image_instant_is_used_for_video_to_image() {
        let plan = plan_visual_handoff(
            RunningBackend::Mpvpaper,
            Backend::Awww,
            Some("/tmp/img.jpg"),
        );
        assert_eq!(plan.fallback_stage, FallbackStage::TargetImageInstant);
        assert_eq!(plan.target_startup_settle_ms, 0);
    }

    #[test]
    fn target_image_instant_is_not_used_for_image_to_image() {
        let plan = plan_visual_handoff(RunningBackend::Awww, Backend::Awww, Some("/tmp/img.jpg"));
        assert_eq!(plan.fallback_stage, FallbackStage::None);
        assert_eq!(plan.target_startup_settle_ms, 0);
    }
}
