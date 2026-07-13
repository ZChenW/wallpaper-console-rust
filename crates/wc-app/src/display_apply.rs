//! Display-aware apply orchestration for AppService.
//!
//! Loads persisted display assignments, plans via `plan_display_apply`, executes
//! Stop/Apply actions, and commits the intended display_state mapping only after
//! every action succeeds. After a destructive Stop followed by failure, persisted
//! state is reconciled so it does not claim stopped renderers still run.

use wc_backend::apply_stage::{self, ApplyStageReporter, NoopReporter};
use wc_backend::display_executor::{
    execute_display_actions, CompletedEvent, DisplayExecAction, DisplayExecContext,
    DisplayExecFailure, DisplayExecReport,
};
use wc_backend::runtime::{BackendRuntime, SystemBackendRuntime};
use wc_backend::ExecutionScope;
use wc_core::types::Backend;
use wc_storage::sqlite::{DisplayStateRow, DisplayStateTarget};

use crate::display_plan::{
    plan_display_apply, DisplayApplyRequest, DisplayTarget, PlannedAction, RejectionReason,
    RunningAssignment,
};
use crate::{AppError, AppService, ApplyTarget};

/// Optional knobs for [`AppService::apply_to_display_with_runtime`].
#[derive(Default)]
pub struct DisplayApplyRuntimeOpts {
    pub request_id: Option<String>,
    pub capability: Option<wc_backend::capability::BackendCapability>,
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
        self.apply_to_display_with_runtime_and_commit_seam(
            path,
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
        let request_id = opts.request_id.as_deref();
        apply_stage::report_stage(reporter, apply_stage::ApplyStage::ResolveTarget, request_id);

        let apply_target = self.resolve_apply_target(path)?;
        let previous_rows = self
            .storage
            .display_state_list()
            .map_err(AppError::from_wc_error)?;
        let running = running_from_display_state(&previous_rows, known_outputs)?;
        let same_backend_already_running = running
            .iter()
            .any(|assignment| assignment.backend == apply_target.backend);

        let request = DisplayApplyRequest {
            target: target.clone(),
            backend: apply_target.backend,
            known_outputs: known_outputs.to_vec(),
            running,
        };
        let plan = match opts.capability {
            Some(cap) => crate::display_plan::plan_display_apply_with_capability(&request, cap),
            None => plan_display_apply(&request),
        }
        .map_err(rejection_to_app_error)?;

        let plan_has_stop = plan
            .actions
            .iter()
            .any(|action| matches!(action, PlannedAction::Stop { .. }));
        let use_instant = plan_has_stop || !same_backend_already_running;
        let actions: Vec<DisplayExecAction> = plan
            .actions
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

        let exec_result = execute_display_actions(
            &self.storage,
            &actions,
            &DisplayExecContext { known_outputs },
            runtime,
            reporter,
            request_id,
        );

        match exec_result {
            Ok(report) => {
                let intended = intended_display_state(
                    &previous_rows,
                    known_outputs,
                    &target,
                    &apply_target.resolved_path,
                    apply_target.backend,
                );
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
                Ok(apply_target)
            }
            Err(failure) => Err(self.handle_exec_failure(
                failure,
                &previous_rows,
                known_outputs,
                before_state_commit,
            )?),
        }
    }

    fn commit_successful_display_state(
        &self,
        target: &DisplayTarget,
        intended: &[(DisplayStateTarget, String, String)],
        apply_target: &ApplyTarget,
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
                            &apply_target.resolved_path,
                            apply_target.backend.as_str(),
                            &retain,
                            true,
                            seam,
                        ),
                    None => self.storage.display_state_commit_all_displays_with_legacy(
                        &apply_target.resolved_path,
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
        apply_target: &ApplyTarget,
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
        if let Some(stop) = failure.uncertain_stop.clone().map(|stop| *stop) {
            let mut conservative_report = failure.report.clone();
            conservative_report
                .events
                .push(CompletedEvent::Stop(stop.clone()));
            conservative_report.completed_stops.push(stop);
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
            persist_result.map_err(|persist_error| AppError {
                code: "display_state_uncertain".into(),
                message: "Renderer stop outcome and persisted display state are uncertain".into(),
                detail: Some(format!(
                    "execution_error={}; reconciliation_error={persist_error}",
                    failure.error
                )),
                recoverable: true,
                suggestion: Some("Refresh renderer status before retrying.".into()),
            })?;
            return Ok(AppError {
                code: "display_state_uncertain".into(),
                message: "Renderer stop was attempted but termination could not be verified".into(),
                detail: Some(failure.error.to_string()),
                recoverable: true,
                suggestion: Some("Refresh renderer status before retrying.".into()),
            });
        }
        let after_stop = failure.after_destructive_stop();
        let had_progress = after_stop || !failure.report.completed_applies.is_empty();
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
        let applies = failure.report.completed_applies.len();
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

pub(crate) fn to_exec_action(
    action: PlannedAction,
    path: &str,
    target: &DisplayTarget,
    known_outputs: &[String],
    use_instant: bool,
) -> Result<DisplayExecAction, AppError> {
    match action {
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
    match raw {
        "awww" => Ok(Backend::Awww),
        "mpvpaper" => Ok(Backend::Mpvpaper),
        "linux-wallpaperengine" => Ok(Backend::LinuxWallpaperEngine),
        other => Err(AppError {
            code: "invalid_display_state".into(),
            message: format!("unsupported display state backend: {other}"),
            detail: None,
            recoverable: true,
            suggestion: None,
        }),
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

fn apply_completed_stop(
    rows: &mut Vec<(DisplayStateTarget, String, String)>,
    stop: &wc_backend::display_executor::CompletedStop,
    known_outputs: &[String],
) {
    let backend = stop.backend.as_str();
    // A process-wide stop only proves connected renderer ownership disappeared.
    // Disconnected rows are restore preferences, not currently running processes.
    let _ = &stop.scope;
    rows.retain(|(target, _, row_backend)| {
        row_backend != backend
            || match target {
                DisplayStateTarget::AllDisplays => false,
                DisplayStateTarget::Output(output) => !known_outputs.contains(output),
            }
    });
}

fn apply_completed_apply(
    rows: &mut Vec<(DisplayStateTarget, String, String)>,
    apply: &wc_backend::display_executor::CompletedApply,
    known_outputs: &[String],
) {
    let backend = apply.backend.as_str().to_string();
    let path = apply.path.clone();
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
    use wc_backend::runtime::AwwwReadiness;
    use wc_core::config::ConfigDir;
    use wc_core::error::WcError;

    #[derive(Default)]
    struct FakeRuntime {
        stop_awww_count: usize,
        stop_mpvpaper_count: usize,
        stop_lwe_count: usize,
        stop_mpvpaper_error: Option<String>,
        command_output_success: bool,
        command_status_success: bool,
        command_output_args: Vec<Vec<String>>,
        command_status_args: Vec<Vec<String>>,
        fail_after_n_status: Option<usize>,
        mpvpaper_ready_pid: Option<u32>,
        running_mpvpaper_pids: Vec<u32>,
        awww_readiness_sequence: RefCell<Vec<AwwwReadiness>>,
        lwe_apply_calls: usize,
        lwe_apply_error: Option<String>,
    }

    impl BackendRuntime for FakeRuntime {
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
            Ok(self.running_mpvpaper_pids.clone())
        }

        fn wait_for_mpvpaper_ready(&mut self, _previous_pids: &[u32]) -> Result<u32, WcError> {
            Ok(self.mpvpaper_ready_pid.unwrap_or(7))
        }

        fn mpvpaper_pid_running(&mut self, _pid: u32) -> Result<bool, WcError> {
            Ok(true)
        }

        fn stop_awww(&mut self) {
            self.stop_awww_count += 1;
        }

        fn stop_mpvpaper(&mut self) {
            self.stop_mpvpaper_count += 1;
        }

        fn stop_mpvpaper_checked(&mut self) -> Result<(), WcError> {
            self.stop_mpvpaper();
            match &self.stop_mpvpaper_error {
                Some(message) => Err(WcError::Other(message.clone())),
                None => Ok(()),
            }
        }

        fn stop_lwe(&mut self, _s: Option<&wc_storage::StorageApi>) {
            self.stop_lwe_count += 1;
        }

        fn apply_lwe_to_outputs(
            &mut self,
            _s: &wc_storage::StorageApi,
            _project: &wc_backend::linux_wallpaperengine::LinuxWallpaperEngineProject,
            _outputs: &[String],
        ) -> Result<(), WcError> {
            self.lwe_apply_calls += 1;
            if let Some(message) = &self.lwe_apply_error {
                return Err(WcError::Other(message.clone()));
            }
            Ok(())
        }

        fn awww_socket_ready(&mut self) -> AwwwReadiness {
            let mut seq = self.awww_readiness_sequence.borrow_mut();
            if seq.len() > 1 {
                seq.remove(0)
            } else if !seq.is_empty() {
                seq[0].clone()
            } else {
                AwwwReadiness::Ready
            }
        }

        fn ensure_awww_daemon_running(&mut self) -> Result<(), WcError> {
            if matches!(self.awww_socket_ready(), AwwwReadiness::Ready) {
                Ok(())
            } else {
                Err(WcError::Other("awww socket not ready".into()))
            }
        }

        fn clear_awww_state_hint(&mut self) {}
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
        wc_core::config::write_config_value(&cd.path, "storage_backend", "sqlite").unwrap();
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
            completed_stops: vec![stop],
            completed_applies: vec![apply.clone()],
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
            completed_stops: vec![cleanup],
            completed_applies: vec![apply],
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
        let mut runtime = FakeRuntime::default();
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
        let mut runtime = FakeRuntime {
            command_output_success: true,
            stop_mpvpaper_error: Some("verification probe failed".into()),
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
                    "mpvpaper".into(),
                ),
            ])
            .unwrap();

        let before = service.storage_for_tests().display_state_list().unwrap();
        let mut rt = FakeRuntime {
            command_output_success: true,
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
}
