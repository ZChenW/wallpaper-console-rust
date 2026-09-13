//! Display-aware apply orchestration for AppService.
//!
//! Loads persisted display assignments, plans via `plan_display_apply`, executes
//! Stop/Apply actions, and commits the intended display_state mapping only after
//! every action succeeds. After a destructive Stop followed by failure, persisted
//! state is reconciled so it does not claim stopped renderers still run.

use wc_backend::apply_stage::{self, ApplyStageReporter, NoopReporter};
use wc_backend::apply_transition::{
    execute_apply_transitions, ApplyTransitionFailure, TransitionRequest, TransitionStart,
    TransitionStep,
};
use wc_backend::display_executor::{
    CompletedEvent, DisplayExecAction, DisplayExecFailure, DisplayExecReport,
};
use wc_backend::runtime::{BackendRuntime, SystemBackendRuntime};
use wc_backend::ExecutionScope;
#[cfg(test)]
use wc_config::ConfigDirExt;
use wc_core::types::{Backend, FileType};
use wc_storage::sqlite::{DisplayStateRow, DisplayStateTarget};

use crate::apply_execution::ApplyExecutionTarget;
use crate::display_plan::{
    plan_display_apply, DisplayApplyRequest, DisplayTarget, PlannedAction, RejectionReason,
    RunningAssignment,
};
use crate::{AppError, AppService, ApplyRequest, ApplyRequestKind, ApplyStageContext, ApplyTarget};

/// Confirmed result of one display-aware apply after renderer execution and
/// display-state persistence both succeed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayApplyExecutionResult {
    pub request_id: Option<String>,
    pub input_path: String,
    pub applied_path: String,
    pub state_path: String,
    pub backend: Backend,
    pub file_type: FileType,
    pub preview: bool,
    pub applied_outputs: Vec<String>,
}

impl DisplayApplyExecutionResult {
    fn into_apply_target(self) -> ApplyTarget {
        ApplyTarget {
            input_path: self.input_path,
            resolved_path: self.applied_path,
            file_type: self.file_type,
            backend: self.backend,
        }
    }
}

/// Optional knobs for [`AppService::apply_to_display_with_runtime`].
#[derive(Default)]
pub struct DisplayApplyRuntimeOpts {
    pub request_id: Option<String>,
    pub capability: Option<wc_backend::capability::BackendCapability>,
    pub on_target_resolved: Option<Box<dyn FnMut(ApplyStageContext) + Send>>,
}

impl AppService {
    /// Apply a wallpaper to an explicit display target.
    pub fn apply_to_display(
        &self,
        path: &str,
        target: DisplayTarget,
        known_outputs: &[String],
    ) -> Result<ApplyTarget, AppError> {
        let mut runtime = SystemBackendRuntime;
        let mut reporter = NoopReporter;
        self.apply_to_display_with_runtime(
            path,
            target,
            known_outputs,
            &mut runtime,
            &mut reporter,
            DisplayApplyRuntimeOpts::default(),
        )
    }

    /// Injectable seam for tests (fake runtime + stage reporter).
    pub fn apply_to_display_with_runtime(
        &self,
        path: &str,
        target: DisplayTarget,
        known_outputs: &[String],
        runtime: &mut dyn BackendRuntime,
        reporter: &mut dyn ApplyStageReporter,
        opts: DisplayApplyRuntimeOpts,
    ) -> Result<ApplyTarget, AppError> {
        let request = ApplyRequest {
            kind: ApplyRequestKind::Apply,
            path: path.to_string(),
            request_id: opts.request_id.clone(),
        };
        self.execute_apply_request_to_display_with_runtime_and_commit_seam(
            request,
            target,
            known_outputs,
            runtime,
            reporter,
            opts,
            None,
        )
        .map(DisplayApplyExecutionResult::into_apply_target)
    }

    /// Execute a complete apply action against an explicit display target.
    /// Unlike the compatibility path-only wrapper, this preserves preview and
    /// retry semantics through planning, execution, and persisted state.
    #[allow(clippy::too_many_arguments)]
    pub fn execute_apply_request_to_display_with_runtime(
        &self,
        request: ApplyRequest,
        target: DisplayTarget,
        known_outputs: &[String],
        runtime: &mut dyn BackendRuntime,
        reporter: &mut dyn ApplyStageReporter,
        opts: DisplayApplyRuntimeOpts,
    ) -> Result<DisplayApplyExecutionResult, AppError> {
        self.execute_apply_request_to_display_with_runtime_and_commit_seam(
            request,
            target,
            known_outputs,
            runtime,
            reporter,
            opts,
            None,
        )
    }

    #[doc(hidden)]
    #[allow(clippy::too_many_arguments)]
    pub fn apply_to_display_with_runtime_and_commit_seam(
        &self,
        path: &str,
        target: DisplayTarget,
        known_outputs: &[String],
        runtime: &mut dyn BackendRuntime,
        reporter: &mut dyn ApplyStageReporter,
        opts: DisplayApplyRuntimeOpts,
        before_state_commit: Option<&mut dyn FnMut() -> Result<(), wc_core::error::WcError>>,
    ) -> Result<ApplyTarget, AppError> {
        let request = ApplyRequest {
            kind: ApplyRequestKind::Apply,
            path: path.to_string(),
            request_id: opts.request_id.clone(),
        };
        self.execute_apply_request_to_display_with_runtime_and_commit_seam(
            request,
            target,
            known_outputs,
            runtime,
            reporter,
            opts,
            before_state_commit,
        )
        .map(DisplayApplyExecutionResult::into_apply_target)
    }

    #[doc(hidden)]
    #[allow(clippy::too_many_arguments)]
    pub fn execute_apply_request_to_display_with_runtime_and_commit_seam(
        &self,
        request: ApplyRequest,
        target: DisplayTarget,
        known_outputs: &[String],
        runtime: &mut dyn BackendRuntime,
        reporter: &mut dyn ApplyStageReporter,
        opts: DisplayApplyRuntimeOpts,
        before_state_commit: Option<&mut dyn FnMut() -> Result<(), wc_core::error::WcError>>,
    ) -> Result<DisplayApplyExecutionResult, AppError> {
        apply_stage::report_stage(
            reporter,
            apply_stage::ApplyStage::ResolveTarget,
            request.request_id.as_deref(),
        );
        let apply_target = self.resolve_apply_request_target(&request)?;
        self.execute_resolved_display_apply(
            request,
            target,
            known_outputs,
            runtime,
            reporter,
            opts,
            before_state_commit,
            apply_target,
            true,
        )
    }

    /// Reuse renderer planning and failure reconciliation for a confirmed runtime
    /// reload, without resolving new routing preferences or publishing a theme.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn execute_resolved_display_apply(
        &self,
        request: ApplyRequest,
        target: DisplayTarget,
        known_outputs: &[String],
        runtime: &mut dyn BackendRuntime,
        reporter: &mut dyn ApplyStageReporter,
        mut opts: DisplayApplyRuntimeOpts,
        before_state_commit: Option<&mut dyn FnMut() -> Result<(), wc_core::error::WcError>>,
        apply_target: ApplyExecutionTarget,
        update_assignment: bool,
    ) -> Result<DisplayApplyExecutionResult, AppError> {
        let _guard = crate::output_recovery::RendererMutationGuard::acquire(&self.storage)
            .map_err(AppError::from_wc_error)?;
        let request_id = request.request_id.as_deref();

        if let Some(on_resolved) = opts.on_target_resolved.as_mut() {
            on_resolved(ApplyStageContext {
                preview: apply_target.preview,
                backend: apply_target.backend,
            });
        }
        let previous_rows = self
            .storage
            .display_state_list()
            .map_err(AppError::from_wc_error)?;
        // Conflict input comes from live renderer ownership, not persisted
        // rows: saved assignments are restore preferences and may describe
        // renderers that have since stopped (e.g. after a global Stop).
        let ownership =
            wc_backend::runtime_observation::observe_output_ownership(known_outputs, runtime);
        let running = running_assignments_from_observation(
            &target,
            &previous_rows,
            known_outputs,
            &ownership,
        )?;
        let same_backend_already_running = running
            .iter()
            .any(|assignment| assignment.backend == apply_target.backend);

        let plan_request = DisplayApplyRequest {
            target: target.clone(),
            backend: apply_target.backend,
            known_outputs: known_outputs.to_vec(),
            running,
        };
        let plan = match opts.capability {
            Some(cap) => {
                crate::display_plan::plan_display_apply_with_capability(&plan_request, cap)
            }
            None => plan_display_apply(&plan_request),
        }
        .map_err(rejection_to_app_error)?;
        let applied_outputs = planned_apply_outputs(&plan.actions);

        let mut planned_actions = plan.actions;
        if matches!(target, DisplayTarget::AllDisplays) {
            // An explicit All Displays replace retires every backend with live
            // evidence, even when per-output ownership was ambiguous. The
            // executor verifies each stop, so this stays fail-closed. The
            // target backend itself is only retired when it cannot replace in
            // place (e.g. an ambiguous mpvpaper process must not be joined by
            // new instances).
            let stopped: std::collections::HashSet<Backend> = planned_actions
                .iter()
                .filter_map(|action| match action {
                    PlannedAction::Stop { backend, .. }
                    | PlannedAction::StopBackend { backend } => Some(*backend),
                    PlannedAction::Apply { .. } => None,
                })
                .collect();
            let target_needs_retirement = matches!(
                plan.capability.same_target_replacement,
                wc_backend::capability::SameTargetReplacement::StopThenApply
            );
            let extra_stops: Vec<PlannedAction> = ownership
                .implicated_backends
                .iter()
                .filter(|backend| {
                    !stopped.contains(*backend)
                        && (**backend != apply_target.backend || target_needs_retirement)
                })
                .map(|backend| PlannedAction::StopBackend { backend: *backend })
                .collect();
            if !extra_stops.is_empty() {
                let first_apply = planned_actions
                    .iter()
                    .position(|action| matches!(action, PlannedAction::Apply { .. }))
                    .unwrap_or(planned_actions.len());
                planned_actions.splice(first_apply..first_apply, extra_stops);
            }
        }

        let plan_has_stop = planned_actions.iter().any(|action| {
            matches!(
                action,
                PlannedAction::Stop { .. } | PlannedAction::StopBackend { .. }
            )
        });
        let use_instant = plan_has_stop || !same_backend_already_running;
        let actions: Vec<DisplayExecAction> = planned_actions
            .into_iter()
            .map(|action| {
                to_exec_action(
                    action,
                    &apply_target.resolved_path,
                    &target,
                    known_outputs,
                    use_instant,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;

        let previous_backend_raw = self
            .storage
            .last_backend_read()
            .map_err(AppError::from_wc_error)?
            .unwrap_or_default();
        let transition_scope = transition_scope_for_target(&target, known_outputs)?;
        let exec_result = execute_apply_transitions(
            &self.storage,
            TransitionRequest {
                start: TransitionStart::Current {
                    previous_backend_raw: &previous_backend_raw,
                },
                known_outputs,
                steps: &[TransitionStep {
                    scope: transition_scope,
                    target: apply_target.backend,
                    fallback_path: apply_target.fallback_path.clone(),
                    core_actions: actions,
                }],
                request_id,
            },
            runtime,
            reporter,
        );

        match exec_result {
            Ok(transition_report) => {
                let report = transition_report.exec;
                let intended = intended_display_state(
                    &previous_rows,
                    known_outputs,
                    &target,
                    &apply_target.state_path,
                    apply_target.backend,
                );
                if update_assignment {
                    if let Err(error) = self.commit_successful_display_state(
                        &target,
                        &intended,
                        &apply_target,
                        before_state_commit,
                    ) {
                        return Err(self.reconcile_commit_failure(
                            error,
                            &previous_rows,
                            &report,
                            known_outputs,
                            &apply_target,
                        ));
                    }
                    if let Some(path) =
                        compat_failure_path_after_success(&request.kind, &apply_target)
                    {
                        let _ = wc_storage::we_compat::clear_failure(path);
                    }
                    let post_apply_ctx = crate::post_apply::build_theme_context(
                        &self.storage,
                        &crate::post_apply::ThemePublishRequest {
                            intended: &intended,
                            known_outputs,
                            changed_outputs: &applied_outputs,
                            applied: Some(crate::post_apply::AppliedThemeSource {
                                wallpaper: &apply_target.resolved_path,
                                backend: apply_target.backend,
                                file_type: apply_target.file_type,
                            }),
                        },
                    );
                    crate::post_apply::publish_theme_and_run_hook(&self.storage, &post_apply_ctx);
                }
                if runtime.supports_output_recovery() {
                    crate::output_recovery::ensure_watcher(&self.storage);
                }
                Ok(DisplayApplyExecutionResult {
                    request_id: request.request_id,
                    input_path: apply_target.input_path,
                    applied_path: apply_target.resolved_path,
                    state_path: apply_target.state_path,
                    backend: apply_target.backend,
                    file_type: apply_target.file_type,
                    preview: apply_target.preview,
                    applied_outputs,
                })
            }
            Err(failure) => {
                let failure = display_exec_failure_from_transition(failure);
                let compat_error = compat_failure_error(&apply_target, &failure.error);
                let error = self.handle_exec_failure(
                    failure,
                    &previous_rows,
                    known_outputs,
                    before_state_commit,
                )?;
                if let Some(compat_error) = compat_error {
                    record_compat_failure(&apply_target, &compat_error);
                }
                Err(error)
            }
        }
    }

    fn commit_successful_display_state(
        &self,
        target: &DisplayTarget,
        intended: &[(DisplayStateTarget, String, String)],
        apply_target: &ApplyExecutionTarget,
        before_commit: Option<&mut dyn FnMut() -> Result<(), wc_core::error::WcError>>,
    ) -> Result<(), wc_core::error::WcError> {
        match target {
            DisplayTarget::AllDisplays => {
                let retain: Vec<(DisplayStateTarget, String, String)> = intended
                    .iter()
                    .filter(|(t, _, _)| !matches!(t, DisplayStateTarget::AllDisplays))
                    .cloned()
                    .collect();
                match before_commit {
                    Some(seam) => self
                        .storage
                        .display_state_commit_all_displays_with_legacy_seam(
                            &apply_target.state_path,
                            apply_target.backend.as_str(),
                            &retain,
                            true,
                            seam,
                        ),
                    None => self.storage.display_state_commit_all_displays_with_legacy(
                        &apply_target.state_path,
                        apply_target.backend.as_str(),
                        &retain,
                        true,
                    ),
                }?;
            }
            DisplayTarget::Output(_) => {
                // Named applies: display_state is authoritative; no best-effort legacy writes.
                match before_commit {
                    Some(seam) => self.storage.display_state_replace_all_seam(intended, seam),
                    None => self.storage.display_state_replace_all(intended),
                }?;
            }
        }
        Ok(())
    }

    fn reconcile_commit_failure(
        &self,
        error: wc_core::error::WcError,
        previous_rows: &[DisplayStateRow],
        report: &DisplayExecReport,
        known_outputs: &[String],
        apply_target: &ApplyExecutionTarget,
    ) -> AppError {
        let reconciled = reconcile_display_state_from_report(previous_rows, report, known_outputs);
        let _ = self.storage.runtime_state_clear();
        if let Err(reconcile_error) = self.storage.display_state_replace_all(&reconciled) {
            return AppError {
                code: "display_state_uncertain".into(),
                message: format!(
                    "Wallpaper applied via {}, but both the state commit and reconciliation failed",
                    apply_target.backend.as_str()
                ),
                detail: Some(format!(
                    "commit_error={error}; reconciliation_error={reconcile_error}"
                )),
                recoverable: true,
                suggestion: Some(
                    "Refresh renderer status before applying another wallpaper.".into(),
                ),
            };
        }
        AppError {
            code: "display_state_commit_failed".into(),
            message: format!(
                "Wallpaper applied via {} but persisting display state failed: {error}",
                apply_target.backend.as_str()
            ),
            detail: Some(error.to_string()),
            recoverable: true,
            suggestion: Some(
                "Re-apply the wallpaper or clear display state before retrying.".into(),
            ),
        }
    }

    pub(crate) fn handle_exec_failure(
        &self,
        failure: DisplayExecFailure,
        previous_rows: &[DisplayStateRow],
        known_outputs: &[String],
        before_reconcile: Option<&mut dyn FnMut() -> Result<(), wc_core::error::WcError>>,
    ) -> Result<AppError, AppError> {
        if failure.cleanup_uncertain {
            // Confirmed progress in the ordered report is always reconciled,
            // even when no stop post-condition failed: a confirmed stop must
            // not remain persisted as a live assignment.
            let clear_result = self.storage.runtime_state_clear();
            let mut conservative_report = failure.report.clone();
            if let Some(stop) = failure.uncertain_stop.clone().map(|stop| *stop) {
                conservative_report.record_stop(stop);
            }
            let reconciled = reconcile_display_state_from_report(
                previous_rows,
                &conservative_report,
                known_outputs,
            );
            let persist_result = match before_reconcile {
                Some(seam) => self
                    .storage
                    .display_state_replace_all_seam(&reconciled, seam),
                None => self.storage.display_state_replace_all(&reconciled),
            };
            let clear_note = clear_result
                .err()
                .map(|error| format!("runtime_state_clear_error={error}"));
            if let Err(persist_error) = persist_result {
                let mut detail = format!(
                    "execution_error={}; reconciliation_error={persist_error}",
                    failure.error
                );
                if let Some(note) = &clear_note {
                    detail.push_str("; ");
                    detail.push_str(note);
                }
                return Err(AppError {
                    code: "display_state_uncertain".into(),
                    message: "Renderer cleanup outcome and persisted display state are uncertain"
                        .into(),
                    detail: Some(detail),
                    recoverable: true,
                    suggestion: Some("Refresh renderer status before retrying.".into()),
                });
            }
            let mut detail = failure.error.to_string();
            if let Some(note) = &clear_note {
                detail.push_str("; ");
                detail.push_str(note);
            }
            return Ok(AppError {
                code: "display_state_uncertain".into(),
                message: "Renderer cleanup could not be verified".into(),
                detail: Some(detail),
                recoverable: true,
                suggestion: Some("Refresh renderer status before retrying.".into()),
            });
        }
        let after_stop = failure.after_destructive_stop();
        let had_progress = after_stop || failure.report.completed_applies().next().is_some();
        if had_progress {
            let reconciled =
                reconcile_display_state_from_report(previous_rows, &failure.report, known_outputs);
            let persist_result = match before_reconcile {
                Some(seam) => self
                    .storage
                    .display_state_replace_all_seam(&reconciled, seam),
                None => self.storage.display_state_replace_all(&reconciled),
            };
            persist_result.map_err(|reconcile_error| AppError {
                code: "display_state_uncertain".into(),
                message:
                    "Wallpaper execution changed live state, but persistence reconciliation failed"
                        .into(),
                detail: Some(format!(
                    "execution_error={}; reconciliation_error={reconcile_error}",
                    failure.error
                )),
                recoverable: true,
                suggestion: Some("Refresh renderer status before retrying.".into()),
            })?;
        }
        let stopped = failure.report.stopped_backends();
        let applies = failure.report.completed_applies().count();
        let mut app_err = AppError::from_wc_error(failure.error);
        if after_stop {
            app_err.code = "display_apply_failed_after_stop".into();
            app_err.detail = Some(format!(
                "destructive_stops={:?}; successful_applies={}",
                stopped, applies
            ));
        } else if had_progress {
            app_err.code = "display_apply_failed_after_partial_apply".into();
            app_err.detail = Some(format!("successful_applies={applies}"));
        }
        Ok(app_err)
    }
}

fn planned_apply_outputs(actions: &[PlannedAction]) -> Vec<String> {
    let mut outputs = Vec::new();
    for action in actions {
        let PlannedAction::Apply {
            outputs: planned, ..
        } = action
        else {
            continue;
        };
        for output in planned {
            if !outputs.contains(output) {
                outputs.push(output.clone());
            }
        }
    }
    outputs
}

fn compat_failure_path_after_success<'a>(
    kind: &ApplyRequestKind,
    target: &'a ApplyExecutionTarget,
) -> Option<&'a str> {
    if target.file_type == FileType::WeScene
        && matches!(
            kind,
            ApplyRequestKind::Apply | ApplyRequestKind::RetryBackendApply
        )
    {
        Some(target.state_path.as_str())
    } else {
        None
    }
}

fn record_compat_failure(target: &ApplyExecutionTarget, error: &AppError) {
    if target.file_type != FileType::WeScene {
        return;
    }
    let backend_status = if error.code == "renderer_limitation" {
        "renderer_limitation"
    } else {
        "failed"
    };
    let _ = wc_storage::we_compat::record_failure(
        &target.state_path,
        backend_status,
        &error.code,
        &error.message,
        error.detail.clone(),
    );
}

fn compat_failure_error(
    target: &ApplyExecutionTarget,
    error: &wc_core::error::WcError,
) -> Option<AppError> {
    if target.file_type != FileType::WeScene || target.backend != Backend::LinuxWallpaperEngine {
        return None;
    }
    let wc_core::error::WcError::LinuxWallpaperEngine { kind, detail } = error else {
        return None;
    };
    Some(AppError::from_wc_error(
        wc_core::error::WcError::LinuxWallpaperEngine {
            kind: kind.clone(),
            detail: detail.clone(),
        },
    ))
}

pub(crate) fn to_exec_action(
    action: PlannedAction,
    path: &str,
    target: &DisplayTarget,
    known_outputs: &[String],
    use_instant: bool,
) -> Result<DisplayExecAction, AppError> {
    match action {
        PlannedAction::StopBackend { backend } => Ok(DisplayExecAction::Stop {
            backend,
            scope: ExecutionScope::AllDisplays,
        }),
        PlannedAction::Stop { backend, outputs } => {
            let scope = stop_scope_for_action(target, &outputs, known_outputs)?;
            Ok(DisplayExecAction::Stop { backend, scope })
        }
        PlannedAction::Apply { backend, outputs } => {
            let scope = match target {
                DisplayTarget::AllDisplays => {
                    // All Displays awww omits --outputs; mpvpaper/LWE still need named groups.
                    if matches!(backend, Backend::Awww) && outputs.len() == known_outputs.len() {
                        ExecutionScope::AllDisplays
                    } else {
                        ExecutionScope::named(outputs).map_err(AppError::from_wc_error)?
                    }
                }
                DisplayTarget::Output(_) => {
                    ExecutionScope::named(outputs).map_err(AppError::from_wc_error)?
                }
            };
            Ok(DisplayExecAction::Apply {
                backend,
                path: path.to_string(),
                scope,
                use_instant,
            })
        }
    }
}

pub(crate) fn transition_scope_for_target(
    target: &DisplayTarget,
    _known_outputs: &[String],
) -> Result<ExecutionScope, AppError> {
    match target {
        DisplayTarget::AllDisplays => Ok(ExecutionScope::AllDisplays),
        DisplayTarget::Output(output) => {
            ExecutionScope::named(vec![output.clone()]).map_err(AppError::from_wc_error)
        }
    }
}

pub(crate) fn display_exec_failure_from_transition(
    failure: ApplyTransitionFailure,
) -> DisplayExecFailure {
    DisplayExecFailure {
        report: failure.exec,
        error: failure.error,
        uncertain_stop: failure.uncertain_stop,
        cleanup_uncertain: failure.cleanup_uncertain,
    }
}

/// All-displays apply stops are intentionally global for the backend.
/// Named-target stops preserve the planned output list (executor rejects partials).
fn stop_scope_for_action(
    target: &DisplayTarget,
    outputs: &[String],
    known_outputs: &[String],
) -> Result<ExecutionScope, AppError> {
    match target {
        DisplayTarget::AllDisplays => Ok(ExecutionScope::AllDisplays),
        DisplayTarget::Output(_) => {
            // If the planned stop already covers every known output, AllDisplays
            // is equivalent and preferred; otherwise preserve Named scope.
            let known: std::collections::HashSet<&str> =
                known_outputs.iter().map(String::as_str).collect();
            let named: std::collections::HashSet<&str> =
                outputs.iter().map(String::as_str).collect();
            if named == known && !known.is_empty() {
                Ok(ExecutionScope::AllDisplays)
            } else {
                ExecutionScope::named(outputs.to_vec()).map_err(AppError::from_wc_error)
            }
        }
    }
}

pub(crate) fn parse_backend(raw: &str) -> Result<Backend, AppError> {
    crate::post_apply::parse_state_backend(raw).ok_or_else(|| AppError {
        code: "invalid_display_state".into(),
        message: format!("unsupported display state backend: {raw}"),
        detail: None,
        recoverable: true,
        suggestion: None,
    })
}

/// Build the planner's running-assignment input from live ownership evidence.
///
/// Named targets require every connected output to be resolvable: an
/// uncertain ownership would make a single-output stop or coexistence check
/// unsafe, so the apply is rejected with the observation reasons. All
/// Displays is an explicit global replacement: confirmed-occupied outputs
/// override saved rows, confirmed-vacant outputs drop stale saved claims, and
/// uncertain outputs keep their saved claim as a possible occupant to retire
/// (every planned stop is verified at execution time).
fn running_assignments_from_observation(
    target: &DisplayTarget,
    previous_rows: &[DisplayStateRow],
    known_outputs: &[String],
    ownership: &wc_backend::runtime_observation::OutputOwnershipSnapshot,
) -> Result<Vec<RunningAssignment>, AppError> {
    use wc_backend::runtime_observation::OutputOwnership;

    if let Some(reason) = &ownership.process_inspection_error {
        return Err(AppError {
            code: "display_observation_failed".into(),
            message: "Cannot verify the running wallpaper renderers before applying.".into(),
            detail: Some(reason.clone()),
            recoverable: true,
            suggestion: Some("Check renderer process inspection, then retry.".into()),
        });
    }

    match target {
        DisplayTarget::Output(_) => {
            let mut uncertain = Vec::new();
            let mut running = Vec::new();
            for (output, ownership) in &ownership.outputs {
                match ownership {
                    OutputOwnership::Occupied(backend) => running.push(RunningAssignment {
                        output: output.clone(),
                        backend: *backend,
                    }),
                    OutputOwnership::Vacant => {}
                    OutputOwnership::Uncertain(reason) => {
                        uncertain.push(format!("{output}: {reason}"));
                    }
                }
            }
            if !uncertain.is_empty() {
                return Err(AppError {
                    code: "display_observation_failed".into(),
                    message: "Cannot verify which renderers currently own each display.".into(),
                    detail: Some(uncertain.join("; ")),
                    recoverable: true,
                    suggestion: Some(
                        "Check the renderer processes named in the details, then retry.".into(),
                    ),
                });
            }
            Ok(running)
        }
        DisplayTarget::AllDisplays => {
            let mut by_output: Vec<(String, Backend)> =
                running_from_display_state(previous_rows, known_outputs)?
                    .into_iter()
                    .map(|assignment| (assignment.output, assignment.backend))
                    .collect();
            for (output, ownership) in &ownership.outputs {
                match ownership {
                    OutputOwnership::Occupied(backend) => {
                        if let Some(entry) = by_output.iter_mut().find(|(name, _)| name == output) {
                            entry.1 = *backend;
                        } else {
                            by_output.push((output.clone(), *backend));
                        }
                    }
                    OutputOwnership::Vacant => {
                        by_output.retain(|(name, _)| name != output);
                    }
                    OutputOwnership::Uncertain(_) => {}
                }
            }
            Ok(by_output
                .into_iter()
                .map(|(output, backend)| RunningAssignment { output, backend })
                .collect())
        }
    }
}

/// Expand persisted rows into concrete per-output running assignments.
pub(crate) fn running_from_display_state(
    rows: &[DisplayStateRow],
    known_outputs: &[String],
) -> Result<Vec<RunningAssignment>, AppError> {
    let mut by_output: Vec<(String, Backend)> = Vec::new();

    let all_displays = rows
        .iter()
        .find(|row| matches!(row.target, DisplayStateTarget::AllDisplays));

    if let Some(row) = all_displays {
        let backend = parse_backend(&row.backend)?;
        for output in known_outputs {
            by_output.push((output.clone(), backend));
        }
    }

    for row in rows {
        let DisplayStateTarget::Output(name) = &row.target else {
            continue;
        };
        if !known_outputs.iter().any(|known| known == name) {
            // Disconnected output — ignore for planning input only.
            continue;
        }
        let backend = parse_backend(&row.backend)?;
        if let Some((_, existing)) = by_output.iter_mut().find(|(output, _)| output == name) {
            *existing = backend;
        } else {
            by_output.push((name.clone(), backend));
        }
    }

    Ok(by_output
        .into_iter()
        .map(|(output, backend)| RunningAssignment { output, backend })
        .collect())
}

/// Compute the display_state rows to persist after a successful apply.
///
/// Disconnected (unknown) output rows from `previous` are preserved.
pub(crate) fn intended_display_state(
    previous: &[DisplayStateRow],
    known_outputs: &[String],
    target: &DisplayTarget,
    wallpaper_path: &str,
    backend: Backend,
) -> Vec<(DisplayStateTarget, String, String)> {
    match target {
        DisplayTarget::AllDisplays => {
            let mut rows = vec![(
                DisplayStateTarget::AllDisplays,
                wallpaper_path.to_string(),
                backend.as_str().to_string(),
            )];
            // Preserve disconnected named rows that are not covered by AllDisplays.
            for row in previous {
                let DisplayStateTarget::Output(output) = &row.target else {
                    continue;
                };
                if known_outputs.iter().any(|known| known == output) {
                    continue;
                }
                rows.push((
                    DisplayStateTarget::Output(output.clone()),
                    row.wallpaper_path.clone(),
                    row.backend.clone(),
                ));
            }
            rows
        }
        DisplayTarget::Output(name) => {
            let mut map: Vec<(String, String, String)> = Vec::new();

            if let Some(all) = previous
                .iter()
                .find(|row| matches!(row.target, DisplayStateTarget::AllDisplays))
            {
                for output in known_outputs {
                    map.push((
                        output.clone(),
                        all.wallpaper_path.clone(),
                        all.backend.clone(),
                    ));
                }
            }

            for row in previous {
                let DisplayStateTarget::Output(output) = &row.target else {
                    continue;
                };
                // Preserve disconnected rows and connected overrides.
                if let Some(entry) = map.iter_mut().find(|(o, _, _)| o == output) {
                    entry.1 = row.wallpaper_path.clone();
                    entry.2 = row.backend.clone();
                } else {
                    map.push((
                        output.clone(),
                        row.wallpaper_path.clone(),
                        row.backend.clone(),
                    ));
                }
            }

            if let Some(entry) = map.iter_mut().find(|(o, _, _)| o == name) {
                entry.1 = wallpaper_path.to_string();
                entry.2 = backend.as_str().to_string();
            } else {
                map.push((
                    name.clone(),
                    wallpaper_path.to_string(),
                    backend.as_str().to_string(),
                ));
            }

            map.into_iter()
                .map(|(output, path, backend)| (DisplayStateTarget::Output(output), path, backend))
                .collect()
        }
    }
}

/// Rebuild persisted display_state from the ordered execution report.
///
/// Completed destructive stops remove affected prior assignments; completed
/// applies then materialize surviving live assignments by scope/path/backend.
/// Unaffected and disconnected rows are preserved.
pub(crate) fn reconcile_display_state_from_report(
    previous: &[DisplayStateRow],
    report: &DisplayExecReport,
    known_outputs: &[String],
) -> Vec<(DisplayStateTarget, String, String)> {
    let mut rows: Vec<(DisplayStateTarget, String, String)> = previous
        .iter()
        .map(|row| {
            (
                row.target.clone(),
                row.wallpaper_path.clone(),
                row.backend.clone(),
            )
        })
        .collect();

    expand_all_display_rows(&mut rows, known_outputs);

    for event in &report.events {
        match event {
            CompletedEvent::Stop(stop) if stop.destructive => {
                apply_completed_stop(&mut rows, stop, known_outputs)
            }
            CompletedEvent::Stop(_) => {}
            CompletedEvent::Apply(apply) => apply_completed_apply(&mut rows, apply, known_outputs),
        }
    }
    rows
}

fn expand_all_display_rows(
    rows: &mut Vec<(DisplayStateTarget, String, String)>,
    known_outputs: &[String],
) {
    if !known_outputs.is_empty() {
        if let Some((_, path, backend)) = rows
            .iter()
            .find(|(target, _, _)| matches!(target, DisplayStateTarget::AllDisplays))
            .cloned()
        {
            rows.retain(|(target, _, _)| !matches!(target, DisplayStateTarget::AllDisplays));
            for output in known_outputs {
                if !rows
                    .iter()
                    .any(|(target, _, _)| target == &DisplayStateTarget::Output(output.clone()))
                {
                    rows.push((
                        DisplayStateTarget::Output(output.clone()),
                        path.clone(),
                        backend.clone(),
                    ));
                }
            }
        }
    }
}

fn apply_completed_stop(
    rows: &mut Vec<(DisplayStateTarget, String, String)>,
    stop: &wc_backend::display_executor::CompletedStop,
    known_outputs: &[String],
) {
    let backend = stop.backend.as_str();
    if matches!(stop.scope, ExecutionScope::Named(_)) {
        expand_all_display_rows(rows, known_outputs);
    }
    match &stop.scope {
        ExecutionScope::AllDisplays => {
            // A process-wide stop only proves connected renderer ownership disappeared.
            // Disconnected rows are restore preferences, not currently running processes.
            rows.retain(|(target, _, row_backend)| {
                row_backend != backend
                    || match target {
                        DisplayStateTarget::AllDisplays => false,
                        DisplayStateTarget::Output(output) => !known_outputs.contains(output),
                    }
            });
        }
        ExecutionScope::Named(outputs) => {
            rows.retain(|(target, _, row_backend)| {
                if row_backend != backend {
                    return true;
                }
                match target {
                    DisplayStateTarget::AllDisplays => true,
                    DisplayStateTarget::Output(output) => !outputs.contains(output),
                }
            });
        }
    }
}

fn apply_completed_apply(
    rows: &mut Vec<(DisplayStateTarget, String, String)>,
    apply: &wc_backend::display_executor::CompletedApply,
    known_outputs: &[String],
) {
    let backend = apply.backend.as_str().to_string();
    let path = apply.path.clone();
    if matches!(apply.scope, ExecutionScope::Named(_)) {
        expand_all_display_rows(rows, known_outputs);
    }
    match &apply.scope {
        ExecutionScope::AllDisplays => {
            rows.retain(|(target, _, _)| match target {
                DisplayStateTarget::AllDisplays => false,
                DisplayStateTarget::Output(output) => !known_outputs.contains(output),
            });
            rows.insert(0, (DisplayStateTarget::AllDisplays, path, backend));
        }
        ExecutionScope::Named(outputs) => {
            rows.retain(|(t, _, _)| !matches!(t, DisplayStateTarget::AllDisplays));
            for output in outputs {
                rows.retain(|(t, _, _)| t != &DisplayStateTarget::Output(output.clone()));
                rows.push((
                    DisplayStateTarget::Output(output.clone()),
                    path.clone(),
                    backend.clone(),
                ));
            }
        }
    }
}

pub(crate) fn rejection_to_app_error(reason: RejectionReason) -> AppError {
    let message = match &reason {
        RejectionReason::UnsupportedBackend => "Unsupported wallpaper backend.".into(),
        RejectionReason::EmptyNamedOutput => "Display target name must not be blank.".into(),
        RejectionReason::UnknownNamedOutput { output } => {
            format!("Unknown display output: {output}")
        }
        RejectionReason::BlankKnownOutput { output } => {
            format!("Known outputs contain a blank entry: {output:?}")
        }
        RejectionReason::DuplicateKnownOutputs { output } => {
            format!("Known outputs contain a duplicate: {output}")
        }
        RejectionReason::BlankRunningAssignmentOutput { output } => {
            format!("Running assignment output is blank: {output:?}")
        }
        RejectionReason::DuplicateRunningAssignment { output } => {
            format!("Duplicate running assignment for output: {output}")
        }
        RejectionReason::ConflictingRunningAssignment { output } => {
            format!("Conflicting running assignment for output: {output}")
        }
        RejectionReason::RunningAssignmentUnknownOutput { output } => {
            format!("Running assignment refers to unknown output: {output}")
        }
        RejectionReason::WouldAffectNonTargetDisplay {
            non_target,
            explanation,
        } => format!("Would affect non-target display {non_target}: {explanation}"),
        RejectionReason::ReliesOnUnknownCoexistence { explanation } => explanation.clone(),
        RejectionReason::UnverifiedTargetScope { explanation } => explanation.clone(),
        RejectionReason::StopWouldAffectNonTarget {
            non_target,
            explanation,
        } => format!("Stop would affect non-target display {non_target}: {explanation}"),
        RejectionReason::NoKnownOutputs => {
            "All Displays requires at least one known output.".into()
        }
    };
    AppError {
        code: "display_apply_rejected".into(),
        message,
        detail: Some(format!("{reason:?}")),
        recoverable: true,
        suggestion: Some(
            "Choose a different display target or clear conflicting wallpapers first.".into(),
        ),
    }
}

/// After a successful legacy `apply(path)`, record explicit All Displays state
/// atomically with legacy current/last_backend keys.
pub(crate) fn commit_legacy_apply_display_state(
    service: &AppService,
    wallpaper_path: &str,
    backend: Backend,
) -> Result<(), AppError> {
    commit_legacy_apply_display_state_with_seam(service, wallpaper_path, backend, None)
}

pub(crate) fn commit_legacy_apply_display_state_with_seam(
    service: &AppService,
    wallpaper_path: &str,
    backend: Backend,
    before_commit: Option<&mut dyn FnMut() -> Result<(), wc_core::error::WcError>>,
) -> Result<(), AppError> {
    // The compatibility API has no connected-output topology. Retaining named
    // rows could let a stale connected override contradict the successful
    // AllDisplays renderer state, so correctness requires clearing them here.
    let result = match before_commit {
        Some(seam) => service
            .storage
            .display_state_commit_all_displays_with_legacy_seam(
                wallpaper_path,
                backend.as_str(),
                &[],
                true,
                seam,
            ),
        None => service
            .storage
            .display_state_commit_all_displays_with_legacy(
                wallpaper_path,
                backend.as_str(),
                &[],
                true,
            ),
    };
    match result {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = service.storage.runtime_state_clear();
            let reconciled = vec![(
                DisplayStateTarget::AllDisplays,
                wallpaper_path.to_string(),
                backend.as_str().to_string(),
            )];
            if let Err(reconcile_error) = service.storage.display_state_replace_all(&reconciled) {
                return Err(AppError {
                    code: "display_state_uncertain".into(),
                    message: "Legacy apply succeeded, but display state reconciliation failed"
                        .into(),
                    detail: Some(format!(
                        "commit_error={e}; reconciliation_error={reconcile_error}"
                    )),
                    recoverable: true,
                    suggestion: Some("Refresh renderer status before retrying.".into()),
                });
            }
            Err(AppError {
                code: "display_state_commit_failed".into(),
                message: format!(
                    "Legacy apply succeeded via {} but persisting display state failed: {e}",
                    backend.as_str()
                ),
                detail: Some(e.to_string()),
                recoverable: true,
                suggestion: Some(
                    "Re-apply the wallpaper or clear display state before retrying.".into(),
                ),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::path::Path;
    use std::process::Command;
    use wc_backend::apply_stage::{ApplyStage, ApplyStageEvent, NoopReporter};
    use wc_backend::runtime::{AwwwReadiness, MpvpaperProcess, ProcessIo};
    use wc_core::config::ConfigDir;
    use wc_core::error::WcError;

    #[derive(Default)]
    struct FakeRuntime {
        stop_awww_count: usize,
        stop_mpvpaper_count: usize,
        stop_mpvpaper_outputs_calls: Vec<Vec<String>>,
        stop_lwe_count: usize,
        stop_lwe_outputs_calls: Vec<Vec<String>>,
        stop_mpvpaper_error: Option<String>,
        command_output_success: bool,
        command_status_success: bool,
        command_output_args: Vec<Vec<String>>,
        command_status_args: Vec<Vec<String>>,
        fail_after_n_status: Option<usize>,
        mpvpaper_ready_pid: Option<u32>,
        running_mpvpaper_pids: Vec<u32>,
        mpvpaper_process_table: Vec<MpvpaperProcess>,
        mpvpaper_processes_error: Option<String>,
        mpvpaper_readiness_error: Option<String>,
        failed_mpvpaper_launch_cleanup_count: usize,
        failed_mpvpaper_launch_cleanup_error: Option<String>,
        process_scan_error: Option<String>,
        lwe_outputs: Vec<String>,
        extra_command_lines: Vec<Vec<String>>,
        awww_displayed: Vec<(String, String)>,
        awww_stopped: bool,
        awww_readiness_sequence: RefCell<Vec<AwwwReadiness>>,
        lwe_apply_calls: usize,
        lwe_apply_error: Option<String>,
        awww_stop_verify_pending: bool,
        stop_awww_error: Option<String>,
        mpvpaper_pids_error: Option<String>,
    }

    impl FakeRuntime {
        fn all_mpvpaper_pids(&self) -> Vec<u32> {
            let mut pids = self.running_mpvpaper_pids.clone();
            for process in &self.mpvpaper_process_table {
                if !pids.contains(&process.pid) {
                    pids.push(process.pid);
                }
            }
            pids
        }

        /// Query evidence: displayed images, with the latest `clear` outputs
        /// reported as transparent (matching daemon behavior after release).
        fn awww_query_payload(&self) -> String {
            let mut entries: Vec<serde_json::Value> = self
                .awww_displayed
                .iter()
                .map(
                    |(name, path)| serde_json::json!({"name": name, "displaying": {"image": path}}),
                )
                .collect();
            if let Some(names) = self
                .command_output_args
                .iter()
                .rev()
                .find(|args| args.first().is_some_and(|arg| arg == "clear"))
                .and_then(|args| args.get(2))
            {
                for name in names.split(',') {
                    entries.retain(|entry| entry["name"].as_str() != Some(name));
                    entries.push(serde_json::json!({"name": name, "displaying": {"color": "#0"}}));
                }
            }
            serde_json::json!({"awww-daemon": entries}).to_string()
        }
    }

    impl ProcessIo for FakeRuntime {
        fn awww_environment(
            &mut self,
            _inspect_daemons: bool,
        ) -> Result<wc_backend::runtime::AwwwEnvironment, WcError> {
            Ok(wc_backend::runtime::AwwwEnvironment {
                niri_socket_present: true,
                version: Some("awww 0.12.0".into()),
                daemon_commands: Vec::new(),
            })
        }

        fn awww_query_json(&mut self) -> Result<String, WcError> {
            Ok(self.awww_query_payload())
        }

        fn renderer_command_lines(
            &mut self,
        ) -> Result<Vec<wc_backend::runtime_observation::ProcessCommandLine>, WcError> {
            if let Some(message) = &self.process_scan_error {
                return Err(WcError::Other(message.clone()));
            }
            let mut lines = Vec::new();
            if !self.awww_displayed.is_empty() {
                lines.push(wc_backend::runtime_observation::ProcessCommandLine {
                    pid: 50,
                    argv: vec![
                        "awww-daemon".into(),
                        "--no-cache".into(),
                        "--format".into(),
                        "argb".into(),
                    ],
                });
            }
            for process in &self.mpvpaper_process_table {
                let selector = match &process.selector {
                    wc_backend::MpvpaperOutputSelector::Single(output) => output.clone(),
                    wc_backend::MpvpaperOutputSelector::Wildcard => "*".to_string(),
                    wc_backend::MpvpaperOutputSelector::Multi(outputs) => outputs.join(" "),
                    wc_backend::MpvpaperOutputSelector::Unparseable => String::new(),
                };
                lines.push(wc_backend::runtime_observation::ProcessCommandLine {
                    pid: process.pid,
                    argv: vec![
                        "mpvpaper".into(),
                        selector,
                        "--".into(),
                        process.path.clone(),
                    ],
                });
            }
            for pid in &self.running_mpvpaper_pids {
                if !self
                    .mpvpaper_process_table
                    .iter()
                    .any(|process| process.pid == *pid)
                {
                    // A pid without cmdline evidence reads as ambiguous.
                    lines.push(wc_backend::runtime_observation::ProcessCommandLine {
                        pid: *pid,
                        argv: vec!["mpvpaper".into()],
                    });
                }
            }
            if !self.lwe_outputs.is_empty() {
                let mut argv = vec!["linux-wallpaperengine".to_string()];
                for output in &self.lwe_outputs {
                    argv.extend([
                        "--screen-root".to_string(),
                        output.clone(),
                        "--bg".to_string(),
                        "wc-lwe".to_string(),
                    ]);
                }
                lines.push(wc_backend::runtime_observation::ProcessCommandLine { pid: 900, argv });
            }
            for (index, argv) in self.extra_command_lines.iter().enumerate() {
                lines.push(wc_backend::runtime_observation::ProcessCommandLine {
                    pid: 1000 + index as u32,
                    argv: argv.clone(),
                });
            }
            Ok(lines)
        }

        fn command_output(
            &mut self,
            command: &mut Command,
        ) -> Result<std::process::Output, WcError> {
            self.command_output_args.push(
                command
                    .get_args()
                    .map(|a| a.to_string_lossy().to_string())
                    .collect(),
            );
            let program = if self.command_output_success {
                "true"
            } else {
                "false"
            };
            Command::new(program)
                .output()
                .map_err(|e| WcError::Other(format!("fake command failed: {e}")))
        }

        fn command_status(
            &mut self,
            command: &mut Command,
        ) -> Result<std::process::ExitStatus, WcError> {
            self.command_status_args.push(
                command
                    .get_args()
                    .map(|a| a.to_string_lossy().to_string())
                    .collect(),
            );
            if self
                .command_status_args
                .last()
                .is_some_and(|args| args.iter().any(|arg| arg == "awww-daemon"))
            {
                // A spawned daemon serves the socket again.
                self.awww_stopped = false;
            }
            if let Some(limit) = self.fail_after_n_status {
                if self.command_status_args.len() > limit {
                    self.command_status_success = false;
                }
            }
            let program = if self.command_status_success {
                "true"
            } else {
                "false"
            };
            Command::new(program)
                .status()
                .map_err(|e| WcError::Other(format!("fake command failed: {e}")))
        }

        fn mpvpaper_pids(&mut self) -> Result<Vec<u32>, WcError> {
            if let Some(message) = &self.mpvpaper_pids_error {
                return Err(WcError::Other(message.clone()));
            }
            Ok(self.all_mpvpaper_pids())
        }

        fn swaybg_pids(&mut self) -> Result<Vec<u32>, WcError> {
            Ok(Vec::new())
        }

        fn mpvpaper_processes(&mut self) -> Result<Vec<MpvpaperProcess>, WcError> {
            if let Some(message) = &self.mpvpaper_processes_error {
                return Err(WcError::Other(message.clone()));
            }
            Ok(self.mpvpaper_process_table.clone())
        }

        fn wait_for_mpvpaper_ready(
            &mut self,
            _previous_pids: &[u32],
            _output: &str,
            _path: &str,
        ) -> Result<u32, WcError> {
            if let Some(message) = &self.mpvpaper_readiness_error {
                return Err(WcError::Other(message.clone()));
            }
            Ok(self.mpvpaper_ready_pid.unwrap_or(7))
        }

        fn mpvpaper_pid_running(&mut self, _pid: u32) -> Result<bool, WcError> {
            Ok(true)
        }

        fn cleanup_failed_mpvpaper_launch(
            &mut self,
            previous_pids: &[u32],
            _output: &str,
            _path: &str,
        ) -> Result<(), WcError> {
            self.failed_mpvpaper_launch_cleanup_count += 1;
            if let Some(message) = &self.failed_mpvpaper_launch_cleanup_error {
                return Err(WcError::Other(message.clone()));
            }
            self.running_mpvpaper_pids
                .retain(|pid| previous_pids.contains(pid));
            self.mpvpaper_process_table
                .retain(|process| previous_pids.contains(&process.pid));
            Ok(())
        }

        fn awww_socket_ready(&mut self) -> AwwwReadiness {
            if self.awww_stop_verify_pending {
                self.awww_stop_verify_pending = false;
                return if self.stop_awww_error.is_some() {
                    AwwwReadiness::Ready
                } else {
                    AwwwReadiness::SocketMissing
                };
            }
            if self.awww_stopped {
                // A verified stop keeps the socket absent until a daemon spawns.
                return AwwwReadiness::SocketMissing;
            }
            let mut seq = self.awww_readiness_sequence.borrow_mut();
            if seq.len() > 1 {
                seq.remove(0)
            } else if !seq.is_empty() {
                seq[0].clone()
            } else {
                AwwwReadiness::Ready
            }
        }
    }

    impl BackendRuntime for FakeRuntime {
        fn stop_awww(&mut self) {
            self.stop_awww_count += 1;
            self.awww_stop_verify_pending = true;
            if self.stop_awww_error.is_none() {
                // A stopped daemon no longer displays any surface.
                self.awww_stopped = true;
                self.awww_displayed.clear();
            }
        }

        fn stop_mpvpaper(&mut self) {
            self.stop_mpvpaper_count += 1;
            if let Some(message) = &self.stop_mpvpaper_error {
                self.mpvpaper_pids_error = Some(message.clone());
            } else {
                self.running_mpvpaper_pids.clear();
                self.mpvpaper_process_table.clear();
            }
        }

        fn stop_mpvpaper_outputs(&mut self, outputs: &[String]) -> Result<(), WcError> {
            self.stop_mpvpaper_outputs_calls.push(outputs.to_vec());
            let mut removed = Vec::new();
            self.mpvpaper_process_table.retain(|process| {
                let remove = process.matches_stop_outputs(outputs);
                if remove {
                    removed.push(process.pid);
                }
                !remove
            });
            self.running_mpvpaper_pids
                .retain(|pid| !removed.contains(pid));
            Ok(())
        }

        fn stop_lwe(&mut self, _s: Option<&wc_storage::StorageApi>) {
            self.stop_lwe_count += 1;
            self.lwe_outputs.clear();
        }

        fn stop_lwe_outputs(&mut self, outputs: &[String]) -> Result<(), wc_core::error::WcError> {
            self.stop_lwe_outputs_calls.push(outputs.to_vec());
            self.lwe_outputs.retain(|output| !outputs.contains(output));
            self.extra_command_lines.retain(|argv| {
                !(argv
                    .first()
                    .is_some_and(|arg| arg == "linux-wallpaperengine")
                    && argv.get(2).is_some_and(|output| outputs.contains(output)))
            });
            Ok(())
        }

        fn apply_lwe_to_outputs(
            &mut self,
            _s: &wc_storage::StorageApi,
            project: &wc_backend::linux_wallpaperengine::LinuxWallpaperEngineProject,
            outputs: &[String],
        ) -> Result<(), WcError> {
            self.lwe_apply_calls += 1;
            if let Some(message) = &self.lwe_apply_error {
                return Err(WcError::Other(message.clone()));
            }
            for output in outputs {
                self.extra_command_lines.push(vec![
                    "linux-wallpaperengine".into(),
                    "--screen-root".into(),
                    output.clone(),
                    "--bg".into(),
                    project
                        .workshop_id
                        .clone()
                        .unwrap_or_else(|| project.project_path.clone()),
                ]);
            }
            Ok(())
        }
    }

    struct CapturingReporter {
        stages: Vec<ApplyStage>,
    }

    impl ApplyStageReporter for CapturingReporter {
        fn emit(&mut self, event: ApplyStageEvent) {
            self.stages.push(event.stage);
        }
    }

    fn temp_service() -> (tempfile::TempDir, AppService) {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("config"),
        };
        cd.init().unwrap();
        wc_config::write_config_value(&cd.path, "storage_backend", "sqlite").unwrap();
        (tmp, AppService::from_config_dir(cd))
    }

    fn write_image(root: &Path, name: &str) -> std::path::PathBuf {
        let path = root.join(name);
        std::fs::write(&path, b"img").unwrap();
        path
    }

    fn write_video(root: &Path, name: &str) -> std::path::PathBuf {
        let path = root.join(name);
        std::fs::write(&path, b"vid").unwrap();
        path
    }

    #[test]
    fn lwe_dual_scene_replace_and_failure_preserve_sibling_process_and_state() {
        for fail in [false, true] {
            let (tmp, service) = temp_service();
            let scene = write_scene_with_preview(tmp.path(), "new-scene");
            service
                .storage_for_tests()
                .display_state_replace_all(&[
                    (
                        DisplayStateTarget::Output("DP-8".into()),
                        "/sibling-scene".into(),
                        "linux-wallpaperengine".into(),
                    ),
                    (
                        DisplayStateTarget::Output("eDP-1".into()),
                        "/old-scene".into(),
                        "linux-wallpaperengine".into(),
                    ),
                ])
                .unwrap();
            let sibling = vec![
                "linux-wallpaperengine",
                "--screen-root",
                "DP-8",
                "--bg",
                "123",
            ]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
            let mut rt = FakeRuntime {
                lwe_outputs: vec!["eDP-1".into()],
                extra_command_lines: vec![sibling.clone()],
                lwe_apply_error: fail.then(|| "injected renderer failure".into()),
                ..Default::default()
            };
            let result = service.apply_to_display_with_runtime(
                &scene.to_string_lossy(),
                DisplayTarget::Output("eDP-1".into()),
                &["DP-8".into(), "eDP-1".into()],
                &mut rt,
                &mut NoopReporter,
                DisplayApplyRuntimeOpts::default(),
            );
            if fail {
                assert_eq!(result.unwrap_err().code, "display_apply_failed_after_stop");
            } else {
                result.unwrap();
            }
            assert_eq!(rt.stop_lwe_count, 0);
            assert_eq!(rt.stop_lwe_outputs_calls, vec![vec!["eDP-1".to_string()]]);
            assert_eq!(rt.extra_command_lines[0], sibling);
            let rows = service.storage_for_tests().display_state_list().unwrap();
            assert!(rows.iter().any(
                |row| row.target == DisplayStateTarget::Output("DP-8".into())
                    && row.wallpaper_path == "/sibling-scene"
            ));
            assert_eq!(
                rows.iter()
                    .any(|row| row.target == DisplayStateTarget::Output("eDP-1".into())),
                !fail
            );
        }
    }

    #[test]
    fn lwe_shared_scene_replacement_is_refused_before_stop_or_apply() {
        let (tmp, service) = temp_service();
        let scene = write_scene_with_preview(tmp.path(), "new-scene");
        let mut rt = FakeRuntime {
            lwe_outputs: vec!["DP-8".into(), "eDP-1".into()],
            ..Default::default()
        };
        let error = service
            .apply_to_display_with_runtime(
                &scene.to_string_lossy(),
                DisplayTarget::Output("eDP-1".into()),
                &["DP-8".into(), "eDP-1".into()],
                &mut rt,
                &mut NoopReporter,
                DisplayApplyRuntimeOpts::default(),
            )
            .unwrap_err();
        assert!(
            error.message.contains("non-target display DP-8"),
            "{error:?}"
        );
        assert_eq!(rt.stop_lwe_count, 0);
        assert!(rt.stop_lwe_outputs_calls.is_empty());
        assert_eq!(rt.lwe_apply_calls, 0);
    }

    fn write_scene_with_preview(root: &Path, name: &str) -> std::path::PathBuf {
        let path = root.join(name);
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("scene.pkg"), b"scene").unwrap();
        std::fs::write(path.join("preview.gif"), b"gif").unwrap();
        std::fs::write(
            path.join("project.json"),
            r#"{"type":"scene","file":"scene.pkg","preview":"preview.gif","workshopid":"42"}"#,
        )
        .unwrap();
        path
    }

    #[test]
    fn targeted_preview_executes_and_persists_preview_state_path() {
        let (tmp, service) = temp_service();
        let scene = write_scene_with_preview(tmp.path(), "scene-preview");
        let request = crate::ApplyRequest {
            kind: crate::ApplyRequestKind::ApplyPreview,
            path: scene.to_string_lossy().to_string(),
            request_id: Some("preview-targeted".into()),
        };
        let mut rt = FakeRuntime {
            command_output_success: true,
            ..Default::default()
        };
        let mut reporter = NoopReporter;

        let result = service
            .execute_apply_request_to_display_with_runtime(
                request,
                DisplayTarget::Output("eDP-1".into()),
                &["eDP-1".into(), "HDMI-A-1".into()],
                &mut rt,
                &mut reporter,
                DisplayApplyRuntimeOpts::default(),
            )
            .unwrap();

        assert!(result.applied_path.ends_with("preview.gif"));
        assert_eq!(result.state_path, result.applied_path);
        assert!(result.preview);
        assert_eq!(result.file_type, wc_core::types::FileType::Gif);
        assert_eq!(result.applied_outputs, ["eDP-1"]);
        let args = &rt.command_output_args[0];
        assert!(args.iter().any(|arg| arg.ends_with("preview.gif")));
        let output_flag = args.iter().position(|arg| arg == "--outputs").unwrap();
        assert_eq!(args[output_flag + 1], "eDP-1");

        let rows = service.storage_for_tests().display_state_list().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].target, DisplayStateTarget::Output("eDP-1".into()));
        assert_eq!(rows[0].wallpaper_path, result.state_path);
        assert_eq!(rows[0].backend, "awww");
    }

    #[test]
    fn all_displays_result_reports_planned_output_snapshot() {
        let (tmp, service) = temp_service();
        let image = write_image(tmp.path(), "snapshot.jpg");
        let request = crate::ApplyRequest {
            kind: crate::ApplyRequestKind::Apply,
            path: image.to_string_lossy().to_string(),
            request_id: Some("all-snapshot".into()),
        };
        let mut rt = FakeRuntime {
            command_output_success: true,
            ..Default::default()
        };
        let mut reporter = NoopReporter;

        let result = service
            .execute_apply_request_to_display_with_runtime(
                request,
                DisplayTarget::AllDisplays,
                &["eDP-1".into(), "HDMI-A-1".into()],
                &mut rt,
                &mut reporter,
                DisplayApplyRuntimeOpts::default(),
            )
            .unwrap();

        assert_eq!(result.applied_outputs, ["eDP-1", "HDMI-A-1"]);
    }

    #[test]
    fn only_successful_scene_retry_selects_compat_failure_for_clearing() {
        let retry_target = crate::apply_execution::ApplyExecutionTarget {
            input_path: "/scene".into(),
            resolved_path: "/scene".into(),
            state_path: "/scene".into(),
            file_type: wc_core::types::FileType::WeScene,
            backend: Backend::LinuxWallpaperEngine,
            preview: false,
            fallback_path: None,
        };
        let preview_target = crate::apply_execution::ApplyExecutionTarget {
            file_type: wc_core::types::FileType::Gif,
            preview: true,
            ..retry_target.clone()
        };

        assert_eq!(
            compat_failure_path_after_success(
                &crate::ApplyRequestKind::RetryBackendApply,
                &retry_target,
            ),
            Some("/scene")
        );
        assert_eq!(
            compat_failure_path_after_success(
                &crate::ApplyRequestKind::ApplyPreview,
                &preview_target,
            ),
            None
        );
    }

    #[test]
    fn compat_failure_preserves_original_renderer_classification() {
        let scene_target = crate::apply_execution::ApplyExecutionTarget {
            input_path: "/scene".into(),
            resolved_path: "/scene".into(),
            state_path: "/scene".into(),
            file_type: wc_core::types::FileType::WeScene,
            backend: Backend::LinuxWallpaperEngine,
            preview: false,
            fallback_path: None,
        };
        let stop_error = wc_core::error::WcError::Other("old backend stop failed".into());
        let renderer_error = wc_core::error::WcError::LinuxWallpaperEngine {
            kind: wc_core::error::BackendErrorKind::RendererLimitation,
            detail: "renderer failed".into(),
        };

        assert!(compat_failure_error(&scene_target, &stop_error).is_none());
        let compat_error = compat_failure_error(&scene_target, &renderer_error).unwrap();
        assert_eq!(compat_error.code, "renderer_limitation");
    }

    #[test]
    fn all_displays_awww_success_commits_all_displays_row_and_legacy() {
        let (tmp, service) = temp_service();
        let img = write_image(tmp.path(), "wall.jpg");
        let mut rt = FakeRuntime {
            command_output_success: true,
            ..Default::default()
        };
        let mut reporter = NoopReporter;

        service
            .apply_to_display_with_runtime(
                &img.to_string_lossy(),
                DisplayTarget::AllDisplays,
                &["eDP-1".into()],
                &mut rt,
                &mut reporter,
                DisplayApplyRuntimeOpts {
                    request_id: Some("req-1".into()),
                    ..Default::default()
                },
            )
            .unwrap();

        let rows = service.storage_for_tests().display_state_list().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].target, DisplayStateTarget::AllDisplays);
        assert_eq!(rows[0].wallpaper_path, img.to_string_lossy());
        assert_eq!(rows[0].backend, "awww");
        assert!(
            !rt.command_output_args[0].iter().any(|a| a == "--outputs"),
            "AllDisplays must omit --outputs"
        );
        // Legacy keys updated atomically with All Displays.
        let current = service
            .storage_for_tests()
            .current_read()
            .unwrap()
            .unwrap_or_default();
        assert_eq!(current, img.to_string_lossy());
    }

    #[test]
    fn named_output_awww_success_expands_prior_all_displays_without_legacy_write() {
        let (tmp, service) = temp_service();
        let old = write_image(tmp.path(), "old.jpg");
        let next = write_image(tmp.path(), "next.jpg");
        service
            .storage_for_tests()
            .display_state_upsert(
                &DisplayStateTarget::AllDisplays,
                &old.to_string_lossy(),
                "awww",
            )
            .unwrap();
        service
            .storage_for_tests()
            .current_write("/walls/stale.jpg")
            .unwrap();

        let mut rt = FakeRuntime {
            command_output_success: true,
            ..Default::default()
        };
        let mut reporter = NoopReporter;
        service
            .apply_to_display_with_runtime(
                &next.to_string_lossy(),
                DisplayTarget::Output("eDP-1".into()),
                &["eDP-1".into(), "HDMI-1".into()],
                &mut rt,
                &mut reporter,
                DisplayApplyRuntimeOpts::default(),
            )
            .unwrap();

        let rows = service.storage_for_tests().display_state_list().unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows
            .iter()
            .all(|r| !matches!(r.target, DisplayStateTarget::AllDisplays)));
        let edp = rows
            .iter()
            .find(|r| r.target == DisplayStateTarget::Output("eDP-1".into()))
            .unwrap();
        assert_eq!(edp.wallpaper_path, next.to_string_lossy());
        let hdmi = rows
            .iter()
            .find(|r| r.target == DisplayStateTarget::Output("HDMI-1".into()))
            .unwrap();
        assert_eq!(hdmi.wallpaper_path, old.to_string_lossy());

        let args = &rt.command_output_args[0];
        let idx = args.iter().position(|a| a == "--outputs").unwrap();
        assert_eq!(args[idx + 1], "eDP-1");

        // Named apply must not best-effort overwrite legacy current.
        let current = service
            .storage_for_tests()
            .current_read()
            .unwrap()
            .unwrap_or_default();
        assert_eq!(current, "/walls/stale.jpg");
    }

    #[test]
    fn named_commit_failure_reconciles_live_apply_and_preserves_other_outputs() {
        let (tmp, service) = temp_service();
        let old = write_image(tmp.path(), "old.jpg");
        let next = write_image(tmp.path(), "next.jpg");
        service
            .storage_for_tests()
            .display_state_replace_all(&[
                (
                    DisplayStateTarget::AllDisplays,
                    old.to_string_lossy().to_string(),
                    "awww".into(),
                ),
                (
                    DisplayStateTarget::Output("DP-ghost".into()),
                    "/walls/ghost.jpg".into(),
                    "awww".into(),
                ),
            ])
            .unwrap();
        let mut rt = FakeRuntime {
            command_output_success: true,
            ..Default::default()
        };
        let mut reporter = NoopReporter;
        let mut fail_once = || Err(WcError::Other("injected commit failure".into()));
        let err = service
            .apply_to_display_with_runtime_and_commit_seam(
                &next.to_string_lossy(),
                DisplayTarget::Output("eDP-1".into()),
                &["eDP-1".into(), "HDMI-1".into()],
                &mut rt,
                &mut reporter,
                DisplayApplyRuntimeOpts::default(),
                Some(&mut fail_once),
            )
            .unwrap_err();
        assert_eq!(err.code, "display_state_commit_failed");

        let rows = service.storage_for_tests().display_state_list().unwrap();
        let path_for = |name: &str| {
            rows.iter()
                .find(|row| row.target == DisplayStateTarget::Output(name.into()))
                .map(|row| row.wallpaper_path.as_str())
        };
        assert_eq!(path_for("eDP-1"), Some(next.to_string_lossy().as_ref()));
        assert_eq!(path_for("HDMI-1"), Some(old.to_string_lossy().as_ref()));
        assert_eq!(path_for("DP-ghost"), Some("/walls/ghost.jpg"));
    }

    #[test]
    fn command_failure_without_stop_preserves_prior_display_state() {
        let (tmp, service) = temp_service();
        let old = write_image(tmp.path(), "old.jpg");
        let next = write_image(tmp.path(), "next.jpg");
        service
            .storage_for_tests()
            .display_state_upsert(
                &DisplayStateTarget::AllDisplays,
                &old.to_string_lossy(),
                "awww",
            )
            .unwrap();

        let mut rt = FakeRuntime {
            command_output_success: false,
            ..Default::default()
        };
        let mut reporter = NoopReporter;
        let err = service
            .apply_to_display_with_runtime(
                &next.to_string_lossy(),
                DisplayTarget::AllDisplays,
                &["eDP-1".into()],
                &mut rt,
                &mut reporter,
                DisplayApplyRuntimeOpts::default(),
            )
            .unwrap_err();
        assert!(err.message.contains("awww") || err.code.contains("fail"));

        let rows = service.storage_for_tests().display_state_list().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].target, DisplayStateTarget::AllDisplays);
        assert_eq!(rows[0].wallpaper_path, old.to_string_lossy());
    }

    #[test]
    fn stop_success_apply_failure_reconciles_stopped_backend_out_of_state() {
        let (tmp, service) = temp_service();
        let img = write_image(tmp.path(), "still.jpg");
        service
            .storage_for_tests()
            .display_state_upsert(
                &DisplayStateTarget::Output("eDP-1".into()),
                "/walls/old.mp4",
                "mpvpaper",
            )
            .unwrap();

        let mut rt = FakeRuntime {
            command_output_success: false,
            mpvpaper_process_table: vec![MpvpaperProcess::for_output(7, "eDP-1", "/walls/old.mp4")],
            ..Default::default()
        };
        let mut reporter = NoopReporter;
        let err = service
            .apply_to_display_with_runtime(
                &img.to_string_lossy(),
                DisplayTarget::Output("eDP-1".into()),
                &["eDP-1".into()],
                &mut rt,
                &mut reporter,
                DisplayApplyRuntimeOpts::default(),
            )
            .unwrap_err();
        assert_eq!(err.code, "display_apply_failed_after_stop");
        assert_eq!(rt.stop_mpvpaper_count, 1);

        let rows = service.storage_for_tests().display_state_list().unwrap();
        assert!(
            rows.iter().all(|r| r.backend != "mpvpaper"),
            "must not claim stopped mpvpaper still runs: {rows:?}"
        );
    }

    #[test]
    fn reconcile_from_report_materializes_surviving_apply_and_preserves_unrelated() {
        use wc_backend::display_executor::{CompletedApply, CompletedStop, DisplayExecReport};

        let previous = vec![
            DisplayStateRow {
                target: DisplayStateTarget::AllDisplays,
                wallpaper_path: "/walls/old.jpg".into(),
                backend: "awww".into(),
                updated_at: "t".into(),
            },
            DisplayStateRow {
                target: DisplayStateTarget::Output("DP-ghost".into()),
                wallpaper_path: "/walls/ghost.mp4".into(),
                backend: "mpvpaper".into(),
                updated_at: "t".into(),
            },
            DisplayStateRow {
                target: DisplayStateTarget::Output("HDMI-1".into()),
                wallpaper_path: "/walls/still.jpg".into(),
                backend: "awww".into(),
                updated_at: "t".into(),
            },
        ];
        let stop = CompletedStop {
            backend: Backend::Awww,
            scope: ExecutionScope::AllDisplays,
            destructive: true,
        };
        let apply = CompletedApply {
            backend: Backend::Mpvpaper,
            scope: ExecutionScope::named(vec!["eDP-1".into()]).unwrap(),
            path: "/walls/clip.mp4".into(),
        };
        let report = DisplayExecReport {
            events: vec![
                CompletedEvent::Stop(stop.clone()),
                CompletedEvent::Apply(apply.clone()),
            ],
        };

        let reconciled = reconcile_display_state_from_report(
            &previous,
            &report,
            &["eDP-1".into(), "HDMI-1".into()],
        );
        assert!(
            reconciled
                .iter()
                .all(|(t, _, b)| !matches!(t, DisplayStateTarget::AllDisplays) && b != "awww"),
            "destructive awww stop must clear awww claims: {reconciled:?}"
        );
        assert!(
            reconciled.iter().any(|(t, p, b)| {
                *t == DisplayStateTarget::Output("eDP-1".into())
                    && p == "/walls/clip.mp4"
                    && b == "mpvpaper"
            }),
            "surviving completed apply must be materialized: {reconciled:?}"
        );
        assert!(
            reconciled.iter().any(|(t, p, b)| {
                *t == DisplayStateTarget::Output("DP-ghost".into())
                    && p == "/walls/ghost.mp4"
                    && b == "mpvpaper"
            }),
            "unaffected disconnected row must be preserved: {reconciled:?}"
        );
        assert!(
            !reconciled
                .iter()
                .any(|(t, _, _)| *t == DisplayStateTarget::Output("HDMI-1".into())),
            "HDMI-1 awww assignment was destroyed by the stop and never re-applied: {reconciled:?}"
        );

        let cleanup = CompletedStop {
            backend: Backend::Mpvpaper,
            scope: ExecutionScope::AllDisplays,
            destructive: true,
        };
        let ordered_cleanup_report = DisplayExecReport {
            events: vec![
                CompletedEvent::Apply(apply.clone()),
                CompletedEvent::Stop(cleanup.clone()),
            ],
        };
        let after_cleanup = reconcile_display_state_from_report(
            &previous,
            &ordered_cleanup_report,
            &["eDP-1".into(), "HDMI-1".into()],
        );
        assert!(!after_cleanup.iter().any(|(target, _, backend)| {
            target == &DisplayStateTarget::Output("eDP-1".into()) && backend == "mpvpaper"
        }));
        assert!(after_cleanup.iter().any(|(target, _, backend)| {
            target == &DisplayStateTarget::Output("DP-ghost".into()) && backend == "mpvpaper"
        }));
    }

    #[test]
    fn destructive_progress_with_reconcile_commit_failure_is_uncertain() {
        let (tmp, service) = temp_service();
        let image = write_image(tmp.path(), "next.jpg");
        service
            .storage_for_tests()
            .display_state_upsert(
                &DisplayStateTarget::Output("eDP-1".into()),
                "/walls/old.mp4",
                "mpvpaper",
            )
            .unwrap();
        let mut runtime = FakeRuntime {
            // The saved mpvpaper assignment is only a conflict input when the
            // renderer verifiably still runs.
            mpvpaper_process_table: vec![MpvpaperProcess::for_output(5, "eDP-1", "/walls/old.mp4")],
            ..Default::default()
        };
        let mut reporter = NoopReporter;
        let mut fail_reconcile = || Err(WcError::Other("reconcile commit failed".into()));
        let err = service
            .apply_to_display_with_runtime_and_commit_seam(
                &image.to_string_lossy(),
                DisplayTarget::Output("eDP-1".into()),
                &["eDP-1".into()],
                &mut runtime,
                &mut reporter,
                DisplayApplyRuntimeOpts::default(),
                Some(&mut fail_reconcile),
            )
            .unwrap_err();
        assert_eq!(err.code, "display_state_uncertain");
        let detail = err.detail.unwrap_or_default();
        assert!(detail.contains("awww") || detail.contains("apply"));
        assert!(detail.contains("reconcile commit failed"));
    }

    #[test]
    fn stop_verification_failure_is_uncertain_and_preserves_disconnected_restore_state() {
        let (tmp, service) = temp_service();
        let image = write_image(tmp.path(), "next.jpg");
        service
            .storage_for_tests()
            .display_state_replace_all(&[
                (
                    DisplayStateTarget::Output("eDP-1".into()),
                    "/walls/live.mp4".into(),
                    "mpvpaper".into(),
                ),
                (
                    DisplayStateTarget::Output("DP-ghost".into()),
                    "/walls/restore.mp4".into(),
                    "mpvpaper".into(),
                ),
            ])
            .unwrap();
        service
            .storage_for_tests()
            .current_write("/walls/stale-runtime.mp4")
            .unwrap();
        let mut runtime = FakeRuntime {
            command_output_success: true,
            stop_mpvpaper_error: Some("verification probe failed".into()),
            mpvpaper_process_table: vec![MpvpaperProcess::for_output(
                7,
                "eDP-1",
                "/walls/live.mp4",
            )],
            ..Default::default()
        };
        let mut reporter = NoopReporter;
        let err = service
            .apply_to_display_with_runtime(
                &image.to_string_lossy(),
                DisplayTarget::Output("eDP-1".into()),
                &["eDP-1".into()],
                &mut runtime,
                &mut reporter,
                DisplayApplyRuntimeOpts::default(),
            )
            .unwrap_err();
        assert_eq!(err.code, "display_state_uncertain");
        let rows = service.storage_for_tests().display_state_list().unwrap();
        assert!(!rows.iter().any(|row| {
            row.target == DisplayStateTarget::Output("eDP-1".into()) && row.backend == "mpvpaper"
        }));
        assert!(rows.iter().any(|row| {
            row.target == DisplayStateTarget::Output("DP-ghost".into()) && row.backend == "mpvpaper"
        }));
        assert_eq!(service.storage_for_tests().current_read().unwrap(), None);
    }

    #[test]
    fn uncertain_target_cleanup_invalidates_legacy_runtime_evidence() {
        let (_tmp, service) = temp_service();
        service
            .storage_for_tests()
            .current_write("/walls/stale-runtime.mp4")
            .unwrap();

        let error = service
            .handle_exec_failure(
                DisplayExecFailure {
                    report: DisplayExecReport::default(),
                    error: WcError::Other("target cleanup probe failed".into()),
                    uncertain_stop: None,
                    cleanup_uncertain: true,
                },
                &[],
                &["eDP-1".into()],
                None,
            )
            .unwrap();

        assert_eq!(error.code, "display_state_uncertain");
        assert_eq!(service.storage_for_tests().current_read().unwrap(), None);
    }

    #[test]
    fn named_apply_after_global_stop_ignores_stale_saved_conflict() {
        // Saved rows and live processes describe a scene on eDP-1 and a video
        // on DP-8. Exercise verified global Stop before a fresh named Apply.
        // The stale rows must not block a legal single-output apply, and the
        // eDP-1 scene preference stays readable for a later Restore.
        let (tmp, service) = temp_service();
        let next = write_video(tmp.path(), "next.mp4");
        service
            .storage_for_tests()
            .display_state_replace_all(&[
                (
                    DisplayStateTarget::Output("eDP-1".into()),
                    "/walls/scene".into(),
                    "linux-wallpaperengine".into(),
                ),
                (
                    DisplayStateTarget::Output("DP-8".into()),
                    "/walls/old.mp4".into(),
                    "mpvpaper".into(),
                ),
            ])
            .unwrap();

        let mut rt = FakeRuntime {
            command_status_success: true,
            mpvpaper_ready_pid: Some(77),
            lwe_outputs: vec!["eDP-1".into()],
            mpvpaper_process_table: vec![MpvpaperProcess::for_output(42, "DP-8", "/walls/old.mp4")],
            ..Default::default()
        };
        let before = service.storage_for_tests().display_state_list().unwrap();
        wc_backend::stop_all_backends_with_runtime(Some(service.storage_for_tests()), &mut rt)
            .unwrap();
        service.storage_for_tests().runtime_state_clear().unwrap();
        assert_eq!(
            service.storage_for_tests().display_state_list().unwrap(),
            before
        );
        let result = service
            .apply_to_display_with_runtime(
                &next.to_string_lossy(),
                DisplayTarget::Output("DP-8".into()),
                &["eDP-1".into(), "DP-8".into()],
                &mut rt,
                &mut NoopReporter,
                DisplayApplyRuntimeOpts::default(),
            )
            .expect("stopped renderers must not conflict as if still running");
        assert_eq!(result.backend, Backend::Mpvpaper);
        assert_eq!(rt.stop_lwe_count, 1);
        assert_eq!(rt.stop_mpvpaper_count, 1);
        assert_eq!(rt.stop_awww_count, 1);

        let rows = service.storage_for_tests().display_state_list().unwrap();
        assert!(
            rows.iter().any(|row| {
                row.target == DisplayStateTarget::Output("eDP-1".into())
                    && row.wallpaper_path == "/walls/scene"
                    && row.backend == "linux-wallpaperengine"
            }),
            "scene restore preference must survive: {rows:?}"
        );
        assert!(rows.iter().any(|row| {
            row.target == DisplayStateTarget::Output("DP-8".into())
                && row.wallpaper_path == next.to_string_lossy()
                && row.backend == "mpvpaper"
        }));
    }

    #[test]
    fn named_mpvpaper_replace_succeeds_beside_live_lwe_sibling() {
        // Was: ReliesOnUnknownCoexistence while LWE remained live on the sibling.
        // Now: verified LWE↔mpvpaper pair; replace only the target video.
        let (tmp, service) = temp_service();
        let next = write_video(tmp.path(), "next.mp4");
        service
            .storage_for_tests()
            .display_state_replace_all(&[
                (
                    DisplayStateTarget::Output("eDP-1".into()),
                    "/walls/scene".into(),
                    "linux-wallpaperengine".into(),
                ),
                (
                    DisplayStateTarget::Output("DP-8".into()),
                    "/walls/old.mp4".into(),
                    "mpvpaper".into(),
                ),
            ])
            .unwrap();

        let mut rt = FakeRuntime {
            command_status_success: true,
            lwe_outputs: vec!["eDP-1".into()],
            mpvpaper_process_table: vec![MpvpaperProcess::for_output(42, "DP-8", "/walls/old.mp4")],
            ..Default::default()
        };
        let applied = service
            .apply_to_display_with_runtime(
                &next.to_string_lossy(),
                DisplayTarget::Output("DP-8".into()),
                &["eDP-1".into(), "DP-8".into()],
                &mut rt,
                &mut NoopReporter,
                DisplayApplyRuntimeOpts::default(),
            )
            .expect("verified lwe/mpvpaper pair must allow sibling-preserving replace");
        assert_eq!(applied.backend, Backend::Mpvpaper);
        assert_eq!(rt.lwe_outputs, vec!["eDP-1".to_string()]);
        assert_eq!(rt.stop_lwe_count, 0);
        assert!(rt.stop_lwe_outputs_calls.is_empty());
        let rows = service.storage_for_tests().display_state_list().unwrap();
        assert!(rows.iter().any(|row| {
            row.target == DisplayStateTarget::Output("eDP-1".into())
                && row.wallpaper_path == "/walls/scene"
                && row.backend == "linux-wallpaperengine"
        }));
        assert!(rows.iter().any(|row| {
            row.target == DisplayStateTarget::Output("DP-8".into())
                && row.wallpaper_path == next.to_string_lossy()
                && row.backend == "mpvpaper"
        }));
    }

    #[test]
    fn named_apply_rejected_with_locatable_error_when_observation_fails() {
        let (tmp, service) = temp_service();
        let next = write_video(tmp.path(), "next.mp4");
        service
            .storage_for_tests()
            .display_state_upsert(
                &DisplayStateTarget::Output("DP-8".into()),
                "/walls/old.mp4",
                "mpvpaper",
            )
            .unwrap();
        let before = service.storage_for_tests().display_state_list().unwrap();

        let mut rt = FakeRuntime {
            process_scan_error: Some("/proc unreadable".into()),
            ..Default::default()
        };
        let error = service
            .apply_to_display_with_runtime(
                &next.to_string_lossy(),
                DisplayTarget::Output("DP-8".into()),
                &["eDP-1".into(), "DP-8".into()],
                &mut rt,
                &mut NoopReporter,
                DisplayApplyRuntimeOpts::default(),
            )
            .unwrap_err();
        assert_eq!(error.code, "display_observation_failed");
        assert!(
            error
                .detail
                .as_deref()
                .unwrap_or_default()
                .contains("/proc unreadable"),
            "observation failure must be locatable: {error:?}"
        );
        assert_eq!(rt.stop_mpvpaper_count, 0);
        assert!(rt.command_status_args.is_empty());
        assert_eq!(
            service.storage_for_tests().display_state_list().unwrap(),
            before,
            "a failed observation must not mutate state"
        );
    }

    #[test]
    fn external_renderer_without_saved_row_still_occupies_output() {
        // No saved rows at all, but an externally started swaybg owns DP-8:
        // applying to eDP-1 must still protect the non-target output.
        let (tmp, service) = temp_service();
        let img = write_image(tmp.path(), "still.jpg");

        let mut rt = FakeRuntime {
            command_output_success: true,
            extra_command_lines: vec![vec![
                "swaybg".into(),
                "-o".into(),
                "DP-8".into(),
                "-i".into(),
                "/walls/external.jpg".into(),
                "-m".into(),
                "fill".into(),
            ]],
            ..Default::default()
        };
        let error = service
            .apply_to_display_with_runtime(
                &img.to_string_lossy(),
                DisplayTarget::Output("eDP-1".into()),
                &["eDP-1".into(), "DP-8".into()],
                &mut rt,
                &mut NoopReporter,
                DisplayApplyRuntimeOpts::default(),
            )
            .unwrap_err();
        assert_eq!(error.code, "display_apply_rejected");
        assert!(
            error.message.contains("DP-8")
                || error.detail.as_deref().unwrap_or_default().contains("DP-8")
        );
        assert_eq!(rt.stop_awww_count, 0);
        assert!(rt.command_output_args.is_empty());
    }

    #[test]
    fn all_displays_after_stop_applies_without_retiring_dead_backends() {
        let (tmp, service) = temp_service();
        let img = write_image(tmp.path(), "wall.jpg");
        service
            .storage_for_tests()
            .display_state_replace_all(&[
                (
                    DisplayStateTarget::Output("eDP-1".into()),
                    "/walls/scene".into(),
                    "linux-wallpaperengine".into(),
                ),
                (
                    DisplayStateTarget::Output("DP-8".into()),
                    "/walls/old.mp4".into(),
                    "mpvpaper".into(),
                ),
            ])
            .unwrap();

        let mut rt = FakeRuntime {
            command_output_success: true,
            ..Default::default()
        };
        service
            .apply_to_display_with_runtime(
                &img.to_string_lossy(),
                DisplayTarget::AllDisplays,
                &["eDP-1".into(), "DP-8".into()],
                &mut rt,
                &mut NoopReporter,
                DisplayApplyRuntimeOpts::default(),
            )
            .expect("All Displays after a global stop must not stop dead backends");
        assert_eq!(rt.stop_lwe_count, 0);
        assert_eq!(rt.stop_mpvpaper_count, 0);
        assert_eq!(rt.stop_awww_count, 0);
        let rows = service.storage_for_tests().display_state_list().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].target, DisplayStateTarget::AllDisplays);
        assert_eq!(rows[0].backend, "awww");
    }

    #[test]
    fn all_displays_rejects_failed_process_enumeration_before_mutation() {
        let (tmp, service) = temp_service();
        let image = write_image(tmp.path(), "wall.jpg");
        let mut runtime = FakeRuntime {
            process_scan_error: Some("/proc unreadable".into()),
            command_output_success: true,
            ..Default::default()
        };
        let error = service
            .apply_to_display_with_runtime(
                &image.to_string_lossy(),
                DisplayTarget::AllDisplays,
                &["eDP-1".into(), "DP-8".into()],
                &mut runtime,
                &mut NoopReporter,
                DisplayApplyRuntimeOpts::default(),
            )
            .expect_err("unknown renderer set cannot be safely retired");
        assert_eq!(error.code, "display_observation_failed");
        assert!(error
            .detail
            .as_deref()
            .unwrap_or_default()
            .contains("/proc unreadable"));
        assert!(runtime.command_output_args.is_empty());
        assert!(runtime.command_status_args.is_empty());
        assert_eq!(runtime.stop_awww_count, 0);
        assert_eq!(runtime.stop_mpvpaper_count, 0);
        assert_eq!(runtime.stop_lwe_count, 0);
        assert!(service
            .storage_for_tests()
            .display_state_list()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn all_displays_retires_implicated_backend_with_ambiguous_ownership() {
        // An unparseable mpvpaper process makes per-output ownership unknown,
        // but an explicit All Displays image replace must still retire the
        // implicated backend (verified at execution) instead of launching
        // alongside it.
        let (tmp, service) = temp_service();
        let img = write_image(tmp.path(), "wall.jpg");
        let mut rt = FakeRuntime {
            command_output_success: true,
            mpvpaper_process_table: vec![MpvpaperProcess {
                pid: 9,
                selector: wc_backend::MpvpaperOutputSelector::Unparseable,
                path: "/walls/stray.mp4".into(),
            }],
            ..Default::default()
        };
        service
            .apply_to_display_with_runtime(
                &img.to_string_lossy(),
                DisplayTarget::AllDisplays,
                &["eDP-1".into(), "DP-8".into()],
                &mut rt,
                &mut NoopReporter,
                DisplayApplyRuntimeOpts::default(),
            )
            .expect("global retirement of an implicated backend is verifiable");
        assert_eq!(rt.stop_mpvpaper_count, 1);
        assert!(rt.mpvpaper_process_table.is_empty());
        assert!(!rt.command_output_args.is_empty(), "image applied");
    }

    #[test]
    fn all_displays_video_retires_ambiguous_same_backend_before_launch() {
        let (tmp, service) = temp_service();
        let video = write_video(tmp.path(), "next.mp4");
        let mut rt = FakeRuntime {
            command_status_success: true,
            mpvpaper_ready_pid: Some(70),
            mpvpaper_process_table: vec![MpvpaperProcess {
                pid: 9,
                selector: wc_backend::MpvpaperOutputSelector::Unparseable,
                path: "/walls/stray.mp4".into(),
            }],
            ..Default::default()
        };
        service
            .apply_to_display_with_runtime(
                &video.to_string_lossy(),
                DisplayTarget::AllDisplays,
                &["eDP-1".into(), "DP-8".into()],
                &mut rt,
                &mut NoopReporter,
                DisplayApplyRuntimeOpts::default(),
            )
            .expect("ambiguous mpvpaper must be retired before new launches");
        assert_eq!(rt.stop_mpvpaper_count, 1);
        assert_eq!(rt.command_status_args.len(), 2, "one launch per output");
    }

    #[test]
    fn uncertain_cleanup_without_uncertain_stop_still_drops_confirmed_stop() {
        let (_tmp, service) = temp_service();
        service
            .storage_for_tests()
            .display_state_replace_all(&[
                (
                    DisplayStateTarget::Output("eDP-1".into()),
                    "/walls/old.mp4".into(),
                    "mpvpaper".into(),
                ),
                (
                    DisplayStateTarget::Output("DP-8".into()),
                    "/walls/sibling.mp4".into(),
                    "mpvpaper".into(),
                ),
            ])
            .unwrap();

        let mut report = DisplayExecReport::default();
        report.record_stop(wc_backend::display_executor::CompletedStop {
            backend: Backend::Mpvpaper,
            scope: ExecutionScope::named(vec!["eDP-1".into()]).unwrap(),
            destructive: true,
        });
        let error = service
            .handle_exec_failure(
                DisplayExecFailure {
                    report,
                    error: WcError::Other("launch failed; cleanup unverifiable".into()),
                    uncertain_stop: None,
                    cleanup_uncertain: true,
                },
                &service.storage_for_tests().display_state_list().unwrap(),
                &["eDP-1".into(), "DP-8".into()],
                None,
            )
            .unwrap();

        assert_eq!(error.code, "display_state_uncertain");
        let rows = service.storage_for_tests().display_state_list().unwrap();
        assert!(
            !rows
                .iter()
                .any(|row| row.target == DisplayStateTarget::Output("eDP-1".into())),
            "confirmed-stopped old video must not remain recorded: {rows:?}"
        );
        assert!(rows.iter().any(|row| {
            row.target == DisplayStateTarget::Output("DP-8".into())
                && row.wallpaper_path == "/walls/sibling.mp4"
                && row.backend == "mpvpaper"
        }));
    }

    #[test]
    fn uncertain_cleanup_persist_failure_reports_both_causes() {
        let (_tmp, service) = temp_service();
        let previous = vec![DisplayStateRow {
            target: DisplayStateTarget::Output("eDP-1".into()),
            wallpaper_path: "/walls/old.mp4".into(),
            backend: "mpvpaper".into(),
            updated_at: "t".into(),
        }];
        let mut report = DisplayExecReport::default();
        report.record_stop(wc_backend::display_executor::CompletedStop {
            backend: Backend::Mpvpaper,
            scope: ExecutionScope::named(vec!["eDP-1".into()]).unwrap(),
            destructive: true,
        });
        let mut fail_reconcile = || Err(WcError::Other("reconcile commit failed".into()));
        let error = service
            .handle_exec_failure(
                DisplayExecFailure {
                    report,
                    error: WcError::Other("launch failed; cleanup unverifiable".into()),
                    uncertain_stop: None,
                    cleanup_uncertain: true,
                },
                &previous,
                &["eDP-1".into()],
                Some(&mut fail_reconcile),
            )
            .unwrap_err();

        assert_eq!(error.code, "display_state_uncertain");
        let detail = error.detail.unwrap_or_default();
        assert!(
            detail.contains("execution_error=") && detail.contains("reconciliation_error="),
            "both causes must be reported: {detail}"
        );
    }

    #[test]
    fn named_video_launch_failure_with_unverifiable_cleanup_drops_stopped_row() {
        // Full driver -> executor -> app chain: the old video on eDP-1 is
        // confirmed stopped, the new launch fails, and cleanup of the new
        // process cannot be verified. The stopped row must be dropped while
        // the untouched sibling keeps its path and process.
        let (tmp, service) = temp_service();
        let old = write_video(tmp.path(), "old.mp4");
        let next = write_video(tmp.path(), "next.mp4");
        service
            .storage_for_tests()
            .display_state_upsert(
                &DisplayStateTarget::AllDisplays,
                &old.to_string_lossy(),
                "mpvpaper",
            )
            .unwrap();

        let mut rt = FakeRuntime {
            command_status_success: false,
            failed_mpvpaper_launch_cleanup_error: Some("cleanup probe failed".into()),
            mpvpaper_process_table: vec![
                MpvpaperProcess::for_output(10, "eDP-1", old.to_string_lossy()),
                MpvpaperProcess::for_output(20, "HDMI-1", old.to_string_lossy()),
            ],
            ..Default::default()
        };
        let mut reporter = NoopReporter;
        let err = service
            .apply_to_display_with_runtime(
                &next.to_string_lossy(),
                DisplayTarget::Output("eDP-1".into()),
                &["eDP-1".into(), "HDMI-1".into()],
                &mut rt,
                &mut reporter,
                DisplayApplyRuntimeOpts::default(),
            )
            .unwrap_err();
        assert_eq!(err.code, "display_state_uncertain");
        assert_eq!(rt.failed_mpvpaper_launch_cleanup_count, 1);
        assert_eq!(rt.stop_mpvpaper_count, 0);
        assert_eq!(
            rt.stop_mpvpaper_outputs_calls,
            vec![vec!["eDP-1".to_string()]]
        );
        assert!(
            rt.mpvpaper_process_table
                .iter()
                .any(|process| process.pid == 20),
            "sibling process must be untouched: {:?}",
            rt.mpvpaper_process_table
        );

        let rows = service.storage_for_tests().display_state_list().unwrap();
        assert!(
            !rows
                .iter()
                .any(|row| matches!(row.target, DisplayStateTarget::AllDisplays)
                    || row.target == DisplayStateTarget::Output("eDP-1".into())),
            "confirmed-stopped eDP-1 video must not remain recorded: {rows:?}"
        );
        assert!(rows.iter().any(|row| {
            row.target == DisplayStateTarget::Output("HDMI-1".into())
                && row.wallpaper_path == old.to_string_lossy()
                && row.backend == "mpvpaper"
        }));
    }

    #[test]
    fn partial_multi_apply_failure_reconciles_after_stop() {
        use wc_backend::capability::{
            capability_for, CrossOutputCoexistence, Evidence, MultiInstanceSupport, StopScope,
        };

        let (tmp, service) = temp_service();
        let video = write_video(tmp.path(), "clip.mp4");
        service
            .storage_for_tests()
            .display_state_replace_all(&[
                (
                    DisplayStateTarget::AllDisplays,
                    "/walls/old.jpg".into(),
                    "awww".into(),
                ),
                (
                    DisplayStateTarget::Output("DP-ghost".into()),
                    "/walls/ghost.mp4".into(),
                    "mpvpaper".into(),
                ),
            ])
            .unwrap();

        let mut capability = capability_for(Backend::Mpvpaper).expect("mpvpaper");
        capability.multi_instance = MultiInstanceSupport::SeparateProcessesVerified;
        capability.multi_instance_evidence = Evidence::CliVerified;
        capability.stop_scope = StopScope::TrackedProcessPerOutput;
        capability.stop_scope_evidence = Evidence::CliVerified;
        capability.cross_output_coexistence = CrossOutputCoexistence::Verified;
        capability.cross_output_coexistence_evidence = Evidence::CliVerified;

        let mut rt = FakeRuntime {
            command_status_success: true,
            mpvpaper_ready_pid: Some(11),
            fail_after_n_status: Some(1),
            // The saved AllDisplays awww assignment is live on both outputs.
            awww_displayed: vec![
                ("eDP-1".into(), "/walls/old.jpg".into()),
                ("HDMI-1".into(), "/walls/old.jpg".into()),
            ],
            ..Default::default()
        };
        let mut reporter = NoopReporter;
        let err = service
            .apply_to_display_with_runtime(
                &video.to_string_lossy(),
                DisplayTarget::AllDisplays,
                &["eDP-1".into(), "HDMI-1".into()],
                &mut rt,
                &mut reporter,
                DisplayApplyRuntimeOpts {
                    capability: Some(capability),
                    ..Default::default()
                },
            )
            .unwrap_err();
        assert_eq!(err.code, "display_apply_failed_after_stop");
        assert_eq!(rt.stop_awww_count, 1);
        assert_eq!(
            rt.stop_mpvpaper_count, 0,
            "launch failure must not kill live first apply"
        );
        assert!(err.detail.unwrap().contains("successful_applies=1"));

        let rows = service.storage_for_tests().display_state_list().unwrap();
        assert!(
            rows.iter().all(|r| r.backend != "awww"),
            "stopped awww must not remain claimed: {rows:?}"
        );
        let live = rows
            .iter()
            .find(|r| r.target == DisplayStateTarget::Output("eDP-1".into()))
            .expect("successful first apply must remain persisted while live");
        assert_eq!(live.backend, "mpvpaper");
        assert_eq!(live.wallpaper_path, video.to_string_lossy());
        assert!(
            rows.iter().any(|r| {
                r.target == DisplayStateTarget::Output("DP-ghost".into()) && r.backend == "mpvpaper"
            }),
            "disconnected unrelated row must survive reconcile: {rows:?}"
        );
    }

    #[test]
    fn rejected_conflict_does_not_stop_or_mutate_state() {
        let (tmp, service) = temp_service();
        let img = write_image(tmp.path(), "a.jpg");
        service
            .storage_for_tests()
            .display_state_replace_all(&[
                (
                    DisplayStateTarget::Output("eDP-1".into()),
                    "/walls/a.jpg".into(),
                    "awww".into(),
                ),
                (
                    DisplayStateTarget::Output("HDMI-1".into()),
                    "/walls/b.jpg".into(),
                    "swaybg".into(),
                ),
            ])
            .unwrap();

        let before = service.storage_for_tests().display_state_list().unwrap();
        let mut rt = FakeRuntime {
            command_output_success: true,
            // Live evidence matching the saved rows: an awww surface on eDP-1
            // and a swaybg process owning HDMI-1.
            awww_displayed: vec![("eDP-1".into(), "/walls/a.jpg".into())],
            extra_command_lines: vec![vec![
                "swaybg".into(),
                "-o".into(),
                "HDMI-1".into(),
                "-i".into(),
                "/walls/b.jpg".into(),
                "-m".into(),
                "fill".into(),
            ]],
            ..Default::default()
        };
        let mut reporter = NoopReporter;
        let err = service
            .apply_to_display_with_runtime(
                &img.to_string_lossy(),
                DisplayTarget::Output("eDP-1".into()),
                &["eDP-1".into(), "HDMI-1".into()],
                &mut rt,
                &mut reporter,
                DisplayApplyRuntimeOpts::default(),
            )
            .unwrap_err();
        assert_eq!(err.code, "display_apply_rejected");
        assert_eq!(rt.stop_awww_count, 0);
        assert_eq!(rt.stop_mpvpaper_count, 0);
        assert!(rt.command_output_args.is_empty());
        let after = service.storage_for_tests().display_state_list().unwrap();
        assert_eq!(before, after);
    }

    #[test]
    fn apply_path_compat_writes_all_displays_after_legacy_success() {
        let (tmp, service) = temp_service();
        let img = write_image(tmp.path(), "legacy.jpg");

        service
            .storage_for_tests()
            .display_state_upsert(
                &DisplayStateTarget::Output("eDP-1".into()),
                "/walls/old.jpg",
                "awww",
            )
            .unwrap();

        commit_legacy_apply_display_state(&service, &img.to_string_lossy(), Backend::Awww).unwrap();

        let rows = service.storage_for_tests().display_state_list().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].target, DisplayStateTarget::AllDisplays);
        assert_eq!(rows[0].wallpaper_path, img.to_string_lossy());
        let running = running_from_display_state(&rows, &["eDP-1".into()]).unwrap();
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].backend, Backend::Awww);
    }

    #[test]
    fn legacy_commit_failure_records_live_all_displays_without_stale_named_override() {
        let (tmp, service) = temp_service();
        let img = write_image(tmp.path(), "legacy.jpg");
        // The renderer success path writes these legacy keys before the
        // display-state finalization step is attempted.
        service
            .storage_for_tests()
            .current_write(&img.to_string_lossy())
            .unwrap();
        service
            .storage_for_tests()
            .last_backend_write("awww")
            .unwrap();
        service
            .storage_for_tests()
            .display_state_upsert(
                &DisplayStateTarget::Output("eDP-1".into()),
                "/walls/old.jpg",
                "awww",
            )
            .unwrap();

        let mut seam = || Err(WcError::Other("injected commit failure".into()));
        let err = commit_legacy_apply_display_state_with_seam(
            &service,
            &img.to_string_lossy(),
            Backend::Awww,
            Some(&mut seam),
        )
        .unwrap_err();
        assert_eq!(err.code, "display_state_commit_failed");
        let rows = service.storage_for_tests().display_state_list().unwrap();
        assert!(rows.iter().any(|row| {
            row.target == DisplayStateTarget::AllDisplays
                && row.wallpaper_path == img.to_string_lossy()
                && row.backend == "awww"
        }));
        assert_eq!(rows.len(), 1);
        assert!(service
            .storage_for_tests()
            .current_read()
            .unwrap()
            .unwrap_or_default()
            .is_empty());
        assert!(service
            .storage_for_tests()
            .last_backend_read()
            .unwrap()
            .unwrap_or_default()
            .is_empty());
    }

    #[test]
    fn display_apply_emits_resolve_and_refresh_stages() {
        let (tmp, service) = temp_service();
        let img = write_image(tmp.path(), "wall.jpg");
        let mut rt = FakeRuntime {
            command_output_success: true,
            ..Default::default()
        };
        let mut reporter = CapturingReporter { stages: Vec::new() };
        service
            .apply_to_display_with_runtime(
                &img.to_string_lossy(),
                DisplayTarget::AllDisplays,
                &["eDP-1".into()],
                &mut rt,
                &mut reporter,
                DisplayApplyRuntimeOpts {
                    request_id: Some("stages".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(reporter.stages.contains(&ApplyStage::ResolveTarget));
        assert!(reporter.stages.contains(&ApplyStage::EnsureAwwwDaemon));
        assert!(reporter.stages.contains(&ApplyStage::RefreshStatus));
    }

    #[test]
    fn named_replace_lwe_on_dual_outputs_preserves_sibling_video() {
        for image in [false, true] {
            let (tmp, service) = temp_service();
            let next = if image {
                write_image(tmp.path(), "next.jpg")
            } else {
                write_video(tmp.path(), "next.mp4")
            };
            service
                .storage_for_tests()
                .display_state_replace_all(&[
                    (
                        DisplayStateTarget::Output("eDP-1".into()),
                        "/walls/scene".into(),
                        "linux-wallpaperengine".into(),
                    ),
                    (
                        DisplayStateTarget::Output("DP-8".into()),
                        "/walls/sibling.mp4".into(),
                        "mpvpaper".into(),
                    ),
                ])
                .unwrap();
            let mut rt = FakeRuntime {
                command_output_success: true,
                command_status_success: true,
                // Live truth matching the saved rows: an LWE process covering
                // eDP-1 and a sibling mpvpaper process on DP-8.
                lwe_outputs: vec!["eDP-1".into()],
                mpvpaper_process_table: vec![MpvpaperProcess::for_output(
                    42,
                    "DP-8",
                    "/walls/sibling.mp4",
                )],
                ..Default::default()
            };
            let result = service
                .apply_to_display_with_runtime(
                    &next.to_string_lossy(),
                    DisplayTarget::Output("eDP-1".into()),
                    &["DP-8".into(), "eDP-1".into()],
                    &mut rt,
                    &mut NoopReporter,
                    DisplayApplyRuntimeOpts::default(),
                )
                .expect("a scene owned only by eDP-1 can be replaced without stopping DP-8 video");
            assert_eq!(
                result.backend,
                if image {
                    Backend::Awww
                } else {
                    Backend::Mpvpaper
                }
            );
            assert_eq!(rt.stop_lwe_count, 0);
            assert_eq!(rt.stop_lwe_outputs_calls, vec![vec!["eDP-1".to_string()]]);
            assert_eq!(rt.stop_mpvpaper_count, 0);
            assert!(rt.stop_mpvpaper_outputs_calls.is_empty());
            assert!(
                rt.mpvpaper_process_table
                    .iter()
                    .any(|process| process.pid == 42),
                "sibling video process must be untouched"
            );
            let rows = service.storage_for_tests().display_state_list().unwrap();
            assert_eq!(rows.len(), 2);
            assert!(rows.iter().any(
                |row| row.target == DisplayStateTarget::Output("DP-8".into())
                    && row.wallpaper_path == "/walls/sibling.mp4"
                    && row.backend == "mpvpaper"
            ));
            assert!(rows.iter().any(|row| row.target
                == DisplayStateTarget::Output("eDP-1".into())
                && row.wallpaper_path == next.to_string_lossy()
                && row.backend == if image { "awww" } else { "mpvpaper" }));
        }
    }

    #[test]
    fn named_replace_shared_lwe_rejects_before_stopping_either_output() {
        let (tmp, service) = temp_service();
        let next = write_video(tmp.path(), "next.mp4");
        service
            .storage_for_tests()
            .display_state_replace_all(&[
                (
                    DisplayStateTarget::Output("eDP-1".into()),
                    "/walls/scene-a".into(),
                    "linux-wallpaperengine".into(),
                ),
                (
                    DisplayStateTarget::Output("DP-8".into()),
                    "/walls/scene-b".into(),
                    "linux-wallpaperengine".into(),
                ),
            ])
            .unwrap();
        let before = service.storage_for_tests().display_state_list().unwrap();
        let mut rt = FakeRuntime {
            lwe_outputs: vec!["eDP-1".into(), "DP-8".into()],
            ..Default::default()
        };
        let error = service
            .apply_to_display_with_runtime(
                &next.to_string_lossy(),
                DisplayTarget::Output("eDP-1".into()),
                &["DP-8".into(), "eDP-1".into()],
                &mut rt,
                &mut NoopReporter,
                DisplayApplyRuntimeOpts::default(),
            )
            .unwrap_err();
        let haystack = format!(
            "{} {}",
            error.message,
            error.detail.as_deref().unwrap_or("")
        );
        // One LWE argv owns both outputs: scoped stop would disturb DP-8.
        // This is not the cross-backend coexistence gate (LWE↔mpvpaper is verified).
        assert!(
            haystack.contains("non-target")
                || haystack.contains("shared")
                || haystack.contains("disturb"),
            "expected shared-process / non-target refusal, got {error:?}"
        );
        assert!(!haystack.to_lowercase().contains("coexistence"));
        assert_eq!(rt.stop_lwe_count, 0);
        assert!(rt.stop_lwe_outputs_calls.is_empty());
        assert!(rt.command_status_args.is_empty());
        assert_eq!(
            service.storage_for_tests().display_state_list().unwrap(),
            before
        );
    }

    #[test]
    fn named_replace_lwe_failed_launch_preserves_sibling_state() {
        let (tmp, service) = temp_service();
        let next = write_video(tmp.path(), "next.mp4");
        service
            .storage_for_tests()
            .display_state_replace_all(&[
                (
                    DisplayStateTarget::Output("eDP-1".into()),
                    "/walls/scene".into(),
                    "linux-wallpaperengine".into(),
                ),
                (
                    DisplayStateTarget::Output("DP-8".into()),
                    "/walls/sibling.mp4".into(),
                    "mpvpaper".into(),
                ),
            ])
            .unwrap();
        let mut rt = FakeRuntime {
            lwe_outputs: vec!["eDP-1".into()],
            mpvpaper_process_table: vec![MpvpaperProcess::for_output(
                42,
                "DP-8",
                "/walls/sibling.mp4",
            )],
            ..Default::default()
        };
        let error = service
            .apply_to_display_with_runtime(
                &next.to_string_lossy(),
                DisplayTarget::Output("eDP-1".into()),
                &["DP-8".into(), "eDP-1".into()],
                &mut rt,
                &mut NoopReporter,
                DisplayApplyRuntimeOpts::default(),
            )
            .unwrap_err();
        assert_eq!(error.code, "display_apply_failed_after_stop");
        assert_eq!(rt.stop_lwe_count, 0);
        assert_eq!(rt.stop_lwe_outputs_calls, vec![vec!["eDP-1".to_string()]]);
        assert_eq!(rt.stop_mpvpaper_count, 0);
        assert!(
            rt.mpvpaper_process_table
                .iter()
                .any(|process| process.pid == 42),
            "sibling video process must be untouched"
        );
        let rows = service.storage_for_tests().display_state_list().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].target, DisplayStateTarget::Output("DP-8".into()));
        assert_eq!(rows[0].wallpaper_path, "/walls/sibling.mp4");
    }

    #[test]
    fn cross_backend_named_replace_stops_previous_then_applies() {
        let (tmp, service) = temp_service();
        let img = write_image(tmp.path(), "still.jpg");
        service
            .storage_for_tests()
            .display_state_upsert(
                &DisplayStateTarget::Output("eDP-1".into()),
                "/walls/old.mp4",
                "mpvpaper",
            )
            .unwrap();

        let mut rt = FakeRuntime {
            command_output_success: true,
            mpvpaper_process_table: vec![MpvpaperProcess::for_output(7, "eDP-1", "/walls/old.mp4")],
            ..Default::default()
        };
        let mut reporter = NoopReporter;
        service
            .apply_to_display_with_runtime(
                &img.to_string_lossy(),
                DisplayTarget::Output("eDP-1".into()),
                &["eDP-1".into()],
                &mut rt,
                &mut reporter,
                DisplayApplyRuntimeOpts::default(),
            )
            .unwrap();

        assert_eq!(rt.stop_mpvpaper_count, 1);
        assert_eq!(rt.command_output_args.len(), 1);
        let rows = service.storage_for_tests().display_state_list().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].backend, "awww");
        assert_eq!(rows[0].wallpaper_path, img.to_string_lossy());
    }

    #[test]
    fn intended_state_preserves_disconnected_output_rows() {
        let previous = vec![
            DisplayStateRow {
                target: DisplayStateTarget::AllDisplays,
                wallpaper_path: "/old.jpg".into(),
                backend: "awww".into(),
                updated_at: "t".into(),
            },
            DisplayStateRow {
                target: DisplayStateTarget::Output("DP-ghost".into()),
                wallpaper_path: "/ghost.jpg".into(),
                backend: "awww".into(),
                updated_at: "t".into(),
            },
        ];
        let intended = intended_display_state(
            &previous,
            &["eDP-1".into(), "HDMI-1".into()],
            &DisplayTarget::Output("eDP-1".into()),
            "/new.jpg",
            Backend::Awww,
        );
        assert!(intended.iter().any(|(t, p, _)| {
            *t == DisplayStateTarget::Output("eDP-1".into()) && p == "/new.jpg"
        }));
        assert!(intended.iter().any(|(t, p, _)| {
            *t == DisplayStateTarget::Output("HDMI-1".into()) && p == "/old.jpg"
        }));
        assert!(
            intended.iter().any(|(t, p, _)| {
                *t == DisplayStateTarget::Output("DP-ghost".into()) && p == "/ghost.jpg"
            }),
            "disconnected row must be preserved: {intended:?}"
        );
    }

    #[test]
    fn intended_all_displays_preserves_disconnected_named_rows() {
        let previous = vec![DisplayStateRow {
            target: DisplayStateTarget::Output("DP-ghost".into()),
            wallpaper_path: "/ghost.jpg".into(),
            backend: "mpvpaper".into(),
            updated_at: "t".into(),
        }];
        let intended = intended_display_state(
            &previous,
            &["eDP-1".into()],
            &DisplayTarget::AllDisplays,
            "/new.jpg",
            Backend::Awww,
        );
        assert_eq!(intended.len(), 2);
        assert!(intended
            .iter()
            .any(|(t, _, _)| *t == DisplayStateTarget::AllDisplays));
        assert!(intended.iter().any(|(t, p, b)| {
            *t == DisplayStateTarget::Output("DP-ghost".into())
                && p == "/ghost.jpg"
                && b == "mpvpaper"
        }));
    }

    #[test]
    fn lwe_named_apply_uses_fake_runtime_not_real_process() {
        let (tmp, service) = temp_service();
        let scene = tmp.path().join("scene");
        std::fs::create_dir_all(&scene).unwrap();
        std::fs::write(
            scene.join("project.json"),
            r#"{"type":"scene","file":"scene.pkg","workshopid":"42"}"#,
        )
        .unwrap();
        service
            .storage_for_tests()
            .config_set("linux_wallpaperengine_enabled", "on")
            .unwrap();

        let mut rt = FakeRuntime::default();
        let mut reporter = CapturingReporter { stages: Vec::new() };
        service
            .apply_to_display_with_runtime(
                &scene.to_string_lossy(),
                DisplayTarget::Output("eDP-1".into()),
                &["eDP-1".into()],
                &mut rt,
                &mut reporter,
                DisplayApplyRuntimeOpts {
                    request_id: Some("lwe".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(rt.lwe_apply_calls, 1);
        assert!(reporter.stages.contains(&ApplyStage::StartLwe));
        assert!(reporter.stages.contains(&ApplyStage::WaitRendererAlive));
    }

    #[test]
    fn named_video_replaces_only_target_output_when_sibling_runs_mpvpaper() {
        let (tmp, service) = temp_service();
        let old = write_video(tmp.path(), "old.mp4");
        let next = write_video(tmp.path(), "next.mp4");
        service
            .storage_for_tests()
            .display_state_upsert(
                &DisplayStateTarget::AllDisplays,
                &old.to_string_lossy(),
                "mpvpaper",
            )
            .unwrap();

        let mut rt = FakeRuntime {
            command_status_success: true,
            mpvpaper_ready_pid: Some(42),
            mpvpaper_process_table: vec![
                MpvpaperProcess::for_output(10, "eDP-1", old.to_string_lossy()),
                MpvpaperProcess::for_output(20, "HDMI-1", old.to_string_lossy()),
            ],
            ..Default::default()
        };
        let mut reporter = NoopReporter;
        service
            .apply_to_display_with_runtime(
                &next.to_string_lossy(),
                DisplayTarget::Output("eDP-1".into()),
                &["eDP-1".into(), "HDMI-1".into()],
                &mut rt,
                &mut reporter,
                DisplayApplyRuntimeOpts::default(),
            )
            .unwrap();

        assert_eq!(rt.stop_mpvpaper_count, 0);
        assert_eq!(
            rt.stop_mpvpaper_outputs_calls,
            vec![vec!["eDP-1".to_string()]]
        );
        assert_eq!(rt.command_status_args.len(), 1);
        assert!(rt.command_status_args[0].iter().any(|a| a == "eDP-1"));

        let rows = service.storage_for_tests().display_state_list().unwrap();
        assert!(
            rows.iter()
                .all(|r| !matches!(r.target, DisplayStateTarget::AllDisplays)),
            "{rows:?}"
        );
        let edp = rows
            .iter()
            .find(|r| r.target == DisplayStateTarget::Output("eDP-1".into()))
            .expect("eDP-1 row");
        let hdmi = rows
            .iter()
            .find(|r| r.target == DisplayStateTarget::Output("HDMI-1".into()))
            .expect("HDMI-1 row");
        assert_eq!(edp.backend, "mpvpaper");
        assert_eq!(edp.wallpaper_path, next.to_string_lossy());
        assert_eq!(hdmi.backend, "mpvpaper");
        assert_eq!(hdmi.wallpaper_path, old.to_string_lossy());
    }

    #[test]
    fn named_video_scoped_stop_then_launch_failure_preserves_sibling_row() {
        let (tmp, service) = temp_service();
        let old = write_video(tmp.path(), "old.mp4");
        let next = write_video(tmp.path(), "next.mp4");
        service
            .storage_for_tests()
            .display_state_upsert(
                &DisplayStateTarget::AllDisplays,
                &old.to_string_lossy(),
                "mpvpaper",
            )
            .unwrap();

        let mut rt = FakeRuntime {
            command_status_success: false,
            mpvpaper_process_table: vec![
                MpvpaperProcess::for_output(10, "eDP-1", old.to_string_lossy()),
                MpvpaperProcess::for_output(20, "HDMI-1", old.to_string_lossy()),
            ],
            ..Default::default()
        };
        let mut reporter = NoopReporter;
        let err = service
            .apply_to_display_with_runtime(
                &next.to_string_lossy(),
                DisplayTarget::Output("eDP-1".into()),
                &["eDP-1".into(), "HDMI-1".into()],
                &mut rt,
                &mut reporter,
                DisplayApplyRuntimeOpts::default(),
            )
            .unwrap_err();
        assert_eq!(err.code, "display_apply_failed_after_stop");
        assert_eq!(rt.stop_mpvpaper_count, 0);
        assert_eq!(
            rt.stop_mpvpaper_outputs_calls,
            vec![vec!["eDP-1".to_string()]]
        );

        let rows = service.storage_for_tests().display_state_list().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].target, DisplayStateTarget::Output("HDMI-1".into()));
        assert_eq!(rows[0].backend, "mpvpaper");
        assert_eq!(rows[0].wallpaper_path, old.to_string_lossy());
    }

    #[test]
    fn named_video_readiness_failure_does_not_kill_sibling() {
        let (tmp, service) = temp_service();
        let old = write_video(tmp.path(), "old.mp4");
        let next = write_video(tmp.path(), "next.mp4");
        service
            .storage_for_tests()
            .display_state_upsert(
                &DisplayStateTarget::AllDisplays,
                &old.to_string_lossy(),
                "mpvpaper",
            )
            .unwrap();

        let mut rt = FakeRuntime {
            command_status_success: true,
            mpvpaper_readiness_error: Some("not ready".into()),
            mpvpaper_process_table: vec![
                MpvpaperProcess::for_output(10, "eDP-1", old.to_string_lossy()),
                MpvpaperProcess::for_output(20, "HDMI-1", old.to_string_lossy()),
            ],
            ..Default::default()
        };
        let mut reporter = NoopReporter;
        let err = service
            .apply_to_display_with_runtime(
                &next.to_string_lossy(),
                DisplayTarget::Output("eDP-1".into()),
                &["eDP-1".into(), "HDMI-1".into()],
                &mut rt,
                &mut reporter,
                DisplayApplyRuntimeOpts::default(),
            )
            .unwrap_err();
        assert_eq!(err.code, "display_apply_failed_after_stop");
        assert_eq!(rt.stop_mpvpaper_count, 0);
        assert_eq!(rt.failed_mpvpaper_launch_cleanup_count, 1);

        let rows = service.storage_for_tests().display_state_list().unwrap();
        assert!(
            rows.iter()
                .any(|r| r.target == DisplayStateTarget::Output("HDMI-1".into())),
            "{rows:?}"
        );
    }

    #[test]
    fn named_video_refused_when_legacy_wildcard_mpvpaper_running() {
        let (tmp, service) = temp_service();
        let old = write_video(tmp.path(), "old.mp4");
        let next = write_video(tmp.path(), "next.mp4");
        service
            .storage_for_tests()
            .display_state_upsert(
                &DisplayStateTarget::AllDisplays,
                &old.to_string_lossy(),
                "mpvpaper",
            )
            .unwrap();
        let before = service.storage_for_tests().display_state_list().unwrap();

        let mut rt = FakeRuntime {
            command_status_success: true,
            mpvpaper_process_table: vec![
                MpvpaperProcess::for_output(10, "eDP-1", old.to_string_lossy()),
                MpvpaperProcess::for_output(99, "*", old.to_string_lossy()),
            ],
            ..Default::default()
        };
        let mut reporter = NoopReporter;
        let err = service
            .apply_to_display_with_runtime(
                &next.to_string_lossy(),
                DisplayTarget::Output("eDP-1".into()),
                &["eDP-1".into(), "HDMI-1".into()],
                &mut rt,
                &mut reporter,
                DisplayApplyRuntimeOpts::default(),
            )
            .unwrap_err();
        assert_ne!(err.code, "display_apply_failed_after_stop");
        assert!(
            err.detail
                .as_deref()
                .unwrap_or(&err.message)
                .contains("multiple/all")
                || err.message.contains("multiple/all")
                || format!("{err:?}").contains("multiple/all")
        );
        assert_eq!(rt.stop_mpvpaper_count, 0);
        assert!(rt.stop_mpvpaper_outputs_calls.is_empty());
        let after = service.storage_for_tests().display_state_list().unwrap();
        assert_eq!(before.len(), after.len());
    }

    #[test]
    fn reconcile_named_stop_removes_only_named_outputs() {
        use wc_backend::display_executor::CompletedStop;

        let mut rows = vec![
            (
                DisplayStateTarget::Output("eDP-1".into()),
                "/a.mp4".into(),
                "mpvpaper".into(),
            ),
            (
                DisplayStateTarget::Output("HDMI-1".into()),
                "/b.mp4".into(),
                "mpvpaper".into(),
            ),
        ];
        apply_completed_stop(
            &mut rows,
            &CompletedStop {
                backend: Backend::Mpvpaper,
                scope: ExecutionScope::named(vec!["eDP-1".into()]).unwrap(),
                destructive: true,
            },
            &["eDP-1".into(), "HDMI-1".into()],
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, DisplayStateTarget::Output("HDMI-1".into()));
    }

    #[test]
    fn successful_apply_writes_theme_state_manifest_for_known_outputs() {
        let (tmp, service) = temp_service();
        let image = write_image(tmp.path(), "theme.jpg");
        service
            .storage_for_tests()
            .config_set("post_apply_enabled", "off")
            .unwrap();

        let request = crate::ApplyRequest {
            kind: crate::ApplyRequestKind::Apply,
            path: image.to_string_lossy().to_string(),
            request_id: Some("theme-manifest".into()),
        };
        let mut rt = FakeRuntime {
            command_output_success: true,
            ..Default::default()
        };
        let mut reporter = NoopReporter;
        service
            .execute_apply_request_to_display_with_runtime(
                request,
                DisplayTarget::AllDisplays,
                &["eDP-1".into(), "DP-8".into()],
                &mut rt,
                &mut reporter,
                DisplayApplyRuntimeOpts::default(),
            )
            .unwrap();

        let manifest = service.storage_for_tests().cd.theme_state_path();
        assert!(
            manifest.is_file(),
            "expected theme-state.json at {manifest:?}"
        );
        let doc: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&manifest).unwrap()).unwrap();
        assert_eq!(doc["version"], 1);
        assert_eq!(doc["theme_source_policy"], "last_applied");
        assert!(doc["outputs"]["eDP-1"]["wallpaper"]
            .as_str()
            .unwrap()
            .ends_with("theme.jpg"));
        assert!(doc["outputs"]["DP-8"]["still"].as_str().is_some());
        assert_eq!(doc["theme_source_output"], "DP-8");
    }

    #[test]
    fn mpv_reapply_preserves_per_output_paths_options_assignments_and_theme() {
        let (tmp, service) = temp_service();
        let a = write_video(tmp.path(), "a.mp4")
            .to_string_lossy()
            .into_owned();
        let b = write_image(tmp.path(), "b.jpg")
            .to_string_lossy()
            .into_owned();
        let storage = service.storage_for_tests();
        storage
            .display_state_upsert(&DisplayStateTarget::AllDisplays, &a, "mpvpaper")
            .unwrap();
        storage
            .display_state_upsert(&DisplayStateTarget::Output("HDMI-1".into()), &b, "mpvpaper")
            .unwrap();
        // The current image routing is awww: reapply must preserve the running mpvpaper backend.
        storage.config_set("image_backend", "awww").unwrap();
        storage
            .config_set("mpvpaper_options", "--volume=37 --hwdec=auto-safe")
            .unwrap();
        let before = storage.display_state_list().unwrap();
        let manifest = storage.cd.path.join("theme-state.json");
        std::fs::write(&manifest, "keep-theme").unwrap();
        let mut rt = FakeRuntime {
            command_status_success: true,
            awww_stopped: true,
            mpvpaper_process_table: vec![
                MpvpaperProcess::for_output(10, "eDP-1", &a),
                MpvpaperProcess::for_output(20, "HDMI-1", &b),
            ],
            ..Default::default()
        };
        let report = service
            .reapply_mpvpaper_with_runtime(&["eDP-1".into(), "HDMI-1".into()], &mut rt)
            .unwrap();
        assert!(report.failures.is_empty(), "{report:?}");
        assert_eq!(report.applied_outputs, ["eDP-1", "HDMI-1"]);
        assert_eq!(rt.stop_mpvpaper_count, 0);
        assert_eq!(rt.command_status_args.len(), 2);
        for (args, path) in rt.command_status_args.iter().zip([a, b]) {
            assert!(args.contains(&path), "{args:?}");
            assert!(
                args.contains(&"--volume=37 --hwdec=auto-safe".into()),
                "{args:?}"
            );
        }
        assert_eq!(storage.display_state_list().unwrap(), before);
        assert_eq!(std::fs::read_to_string(manifest).unwrap(), "keep-theme");
    }

    #[test]
    fn mpv_reapply_does_not_revive_stopped_disconnected_or_mismatched_wallpapers() {
        let (tmp, service) = temp_service();
        let path = write_video(tmp.path(), "old.mp4")
            .to_string_lossy()
            .into_owned();
        let other = write_video(tmp.path(), "other.mp4")
            .to_string_lossy()
            .into_owned();
        service
            .storage_for_tests()
            .display_state_upsert(&DisplayStateTarget::AllDisplays, &path, "mpvpaper")
            .unwrap();
        for processes in [
            vec![],
            vec![MpvpaperProcess::for_output(10, "disconnected", &path)],
            vec![MpvpaperProcess::for_output(10, "eDP-1", &other)],
        ] {
            let mut rt = FakeRuntime {
                mpvpaper_process_table: processes,
                awww_stopped: true,
                ..Default::default()
            };
            let report = service
                .reapply_mpvpaper_with_runtime(&["eDP-1".into()], &mut rt)
                .unwrap();
            assert!(report.applied_outputs.is_empty());
            assert!(rt.stop_mpvpaper_outputs_calls.is_empty());
            assert!(rt.command_status_args.is_empty());
        }
    }

    #[test]
    fn mpv_reapply_leaves_other_renderer_alone_and_reconciles_launch_failure() {
        for fail in [false, true] {
            let (tmp, service) = temp_service();
            let video = write_video(tmp.path(), "video.mp4")
                .to_string_lossy()
                .into_owned();
            let image = write_image(tmp.path(), "image.jpg")
                .to_string_lossy()
                .into_owned();
            let storage = service.storage_for_tests();
            storage
                .display_state_upsert(
                    &DisplayStateTarget::Output("eDP-1".into()),
                    &video,
                    "mpvpaper",
                )
                .unwrap();
            storage
                .display_state_upsert(&DisplayStateTarget::Output("HDMI-1".into()), &image, "awww")
                .unwrap();
            let mut rt = FakeRuntime {
                command_status_success: !fail,
                command_output_success: true,
                mpvpaper_process_table: vec![MpvpaperProcess::for_output(10, "eDP-1", &video)],
                awww_displayed: vec![("HDMI-1".into(), image.clone())],
                ..Default::default()
            };
            let report = service
                .reapply_mpvpaper_with_runtime(&["eDP-1".into(), "HDMI-1".into()], &mut rt)
                .unwrap();
            assert_eq!(report.failures.len(), usize::from(fail), "{report:?}");
            assert_eq!(report.applied_outputs.len(), usize::from(!fail));
            assert_eq!(rt.stop_awww_count, 0);
            assert_eq!(rt.stop_mpvpaper_count, 0);
            assert_eq!(
                rt.stop_mpvpaper_outputs_calls,
                vec![vec!["eDP-1".to_string()]]
            );
            assert!(rt
                .command_output_args
                .iter()
                .all(|args| !args.contains(&"HDMI-1".to_string())));
            let rows = storage.display_state_list().unwrap();
            assert!(rows
                .iter()
                .any(|row| row.wallpaper_path == image && row.backend == "awww"));
            assert_eq!(rows.iter().any(|row| row.wallpaper_path == video), !fail);
        }
    }

    #[test]
    fn mpv_reapply_missing_file_is_rejected_before_stopping() {
        let (_tmp, service) = temp_service();
        let path = "/missing-mpv-reapply-test.mp4";
        service
            .storage_for_tests()
            .display_state_upsert(
                &DisplayStateTarget::Output("eDP-1".into()),
                path,
                "mpvpaper",
            )
            .unwrap();
        let mut rt = FakeRuntime {
            awww_stopped: true,
            mpvpaper_process_table: vec![MpvpaperProcess::for_output(10, "eDP-1", path)],
            ..Default::default()
        };
        let report = service
            .reapply_mpvpaper_with_runtime(&["eDP-1".into()], &mut rt)
            .unwrap();
        assert_eq!(report.failures.len(), 1);
        assert!(rt.stop_mpvpaper_outputs_calls.is_empty());
        assert!(rt.command_status_args.is_empty());
    }
}
