//! Display-aware restore orchestration for AppService.
//!
//! Recreates persisted connected display assignments via the same capability
//! planner and display executor used by apply. Disconnected rows stay as
//! preferences. Successful restore leaves the preference map unchanged;
//! failures after destructive progress reconcile live truth without claiming
//! overall success.

use std::path::Path;

use wc_backend::apply_stage::{self, ApplyStageReporter, NoopReporter};
use wc_backend::display_executor::{
    execute_display_actions, DisplayExecAction, DisplayExecContext,
};
use wc_backend::runtime::{BackendRuntime, SystemBackendRuntime};
use wc_backend::ExecutionScope;
use wc_core::types::Backend;
use wc_storage::sqlite::{DisplayStateRow, DisplayStateTarget};

use crate::display_apply::{parse_backend, rejection_to_app_error, to_exec_action};
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
        let request_id = opts.request_id.as_deref();
        apply_stage::report_stage(reporter, apply_stage::ApplyStage::ResolveTarget, request_id);

        let previous_rows = self
            .storage
            .display_state_list()
            .map_err(AppError::from_wc_error)?;
        let steps = build_restore_steps(&previous_rows, known_outputs)?;
        if steps.is_empty() {
            return Ok(());
        }

        for step in &steps {
            ensure_wallpaper_present(&step.path)?;
        }

        // Preflight the full sequence with accumulating live assignments so a
        // later coexistence/capability rejection never partially executes.
        let mut all_actions = vec![
            DisplayExecAction::Stop {
                backend: Backend::Awww,
                scope: ExecutionScope::AllDisplays,
            },
            DisplayExecAction::Stop {
                backend: Backend::Mpvpaper,
                scope: ExecutionScope::AllDisplays,
            },
            DisplayExecAction::Stop {
                backend: Backend::LinuxWallpaperEngine,
                scope: ExecutionScope::AllDisplays,
            },
        ];
        let mut running: Vec<RunningAssignment> = Vec::new();
        for step in steps {
            let same_backend_already_running = running
                .iter()
                .any(|assignment| assignment.backend == step.backend);
            let request = DisplayApplyRequest {
                target: step.target.clone(),
                backend: step.backend,
                known_outputs: known_outputs.to_vec(),
                running: running.clone(),
            };
            let plan = plan_display_apply(&request).map_err(rejection_to_app_error)?;
            let plan_has_stop = plan
                .actions
                .iter()
                .any(|action| matches!(action, PlannedAction::Stop { .. }));
            let use_instant = plan_has_stop || !same_backend_already_running;
            let actions: Vec<_> = plan
                .actions
                .into_iter()
                .map(|action| {
                    to_exec_action(action, &step.path, &step.target, known_outputs, use_instant)
                })
                .collect::<Result<Vec<_>, _>>()?;
            update_running_after_step(&mut running, known_outputs, &step);
            all_actions.extend(actions);
        }

        let exec_result = execute_display_actions(
            &self.storage,
            &all_actions,
            &DisplayExecContext { known_outputs },
            runtime,
            reporter,
            request_id,
        );
        if let Err(failure) = exec_result {
            return Err(self.handle_exec_failure(failure, &previous_rows, known_outputs, None)?);
        }
        Ok(())
    }
}

fn build_restore_steps(
    rows: &[DisplayStateRow],
    known_outputs: &[String],
) -> Result<Vec<RestoreStep>, AppError> {
    let all_row = rows
        .iter()
        .find(|row| matches!(row.target, DisplayStateTarget::AllDisplays));

    let mut connected_overrides: Vec<(String, String, Backend)> = Vec::new();
    for row in rows {
        let DisplayStateTarget::Output(name) = &row.target else {
            continue;
        };
        if !known_outputs.iter().any(|known| known == name) {
            continue;
        }
        let backend = parse_backend(&row.backend)?;
        if let Some(existing) = connected_overrides
            .iter_mut()
            .find(|(output, _, _)| output == name)
        {
            existing.1 = row.wallpaper_path.clone();
            existing.2 = backend;
        } else {
            connected_overrides.push((name.clone(), row.wallpaper_path.clone(), backend));
        }
    }

    let mut steps = Vec::new();
    if let Some(all) = all_row {
        let backend = parse_backend(&all.backend)?;
        steps.push(RestoreStep {
            target: DisplayTarget::AllDisplays,
            path: all.wallpaper_path.clone(),
            backend,
        });
    }

    for (output, path, backend) in connected_overrides {
        if let Some(all) = all_row {
            let all_backend = parse_backend(&all.backend)?;
            if path == all.wallpaper_path && backend == all_backend {
                // Covered by the AllDisplays step; named row is redundant.
                continue;
            }
        }
        steps.push(RestoreStep {
            target: DisplayTarget::Output(output),
            path,
            backend,
        });
    }

    Ok(steps)
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
    use wc_backend::runtime::AwwwReadiness;
    use wc_core::config::ConfigDir;
    use wc_core::error::WcError;

    #[derive(Default)]
    struct FakeRuntime {
        stop_awww_count: usize,
        stop_mpvpaper_count: usize,
        stop_lwe_count: usize,
        stop_awww_error: Option<String>,
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

        fn stop_awww_checked(&mut self) -> Result<(), WcError> {
            self.stop_awww();
            match &self.stop_awww_error {
                Some(message) => Err(WcError::Other(message.clone())),
                None => Ok(()),
            }
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
    fn conflicting_backend_combination_rejects_without_deleting_preferences() {
        let (tmp, service) = temp_service();
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
}
