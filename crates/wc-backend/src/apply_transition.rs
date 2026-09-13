//! ApplyTransition — previous→target visual transition for Apply and Restore.
//!
//! Owns fallback/settle adornments around a display_plan Stop/Apply skeleton.
//! Instant awww fallback runs only for [`ExecutionScope::AllDisplays`]; named
//! scopes keep settle only; scoped stops belong to the core action list.

use wc_core::error::WcError;
use wc_core::types::Backend;
use wc_storage::StorageApi;

use crate::apply_stage::ApplyStageReporter;
use crate::display_executor::{
    execute_prepared_display_actions, prepare_display_actions, CompletedStop, DisplayExecAction,
    DisplayExecContext, DisplayExecFailure, DisplayExecReport, PreparedDisplayActions,
};
use crate::lifecycle::{self, StopPlan};
use crate::runtime::BackendRuntime;
use crate::target_commands::ExecutionScope;
use crate::visual_handoff::{self, FallbackStage};

pub use lifecycle::AWWW_CROSS_BACKEND_SETTLE_MS;
pub use visual_handoff::{
    AWWW_FALLBACK_SETTLE_MS, LWE_STARTUP_SETTLE_MS, MPVPAPER_STARTUP_SETTLE_MS,
};

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
    FallbackInstantAwww {
        path: String,
    },
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
    pub cleanup_uncertain: bool,
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
    let scope_degraded =
        !allow_global_fallback && full_handoff.fallback_stage == FallbackStage::TargetImageInstant;

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
    if allow_global_fallback && lifecycle.post_success_stop != StopPlan::None {
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

/// A caller supplies intent; preparation, cleanup ordering and progress stay here.
pub struct TransitionStep {
    pub scope: ExecutionScope,
    pub target: Backend,
    pub fallback_path: Option<String>,
    pub core_actions: Vec<DisplayExecAction>,
}

pub enum TransitionStart<'a> {
    Current { previous_backend_raw: &'a str },
    Restore,
}

pub struct TransitionRequest<'a> {
    pub start: TransitionStart<'a>,
    pub known_outputs: &'a [String],
    pub steps: &'a [TransitionStep],
    pub request_id: Option<&'a str>,
}

impl From<DisplayExecFailure> for ApplyTransitionFailure {
    fn from(failure: DisplayExecFailure) -> Self {
        Self {
            exec: failure.report,
            error: failure.error,
            uncertain_stop: failure.uncertain_stop,
            cleanup_uncertain: failure.cleanup_uncertain,
            rollback_note: None,
        }
    }
}

impl From<WcError> for ApplyTransitionFailure {
    fn from(error: WcError) -> Self {
        Self {
            exec: DisplayExecReport::default(),
            error,
            uncertain_stop: None,
            cleanup_uncertain: false,
            rollback_note: None,
        }
    }
}

/// Prepare the entire sequence before any renderer side effect, then consume it.
#[allow(clippy::result_large_err)]
pub fn execute_apply_transitions(
    storage: &StorageApi,
    request: TransitionRequest<'_>,
    runtime: &mut dyn BackendRuntime,
    reporter: &mut dyn ApplyStageReporter,
) -> Result<ApplyTransitionReport, ApplyTransitionFailure> {
    let mut plans = Vec::new();
    let mut previous = match request.start {
        TransitionStart::Current {
            previous_backend_raw,
        } => previous_backend_raw.to_string(),
        TransitionStart::Restore => {
            if request
                .steps
                .iter()
                .any(|step| step.target == Backend::Awww)
                && request
                    .steps
                    .iter()
                    .any(|step| step.target == Backend::Mpvpaper)
            {
                crate::driver::preflight_awww_transparency(runtime, true)?;
            }
            plans.push(plan_apply_transition(&ApplyTransitionRequest {
                scope: ExecutionScope::AllDisplays,
                target: Backend::Awww,
                previous_backend_raw: "",
                fallback_path: None,
                core_actions: &stop_actions(StopPlan::All),
            })?);
            String::new()
        }
    };
    for step in request.steps {
        plans.push(plan_apply_transition(&ApplyTransitionRequest {
            scope: step.scope.clone(),
            target: step.target,
            previous_backend_raw: &previous,
            fallback_path: step.fallback_path.as_deref(),
            core_actions: &step.core_actions,
        })?);
        previous = step.target.as_str().to_owned();
    }
    execute_plans(
        storage,
        &plans,
        &DisplayExecContext {
            known_outputs: request.known_outputs,
        },
        runtime,
        reporter,
        request.request_id,
    )
}

enum PreparedAdornment {
    Actions(PreparedDisplayActions),
    Settle(u64),
}
struct PreparedTransition {
    prefix: Vec<PreparedAdornment>,
    core: PreparedDisplayActions,
    suffix: Vec<PreparedAdornment>,
    rollback: PreparedDisplayActions,
}

// Matches the existing Restore renderer set. Swaybg is only stopped by its explicit plan.
fn stop_actions(stop: StopPlan) -> Vec<DisplayExecAction> {
    let backends: &[Backend] = match stop {
        StopPlan::All => &[
            Backend::Awww,
            Backend::Mpvpaper,
            Backend::LinuxWallpaperEngine,
        ],
        StopPlan::NonLwe => &[Backend::Awww, Backend::Mpvpaper],
        StopPlan::AwwwDaemonOnly => &[Backend::Awww],
        StopPlan::MpvpaperOnly => &[Backend::Mpvpaper],
        StopPlan::SwaybgOnly => &[Backend::Swaybg],
        StopPlan::LweOnly => &[Backend::LinuxWallpaperEngine],
        StopPlan::None => &[],
    };
    backends
        .iter()
        .map(|backend| DisplayExecAction::Stop {
            backend: *backend,
            scope: ExecutionScope::AllDisplays,
        })
        .collect()
}

#[allow(clippy::result_large_err)]
fn prepare_adornments(
    storage: &StorageApi,
    adornments: &[ApplyTransitionAdornment],
    ctx: &DisplayExecContext<'_>,
    runtime: &mut dyn BackendRuntime,
    request_id: Option<&str>,
    reset_backends: &mut Vec<Backend>,
) -> Result<Vec<PreparedAdornment>, ApplyTransitionFailure> {
    adornments
        .iter()
        .map(|adornment| {
            let actions = match adornment {
                ApplyTransitionAdornment::SettleMs(ms) => {
                    return Ok(PreparedAdornment::Settle(*ms))
                }
                ApplyTransitionAdornment::FallbackInstantAwww { path } => {
                    vec![DisplayExecAction::Apply {
                        backend: Backend::Awww,
                        path: path.clone(),
                        scope: ExecutionScope::AllDisplays,
                        use_instant: true,
                    }]
                }
                ApplyTransitionAdornment::LifecycleStop(stop) => stop_actions(*stop),
            };
            Ok(PreparedAdornment::Actions(prepare_display_actions(
                storage,
                &actions,
                ctx,
                runtime,
                request_id,
                reset_backends,
            )?))
        })
        .collect()
}

#[allow(clippy::result_large_err)]
fn execute_plans(
    storage: &StorageApi,
    plans: &[ApplyTransitionPlan],
    ctx: &DisplayExecContext<'_>,
    runtime: &mut dyn BackendRuntime,
    reporter: &mut dyn ApplyStageReporter,
    request_id: Option<&str>,
) -> Result<ApplyTransitionReport, ApplyTransitionFailure> {
    let mut prepared = Vec::new();
    // These resets invalidate pre-batch observations even after a planned new Apply.
    // The execution path always probes the new live renderer before touching it.
    let mut reset_backends = Vec::new();
    for plan in plans {
        let prefix = prepare_adornments(
            storage,
            &plan.prefix,
            ctx,
            runtime,
            request_id,
            &mut reset_backends,
        )?;
        let core = prepare_display_actions(
            storage,
            &plan.core_actions,
            ctx,
            runtime,
            request_id,
            &mut reset_backends,
        )?;
        let suffix = prepare_adornments(
            storage,
            &plan.suffix,
            ctx,
            runtime,
            request_id,
            &mut reset_backends,
        )?;
        let rollback = if plan
            .prefix
            .iter()
            .any(|a| matches!(a, ApplyTransitionAdornment::FallbackInstantAwww { .. }))
        {
            let old = (plan.previous == lifecycle::RunningBackend::Awww)
                .then(|| storage.current_read().ok().flatten())
                .flatten()
                .filter(|path| std::path::Path::new(path).is_file());
            let restore = old.and_then(|path| {
                prepare_display_actions(
                    storage,
                    &[DisplayExecAction::Apply {
                        backend: Backend::Awww,
                        path,
                        scope: ExecutionScope::AllDisplays,
                        use_instant: true,
                    }],
                    ctx,
                    runtime,
                    request_id,
                    &mut Vec::new(),
                )
                .ok()
            });
            match restore {
                Some(restore) => restore,
                None => prepare_display_actions(
                    storage,
                    &stop_actions(StopPlan::AwwwDaemonOnly),
                    ctx,
                    runtime,
                    request_id,
                    &mut Vec::new(),
                )?,
            }
        } else {
            Vec::new()
        };
        prepared.push(PreparedTransition {
            prefix,
            core,
            suffix,
            rollback,
        });
    }
    let mut exec = DisplayExecReport::default();
    let mut any_fallback = false;
    for transition in prepared {
        let mut fallback_applied = false;
        let result = (|| -> Result<(), DisplayExecFailure> {
            for adornment in transition.prefix {
                match adornment {
                    PreparedAdornment::Settle(ms) => {
                        std::thread::sleep(std::time::Duration::from_millis(ms))
                    }
                    PreparedAdornment::Actions(actions) => {
                        exec.append(execute_prepared_display_actions(
                            storage, actions, runtime, reporter, request_id,
                        )?);
                        fallback_applied = true;
                    }
                }
            }
            exec.append(execute_prepared_display_actions(
                storage,
                transition.core,
                runtime,
                reporter,
                request_id,
            )?);
            Ok(())
        })();
        if let Err(failure) = result {
            exec.append(failure.report);
            let mut failure = ApplyTransitionFailure {
                exec,
                error: failure.error,
                uncertain_stop: failure.uncertain_stop,
                cleanup_uncertain: failure.cleanup_uncertain,
                rollback_note: None,
            };
            if fallback_applied {
                match execute_prepared_display_actions(
                    storage,
                    transition.rollback,
                    runtime,
                    reporter,
                    request_id,
                ) {
                    Ok(report) => {
                        let restored = report.completed_applies().next().is_some();
                        failure.exec.append(report);
                        failure.rollback_note = Some(
                            if restored {
                                "rollback: restored previous awww wallpaper"
                            } else {
                                "rollback: stopped fallback"
                            }
                            .into(),
                        );
                    }
                    Err(rollback) => {
                        failure.exec.append(rollback.report);
                        failure.cleanup_uncertain = true;
                        if failure.uncertain_stop.is_none() {
                            failure.uncertain_stop = rollback.uncertain_stop;
                        }
                        failure.rollback_note =
                            Some(format!("rollback failed: {}", rollback.error));
                    }
                }
            }
            return Err(failure);
        }
        any_fallback |= fallback_applied;
        for adornment in transition.suffix {
            match adornment {
                PreparedAdornment::Settle(ms) => {
                    std::thread::sleep(std::time::Duration::from_millis(ms))
                }
                PreparedAdornment::Actions(actions) => match execute_prepared_display_actions(
                    storage, actions, runtime, reporter, request_id,
                ) {
                    Ok(report) => exec.append(report),
                    Err(failure) => {
                        exec.append(failure.report);
                        return Err(ApplyTransitionFailure {
                            exec,
                            error: failure.error,
                            uncertain_stop: failure.uncertain_stop,
                            cleanup_uncertain: failure.cleanup_uncertain,
                            rollback_note: None,
                        });
                    }
                },
            }
        }
    }
    Ok(ApplyTransitionReport {
        exec,
        fallback_applied: any_fallback,
    })
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
        assert!(!plan
            .suffix
            .iter()
            .any(|a| matches!(a, ApplyTransitionAdornment::LifecycleStop(_))));
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
