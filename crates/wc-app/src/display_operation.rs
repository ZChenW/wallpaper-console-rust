//! One prepare/execute/commit/compensate protocol for all display mutations.
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::{
    output_recovery::{renderer_session_id, RendererMutationGuard},
    AppError,
};
use serde::{Deserialize, Serialize};
use wc_backend::apply_stage::ApplyStageReporter;
use wc_backend::apply_transition::{
    execute_prepared_transitions, prepare_apply_transitions, PreparedTransitions,
    TransitionRequest, TransitionStart, TransitionStep,
};
use wc_backend::display_executor::{
    execute_display_actions, DisplayExecAction, DisplayExecContext,
};
use wc_backend::runtime::BackendRuntime;
use wc_backend::runtime_observation::{observe_output_ownership, OutputOwnership};
use wc_backend::ExecutionScope;
use wc_core::{display_assignment::RenderRecipe, error::WcError};
use wc_storage::{
    sqlite::{self, display_operations, DisplayStateTarget},
    StorageApi,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwitchOutcome {
    Applied,
    Stopped,
    AlreadySatisfied,
    Unchanged,
    RestoredPrevious,
    Partial,
    RecoveryFailed,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputResult {
    pub output: String,
    pub outcome: SwitchOutcome,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchReport {
    pub operation_id: String,
    pub outcome: SwitchOutcome,
    pub original_error: Option<String>,
    pub original_error_code: Option<String>,
    pub outputs: Vec<OutputResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timings: Option<crate::operation_timing::SwitchTimings>,
}

/// Keep structured outcome evidence intact until the transport boundary.
#[derive(Debug)]
pub(crate) struct OperationFailure {
    pub error: AppError,
    pub report: Option<Box<SwitchReport>>,
}
impl From<AppError> for OperationFailure {
    fn from(error: AppError) -> Self {
        Self {
            error,
            report: None,
        }
    }
}
impl From<OperationFailure> for AppError {
    fn from(failure: OperationFailure) -> Self {
        failure.error
    }
}

pub(crate) struct OperationBatch<'a> {
    pub outputs: &'a [String],
    pub known_outputs: &'a [String],
    pub steps: &'a [TransitionStep],
    pub request_id: Option<&'a str>,
}

fn prepare(
    storage: &StorageApi,
    steps: &[TransitionStep],
    known: &[String],
    runtime: &mut dyn BackendRuntime,
    id: &str,
) -> Result<PreparedTransitions, AppError> {
    prepare_apply_transitions(
        storage,
        TransitionRequest {
            start: TransitionStart::Current {
                previous_backend_raw: "",
            },
            known_outputs: known,
            steps,
            request_id: Some(id),
        },
        runtime,
    )
    .map_err(|e| AppError::from_wc_error(e.error))
}

fn scoped_recipe(output: &str, recipe: &RenderRecipe) -> TransitionStep {
    let scope = ExecutionScope::Named(vec![output.into()]);
    TransitionStep {
        scope: scope.clone(),
        target: recipe.options.backend(),
        fallback_path: None,
        recipe: Some(recipe.clone()),
        core_actions: vec![DisplayExecAction::Apply {
            backend: recipe.options.backend(),
            path: recipe.media_path.clone(),
            scope,
            use_instant: true,
        }],
    }
}

fn topology_still_matches(
    runtime: &dyn BackendRuntime,
    expected: &[String],
) -> Result<(), AppError> {
    if runtime.supports_output_recovery() {
        let actual = crate::discover_connected_outputs()?;
        if actual.iter().collect::<BTreeSet<_>>() != expected.iter().collect() {
            return Err(AppError::from_wc_error(WcError::Other(
                "connected outputs changed; retry the display operation".into(),
            )));
        }
    }
    Ok(())
}

fn assignment_revisions(storage: &StorageApi) -> Result<Vec<(String, i64)>, AppError> {
    let conn = sqlite::open_runtime_connection(&storage.cd).map_err(AppError::from_wc_error)?;
    sqlite::assignment_revision_fingerprint(&conn).map_err(AppError::from_wc_error)
}

fn assignment_revisions_unchanged(
    storage: &StorageApi,
    expected: &[(String, i64)],
) -> Result<(), AppError> {
    if assignment_revisions(storage)? != expected {
        return Err(AppError::from_wc_error(WcError::Other(
            "display assignments changed; retry the display operation".into(),
        )));
    }
    Ok(())
}

/// The caller owns request resolution; only this function executes or compensates.
/// Preferences are committed exactly once, after all requested renderers verify.
pub(crate) fn execute<F>(
    storage: &StorageApi,
    batch: OperationBatch<'_>,
    runtime: &mut dyn BackendRuntime,
    reporter: &mut dyn ApplyStageReporter,
    commit: F,
) -> Result<SwitchReport, OperationFailure>
where
    F: FnOnce() -> Result<(), WcError>,
{
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let id = format!(
        "{}-{}-{}-{}",
        renderer_session_id(),
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    let mut timer = crate::operation_timing::OperationTimer::new(&id);
    let _guard = RendererMutationGuard::acquire(storage).map_err(AppError::from_wc_error)?;
    let Preflight {
        prepared,
        before,
        recovery,
        revisions,
        ownership,
        identities,
    } = preflight(storage, &batch, runtime, &id, &mut timer)?;
    timer.enter("journal_begin");
    journal_begin(storage, &batch, &id, &before, &ownership, &identities)?;
    assignment_revisions_unchanged(storage, &revisions)?;

    timer.enter("execute");
    let execution = execute_prepared_transitions(prepared, runtime, reporter);
    timer.enter("verify_recipe");
    let (progress, error, uncertain) = match execution {
        Ok(report) => {
            let verified = batch.steps.iter().try_for_each(|step| {
                if let Some(recipe) = &step.recipe {
                    for output in step.scope.named_outputs().unwrap_or(batch.outputs) {
                        runtime.verify_recipe(output, recipe, batch.known_outputs)?;
                    }
                }
                Ok::<_, WcError>(())
            });
            timer.enter("commit");
            match verified.and_then(|()| commit()) {
                Ok(()) => {
                    if let Err(error) =
                        crate::output_recovery::set_output_stopped(storage, batch.outputs, false)
                    {
                        log::warn!(
                            "wallpaper applied but output recovery could not be armed: {error}"
                        );
                    }
                    let result = SwitchReport {
                        operation_id: id.clone(),
                        timings: Some(timer.snapshot()),
                        outcome: SwitchOutcome::Applied,
                        original_error: None,
                        original_error_code: None,
                        outputs: batch
                            .outputs
                            .iter()
                            .map(|output| OutputResult {
                                output: output.clone(),
                                outcome: SwitchOutcome::Applied,
                                error: None,
                            })
                            .collect(),
                    };
                    finish(storage, &result);
                    return Ok(result);
                }
                Err(error) => (report.exec, AppError::from_wc_error(error), false),
            }
        }
        Err(failure) => (
            failure.exec,
            AppError::from_wc_error(failure.error),
            failure.cleanup_uncertain,
        ),
    };
    timer.enter("compensate");
    let mut result = compensate(
        storage,
        &batch,
        runtime,
        reporter,
        Compensation {
            id,
            progress,
            error,
            uncertain,
            before,
            recovery,
        },
    );
    result.timings = Some(timer.snapshot());
    finish(storage, &result);
    Err(OperationFailure {
        error: report_error(&result),
        report: Some(Box::new(result)),
    })
}

struct Preflight {
    prepared: PreparedTransitions,
    before: Vec<(String, Option<RenderRecipe>)>,
    recovery: Vec<(String, PreparedTransitions)>,
    revisions: Vec<(String, i64)>,
    ownership: wc_backend::runtime_observation::OutputOwnershipSnapshot,
    identities: String,
}

fn preflight(
    storage: &StorageApi,
    batch: &OperationBatch<'_>,
    runtime: &mut dyn BackendRuntime,
    id: &str,
    timer: &mut crate::operation_timing::OperationTimer,
) -> Result<Preflight, AppError> {
    crate::display_target::validate_known_outputs(batch.outputs)
        .map_err(|e| AppError::from_wc_error(WcError::Other(e)))?;
    if batch.outputs.is_empty() || batch.outputs.len() > 32 {
        return Err(AppError::from_wc_error(WcError::Other(
            "display operation requires 1 to 32 outputs".into(),
        )));
    }
    timer.measure("topology", || {
        topology_still_matches(runtime, batch.known_outputs)
    })?;
    let revisions = assignment_revisions(storage)?;
    if batch
        .steps
        .iter()
        .any(|step| step.target == wc_core::types::Backend::Feh)
    {
        return Err(AppError::from_wc_error(WcError::Other(
            wc_core::types::Backend::FEH_REMOVED_MESSAGE.into(),
        )));
    }
    let ownership = timer.measure("ownership", || {
        observe_output_ownership(batch.known_outputs, runtime)
    });
    // Validate the environment for every surviving or newly planned sibling,
    // not just the pair on the first output in a batch.
    for step in batch.steps {
        for (output, state) in &ownership.outputs {
            if !step
                .scope
                .named_outputs()
                .is_some_and(|names| names.contains(output))
            {
                if let OutputOwnership::Occupied(backend) = state {
                    wc_backend::capability::preflight_pair(step.target, *backend, runtime)
                        .map_err(AppError::from_wc_error)?;
                }
            }
        }
        for other in batch.steps {
            wc_backend::capability::preflight_pair(step.target, other.target, runtime)
                .map_err(AppError::from_wc_error)?;
        }
    }
    let identities = timer
        .measure("identity", || runtime.renderer_identity_snapshot())
        .map_err(AppError::from_wc_error)?;
    let rows = storage
        .display_state_list()
        .map_err(AppError::from_wc_error)?;
    // Validate the new request before preparing compensation. Both complete
    // before any mutation; invalid new media must not be hidden by stale history.
    timer.enter("prepare");
    let prepared = timer.measure("request", || {
        prepare(storage, batch.steps, batch.known_outputs, runtime, id)
    })?;
    timer.enter("prepare_recovery");
    let mut before = Vec::new();
    let mut recovery = Vec::new();
    for output in batch.outputs {
        let row = rows
            .iter()
            .find(|r| r.target == DisplayStateTarget::Output(output.clone()))
            .or_else(|| {
                rows.iter()
                    .find(|r| r.target == DisplayStateTarget::AllDisplays)
            });
        // Validate even vacant assignments before any renderer or journal write.
        let saved = row
            .map(|r| storage.display_recipe(&r.target))
            .transpose()
            .map_err(AppError::from_wc_error)?
            .flatten();
        let observed = ownership
            .outputs
            .iter()
            .find(|(name, _)| name == output)
            .map(|(_, state)| state);
        let previous = match observed {
            Some(OutputOwnership::Vacant) => None,
            Some(OutputOwnership::Occupied(backend)) => {
                let recipe = saved.filter(|r| r.options.backend() == *backend).ok_or_else(||
                    AppError::from_wc_error(WcError::Other(format!("{output} has a running renderer without a matching recovery recipe; stop it explicitly first"))))?;
                timer
                    .measure("verify_recipe", || {
                        runtime.verify_recipe(output, &recipe, batch.known_outputs)
                    })
                    .map_err(AppError::from_wc_error)?;
                let prepared = timer.measure("prepare", || {
                    prepare(
                        storage,
                        &[scoped_recipe(output, &recipe)],
                        batch.known_outputs,
                        runtime,
                        id,
                    )
                })?;
                recovery.push((output.clone(), prepared));
                Some(recipe)
            }
            _ => {
                return Err(AppError::from_wc_error(WcError::Other(format!(
                    "cannot establish recoverable ownership for {output}: {observed:?}"
                ))))
            }
        };
        before.push((output.clone(), previous));
    }
    timer.enter("revalidate");
    timer
        .measure("verify_media", || prepared.verify_media())
        .map_err(AppError::from_wc_error)?;
    for (_, prepared_recovery) in &recovery {
        timer
            .measure("verify_media", || prepared_recovery.verify_media())
            .map_err(AppError::from_wc_error)?;
    }
    timer.measure("topology", || {
        topology_still_matches(runtime, batch.known_outputs)
    })?;
    assignment_revisions_unchanged(storage, &revisions)?;
    if timer.measure("ownership", || {
        observe_output_ownership(batch.known_outputs, runtime)
    }) != ownership
        || timer
            .measure("identity", || runtime.renderer_identity_snapshot())
            .map_err(AppError::from_wc_error)?
            != identities
    {
        return Err(AppError::from_wc_error(WcError::Other(
            "renderer ownership changed during preflight; retry".into(),
        )));
    }
    Ok(Preflight {
        prepared,
        before,
        recovery,
        revisions,
        ownership,
        identities,
    })
}

fn journal_begin(
    storage: &StorageApi,
    batch: &OperationBatch<'_>,
    id: &str,
    before: &[(String, Option<RenderRecipe>)],
    ownership: &wc_backend::runtime_observation::OutputOwnershipSnapshot,
    identities: &str,
) -> Result<(), AppError> {
    let conn = sqlite::open_runtime_connection(&storage.cd).map_err(AppError::from_wc_error)?;
    let snapshot = serde_json::json!({"ownership":format!("{ownership:?}")}).to_string();
    display_operations::reconcile_interrupted(&conn, &renderer_session_id(), &snapshot)
        .map_err(AppError::from_wc_error)?;
    let desired: Vec<_> = batch
        .steps
        .iter()
        .map(|step| {
            serde_json::json!({
                "outputs": step.scope.named_outputs(), "recipe":step.recipe,
            })
        })
        .collect();
    display_operations::begin(&conn, id, &renderer_session_id(), &serde_json::json!({
        "requestId":batch.request_id,"outputs":batch.outputs,"before":before,"desired":desired,"instances":identities,
    }).to_string()).map_err(AppError::from_wc_error)?;
    drop(conn);
    Ok(())
}

struct Compensation {
    id: String,
    progress: wc_backend::display_executor::DisplayExecReport,
    error: AppError,
    uncertain: bool,
    before: Vec<(String, Option<RenderRecipe>)>,
    recovery: Vec<(String, PreparedTransitions)>,
}

fn compensate(
    storage: &StorageApi,
    batch: &OperationBatch<'_>,
    runtime: &mut dyn BackendRuntime,
    reporter: &mut dyn ApplyStageReporter,
    failure: Compensation,
) -> SwitchReport {
    let Compensation {
        id,
        progress,
        error,
        uncertain,
        before,
        mut recovery,
    } = failure;
    let changed: BTreeSet<String> = progress
        .attempted
        .iter()
        .flat_map(|(_, scope)| {
            scope
                .named_outputs()
                .unwrap_or(batch.outputs)
                .iter()
                .cloned()
        })
        .collect();
    // Execution stops at the first failed action. Only that action's scope has
    // uncertain cleanup; earlier independent outputs can still be compensated.
    let uncertain_outputs: BTreeSet<String> = if uncertain {
        progress
            .attempted
            .last()
            .map(|(_, scope)| {
                scope
                    .named_outputs()
                    .unwrap_or(batch.outputs)
                    .iter()
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    } else {
        BTreeSet::new()
    };
    let mut result = SwitchReport {
        operation_id: id,
        timings: None,
        outcome: SwitchOutcome::Unchanged,
        original_error: Some(error.message),
        original_error_code: Some(error.code),
        outputs: Vec::new(),
    };
    let deadline = Instant::now() + Duration::from_secs(90);
    for (output, previous) in before {
        if !changed.contains(&output) {
            result.outputs.push(OutputResult {
                output,
                outcome: SwitchOutcome::Unchanged,
                error: None,
            });
            continue;
        }
        let output_uncertain = uncertain_outputs.contains(&output);
        let restored = (|| -> Result<(), WcError> {
            if output_uncertain {
                return Err(WcError::Other(
                    "failed renderer cleanup could not be confirmed; automatic recovery stopped"
                        .into(),
                ));
            }
            if Instant::now() >= deadline {
                return Err(WcError::Other("recovery budget exhausted".into()));
            }
            let mut backends = Vec::new();
            for (backend, scope) in &progress.attempted {
                if scope
                    .named_outputs()
                    .unwrap_or(batch.outputs)
                    .contains(&output)
                    && !backends.contains(backend)
                {
                    backends.push(*backend);
                }
            }
            let actions: Vec<_> = backends
                .into_iter()
                .map(|backend| DisplayExecAction::Stop {
                    backend,
                    scope: ExecutionScope::Named(vec![output.clone()]),
                })
                .collect();
            execute_display_actions(
                storage,
                &actions,
                &DisplayExecContext {
                    known_outputs: batch.known_outputs,
                },
                runtime,
                reporter,
                Some(&result.operation_id),
            )
            .map_err(|e| e.error)?;
            if let Some(recipe) = &previous {
                let index = recovery
                    .iter()
                    .position(|(name, _)| name == &output)
                    .ok_or_else(|| WcError::Other("prepared recovery missing".into()))?;
                let (_, prepared) = recovery.remove(index);
                execute_prepared_transitions(prepared, runtime, reporter).map_err(|e| e.error)?;
                runtime.verify_recipe(&output, recipe, batch.known_outputs)?;
            } else {
                let observed = observe_output_ownership(batch.known_outputs, runtime);
                if !observed
                    .outputs
                    .iter()
                    .any(|(name, state)| name == &output && *state == OutputOwnership::Vacant)
                {
                    return Err(WcError::Other(
                        "target was previously vacant but cleanup is not confirmed".into(),
                    ));
                }
            }
            Ok(())
        })();
        result.outputs.push(OutputResult {
            output,
            outcome: if restored.is_ok() {
                SwitchOutcome::RestoredPrevious
            } else if output_uncertain {
                SwitchOutcome::Unknown
            } else {
                SwitchOutcome::RecoveryFailed
            },
            error: restored.err().map(|e| e.to_string()),
        });
    }
    let restored = result
        .outputs
        .iter()
        .filter(|r| r.outcome == SwitchOutcome::RestoredPrevious)
        .count();
    let failures = result
        .outputs
        .iter()
        .filter(|r| {
            matches!(
                r.outcome,
                SwitchOutcome::RecoveryFailed | SwitchOutcome::Unknown
            )
        })
        .count();
    result.outcome = if changed.is_empty() {
        SwitchOutcome::Unchanged
    } else if failures == 0 {
        SwitchOutcome::RestoredPrevious
    } else if restored > 0 {
        SwitchOutcome::Partial
    } else if uncertain {
        SwitchOutcome::Unknown
    } else {
        SwitchOutcome::RecoveryFailed
    };
    result
}

fn finish(storage: &StorageApi, report: &SwitchReport) {
    let recorded = sqlite::open_runtime_connection(&storage.cd).and_then(|conn| {
        display_operations::finish(
            &conn,
            &report.operation_id,
            &serde_json::to_string(report).unwrap_or_default(),
        )
    });
    if let Err(error) = recorded {
        log::warn!("could not persist display operation outcome: {error}");
    }
}

fn report_error(report: &SwitchReport) -> AppError {
    let (code, message) = match report.outcome {
        SwitchOutcome::Unchanged => (
            "display_unchanged",
            "Wallpaper unchanged; the new request failed.",
        ),
        SwitchOutcome::RestoredPrevious => (
            "display_restored_previous",
            "Apply failed; the previous wallpapers were restored.",
        ),
        SwitchOutcome::Partial => (
            "display_partial",
            "Apply failed; some displays still need recovery.",
        ),
        _ => (
            "display_recovery_failed",
            "Apply failed; display recovery could not be confirmed.",
        ),
    };
    AppError {
        code: code.into(),
        message: message.into(),
        detail: serde_json::to_string(&report).ok(),
        recoverable: true,
        suggestion: Some("Inspect the per-display result before retrying or restoring.".into()),
    }
}

impl crate::AppService {
    /// Explicit stop retains recipes and never compensates by restarting them.
    pub fn stop_displays(
        &self,
        target: crate::DisplayTarget,
        known: &[String],
    ) -> Result<SwitchReport, AppError> {
        self.stop_displays_with_runtime(
            target,
            known,
            &mut wc_backend::runtime::SystemBackendRuntime,
        )
    }

    pub fn stop_displays_with_runtime(
        &self,
        target: crate::DisplayTarget,
        known: &[String],
        runtime: &mut dyn BackendRuntime,
    ) -> Result<SwitchReport, AppError> {
        let _guard =
            RendererMutationGuard::acquire(&self.storage).map_err(AppError::from_wc_error)?;
        let outputs = match target {
            crate::DisplayTarget::AllDisplays => known.to_vec(),
            crate::DisplayTarget::Output(name) => vec![name],
            crate::DisplayTarget::Outputs(names) => names,
        };
        crate::display_target::validate_known_outputs(&outputs)
            .map_err(|e| AppError::from_wc_error(WcError::Other(e)))?;
        if outputs.is_empty() || outputs.len() > 32 {
            return Err(AppError::from_wc_error(WcError::Other(
                "stop requires 1 to 32 outputs".into(),
            )));
        }
        // Disconnected targets also receive an intent; never substitute another
        // connected output or erase the saved assignment.
        crate::output_recovery::set_output_stopped(&self.storage, &outputs, true)
            .map_err(AppError::from_wc_error)?;
        topology_still_matches(runtime, known)?;
        let revisions = assignment_revisions(&self.storage)?;
        let mut observed_names = known.to_vec();
        for name in &outputs {
            if !observed_names.contains(name) {
                observed_names.push(name.clone());
            }
        }
        let ownership = observe_output_ownership(&observed_names, runtime);
        let identity = runtime
            .renderer_identity_snapshot()
            .map_err(AppError::from_wc_error)?;
        let mut groups: Vec<(wc_core::types::Backend, Vec<String>)> = Vec::new();
        for name in &outputs {
            match ownership
                .outputs
                .iter()
                .find(|(output, _)| output == name)
                .map(|(_, state)| state)
            {
                Some(OutputOwnership::Vacant) => {}
                Some(OutputOwnership::Occupied(backend)) => {
                    if let Some((_, names)) = groups.iter_mut().find(|(b, _)| b == backend) {
                        names.push(name.clone());
                    } else {
                        groups.push((*backend, vec![name.clone()]));
                    }
                }
                other => {
                    return Err(AppError::from_wc_error(WcError::Other(format!(
                        "cannot safely stop {name}: {other:?}"
                    ))))
                }
            }
        }
        let id = format!(
            "{}-stop-{}-{}",
            renderer_session_id(),
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        let steps: Vec<_> = groups
            .into_iter()
            .map(|(backend, names)| {
                let scope = ExecutionScope::Named(names);
                TransitionStep {
                    scope: scope.clone(),
                    target: backend,
                    fallback_path: None,
                    recipe: None,
                    core_actions: vec![DisplayExecAction::Stop { backend, scope }],
                }
            })
            .collect();
        let prepared = prepare(&self.storage, &steps, &observed_names, runtime, &id)?;
        topology_still_matches(runtime, known)?;
        assignment_revisions_unchanged(&self.storage, &revisions)?;
        if observe_output_ownership(&observed_names, runtime) != ownership
            || runtime
                .renderer_identity_snapshot()
                .map_err(AppError::from_wc_error)?
                != identity
        {
            return Err(AppError::from_wc_error(WcError::Other(
                "renderer ownership changed during stop preflight".into(),
            )));
        }
        let conn =
            sqlite::open_runtime_connection(&self.storage.cd).map_err(AppError::from_wc_error)?;
        display_operations::begin(
            &conn,
            &id,
            &renderer_session_id(),
            &serde_json::json!({"kind":"stop","outputs":outputs,"instances":identity}).to_string(),
        )
        .map_err(AppError::from_wc_error)?;
        drop(conn);
        assignment_revisions_unchanged(&self.storage, &revisions)?;
        let execution = execute_prepared_transitions(
            prepared,
            runtime,
            &mut wc_backend::apply_stage::NoopReporter,
        );
        let after = observe_output_ownership(&observed_names, runtime);
        let mut report = SwitchReport {
            operation_id: id,
            timings: None,
            outcome: SwitchOutcome::Stopped,
            original_error: execution.err().map(|e| e.error.to_string()),
            original_error_code: None,
            outputs: outputs
                .into_iter()
                .map(|output| {
                    let vacant = after
                        .outputs
                        .iter()
                        .any(|(name, state)| name == &output && *state == OutputOwnership::Vacant);
                    OutputResult {
                        output,
                        outcome: if vacant {
                            SwitchOutcome::Stopped
                        } else {
                            SwitchOutcome::Unknown
                        },
                        error: (!vacant).then(|| "renderer absence could not be confirmed".into()),
                    }
                })
                .collect(),
        };
        if report.original_error.is_some()
            || report
                .outputs
                .iter()
                .any(|r| r.outcome != SwitchOutcome::Stopped)
        {
            report.outcome = SwitchOutcome::Partial;
            finish(&self.storage, &report);
            return Err(AppError { code: "display_stop_partial".into(), message: "Some displays could not be stopped; automatic recovery remains disabled for the selected outputs.".into(), detail: serde_json::to_string(&report).ok(), recoverable: true, suggestion: Some("Inspect each output before retrying Stop.".into()) });
        }
        finish(&self.storage, &report);
        Ok(report)
    }
}
