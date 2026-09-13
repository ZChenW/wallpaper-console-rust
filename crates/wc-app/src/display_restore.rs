//! Display-aware restore orchestration for AppService.
//!
//! Recreates persisted connected display assignments via the same capability
//! planner and display executor used by apply. Disconnected rows stay as
//! preferences. Successful restore keeps wallpaper mappings intact while
//! updating connected rows to the renderer that actually started; failures
//! after destructive progress reconcile live truth without claiming success.

use std::path::Path;

use wc_backend::apply_stage::{self, ApplyStageReporter, NoopReporter};
use wc_backend::apply_transition::{
    execute_apply_transitions, TransitionRequest, TransitionStart, TransitionStep,
};
use wc_backend::display_executor::DisplayExecReport;
use wc_backend::runtime::{BackendRuntime, SystemBackendRuntime};
#[cfg(test)]
use wc_config::ConfigDirExt;
use wc_core::types::Backend;
use wc_storage::sqlite::{DisplayStateRow, DisplayStateTarget};

use crate::display_apply::{
    display_exec_failure_from_transition, reconcile_display_state_from_report,
    rejection_to_app_error, to_exec_action, transition_scope_for_target,
};
use crate::display_plan::{
    plan_display_apply, DisplayApplyRequest, DisplayTarget, PlannedAction, RunningAssignment,
};
use crate::{AppError, AppService};

/// Optional knobs for [`AppService::restore_displays_with_runtime`].
#[derive(Default)]
pub struct DisplayRestoreRuntimeOpts {
    pub request_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RestoreStep {
    target: DisplayTarget,
    path: String,
    backend: Backend,
}

impl AppService {
    /// Restore persisted connected display assignments for the given outputs.
    pub fn restore_displays(&self, known_outputs: &[String]) -> Result<(), AppError> {
        let mut runtime = SystemBackendRuntime;
        let mut reporter = NoopReporter;
        self.restore_displays_with_runtime(
            known_outputs,
            &mut runtime,
            &mut reporter,
            DisplayRestoreRuntimeOpts::default(),
        )
    }

    /// Injectable seam for tests (fake runtime + stage reporter).
    pub fn restore_displays_with_runtime(
        &self,
        known_outputs: &[String],
        runtime: &mut dyn BackendRuntime,
        reporter: &mut dyn ApplyStageReporter,
        opts: DisplayRestoreRuntimeOpts,
    ) -> Result<(), AppError> {
        self.restore_displays_with_runtime_and_commit_seam(
            known_outputs,
            runtime,
            reporter,
            opts,
            None,
        )
    }

    /// Same as [`Self::restore_displays_with_runtime`] with a transaction-level
    /// state-commit seam for failure-injection tests.
    #[doc(hidden)]
    pub fn restore_displays_with_runtime_and_commit_seam(
        &self,
        known_outputs: &[String],
        runtime: &mut dyn BackendRuntime,
        reporter: &mut dyn ApplyStageReporter,
        opts: DisplayRestoreRuntimeOpts,
        mut before_state_commit: Option<&mut dyn FnMut() -> Result<(), wc_core::error::WcError>>,
    ) -> Result<(), AppError> {
        let _guard = crate::output_recovery::RendererMutationGuard::acquire(&self.storage)
            .map_err(AppError::from_wc_error)?;
        let request_id = opts.request_id.as_deref();
        apply_stage::report_stage(reporter, apply_stage::ApplyStage::ResolveTarget, request_id);

        let previous_rows = self
            .storage
            .display_state_list()
            .map_err(AppError::from_wc_error)?;
        let mut steps = Vec::new();
        for step in build_restore_steps(&previous_rows, known_outputs) {
            let user_unsupported = self
                .storage
                .user_unsupported_contains_path(&step.path)
                .map_err(AppError::from_wc_error)?;
            if !user_unsupported {
                steps.push(step);
            }
        }
        if steps.is_empty() {
            return Ok(());
        }

        for step in &mut steps {
            ensure_wallpaper_present(&step.path)?;
            let target = self.resolve_apply_target(&step.path)?;
            step.path = target.resolved_path;
            step.backend = target.backend;
        }
        let restored_state = restored_display_state(&previous_rows, known_outputs, &steps);
        let mixed = steps.iter().any(|step| step.backend == Backend::Awww)
            && steps.iter().any(|step| step.backend == Backend::Mpvpaper);
        if mixed {
            // Resolve legacy AllDisplays + named overrides before reordering renderers.
            if let Some(base) = steps
                .iter()
                .find(|step| step.target == DisplayTarget::AllDisplays)
            {
                steps = known_outputs
                    .iter()
                    .map(|output| {
                        let effective = steps
                            .iter()
                            .rev()
                            .find(|step| step.target == DisplayTarget::Output(output.clone()))
                            .unwrap_or(base);
                        RestoreStep {
                            target: DisplayTarget::Output(output.clone()),
                            path: effective.path.clone(),
                            backend: effective.backend,
                        }
                    })
                    .collect();
            }
        }
        // Establish the shared image daemon before starting videos. Its unused
        // surfaces are transparently released by each named mpvpaper apply.
        // Keep order within each backend stable (including saved overrides).
        steps.sort_by_key(|step| if step.backend == Backend::Awww { 0 } else { 1 });

        // Preflight the full sequence with accumulating live assignments so a
        // later coexistence/capability rejection never partially executes.
        let mut preflight_running: Vec<RunningAssignment> = Vec::new();
        let mut prepared_steps = Vec::new();
        for step in steps {
            let same_backend_already_running = preflight_running
                .iter()
                .any(|assignment| assignment.backend == step.backend);
            let request = DisplayApplyRequest {
                target: step.target.clone(),
                backend: step.backend,
                known_outputs: known_outputs.to_vec(),
                running: preflight_running.clone(),
            };
            let plan = plan_display_apply(&request).map_err(rejection_to_app_error)?;
            let plan_has_stop = plan.actions.iter().any(|action| {
                matches!(
                    action,
                    PlannedAction::Stop { .. } | PlannedAction::StopBackend { .. }
                )
            });
            let use_instant = plan_has_stop || !same_backend_already_running;
            let actions: Vec<_> = plan
                .actions
                .into_iter()
                .map(|action| {
                    to_exec_action(action, &step.path, &step.target, known_outputs, use_instant)
                })
                .collect::<Result<Vec<_>, _>>()?;
            let fallback_path = restore_fallback_path(&step.path);
            update_running_after_step(&mut preflight_running, known_outputs, &step);
            prepared_steps.push(TransitionStep {
                scope: transition_scope_for_target(&step.target, known_outputs)?,
                target: step.backend,
                core_actions: actions,
                fallback_path,
            });
        }

        let report = match execute_apply_transitions(
            &self.storage,
            TransitionRequest {
                start: TransitionStart::Restore,
                known_outputs,
                steps: &prepared_steps,
                request_id,
            },
            runtime,
            reporter,
        ) {
            Ok(report) => report.exec,
            Err(failure) => {
                return Err(self.handle_exec_failure(
                    display_exec_failure_from_transition(failure),
                    &previous_rows,
                    known_outputs,
                    None,
                )?)
            }
        };

        let commit_result = match before_state_commit.as_deref_mut() {
            Some(seam) => self
                .storage
                .display_state_replace_all_seam(&restored_state, seam),
            None => self.storage.display_state_replace_all(&restored_state),
        };
        if let Err(commit_error) = commit_result {
            return Err(self.reconcile_restore_commit_failure(
                commit_error,
                &previous_rows,
                &report,
                known_outputs,
                before_state_commit,
            ));
        }

        if self.storage.config_get("post_apply_on_restore", "on") == "on" {
            self.publish_theme_after_restore(&restored_state, known_outputs);
        }
        if runtime.supports_output_recovery() {
            crate::output_recovery::ensure_watcher(&self.storage);
        }
        Ok(())
    }

    fn publish_theme_after_restore(
        &self,
        restored_state: &[(DisplayStateTarget, String, String)],
        known_outputs: &[String],
    ) {
        let ctx = crate::post_apply::build_theme_context(
            &self.storage,
            &crate::post_apply::ThemePublishRequest {
                intended: restored_state,
                known_outputs,
                changed_outputs: known_outputs,
                applied: None,
            },
        );
        crate::post_apply::publish_theme_and_run_hook(&self.storage, &ctx);
    }

    fn reconcile_restore_commit_failure(
        &self,
        commit_error: wc_core::error::WcError,
        previous_rows: &[DisplayStateRow],
        report: &DisplayExecReport,
        known_outputs: &[String],
        before_reconcile: Option<&mut dyn FnMut() -> Result<(), wc_core::error::WcError>>,
    ) -> AppError {
        let reconciled = reconcile_display_state_from_report(previous_rows, report, known_outputs);
        let reconciliation_result = self
            .storage
            .display_state_replace_all_and_clear_legacy(&reconciled, before_reconcile);

        match reconciliation_result {
            Ok(()) => AppError {
                code: "display_restore_state_commit_failed".into(),
                message: "Wallpapers were restored. The intended state commit failed, but the \
                          actual live display state was reconciled."
                    .into(),
                detail: Some(format!(
                    "commit_error={commit_error}; reconciliation=ok; legacy_state_clear=ok"
                )),
                recoverable: true,
                suggestion: Some(
                    "Review display status and retry restore if the intended assignments differ."
                        .into(),
                ),
            },
            Err(error) => AppError {
                code: "display_state_uncertain".into(),
                message: "Wallpapers were restored, but persisted display state is uncertain."
                    .into(),
                detail: Some(format!(
                    "commit_error={commit_error}; reconciliation_transaction_error={error}"
                )),
                recoverable: true,
                suggestion: Some(
                    "Refresh renderer status before applying or restoring another wallpaper."
                        .into(),
                ),
            },
        }
    }
}

fn restored_display_state(
    rows: &[DisplayStateRow],
    known_outputs: &[String],
    steps: &[RestoreStep],
) -> Vec<(DisplayStateTarget, String, String)> {
    let all_path = rows.iter().find_map(|row| {
        matches!(row.target, DisplayStateTarget::AllDisplays).then_some(row.wallpaper_path.as_str())
    });
    let all_backend = steps
        .iter()
        .find_map(|step| matches!(step.target, DisplayTarget::AllDisplays).then_some(step.backend));

    rows.iter()
        .map(|row| {
            let restored_backend = match &row.target {
                DisplayStateTarget::AllDisplays => all_backend,
                DisplayStateTarget::Output(output) if known_outputs.contains(output) => steps
                    .iter()
                    .find_map(|step| match &step.target {
                        DisplayTarget::Output(target) if target == output => Some(step.backend),
                        _ => None,
                    })
                    .or_else(|| {
                        (all_path == Some(row.wallpaper_path.as_str()))
                            .then_some(all_backend)
                            .flatten()
                    }),
                DisplayStateTarget::Output(_) => None,
            };
            (
                row.target.clone(),
                row.wallpaper_path.clone(),
                restored_backend
                    .map(|backend| backend.as_str().to_string())
                    .unwrap_or_else(|| row.backend.clone()),
            )
        })
        .collect()
}

fn restore_fallback_path(path: &str) -> Option<String> {
    let entry = wc_scan::make_entry(path)?;
    match entry.file_type {
        wc_core::types::FileType::Image | wc_core::types::FileType::Gif => {
            Some(entry.path.to_string())
        }
        _ => None,
    }
}

fn build_restore_steps(rows: &[DisplayStateRow], known_outputs: &[String]) -> Vec<RestoreStep> {
    let all_row = rows
        .iter()
        .find(|row| matches!(row.target, DisplayStateTarget::AllDisplays));

    let mut connected_overrides: Vec<(String, String)> = Vec::new();
    for row in rows {
        let DisplayStateTarget::Output(name) = &row.target else {
            continue;
        };
        if !known_outputs.iter().any(|known| known == name) {
            continue;
        }
        if let Some(existing) = connected_overrides
            .iter_mut()
            .find(|(output, _)| output == name)
        {
            existing.1 = row.wallpaper_path.clone();
        } else {
            connected_overrides.push((name.clone(), row.wallpaper_path.clone()));
        }
    }

    let mut steps = Vec::new();
    if let Some(all) = all_row {
        steps.push(RestoreStep {
            target: DisplayTarget::AllDisplays,
            path: all.wallpaper_path.clone(),
            backend: Backend::Unsupported,
        });
    }

    for (output, path) in connected_overrides {
        if let Some(all) = all_row {
            if path == all.wallpaper_path {
                // Covered by the AllDisplays step; named row is redundant.
                continue;
            }
        }
        steps.push(RestoreStep {
            target: DisplayTarget::Output(output),
            path,
            backend: Backend::Unsupported,
        });
    }

    steps
}

fn ensure_wallpaper_present(path: &str) -> Result<(), AppError> {
    let p = Path::new(path);
    if p.is_file() || p.is_dir() {
        if std::fs::File::open(p).is_err() {
            return Err(AppError {
                code: "wallpaper_unreadable".into(),
                message: format!("Cannot restore unreadable wallpaper: {path}"),
                detail: None,
                recoverable: true,
                suggestion: Some("Fix file permissions or re-apply a readable wallpaper.".into()),
            });
        }
        return Ok(());
    }
    Err(AppError {
        code: "wallpaper_missing".into(),
        message: format!("Cannot restore missing wallpaper: {path}"),
        detail: Some(path.to_string()),
        recoverable: true,
        suggestion: Some(
            "Re-apply an available wallpaper or remove the stale display preference.".into(),
        ),
    })
}

fn update_running_after_step(
    running: &mut Vec<RunningAssignment>,
    known_outputs: &[String],
    step: &RestoreStep,
) {
    match &step.target {
        DisplayTarget::AllDisplays => {
            *running = known_outputs
                .iter()
                .map(|output| RunningAssignment {
                    output: output.clone(),
                    backend: step.backend,
                })
                .collect();
        }
        DisplayTarget::Output(output) => {
            if let Some(existing) = running.iter_mut().find(|a| a.output == *output) {
                existing.backend = step.backend;
            } else {
                running.push(RunningAssignment {
                    output: output.clone(),
                    backend: step.backend,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::path::Path;
    use std::process::Command;
    use wc_backend::apply_stage::NoopReporter;
    use wc_backend::runtime::{AwwwReadiness, MpvpaperProcess, ProcessIo};
    use wc_core::config::ConfigDir;
    use wc_core::error::WcError;

    #[derive(Default)]
    struct FakeRuntime {
        missing_backend: Option<Backend>,
        stop_awww_count: usize,
        stop_mpvpaper_count: usize,
        stop_mpvpaper_outputs_calls: Vec<Vec<String>>,
        stop_lwe_count: usize,
        stop_awww_error: Option<String>,
        stop_mpvpaper_error: Option<String>,
        command_output_success: bool,
        command_status_success: bool,
        command_output_args: Vec<Vec<String>>,
        command_status_args: Vec<Vec<String>>,
        fail_after_n_status: Option<usize>,
        mpvpaper_events: Vec<String>,
        mpvpaper_ready_pid: Option<u32>,
        running_mpvpaper_pids: Vec<u32>,
        mpvpaper_process_table: Vec<MpvpaperProcess>,
        awww_readiness_sequence: RefCell<Vec<AwwwReadiness>>,
        lwe_apply_calls: usize,
        lwe_apply_error: Option<String>,
        awww_stop_verify_pending: bool,
        mpvpaper_pids_error: Option<String>,
        failed_launch_cleanup_error: Option<String>,
        awww_version: Option<String>,
    }

    impl ProcessIo for FakeRuntime {
        fn awww_environment(
            &mut self,
            _inspect_daemons: bool,
        ) -> Result<wc_backend::runtime::AwwwEnvironment, WcError> {
            Ok(wc_backend::runtime::AwwwEnvironment {
                niri_socket_present: true,
                version: Some(
                    self.awww_version
                        .clone()
                        .unwrap_or_else(|| "awww 0.12.0".into()),
                ),
                daemon_commands: Vec::new(),
            })
        }

        fn awww_query_json(&mut self) -> Result<String, WcError> {
            let outputs = self
                .command_output_args
                .iter()
                .rev()
                .find(|args| args.first().is_some_and(|arg| arg == "clear"))
                .and_then(|args| args.get(2))
                .map(|names| {
                    names
                        .split(',')
                        .map(|name| serde_json::json!({"name":name,"displaying":{"color":"#0"}}))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            Ok(serde_json::json!({"awww-daemon":outputs}).to_string())
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
            let args = self.command_status_args.last().unwrap();
            if args.iter().any(|arg| arg == "mpvpaper") {
                let separator = args.iter().position(|arg| arg == "--").unwrap();
                self.mpvpaper_events
                    .push(format!("apply:{}", args[separator - 1]));
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
            let mut pids = self.running_mpvpaper_pids.clone();
            for process in &self.mpvpaper_process_table {
                if !pids.contains(&process.pid) {
                    pids.push(process.pid);
                }
            }
            Ok(pids)
        }

        fn mpvpaper_processes(&mut self) -> Result<Vec<MpvpaperProcess>, WcError> {
            Ok(self.mpvpaper_process_table.clone())
        }

        fn wait_for_mpvpaper_ready(
            &mut self,
            _previous_pids: &[u32],
            _output: &str,
            _path: &str,
        ) -> Result<u32, WcError> {
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
            if let Some(error) = &self.failed_launch_cleanup_error {
                return Err(WcError::Other(error.clone()));
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
        fn ensure_backend_available(
            &mut self,
            backend: Backend,
            _storage: &wc_storage::StorageApi,
        ) -> Result<(), WcError> {
            if self.missing_backend == Some(backend) {
                Err(WcError::BackendNotFound(backend.as_str().into()))
            } else {
                Ok(())
            }
        }

        fn stop_awww(&mut self) {
            self.stop_awww_count += 1;
            self.awww_stop_verify_pending = true;
        }

        fn stop_mpvpaper(&mut self) {
            self.stop_mpvpaper_count += 1;
            self.mpvpaper_events.push("stop".into());
            if let Some(message) = &self.stop_mpvpaper_error {
                self.mpvpaper_pids_error = Some(message.clone());
            } else {
                self.running_mpvpaper_pids.clear();
                self.mpvpaper_process_table.clear();
            }
        }

        fn stop_mpvpaper_outputs(&mut self, outputs: &[String]) -> Result<(), WcError> {
            self.stop_mpvpaper_outputs_calls.push(outputs.to_vec());
            self.mpvpaper_events
                .push(format!("stop-outputs:[{}]", outputs.join(",")));
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

    fn restore_service_with_changed_backend() -> (tempfile::TempDir, AppService, std::path::PathBuf)
    {
        let (tmp, service) = temp_service();
        let image = write_image(tmp.path(), "changed-backend.jpg");
        service
            .storage_for_tests()
            .config_set("image_backend", "mpvpaper")
            .unwrap();
        service
            .storage_for_tests()
            .display_state_upsert(
                &DisplayStateTarget::AllDisplays,
                &image.to_string_lossy(),
                "awww",
            )
            .unwrap();
        service
            .storage_for_tests()
            .runtime_state_write_pair(&image.to_string_lossy(), "awww")
            .unwrap();
        (tmp, service, image)
    }

    #[test]
    fn all_displays_state_restores_to_connected_outputs() {
        let (tmp, service) = temp_service();
        let img = write_image(tmp.path(), "all.jpg");
        service
            .storage_for_tests()
            .display_state_upsert(
                &DisplayStateTarget::AllDisplays,
                &img.to_string_lossy(),
                "awww",
            )
            .unwrap();

        let mut rt = FakeRuntime {
            command_output_success: true,
            command_status_success: true,
            ..Default::default()
        };
        let mut reporter = NoopReporter;
        service
            .restore_displays_with_runtime(
                &["eDP-1".into(), "HDMI-1".into()],
                &mut rt,
                &mut reporter,
                DisplayRestoreRuntimeOpts::default(),
            )
            .unwrap();

        assert!(
            !rt.command_output_args.is_empty(),
            "expected at least one awww apply"
        );
        assert!(
            !rt.command_output_args[0].iter().any(|a| a == "--outputs"),
            "AllDisplays restore must omit --outputs: {:?}",
            rt.command_output_args[0]
        );
        let rows = service.storage_for_tests().display_state_list().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].target, DisplayStateTarget::AllDisplays);
        assert_eq!(rows[0].wallpaper_path, img.to_string_lossy());
        assert_eq!(rt.stop_awww_count, 1);
        assert_eq!(rt.stop_mpvpaper_count, 1);
        assert_eq!(rt.stop_lwe_count, 1);
        assert_eq!(rt.lwe_apply_calls, 0);
    }

    fn dual_video_service() -> (tempfile::TempDir, AppService, std::path::PathBuf) {
        let (tmp, service) = temp_service();
        let video = write_video(tmp.path(), "dual.mp4");
        service
            .storage_for_tests()
            .config_set("video_backend", "mpvpaper")
            .unwrap();
        service
            .storage_for_tests()
            .display_state_upsert(
                &DisplayStateTarget::AllDisplays,
                &video.to_string_lossy(),
                "mpvpaper",
            )
            .unwrap();
        (tmp, service, video)
    }

    #[test]
    fn dual_video_restore_restarts_once_and_applies_each_named_output() {
        let (_tmp, service, video) = dual_video_service();
        let outputs = ["eDP-1".into(), "HDMI-1".into()];
        let mut runtime = FakeRuntime {
            command_output_success: true,
            command_status_success: true,
            ..Default::default()
        };
        // Repeated restore clears the renderer set once per restore, with no
        // stop between launching the first and second screen.
        for round in 1..=2 {
            service
                .restore_displays_with_runtime(
                    &outputs,
                    &mut runtime,
                    &mut NoopReporter,
                    DisplayRestoreRuntimeOpts::default(),
                )
                .unwrap();
            assert_eq!(runtime.stop_mpvpaper_count, round);
            assert_eq!(runtime.command_status_args.len(), round * 2);
            assert_eq!(
                runtime.mpvpaper_events,
                ["stop", "apply:eDP-1", "apply:HDMI-1"].repeat(round)
            );
            let rows = service.storage_for_tests().display_state_list().unwrap();
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].target, DisplayStateTarget::AllDisplays);
            assert_eq!(rows[0].wallpaper_path, video.to_string_lossy());
            assert_eq!(rows[0].backend, "mpvpaper");
        }
    }

    #[test]
    fn restore_uncertain_second_launch_keeps_first_output_progress() {
        let (_tmp, service, video) = dual_video_service();
        let mut runtime = FakeRuntime {
            command_output_success: true,
            command_status_success: true,
            fail_after_n_status: Some(1),
            failed_launch_cleanup_error: Some("injected unverifiable cleanup".into()),
            ..Default::default()
        };
        let error = service
            .restore_displays_with_runtime(
                &["eDP-1".into(), "DP-8".into()],
                &mut runtime,
                &mut NoopReporter,
                DisplayRestoreRuntimeOpts::default(),
            )
            .unwrap_err();
        assert_eq!(error.code, "display_state_uncertain");
        assert!(error
            .detail
            .unwrap()
            .contains("injected unverifiable cleanup"));
        let rows = service.storage_for_tests().display_state_list().unwrap();
        assert_eq!(
            rows.len(),
            1,
            "must retain only the confirmed first Apply: {rows:?}"
        );
        assert_eq!(rows[0].target, DisplayStateTarget::Output("eDP-1".into()));
        assert_eq!(rows[0].wallpaper_path, video.to_string_lossy());
        assert_eq!(
            runtime.mpvpaper_events,
            ["stop", "apply:eDP-1", "apply:DP-8"]
        );
    }

    #[test]
    fn dual_video_restore_second_launch_failure_removes_stopped_output_state() {
        let (_tmp, service, video) = dual_video_service();
        let mut runtime = FakeRuntime {
            command_output_success: true,
            command_status_success: true,
            fail_after_n_status: Some(1),
            ..Default::default()
        };
        let err = service
            .restore_displays_with_runtime(
                &["eDP-1".into(), "HDMI-1".into()],
                &mut runtime,
                &mut NoopReporter,
                DisplayRestoreRuntimeOpts::default(),
            )
            .unwrap_err();
        let rows = service.storage_for_tests().display_state_list().unwrap();
        assert_eq!(
            rows.len(),
            1,
            "failed output must not retain stopped state: {rows:?}"
        );
        assert_eq!(rows[0].target, DisplayStateTarget::Output("eDP-1".into()));
        assert_eq!(rows[0].wallpaper_path, video.to_string_lossy());
        assert_eq!(rows[0].backend, "mpvpaper");
        assert_eq!(err.code, "display_apply_failed_after_stop");
        assert_eq!(
            runtime.mpvpaper_events,
            ["stop", "apply:eDP-1", "apply:HDMI-1"]
        );
    }

    #[test]
    fn dual_video_restore_first_launch_failure_clears_stopped_state() {
        let (_tmp, service, _video) = dual_video_service();
        let mut runtime = FakeRuntime {
            command_output_success: true,
            command_status_success: false,
            ..Default::default()
        };
        let err = service
            .restore_displays_with_runtime(
                &["eDP-1".into(), "HDMI-1".into()],
                &mut runtime,
                &mut NoopReporter,
                DisplayRestoreRuntimeOpts::default(),
            )
            .unwrap_err();
        let rows = service.storage_for_tests().display_state_list().unwrap();
        assert!(
            rows.is_empty(),
            "stopped renderers must not remain saved as live: {rows:?}"
        );
        assert_eq!(err.code, "display_apply_failed_after_stop");
        assert_eq!(runtime.mpvpaper_events, ["stop", "apply:eDP-1"]);
    }

    #[test]
    fn display_restore_re_resolves_backend_from_current_safe_settings() {
        let (tmp, service) = temp_service();
        let image = write_image(tmp.path(), "configured.jpg");
        service
            .storage_for_tests()
            .config_set("image_backend", "mpvpaper")
            .unwrap();
        service
            .storage_for_tests()
            .display_state_upsert(
                &DisplayStateTarget::Output("eDP-1".into()),
                &image.to_string_lossy(),
                "awww",
            )
            .unwrap();
        let mut runtime = FakeRuntime {
            command_output_success: true,
            command_status_success: true,
            ..Default::default()
        };
        let mut reporter = NoopReporter;

        service
            .restore_displays_with_runtime(
                &["eDP-1".into()],
                &mut runtime,
                &mut reporter,
                DisplayRestoreRuntimeOpts::default(),
            )
            .unwrap();

        assert!(runtime.command_output_args.is_empty());
        assert!(runtime
            .command_status_args
            .iter()
            .flatten()
            .any(|arg| arg == "mpvpaper"));
        let rows = service.storage_for_tests().display_state_list().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].backend, "mpvpaper");
    }

    #[test]
    fn display_restore_clamps_legacy_video_awww_state_to_mpvpaper() {
        let (tmp, service) = temp_service();
        let video = write_video(tmp.path(), "legacy.mp4");
        service
            .storage_for_tests()
            .config_set("video_backend", "awww")
            .unwrap();
        service
            .storage_for_tests()
            .display_state_upsert(
                &DisplayStateTarget::Output("eDP-1".into()),
                &video.to_string_lossy(),
                "awww",
            )
            .unwrap();
        let mut runtime = FakeRuntime {
            command_output_success: true,
            command_status_success: true,
            ..Default::default()
        };
        let mut reporter = NoopReporter;

        service
            .restore_displays_with_runtime(
                &["eDP-1".into()],
                &mut runtime,
                &mut reporter,
                DisplayRestoreRuntimeOpts::default(),
            )
            .unwrap();

        assert!(runtime.command_output_args.is_empty());
        assert!(runtime
            .command_status_args
            .iter()
            .flatten()
            .any(|arg| arg == "mpvpaper"));
    }

    #[test]
    fn mixed_legacy_override_is_expanded_before_renderer_ordering() {
        let (tmp, service) = temp_service();
        let video = write_video(tmp.path(), "base.mp4");
        let image = write_image(tmp.path(), "override.jpg");
        service
            .storage_for_tests()
            .display_state_replace_all(&[
                (
                    DisplayStateTarget::AllDisplays,
                    video.to_string_lossy().into(),
                    "mpvpaper".into(),
                ),
                (
                    DisplayStateTarget::Output("eDP-1".into()),
                    image.to_string_lossy().into(),
                    "awww".into(),
                ),
            ])
            .unwrap();
        let mut runtime = FakeRuntime {
            command_output_success: true,
            command_status_success: true,
            ..Default::default()
        };
        service
            .restore_displays_with_runtime(
                &["eDP-1".into(), "HDMI-1".into()],
                &mut runtime,
                &mut NoopReporter,
                DisplayRestoreRuntimeOpts::default(),
            )
            .unwrap();
        assert_eq!(runtime.mpvpaper_events, ["stop", "apply:HDMI-1"]);
        let image_args = runtime
            .command_output_args
            .iter()
            .find(|args| args.iter().any(|arg| arg == image.to_str().unwrap()))
            .unwrap();
        assert!(image_args
            .windows(2)
            .any(|pair| pair == ["--outputs", "eDP-1"]));
        let rows = service.storage_for_tests().display_state_list().unwrap();
        assert!(rows.iter().any(
            |row| row.target == DisplayStateTarget::Output("eDP-1".into()) && row.backend == "awww"
        ));
    }

    #[test]
    fn fully_overridden_video_base_does_not_require_transparency() {
        let (tmp, service) = temp_service();
        let video = write_video(tmp.path(), "base.mp4");
        let image = write_image(tmp.path(), "override.jpg");
        service
            .storage_for_tests()
            .display_state_replace_all(&[
                (
                    DisplayStateTarget::AllDisplays,
                    video.to_string_lossy().into(),
                    "mpvpaper".into(),
                ),
                (
                    DisplayStateTarget::Output("eDP-1".into()),
                    image.to_string_lossy().into(),
                    "awww".into(),
                ),
            ])
            .unwrap();
        let mut runtime = FakeRuntime {
            awww_version: Some("awww 0.11.0".into()),
            command_output_success: true,
            command_status_success: true,
            ..Default::default()
        };
        service
            .restore_displays_with_runtime(
                &["eDP-1".into()],
                &mut runtime,
                &mut NoopReporter,
                DisplayRestoreRuntimeOpts::default(),
            )
            .unwrap();
        assert_eq!(runtime.mpvpaper_events, ["stop"]);
    }

    #[test]
    fn unsupported_cold_mixed_restore_is_rejected_before_any_stop() {
        let (tmp, service) = temp_service();
        let video = write_video(tmp.path(), "base.mp4");
        let image = write_image(tmp.path(), "override.jpg");
        service
            .storage_for_tests()
            .display_state_replace_all(&[
                (
                    DisplayStateTarget::Output("HDMI-1".into()),
                    video.to_string_lossy().into(),
                    "mpvpaper".into(),
                ),
                (
                    DisplayStateTarget::Output("eDP-1".into()),
                    image.to_string_lossy().into(),
                    "awww".into(),
                ),
            ])
            .unwrap();
        let before = service.storage_for_tests().display_state_list().unwrap();
        let mut runtime = FakeRuntime {
            awww_version: Some("awww 0.11.0".into()),
            ..Default::default()
        };
        let result = service.restore_displays_with_runtime(
            &["eDP-1".into(), "HDMI-1".into()],
            &mut runtime,
            &mut NoopReporter,
            DisplayRestoreRuntimeOpts::default(),
        );
        assert!(result.is_err());
        assert!(runtime.mpvpaper_events.is_empty());
        assert_eq!(runtime.stop_awww_count, 0);
        assert!(runtime.command_output_args.is_empty());
        assert_eq!(
            service.storage_for_tests().display_state_list().unwrap(),
            before
        );
    }

    #[test]
    fn named_override_wins_over_all_displays_on_restore() {
        let (tmp, service) = temp_service();
        let all = write_image(tmp.path(), "all.jpg");
        let named = write_image(tmp.path(), "named.jpg");
        service
            .storage_for_tests()
            .display_state_replace_all(&[
                (
                    DisplayStateTarget::AllDisplays,
                    all.to_string_lossy().into(),
                    "awww".into(),
                ),
                (
                    DisplayStateTarget::Output("eDP-1".into()),
                    named.to_string_lossy().into(),
                    "awww".into(),
                ),
            ])
            .unwrap();

        let mut rt = FakeRuntime {
            command_output_success: true,
            command_status_success: true,
            ..Default::default()
        };
        let mut reporter = NoopReporter;
        service
            .restore_displays_with_runtime(
                &["eDP-1".into(), "HDMI-1".into()],
                &mut rt,
                &mut reporter,
                DisplayRestoreRuntimeOpts::default(),
            )
            .unwrap();

        assert!(
            rt.command_output_args.len() >= 2,
            "AllDisplays then named override: {:?}",
            rt.command_output_args
        );
        assert!(
            !rt.command_output_args[0].iter().any(|a| a == "--outputs"),
            "first step is AllDisplays"
        );
        let named_args = rt
            .command_output_args
            .iter()
            .find(|args| args.iter().any(|a| a == "--outputs"))
            .expect("named override must use --outputs");
        let idx = named_args.iter().position(|a| a == "--outputs").unwrap();
        assert_eq!(named_args[idx + 1], "eDP-1");
        assert!(
            named_args.iter().any(|a| a.contains("named.jpg")),
            "named wallpaper must win: {named_args:?}"
        );

        let rows = service.storage_for_tests().display_state_list().unwrap();
        assert!(rows
            .iter()
            .any(|r| r.target == DisplayStateTarget::AllDisplays));
        assert!(rows.iter().any(|r| {
            r.target == DisplayStateTarget::Output("eDP-1".into())
                && r.wallpaper_path == named.to_string_lossy()
        }));
    }

    #[test]
    fn disconnected_rows_are_preserved_as_preferences() {
        let (tmp, service) = temp_service();
        let img = write_image(tmp.path(), "all.jpg");
        service
            .storage_for_tests()
            .display_state_replace_all(&[
                (
                    DisplayStateTarget::AllDisplays,
                    img.to_string_lossy().into(),
                    "awww".into(),
                ),
                (
                    DisplayStateTarget::Output("DP-ghost".into()),
                    "/walls/ghost.jpg".into(),
                    "mpvpaper".into(),
                ),
            ])
            .unwrap();

        let mut rt = FakeRuntime {
            command_output_success: true,
            command_status_success: true,
            ..Default::default()
        };
        let mut reporter = NoopReporter;
        service
            .restore_displays_with_runtime(
                &["eDP-1".into()],
                &mut rt,
                &mut reporter,
                DisplayRestoreRuntimeOpts::default(),
            )
            .unwrap();

        let rows = service.storage_for_tests().display_state_list().unwrap();
        assert!(
            rows.iter().any(|r| {
                r.target == DisplayStateTarget::Output("DP-ghost".into())
                    && r.wallpaper_path == "/walls/ghost.jpg"
                    && r.backend == "mpvpaper"
            }),
            "disconnected preference must remain: {rows:?}"
        );
        assert!(
            !rt.command_output_args
                .iter()
                .any(|args| args.iter().any(|a| a.contains("ghost"))),
            "must not attempt to restore disconnected output"
        );
    }

    #[test]
    fn missing_wallpaper_returns_structured_error_without_deleting_preferences() {
        let (_tmp, service) = temp_service();
        service
            .storage_for_tests()
            .display_state_replace_all(&[
                (
                    DisplayStateTarget::AllDisplays,
                    "/missing/all.jpg".into(),
                    "awww".into(),
                ),
                (
                    DisplayStateTarget::Output("DP-ghost".into()),
                    "/walls/ghost.jpg".into(),
                    "awww".into(),
                ),
            ])
            .unwrap();
        let before = service.storage_for_tests().display_state_list().unwrap();

        let mut rt = FakeRuntime {
            command_output_success: true,
            command_status_success: true,
            ..Default::default()
        };
        let mut reporter = NoopReporter;
        let err = service
            .restore_displays_with_runtime(
                &["eDP-1".into()],
                &mut rt,
                &mut reporter,
                DisplayRestoreRuntimeOpts::default(),
            )
            .unwrap_err();

        assert_eq!(err.code, "wallpaper_missing");
        assert!(rt.command_output_args.is_empty());
        assert_eq!(rt.stop_awww_count, 0);
        let after = service.storage_for_tests().display_state_list().unwrap();
        assert_eq!(after, before);
    }

    #[test]
    fn unavailable_restore_backend_is_rejected_before_global_stop() {
        let (tmp, service) = temp_service();
        let video = write_video(tmp.path(), "motion.mp4");
        service
            .storage_for_tests()
            .display_state_upsert(
                &DisplayStateTarget::AllDisplays,
                &video.to_string_lossy(),
                "mpvpaper",
            )
            .unwrap();
        let mut runtime = FakeRuntime {
            missing_backend: Some(Backend::Mpvpaper),
            command_status_success: true,
            ..Default::default()
        };
        let mut reporter = NoopReporter;

        let error = service
            .restore_displays_with_runtime(
                &["eDP-1".into()],
                &mut runtime,
                &mut reporter,
                DisplayRestoreRuntimeOpts::default(),
            )
            .unwrap_err();

        assert!(error.message.contains("mpvpaper"));
        assert_eq!(runtime.stop_awww_count, 0);
        assert_eq!(runtime.stop_mpvpaper_count, 0);
        assert_eq!(runtime.stop_lwe_count, 0);
    }

    #[test]
    fn conflicting_backend_combination_rejects_without_deleting_preferences() {
        let (tmp, service) = temp_service();
        service
            .storage_for_tests()
            .config_set("image_backend", "swaybg")
            .unwrap();
        let img = write_image(tmp.path(), "still.jpg");
        let vid = write_video(tmp.path(), "motion.mp4");
        service
            .storage_for_tests()
            .display_state_replace_all(&[
                (
                    DisplayStateTarget::Output("eDP-1".into()),
                    img.to_string_lossy().into(),
                    "awww".into(),
                ),
                (
                    DisplayStateTarget::Output("HDMI-1".into()),
                    vid.to_string_lossy().into(),
                    "mpvpaper".into(),
                ),
            ])
            .unwrap();
        let before = service.storage_for_tests().display_state_list().unwrap();

        let mut rt = FakeRuntime {
            command_output_success: true,
            command_status_success: true,
            ..Default::default()
        };
        let mut reporter = NoopReporter;
        let err = service
            .restore_displays_with_runtime(
                &["eDP-1".into(), "HDMI-1".into()],
                &mut rt,
                &mut reporter,
                DisplayRestoreRuntimeOpts::default(),
            )
            .unwrap_err();

        assert_eq!(err.code, "display_apply_rejected");
        assert!(
            err.message.contains("coexistence")
                || err.detail.as_deref().unwrap_or("").contains("Coexistence"),
            "expected coexistence rejection: {err:?}"
        );
        assert!(
            rt.command_output_args.is_empty(),
            "must not execute after reject"
        );
        assert_eq!(rt.stop_awww_count, 0);
        assert_eq!(rt.stop_mpvpaper_count, 0);
        let after = service.storage_for_tests().display_state_list().unwrap();
        assert_eq!(after, before);
    }

    #[test]
    fn partial_failure_after_stop_reconciles_truthfully_and_preserves_disconnected() {
        let (tmp, service) = temp_service();
        let img = write_image(tmp.path(), "all.jpg");
        let vid = write_video(tmp.path(), "override.mp4");
        service
            .storage_for_tests()
            .display_state_replace_all(&[
                (
                    DisplayStateTarget::AllDisplays,
                    img.to_string_lossy().into(),
                    "awww".into(),
                ),
                (
                    DisplayStateTarget::Output("eDP-1".into()),
                    vid.to_string_lossy().into(),
                    "mpvpaper".into(),
                ),
                (
                    DisplayStateTarget::Output("DP-ghost".into()),
                    "/walls/ghost.mp4".into(),
                    "mpvpaper".into(),
                ),
            ])
            .unwrap();

        let mut rt = FakeRuntime {
            command_output_success: true,
            // awww uses command_output; mpvpaper launch uses command_status.
            command_status_success: false,
            mpvpaper_ready_pid: Some(9),
            ..Default::default()
        };
        let mut reporter = NoopReporter;
        let err = service
            .restore_displays_with_runtime(
                &["eDP-1".into()],
                &mut rt,
                &mut reporter,
                DisplayRestoreRuntimeOpts::default(),
            )
            .unwrap_err();

        assert!(
            err.code == "display_apply_failed_after_stop"
                || err.code == "display_state_uncertain"
                || err.code == "command_failed",
            "must not claim success after partial failure: {err:?}"
        );
        assert!(rt.stop_awww_count >= 1, "override should stop prior awww");
        let rows = service.storage_for_tests().display_state_list().unwrap();
        assert!(
            rows.iter().any(|r| {
                r.target == DisplayStateTarget::Output("DP-ghost".into()) && r.backend == "mpvpaper"
            }),
            "disconnected preference preserved: {rows:?}"
        );
        assert!(
            rows.iter().all(|r| {
                !(matches!(r.target, DisplayStateTarget::AllDisplays) && r.backend == "awww")
            }),
            "must not claim stopped AllDisplays awww still runs: {rows:?}"
        );
    }

    #[test]
    fn uncertain_stop_does_not_claim_success_and_preserves_disconnected() {
        let (tmp, service) = temp_service();
        let img = write_image(tmp.path(), "all.jpg");
        let vid = write_video(tmp.path(), "override.mp4");
        service
            .storage_for_tests()
            .display_state_replace_all(&[
                (
                    DisplayStateTarget::AllDisplays,
                    img.to_string_lossy().into(),
                    "awww".into(),
                ),
                (
                    DisplayStateTarget::Output("eDP-1".into()),
                    vid.to_string_lossy().into(),
                    "mpvpaper".into(),
                ),
                (
                    DisplayStateTarget::Output("DP-ghost".into()),
                    "/walls/ghost.mp4".into(),
                    "mpvpaper".into(),
                ),
            ])
            .unwrap();

        let mut rt = FakeRuntime {
            command_output_success: true,
            command_status_success: true,
            stop_awww_error: Some("verification probe failed".into()),
            mpvpaper_ready_pid: Some(9),
            ..Default::default()
        };
        let mut reporter = NoopReporter;
        let err = service
            .restore_displays_with_runtime(
                &["eDP-1".into()],
                &mut rt,
                &mut reporter,
                DisplayRestoreRuntimeOpts::default(),
            )
            .unwrap_err();

        assert_eq!(err.code, "display_state_uncertain");
        let rows = service.storage_for_tests().display_state_list().unwrap();
        assert!(
            rows.iter().any(|r| {
                r.target == DisplayStateTarget::Output("DP-ghost".into()) && r.backend == "mpvpaper"
            }),
            "disconnected preference preserved: {rows:?}"
        );
        assert!(rt.stop_awww_count >= 1);
    }

    #[test]
    fn successful_restore_reconciles_live_report_after_primary_state_commit_failure() {
        let (_tmp, service, image) = restore_service_with_changed_backend();
        let mut runtime = FakeRuntime {
            command_output_success: true,
            command_status_success: true,
            mpvpaper_ready_pid: Some(9),
            ..Default::default()
        };
        let mut reporter = NoopReporter;
        let mut commit_attempts = 0;
        let mut fail_primary_commit = || {
            commit_attempts += 1;
            if commit_attempts == 1 {
                Err(WcError::Other("injected primary commit failure".into()))
            } else {
                Ok(())
            }
        };

        let error = service
            .restore_displays_with_runtime_and_commit_seam(
                &["eDP-1".into()],
                &mut runtime,
                &mut reporter,
                DisplayRestoreRuntimeOpts::default(),
                Some(&mut fail_primary_commit),
            )
            .unwrap_err();

        assert_eq!(commit_attempts, 2);
        assert_eq!(error.code, "display_restore_state_commit_failed");
        let detail = error.detail.as_deref().unwrap_or_default();
        assert!(
            detail.contains("injected primary commit failure"),
            "{detail}"
        );
        assert!(detail.contains("reconciliation=ok"), "{detail}");

        let rows = service.storage_for_tests().display_state_list().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].target,
            DisplayStateTarget::Output("eDP-1".into()),
            "reconciliation must persist the executor's per-output live scope"
        );
        assert_eq!(rows[0].wallpaper_path, image.to_string_lossy());
        assert_eq!(
            rows[0].backend, "mpvpaper",
            "reconciliation must persist the backend that actually started"
        );
        assert_eq!(service.storage_for_tests().current_read().unwrap(), None);
        assert_eq!(
            service.storage_for_tests().last_backend_read().unwrap(),
            None
        );
    }

    #[test]
    fn successful_restore_reports_both_errors_when_reconciliation_commit_also_fails() {
        let (_tmp, service, image) = restore_service_with_changed_backend();
        let mut runtime = FakeRuntime {
            command_output_success: true,
            command_status_success: true,
            mpvpaper_ready_pid: Some(9),
            ..Default::default()
        };
        let mut reporter = NoopReporter;
        let mut commit_attempts = 0;
        let mut fail_both_commits = || {
            commit_attempts += 1;
            let message = if commit_attempts == 1 {
                "injected primary commit failure"
            } else {
                "injected reconciliation commit failure"
            };
            Err(WcError::Other(message.into()))
        };

        let error = service
            .restore_displays_with_runtime_and_commit_seam(
                &["eDP-1".into()],
                &mut runtime,
                &mut reporter,
                DisplayRestoreRuntimeOpts::default(),
                Some(&mut fail_both_commits),
            )
            .unwrap_err();

        assert_eq!(commit_attempts, 2);
        assert_eq!(error.code, "display_state_uncertain");
        let detail = error.detail.as_deref().unwrap_or_default();
        assert!(detail.contains("commit_error=injected primary commit failure"));
        assert!(
            detail.contains(
                "reconciliation_transaction_error=injected reconciliation commit failure"
            ),
            "{detail}"
        );

        let rows = service.storage_for_tests().display_state_list().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].backend, "awww",
            "both injected transactions must roll back without partial persistence"
        );
        assert_eq!(
            service
                .storage_for_tests()
                .current_read()
                .unwrap()
                .as_deref(),
            Some(image.to_string_lossy().as_ref()),
            "failed reconciliation transaction must preserve legacy state"
        );
        assert_eq!(
            service
                .storage_for_tests()
                .last_backend_read()
                .unwrap()
                .as_deref(),
            Some("awww"),
            "failed reconciliation transaction must preserve the legacy backend"
        );
    }

    #[test]
    fn per_output_dual_video_rows_restore_without_scoped_stop() {
        let (tmp, service) = temp_service();
        let a = write_video(tmp.path(), "a.mp4");
        let b = write_video(tmp.path(), "b.mp4");
        service
            .storage_for_tests()
            .display_state_replace_all(&[
                (
                    DisplayStateTarget::Output("eDP-1".into()),
                    a.to_string_lossy().into(),
                    "mpvpaper".into(),
                ),
                (
                    DisplayStateTarget::Output("HDMI-1".into()),
                    b.to_string_lossy().into(),
                    "mpvpaper".into(),
                ),
            ])
            .unwrap();

        let mut runtime = FakeRuntime {
            command_status_success: true,
            mpvpaper_ready_pid: Some(3),
            ..Default::default()
        };
        service
            .restore_displays_with_runtime(
                &["eDP-1".into(), "HDMI-1".into()],
                &mut runtime,
                &mut NoopReporter,
                DisplayRestoreRuntimeOpts::default(),
            )
            .unwrap();

        assert_eq!(runtime.stop_mpvpaper_count, 1);
        assert!(runtime.stop_mpvpaper_outputs_calls.is_empty());
        assert_eq!(runtime.mpvpaper_events[0], "stop");
        let applies: Vec<_> = runtime.mpvpaper_events[1..].to_vec();
        assert_eq!(applies.len(), 2);
        assert!(applies.contains(&"apply:eDP-1".to_string()));
        assert!(applies.contains(&"apply:HDMI-1".to_string()));
    }

    #[test]
    fn all_displays_plus_named_override_restore_uses_scoped_stop_for_override() {
        let (tmp, service) = temp_service();
        let base = write_video(tmp.path(), "base.mp4");
        let override_video = write_video(tmp.path(), "override.mp4");
        service
            .storage_for_tests()
            .display_state_replace_all(&[
                (
                    DisplayStateTarget::AllDisplays,
                    base.to_string_lossy().into(),
                    "mpvpaper".into(),
                ),
                (
                    DisplayStateTarget::Output("HDMI-1".into()),
                    override_video.to_string_lossy().into(),
                    "mpvpaper".into(),
                ),
            ])
            .unwrap();

        let mut runtime = FakeRuntime {
            command_status_success: true,
            mpvpaper_ready_pid: Some(3),
            ..Default::default()
        };
        service
            .restore_displays_with_runtime(
                &["eDP-1".into(), "HDMI-1".into()],
                &mut runtime,
                &mut NoopReporter,
                DisplayRestoreRuntimeOpts::default(),
            )
            .unwrap();

        assert_eq!(
            runtime.mpvpaper_events,
            [
                "stop",
                "apply:eDP-1",
                "apply:HDMI-1",
                "stop-outputs:[HDMI-1]",
                "apply:HDMI-1",
            ]
        );
        assert_eq!(
            runtime.stop_mpvpaper_outputs_calls,
            vec![vec!["HDMI-1".to_string()]]
        );
    }

    fn write_executable_script(path: &std::path::Path, body: &str) -> std::io::Result<()> {
        use std::io::Write;
        let mut file = std::fs::File::create(path)?;
        file.write_all(body.as_bytes())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = file.metadata()?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(path, perms)?;
        }
        Ok(())
    }

    #[test]
    fn successful_restore_runs_post_apply_hook_when_enabled() {
        let (tmp, service) = temp_service();
        let img = write_image(tmp.path(), "restore-theme.jpg");
        let marker = tmp.path().join("hook-ran.txt");
        let script = tmp.path().join("hook.sh");
        write_executable_script(
            &script,
            &format!("#!/bin/sh\necho ran >> '{}'\n", marker.display()),
        )
        .unwrap();

        service
            .storage_for_tests()
            .display_state_upsert(
                &DisplayStateTarget::AllDisplays,
                &img.to_string_lossy(),
                "awww",
            )
            .unwrap();
        service
            .storage_for_tests()
            .config_set("post_apply_enabled", "on")
            .unwrap();
        service
            .storage_for_tests()
            .config_set("post_apply_on_restore", "on")
            .unwrap();
        service
            .storage_for_tests()
            .config_set("post_apply_command", &format!("\"{}\"", script.display()))
            .unwrap();

        let mut rt = FakeRuntime {
            command_output_success: true,
            command_status_success: true,
            ..Default::default()
        };
        service
            .restore_displays_with_runtime(
                &["eDP-1".into()],
                &mut rt,
                &mut NoopReporter,
                DisplayRestoreRuntimeOpts::default(),
            )
            .unwrap();

        let body = std::fs::read_to_string(&marker).unwrap();
        assert_eq!(body.lines().count(), 1, "hook should run once: {body}");
        assert!(service.storage_for_tests().cd.theme_state_path().is_file());
    }

    #[test]
    fn failed_restore_does_not_run_post_apply_hook() {
        let (tmp, service) = temp_service();
        let img = write_image(tmp.path(), "restore-fail.jpg");
        let marker = tmp.path().join("hook-ran.txt");
        let script = tmp.path().join("hook.sh");
        write_executable_script(
            &script,
            &format!("#!/bin/sh\necho ran >> '{}'\n", marker.display()),
        )
        .unwrap();

        service
            .storage_for_tests()
            .display_state_upsert(
                &DisplayStateTarget::AllDisplays,
                &img.to_string_lossy(),
                "awww",
            )
            .unwrap();
        service
            .storage_for_tests()
            .config_set("post_apply_enabled", "on")
            .unwrap();
        service
            .storage_for_tests()
            .config_set("post_apply_on_restore", "on")
            .unwrap();
        service
            .storage_for_tests()
            .config_set("post_apply_command", &format!("\"{}\"", script.display()))
            .unwrap();

        let mut rt = FakeRuntime {
            command_output_success: false,
            command_status_success: true,
            ..Default::default()
        };
        let err = service
            .restore_displays_with_runtime(
                &["eDP-1".into()],
                &mut rt,
                &mut NoopReporter,
                DisplayRestoreRuntimeOpts::default(),
            )
            .unwrap_err();
        assert!(!err.code.is_empty());
        assert!(!marker.exists(), "hook must not run on restore failure");
    }

    #[test]
    fn restore_skips_hook_when_post_apply_on_restore_off() {
        let (tmp, service) = temp_service();
        let img = write_image(tmp.path(), "restore-skip.jpg");
        let marker = tmp.path().join("hook-ran.txt");
        let script = tmp.path().join("hook.sh");
        write_executable_script(
            &script,
            &format!("#!/bin/sh\necho ran >> '{}'\n", marker.display()),
        )
        .unwrap();

        service
            .storage_for_tests()
            .display_state_upsert(
                &DisplayStateTarget::AllDisplays,
                &img.to_string_lossy(),
                "awww",
            )
            .unwrap();
        service
            .storage_for_tests()
            .config_set("post_apply_enabled", "on")
            .unwrap();
        service
            .storage_for_tests()
            .config_set("post_apply_on_restore", "off")
            .unwrap();
        service
            .storage_for_tests()
            .config_set("post_apply_command", &format!("\"{}\"", script.display()))
            .unwrap();

        let mut rt = FakeRuntime {
            command_output_success: true,
            command_status_success: true,
            ..Default::default()
        };
        service
            .restore_displays_with_runtime(
                &["eDP-1".into()],
                &mut rt,
                &mut NoopReporter,
                DisplayRestoreRuntimeOpts::default(),
            )
            .unwrap();
        assert!(!marker.exists());
    }
}
