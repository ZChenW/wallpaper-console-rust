use crate::lifecycle::RunningBackend;
use wc_core::types::Backend;

pub const MPVPAPER_STARTUP_SETTLE_MS: u64 = 700;
pub const LWE_STARTUP_SETTLE_MS: u64 = 1200;
pub const AWWW_FALLBACK_SETTLE_MS: u64 = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FallbackStage {
    None,
    TargetImageInstant,
    TargetPreviewInstant,
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
    fallback_path: Option<&str>,
) -> VisualHandoffPlan {
    match previous {
        RunningBackend::Mpvpaper => {
            // video -> video: known limitation, may still black briefly
            // If fallback_path is available, use preview instant fallback
            let stage = if fallback_path.is_some() {
                FallbackStage::TargetPreviewInstant
            } else {
                FallbackStage::None
            };
            VisualHandoffPlan {
                fallback_stage: stage,
                target_startup_settle_ms: MPVPAPER_STARTUP_SETTLE_MS,
                stop_previous_after_fallback: fallback_path.is_some(),
                stop_fallback_after_target_settle: fallback_path.is_some(),
            }
        }
        RunningBackend::Awww => VisualHandoffPlan {
            fallback_stage: FallbackStage::None,
            target_startup_settle_ms: MPVPAPER_STARTUP_SETTLE_MS,
            stop_previous_after_fallback: false,
            stop_fallback_after_target_settle: false,
        },
        RunningBackend::LinuxWallpaperEngine => {
            let stage = if fallback_path.is_some() {
                FallbackStage::TargetPreviewInstant
            } else {
                FallbackStage::None
            };
            VisualHandoffPlan {
                fallback_stage: stage,
                target_startup_settle_ms: MPVPAPER_STARTUP_SETTLE_MS,
                stop_previous_after_fallback: false,
                stop_fallback_after_target_settle: fallback_path.is_some(),
            }
        }
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

fn plan_lwe_handoff(previous: RunningBackend, fallback_path: Option<&str>) -> VisualHandoffPlan {
    match previous {
        RunningBackend::LinuxWallpaperEngine => {
            let stage = if fallback_path.is_some() {
                FallbackStage::TargetPreviewInstant
            } else {
                FallbackStage::None
            };
            VisualHandoffPlan {
                fallback_stage: stage,
                target_startup_settle_ms: LWE_STARTUP_SETTLE_MS,
                stop_previous_after_fallback: false,
                stop_fallback_after_target_settle: fallback_path.is_some(),
            }
        }
        RunningBackend::Awww | RunningBackend::Mpvpaper => {
            let stage = if fallback_path.is_some() {
                FallbackStage::TargetPreviewInstant
            } else {
                FallbackStage::None
            };
            VisualHandoffPlan {
                fallback_stage: stage,
                target_startup_settle_ms: LWE_STARTUP_SETTLE_MS,
                stop_previous_after_fallback: false,
                stop_fallback_after_target_settle: fallback_path.is_some(),
            }
        }
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
    fn video_after_image_waits_before_stopping_awww() {
        let plan = plan_visual_handoff(RunningBackend::Awww, Backend::Mpvpaper, None);
        assert_eq!(plan.fallback_stage, FallbackStage::None);
        assert_eq!(plan.target_startup_settle_ms, MPVPAPER_STARTUP_SETTLE_MS);
        assert!(!plan.stop_previous_after_fallback);
    }

    #[test]
    fn scene_after_image_waits_before_stopping_awww() {
        let plan = plan_visual_handoff(RunningBackend::Awww, Backend::LinuxWallpaperEngine, None);
        assert_eq!(plan.fallback_stage, FallbackStage::None);
        assert_eq!(plan.target_startup_settle_ms, LWE_STARTUP_SETTLE_MS);
        assert!(!plan.stop_previous_after_fallback);
    }

    #[test]
    fn scene_with_preview_uses_preview_fallback() {
        let plan = plan_visual_handoff(
            RunningBackend::Awww,
            Backend::LinuxWallpaperEngine,
            Some("/tmp/preview.gif"),
        );
        assert_eq!(plan.fallback_stage, FallbackStage::TargetPreviewInstant);
        assert_eq!(plan.target_startup_settle_ms, LWE_STARTUP_SETTLE_MS);
        assert!(plan.stop_fallback_after_target_settle);
    }

    #[test]
    fn video_without_fallback_records_no_fallback_stage() {
        let plan = plan_visual_handoff(RunningBackend::Mpvpaper, Backend::Mpvpaper, None);
        assert_eq!(plan.fallback_stage, FallbackStage::None);
        assert_eq!(plan.target_startup_settle_ms, MPVPAPER_STARTUP_SETTLE_MS);
        // Known limitation: video -> video without fallback may still black briefly.
        // stop_previous_after_fallback is false because there is no fallback.
        assert!(!plan.stop_previous_after_fallback);
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
        assert_eq!(plan.fallback_stage, FallbackStage::TargetPreviewInstant);
        assert_eq!(plan.target_startup_settle_ms, LWE_STARTUP_SETTLE_MS);
        assert!(!plan.stop_previous_after_fallback);
        assert!(plan.stop_fallback_after_target_settle);
    }

    #[test]
    fn scene_after_video_with_preview_cleans_preview_fallback_after_settle() {
        let plan = plan_visual_handoff(
            RunningBackend::Mpvpaper,
            Backend::LinuxWallpaperEngine,
            Some("/tmp/preview.gif"),
        );
        assert_eq!(plan.fallback_stage, FallbackStage::TargetPreviewInstant);
        assert!(plan.stop_fallback_after_target_settle);
    }

    #[test]
    fn video_after_scene_with_preview_cleans_preview_fallback_after_settle() {
        let plan = plan_visual_handoff(
            RunningBackend::LinuxWallpaperEngine,
            Backend::Mpvpaper,
            Some("/tmp/preview.gif"),
        );
        assert_eq!(plan.fallback_stage, FallbackStage::TargetPreviewInstant);
        assert!(plan.stop_fallback_after_target_settle);
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
