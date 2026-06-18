use wc_core::types::Backend;

use crate::LWE_BACKEND_NAME;

pub const AWWW_CROSS_BACKEND_SETTLE_MS: u64 = 180;
pub const MPVPAPER_CROSS_BACKEND_SETTLE_MS: u64 = 150;
pub const LWE_CROSS_BACKEND_SETTLE_MS: u64 = 250;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunningBackend {
    None,
    Awww,
    Mpvpaper,
    LinuxWallpaperEngine,
    Unsupported,
    Unknown,
}

impl RunningBackend {
    pub fn from_last_backend(raw: &str) -> Self {
        match raw.trim() {
            "" => RunningBackend::None,
            "awww" | "swww" => RunningBackend::Awww,
            "mpvpaper" => RunningBackend::Mpvpaper,
            LWE_BACKEND_NAME => RunningBackend::LinuxWallpaperEngine,
            "unsupported" => RunningBackend::Unsupported,
            _ => RunningBackend::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopPlan {
    All,
    AwwwDaemonOnly,
    LweOnly,
    MpvpaperOnly,
    NonLwe,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApplyLifecyclePlan {
    pub previous: RunningBackend,
    pub target: Backend,
    pub pre_stop: StopPlan,
    pub post_success_settle_ms: u64,
    pub post_success_stop: StopPlan,
}

pub fn plan_apply_lifecycle(previous_raw: &str, target: Backend) -> ApplyLifecyclePlan {
    let previous = RunningBackend::from_last_backend(previous_raw);
    ApplyLifecyclePlan {
        previous,
        target,
        pre_stop: pre_stop_plan(previous, target),
        post_success_settle_ms: post_success_settle_ms(previous, target),
        post_success_stop: post_success_stop_plan(previous, target),
    }
}

pub fn pre_stop_plan(previous: RunningBackend, target: Backend) -> StopPlan {
    match target {
        Backend::Awww => StopPlan::None,
        Backend::Mpvpaper => match previous {
            RunningBackend::Awww => StopPlan::AwwwDaemonOnly,
            RunningBackend::Mpvpaper => StopPlan::MpvpaperOnly,
            RunningBackend::LinuxWallpaperEngine => StopPlan::LweOnly,
            RunningBackend::None => StopPlan::All,
            RunningBackend::Unsupported | RunningBackend::Unknown => StopPlan::None,
        },
        Backend::LinuxWallpaperEngine => match previous {
            RunningBackend::Awww => StopPlan::AwwwDaemonOnly,
            RunningBackend::Mpvpaper => StopPlan::MpvpaperOnly,
            RunningBackend::LinuxWallpaperEngine => StopPlan::NonLwe,
            RunningBackend::None | RunningBackend::Unsupported | RunningBackend::Unknown => {
                StopPlan::None
            }
        },
        Backend::Unsupported => StopPlan::All,
    }
}

pub fn post_success_stop_plan(previous: RunningBackend, target: Backend) -> StopPlan {
    match target {
        Backend::Awww => match previous {
            RunningBackend::Awww => StopPlan::MpvpaperOnly,
            RunningBackend::Mpvpaper => StopPlan::MpvpaperOnly,
            RunningBackend::LinuxWallpaperEngine => StopPlan::LweOnly,
            RunningBackend::None | RunningBackend::Unsupported | RunningBackend::Unknown => {
                StopPlan::None
            }
        },
        Backend::Mpvpaper => StopPlan::None,
        Backend::LinuxWallpaperEngine => StopPlan::None,
        Backend::Unsupported => StopPlan::None,
    }
}

pub fn post_success_settle_ms(previous: RunningBackend, target: Backend) -> u64 {
    match (previous, target) {
        (RunningBackend::None, _)
        | (RunningBackend::Unknown, _)
        | (RunningBackend::Unsupported, _) => 0,
        (RunningBackend::Awww, Backend::Awww)
        | (RunningBackend::Mpvpaper, Backend::Mpvpaper)
        | (RunningBackend::LinuxWallpaperEngine, Backend::LinuxWallpaperEngine) => 0,
        (RunningBackend::Mpvpaper | RunningBackend::LinuxWallpaperEngine, Backend::Awww) => {
            AWWW_CROSS_BACKEND_SETTLE_MS
        }
        (RunningBackend::Awww | RunningBackend::LinuxWallpaperEngine, Backend::Mpvpaper) => {
            MPVPAPER_CROSS_BACKEND_SETTLE_MS
        }
        (RunningBackend::Awww | RunningBackend::Mpvpaper, Backend::LinuxWallpaperEngine) => {
            LWE_CROSS_BACKEND_SETTLE_MS
        }
        (_, Backend::Unsupported) => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_backend_parses_legacy_swww_as_awww() {
        assert_eq!(
            RunningBackend::from_last_backend("swww"),
            RunningBackend::Awww
        );
    }

    #[test]
    fn image_after_video_keeps_old_video_until_new_image_succeeds_then_stops_video() {
        let plan = plan_apply_lifecycle("mpvpaper", Backend::Awww);
        assert_eq!(plan.pre_stop, StopPlan::None);
        assert_eq!(plan.post_success_settle_ms, AWWW_CROSS_BACKEND_SETTLE_MS);
        assert_eq!(plan.post_success_stop, StopPlan::MpvpaperOnly);
    }

    #[test]
    fn video_after_image_stops_old_image_before_new_video() {
        let plan = plan_apply_lifecycle("awww", Backend::Mpvpaper);
        assert_eq!(plan.pre_stop, StopPlan::AwwwDaemonOnly);
        assert_eq!(plan.post_success_stop, StopPlan::None);
    }

    #[test]
    fn scene_after_image_stops_old_image_before_scene() {
        let plan = plan_apply_lifecycle("awww", Backend::LinuxWallpaperEngine);
        assert_eq!(plan.pre_stop, StopPlan::AwwwDaemonOnly);
        assert_eq!(plan.post_success_stop, StopPlan::None);
    }

    #[test]
    fn scene_after_video_stops_old_video_before_scene() {
        let plan = plan_apply_lifecycle("mpvpaper", Backend::LinuxWallpaperEngine);
        assert_eq!(plan.pre_stop, StopPlan::MpvpaperOnly);
        assert_eq!(plan.post_success_stop, StopPlan::None);
    }

    #[test]
    fn scene_after_scene_uses_lwe_handoff_and_does_not_stop_all() {
        let plan = plan_apply_lifecycle(LWE_BACKEND_NAME, Backend::LinuxWallpaperEngine);
        assert_eq!(plan.pre_stop, StopPlan::NonLwe);
        assert_eq!(plan.post_success_settle_ms, 0);
        assert_eq!(plan.post_success_stop, StopPlan::None);
    }

    #[test]
    fn image_after_scene_stops_lwe_only_after_new_image_succeeds() {
        let plan = plan_apply_lifecycle(LWE_BACKEND_NAME, Backend::Awww);
        assert_eq!(plan.pre_stop, StopPlan::None);
        assert_eq!(plan.post_success_settle_ms, AWWW_CROSS_BACKEND_SETTLE_MS);
        assert_eq!(plan.post_success_stop, StopPlan::LweOnly);
    }

    #[test]
    fn unknown_previous_never_triggers_post_success_stop_all() {
        assert_eq!(
            plan_apply_lifecycle("unknown-backend", Backend::Awww).post_success_stop,
            StopPlan::None
        );
        assert_eq!(
            plan_apply_lifecycle("unknown-backend", Backend::Mpvpaper).post_success_stop,
            StopPlan::None
        );
        assert_eq!(
            plan_apply_lifecycle("unknown-backend", Backend::LinuxWallpaperEngine)
                .post_success_stop,
            StopPlan::None
        );
    }

    #[test]
    fn unknown_previous_settle_zero_for_awww() {
        let plan = plan_apply_lifecycle("unknown-backend", Backend::Awww);
        assert_eq!(plan.post_success_settle_ms, 0);
    }

    #[test]
    fn unknown_previous_settle_zero_for_mpvpaper() {
        let plan = plan_apply_lifecycle("unknown-backend", Backend::Mpvpaper);
        assert_eq!(plan.post_success_settle_ms, 0);
    }

    #[test]
    fn unsupported_previous_settle_zero_for_awww() {
        let plan = plan_apply_lifecycle("unsupported", Backend::Awww);
        assert_eq!(plan.post_success_settle_ms, 0);
    }

    #[test]
    fn unsupported_previous_settle_zero_for_lwe() {
        let plan = plan_apply_lifecycle("unsupported", Backend::LinuxWallpaperEngine);
        assert_eq!(plan.post_success_settle_ms, 0);
    }
}
