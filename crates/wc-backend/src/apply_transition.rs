//! ApplyTransition — previous→target visual transition for Apply and Restore.
//!
//! Owns fallback/settle adornments around a display_plan Stop/Apply skeleton.
//! Instant awww fallback runs only for [`ExecutionScope::AllDisplays`]; named
//! scopes keep settle/stop suffix only (no global flash).

use wc_core::error::WcError;
use wc_core::types::Backend;
use wc_storage::StorageApi;

use crate::apply_stage::ApplyStageReporter;
use crate::display_executor::{
    execute_display_actions, CompletedStop, DisplayExecAction, DisplayExecContext,
    DisplayExecReport,
};
use crate::driver;
use crate::lifecycle::{self, StopPlan};
use crate::runtime::BackendRuntime;
use crate::target_commands::ExecutionScope;
use crate::visual_handoff::{self, FallbackStage};

pub use lifecycle::AWWW_CROSS_BACKEND_SETTLE_MS;
pub use visual_handoff::{AWWW_FALLBACK_SETTLE_MS, LWE_STARTUP_SETTLE_MS, MPVPAPER_STARTUP_SETTLE_MS};

/// Pure planning input. `core_actions` is the Stop/Apply skeleton from display_plan.
pub struct ApplyTransitionRequest<'a> {
    pub scope: ExecutionScope,
    pub target: Backend,
    pub previous_backend_raw: &'a str,
    pub fallback_path: Option<&'a str>,
    pub core_actions: &'a [DisplayExecAction],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyTransitionAdornment {
    /// awww instant (`--transition-type none`). Only planned for AllDisplays.
    FallbackInstantAwww { path: String },
    SettleMs(u64),
    LifecycleStop(StopPlan),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyTransitionPlan {
    pub prefix: Vec<ApplyTransitionAdornment>,
    pub core_actions: Vec<DisplayExecAction>,
    pub suffix: Vec<ApplyTransitionAdornment>,
    /// True when Named scope forced TargetImageInstant off.
    pub scope_degraded: bool,
    previous: lifecycle::RunningBackend,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyTransitionReport {
    pub exec: DisplayExecReport,
    pub fallback_applied: bool,
}

#[derive(Debug)]
pub struct ApplyTransitionFailure {
    pub exec: DisplayExecReport,
    pub error: WcError,
    pub uncertain_stop: Option<Box<CompletedStop>>,
    pub rollback_note: Option<String>,
}

impl ApplyTransitionFailure {
    pub fn after_destructive_stop(&self) -> bool {
        self.exec.had_destructive_stop()
            || self
                .uncertain_stop
                .as_ref()
                .is_some_and(|stop| stop.destructive)
    }
}

pub fn plan_apply_transition(
    request: &ApplyTransitionRequest<'_>,
) -> Result<ApplyTransitionPlan, WcError> {
    request.scope.validate()?;
    let lifecycle = lifecycle::plan_apply_lifecycle(request.previous_backend_raw, request.target);
    let full_handoff = visual_handoff::plan_visual_handoff(
        lifecycle.previous,
        request.target,
        request.fallback_path,
    );

    let allow_global_fallback = matches!(request.scope, ExecutionScope::AllDisplays);
    let scope_degraded = !allow_global_fallback
        && full_handoff.fallback_stage == FallbackStage::TargetImageInstant;

    let mut prefix = Vec::new();
    if allow_global_fallback && full_handoff.fallback_stage == FallbackStage::TargetImageInstant {
        if let Some(path) = request.fallback_path {
            prefix.push(ApplyTransitionAdornment::FallbackInstantAwww {
                path: path.to_string(),
            });
            prefix.push(ApplyTransitionAdornment::SettleMs(AWWW_FALLBACK_SETTLE_MS));
        }
    }

    let mut suffix = Vec::new();
    if full_handoff.target_startup_settle_ms > 0 {
        suffix.push(ApplyTransitionAdornment::SettleMs(
            full_handoff.target_startup_settle_ms,
        ));
    }
    if lifecycle.post_success_settle_ms > 0 {
        suffix.push(ApplyTransitionAdornment::SettleMs(
            lifecycle.post_success_settle_ms,
        ));
    }
    if lifecycle.post_success_stop != StopPlan::None {
        suffix.push(ApplyTransitionAdornment::LifecycleStop(
            lifecycle.post_success_stop,
        ));
    }

    Ok(ApplyTransitionPlan {
        prefix,
        core_actions: request.core_actions.to_vec(),
        suffix,
        scope_degraded,
        previous: lifecycle.previous,
    })
}

pub fn execute_apply_transition(
    storage: &StorageApi,
    plan: &ApplyTransitionPlan,
    ctx: &DisplayExecContext<'_>,
    runtime: &mut dyn BackendRuntime,
    reporter: &mut dyn ApplyStageReporter,
    request_id: Option<&str>,
) -> Result<ApplyTransitionReport, ApplyTransitionFailure> {
    let mut fallback_applied = false;
    for adornment in &plan.prefix {
        match adornment {
            ApplyTransitionAdornment::FallbackInstantAwww { path } => {
                if let Err(error) = runtime.ensure_backend_available(Backend::Awww, storage) {
                    return Err(ApplyTransitionFailure {
                        exec: DisplayExecReport::default(),
                        error,
                        uncertain_stop: None,
                        rollback_note: None,
                    });
                }
                if let Err(error) = driver::apply_awww_instant(
                    storage,
                    path,
                    &ExecutionScope::AllDisplays,
                    runtime,
                    Some(reporter),
                    request_id,
                ) {
                    return Err(ApplyTransitionFailure {
                        exec: DisplayExecReport::default(),
                        error,
                        uncertain_stop: None,
                        rollback_note: None,
                    });
                }
                fallback_applied = true;
            }
            ApplyTransitionAdornment::SettleMs(ms) => {
                std::thread::sleep(std::time::Duration::from_millis(*ms));
            }
            ApplyTransitionAdornment::LifecycleStop(_) => {
                // Prefix never emits stops.
            }
        }
    }

    let exec_result = execute_display_actions(
        storage,
        &plan.core_actions,
        ctx,
        runtime,
        reporter,
        request_id,
    );

    match exec_result {
        Ok(exec) => {
            for adornment in &plan.suffix {
                match adornment {
                    ApplyTransitionAdornment::SettleMs(ms) => {
                        std::thread::sleep(std::time::Duration::from_millis(*ms));
                    }
                    ApplyTransitionAdornment::LifecycleStop(stop) => {
                        if let Err(error) =
                            crate::execute_stop_plan_with_runtime(storage, *stop, runtime)
                        {
                            return Err(ApplyTransitionFailure {
                                exec,
                                error,
                                uncertain_stop: None,
                                rollback_note: None,
                            });
                        }
                    }
                    ApplyTransitionAdornment::FallbackInstantAwww { .. } => {}
                }
            }
            Ok(ApplyTransitionReport {
                exec,
                fallback_applied,
            })
        }
        Err(failure) => {
            let rollback_note = rollback_visual_fallback(
                storage,
                plan.previous,
                fallback_applied,
                runtime,
            );
            Err(ApplyTransitionFailure {
                exec: failure.report,
                error: failure.error,
                uncertain_stop: failure.uncertain_stop,
                rollback_note,
            })
        }
    }
}

fn rollback_visual_fallback(
    s: &StorageApi,
    previous: lifecycle::RunningBackend,
    fallback_ok: bool,
    runtime: &mut dyn BackendRuntime,
) -> Option<String> {
    if !fallback_ok {
        return None;
    }

    if previous == lifecycle::RunningBackend::Awww {
        if let Some(old_path) = s.current_read().ok().flatten() {
            let p = std::path::Path::new(&old_path);
            if p.is_file() {
                match driver::apply_awww_instant(
                    s,
                    &old_path,
                    &ExecutionScope::AllDisplays,
                    runtime,
                    None,
                    None,
                ) {
                    Ok(()) => Some(format!(
                        "rollback: restored previous awww wallpaper {}",
                        p.file_name().and_then(|n| n.to_str()).unwrap_or(&old_path)
                    )),
                    Err(rollback_err) => Some(format!(
                        "rollback: failed to restore previous awww wallpaper {}: {}",
                        old_path, rollback_err
                    )),
                }
            } else {
                runtime.stop_awww();
                Some(format!(
                    "rollback: previous awww path {} not found, stopped fallback",
                    old_path
                ))
            }
        } else {
            runtime.stop_awww();
            Some("rollback: no previous awww state, stopped fallback".into())
        }
    } else {
        runtime.stop_awww();
        Some("rollback: stopped fallback after non-awww target failure".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lifecycle::RunningBackend;

    fn plan_for(
        previous: &str,
        target: Backend,
        scope: ExecutionScope,
        fallback: Option<&str>,
    ) -> ApplyTransitionPlan {
        plan_apply_transition(&ApplyTransitionRequest {
            scope,
            target,
            previous_backend_raw: previous,
            fallback_path: fallback,
            core_actions: &[],
        })
        .expect("plan")
    }

    #[test]
    fn all_displays_video_to_image_plans_instant_fallback() {
        let plan = plan_for(
            "mpvpaper",
            Backend::Awww,
            ExecutionScope::AllDisplays,
            Some("/tmp/img.jpg"),
        );
        assert!(!plan.scope_degraded);
        assert!(matches!(
            plan.prefix.first(),
            Some(ApplyTransitionAdornment::FallbackInstantAwww { path }) if path == "/tmp/img.jpg"
        ));
        assert!(plan.suffix.iter().any(|a| matches!(
            a,
            ApplyTransitionAdornment::SettleMs(ms) if *ms == AWWW_CROSS_BACKEND_SETTLE_MS
        )));
        assert!(plan.suffix.iter().any(|a| matches!(
            a,
            ApplyTransitionAdornment::LifecycleStop(StopPlan::MpvpaperOnly)
        )));
    }

    #[test]
    fn named_scope_degrades_instant_fallback() {
        let plan = plan_for(
            "mpvpaper",
            Backend::Awww,
            ExecutionScope::named(vec!["eDP-1".into()]).unwrap(),
            Some("/tmp/img.jpg"),
        );
        assert!(plan.scope_degraded);
        assert!(plan.prefix.is_empty());
        assert!(plan.suffix.iter().any(|a| matches!(
            a,
            ApplyTransitionAdornment::LifecycleStop(StopPlan::MpvpaperOnly)
        )));
    }

    #[test]
    fn mpvpaper_target_keeps_startup_settle_on_named_scope() {
        let plan = plan_for(
            "awww",
            Backend::Mpvpaper,
            ExecutionScope::named(vec!["eDP-1".into()]).unwrap(),
            None,
        );
        assert!(!plan.scope_degraded);
        assert!(plan.prefix.is_empty());
        assert!(plan.suffix.iter().any(|a| matches!(
            a,
            ApplyTransitionAdornment::SettleMs(ms) if *ms == MPVPAPER_STARTUP_SETTLE_MS
        )));
    }

    #[test]
    fn image_after_image_has_no_fallback_prefix() {
        let plan = plan_for(
            "awww",
            Backend::Awww,
            ExecutionScope::AllDisplays,
            Some("/tmp/img.jpg"),
        );
        assert!(plan.prefix.is_empty());
        assert!(!plan.scope_degraded);
    }

    #[test]
    fn running_backend_helper_still_parses_swww() {
        assert_eq!(
            RunningBackend::from_last_backend("swww"),
            RunningBackend::Awww
        );
    }
}
