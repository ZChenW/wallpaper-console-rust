//! Execute a display-scoped Stop/Apply action list via BackendRuntime.
//!
//! Does not read or write `display_state` — callers commit intended mappings
//! only after every action succeeds, and reconcile after destructive stops
//! when a later action fails. Stop runs only when present in the list.

use std::collections::HashSet;

use wc_core::error::WcError;
use wc_core::types::Backend;
use wc_storage::StorageApi;

use crate::apply_stage::{self, ApplyStageReporter};
use crate::awww::{normalize_awww_resize, normalize_awww_transition_type};
use crate::mpvpaper::normalize_mpvpaper_options;
use crate::runtime::BackendRuntime;
use crate::target_commands::{
    build_awww_img_command_for_scope, build_awww_instant_command_for_scope,
    build_mpvpaper_launch_command_for_output, ExecutionScope,
};

/// Execution context shared across a display action list.
#[derive(Debug, Clone)]
pub struct DisplayExecContext<'a> {
    pub known_outputs: &'a [String],
}

/// One executable step produced from a display apply plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisplayExecAction {
    /// Stop the backend for the given scope.
    ///
    /// Current stop implementations are process/daemon-wide. A named scope that
    /// covers fewer than all known connected outputs is rejected (never silently
    /// broadened to a global stop).
    Stop {
        backend: Backend,
        scope: ExecutionScope,
    },
    /// Apply wallpaper to an output group in one CLI invocation group.
    Apply {
        backend: Backend,
        path: String,
        scope: ExecutionScope,
        /// Prefer instant awww transition (first apply or after a Stop).
        use_instant: bool,
    },
}

/// Progress captured as actions succeed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DisplayExecReport {
    pub events: Vec<CompletedEvent>,
    pub completed_stops: Vec<CompletedStop>,
    pub completed_applies: Vec<CompletedApply>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletedEvent {
    Stop(CompletedStop),
    Apply(CompletedApply),
}

impl DisplayExecReport {
    fn record_stop(&mut self, stop: CompletedStop) {
        self.events.push(CompletedEvent::Stop(stop.clone()));
        self.completed_stops.push(stop);
    }

    fn record_apply(&mut self, apply: CompletedApply) {
        self.events.push(CompletedEvent::Apply(apply.clone()));
        self.completed_applies.push(apply);
    }
    pub fn had_destructive_stop(&self) -> bool {
        !self.completed_stops.is_empty()
    }

    pub fn stopped_backends(&self) -> Vec<Backend> {
        self.completed_stops
            .iter()
            .map(|stop| stop.backend)
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletedStop {
    pub backend: Backend,
    pub scope: ExecutionScope,
    /// Global stop APIs destroy prior renderer ownership for the backend.
    pub destructive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletedApply {
    pub backend: Backend,
    pub scope: ExecutionScope,
    pub path: String,
}

/// Failed execution with structured progress up to the failure point.
#[derive(Debug)]
pub struct DisplayExecFailure {
    pub report: DisplayExecReport,
    pub error: WcError,
    /// A destructive stop was attempted, but its post-stop verification failed.
    pub uncertain_stop: Option<Box<CompletedStop>>,
}

impl DisplayExecFailure {
    pub fn after_destructive_stop(&self) -> bool {
        self.report.had_destructive_stop()
    }
}

impl std::fmt::Display for DisplayExecFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.error)
    }
}

impl std::error::Error for DisplayExecFailure {}

/// Execute planned display actions without persisting display_state.
pub fn execute_display_actions(
    s: &StorageApi,
    actions: &[DisplayExecAction],
    ctx: &DisplayExecContext<'_>,
    runtime: &mut dyn BackendRuntime,
    reporter: &mut dyn ApplyStageReporter,
    request_id: Option<&str>,
) -> Result<DisplayExecReport, DisplayExecFailure> {
    let mut report = DisplayExecReport::default();
    for action in actions {
        if let DisplayExecAction::Apply { backend, .. } = action {
            if let Err(error) = runtime.ensure_backend_available(*backend, s) {
                return Err(DisplayExecFailure {
                    report,
                    error,
                    uncertain_stop: None,
                });
            }
        }
    }
    let mut saw_stop = false;
    for action in actions {
        match action {
            DisplayExecAction::Stop { backend, scope } => {
                if let Err(error) = validate_stop_scope(scope, ctx.known_outputs) {
                    return Err(DisplayExecFailure {
                        report,
                        error,
                        uncertain_stop: None,
                    });
                }
                if let Err(error) = stop_backend(s, *backend, runtime) {
                    return Err(DisplayExecFailure {
                        report,
                        error,
                        uncertain_stop: Some(Box::new(CompletedStop {
                            backend: *backend,
                            scope: scope.clone(),
                            destructive: true,
                        })),
                    });
                }
                saw_stop = true;
                report.record_stop(CompletedStop {
                    backend: *backend,
                    scope: scope.clone(),
                    destructive: true,
                });
            }
            DisplayExecAction::Apply {
                backend,
                path,
                scope,
                use_instant,
            } => {
                if let Err(error) = scope.validate() {
                    return Err(DisplayExecFailure {
                        report,
                        error,
                        uncertain_stop: None,
                    });
                }
                if let Err(failure) = apply_backend(
                    s,
                    *backend,
                    path,
                    scope,
                    *use_instant || saw_stop,
                    runtime,
                    reporter,
                    request_id,
                ) {
                    for stop in failure.completed_stops {
                        report.record_stop(stop);
                    }
                    return Err(DisplayExecFailure {
                        report,
                        error: failure.error,
                        uncertain_stop: failure.uncertain_stop,
                    });
                }
                report.record_apply(CompletedApply {
                    backend: *backend,
                    scope: scope.clone(),
                    path: path.clone(),
                });
            }
        }
    }
    apply_stage::report_stage(reporter, apply_stage::ApplyStage::RefreshStatus, request_id);
    Ok(report)
}

/// Named stop is only executable when it covers every known connected output.
/// AllDisplays stops are always allowed. Never broaden a partial named stop.
pub fn validate_stop_scope(
    scope: &ExecutionScope,
    known_outputs: &[String],
) -> Result<(), WcError> {
    scope.validate()?;
    match scope {
        ExecutionScope::AllDisplays => Ok(()),
        ExecutionScope::Named(outputs) => {
            let known: HashSet<&str> = known_outputs.iter().map(String::as_str).collect();
            let named: HashSet<&str> = outputs.iter().map(String::as_str).collect();
            if named == known && !known.is_empty() {
                Ok(())
            } else {
                Err(WcError::Other(format!(
                    "named stop scope {:?} covers fewer than all known connected outputs {:?}; \
                     refusing to broaden a global stop silently",
                    outputs, known_outputs
                )))
            }
        }
    }
}

fn stop_backend(
    s: &StorageApi,
    backend: Backend,
    runtime: &mut dyn BackendRuntime,
) -> Result<(), WcError> {
    match backend {
        Backend::Awww => runtime.stop_awww_checked(),
        Backend::Mpvpaper => runtime.stop_mpvpaper_checked(),
        Backend::LinuxWallpaperEngine => runtime.stop_lwe_checked(Some(s)),
        Backend::Unsupported => Ok(()),
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_backend(
    s: &StorageApi,
    backend: Backend,
    path: &str,
    scope: &ExecutionScope,
    after_stop: bool,
    runtime: &mut dyn BackendRuntime,
    reporter: &mut dyn ApplyStageReporter,
    request_id: Option<&str>,
) -> Result<(), ApplyBackendFailure> {
    let p = std::path::Path::new(path);
    match backend {
        Backend::Unsupported => Err(WcError::UnsupportedFileType(path.to_string()).into()),
        Backend::Awww => {
            if !p.is_file() {
                return Err(WcError::NotRegularFile(p.to_path_buf()).into());
            }
            apply_awww(s, path, scope, after_stop, runtime, reporter, request_id)
                .map_err(Into::into)
        }
        Backend::Mpvpaper => {
            if !p.is_file() {
                return Err(WcError::NotRegularFile(p.to_path_buf()).into());
            }
            let outputs = scope.named_outputs().ok_or_else(|| {
                ApplyBackendFailure::from(WcError::Other(
                    "mpvpaper apply requires a named single-output execution scope".into(),
                ))
            })?;
            if outputs.len() != 1 {
                return Err(WcError::Other(format!(
                    "mpvpaper apply expects exactly one output per invocation, got {}",
                    outputs.len()
                ))
                .into());
            }
            apply_mpvpaper(s, path, &outputs[0], runtime)
        }
        Backend::LinuxWallpaperEngine => {
            let outputs = match scope {
                ExecutionScope::AllDisplays => {
                    return Err(WcError::Other(
                        "linux-wallpaperengine apply requires explicit named outputs".into(),
                    )
                    .into());
                }
                ExecutionScope::Named(outputs) => outputs.as_slice(),
            };
            apply_stage::report_stage(reporter, apply_stage::ApplyStage::StartLwe, request_id);
            let project = crate::linux_wallpaperengine::project_from_path(path)
                .map_err(ApplyBackendFailure::from)?;
            runtime
                .apply_lwe_to_outputs(s, &project, outputs)
                .map_err(ApplyBackendFailure::from)?;
            apply_stage::report_stage(
                reporter,
                apply_stage::ApplyStage::WaitRendererAlive,
                request_id,
            );
            Ok(())
        }
    }
}

#[derive(Debug)]
struct ApplyBackendFailure {
    error: WcError,
    completed_stops: Vec<CompletedStop>,
    uncertain_stop: Option<Box<CompletedStop>>,
}

impl From<WcError> for ApplyBackendFailure {
    fn from(error: WcError) -> Self {
        Self {
            error,
            completed_stops: Vec::new(),
            uncertain_stop: None,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_awww(
    s: &StorageApi,
    path: &str,
    scope: &ExecutionScope,
    after_stop: bool,
    runtime: &mut dyn BackendRuntime,
    reporter: &mut dyn ApplyStageReporter,
    request_id: Option<&str>,
) -> Result<(), WcError> {
    apply_stage::report_stage(
        reporter,
        apply_stage::ApplyStage::EnsureAwwwDaemon,
        request_id,
    );
    runtime.ensure_awww_daemon_running()?;
    apply_stage::report_stage(
        reporter,
        apply_stage::ApplyStage::AwwwSocketReady,
        request_id,
    );

    let fps_raw = s.config_get("wallpaper_transition_fps", "60");
    let fps = wc_core::config_normalizer::normalize_awww_transition_fps(&fps_raw);
    let resize_raw = s.config_get("awww_resize", "crop");
    let resize = normalize_awww_resize(&resize_raw);

    let mut cmd = if after_stop {
        build_awww_instant_command_for_scope(path, resize, &fps, scope)?
    } else {
        let transition_raw = s.config_get("awww_transition_type", "fade");
        let transition_type = normalize_awww_transition_type(&transition_raw);
        let duration_raw = s.config_get("awww_transition_duration", "1");
        let duration =
            wc_core::config_normalizer::normalize_awww_transition_duration(&duration_raw);
        let mut cmd = build_awww_img_command_for_scope(
            path,
            resize,
            transition_type,
            &duration,
            &fps,
            scope,
        )?;
        cmd.arg("--filter").arg("Lanczos3");
        cmd
    };

    let output = runtime
        .command_output(&mut cmd)
        .map_err(|e| WcError::Other(format!("awww failed: {}", e)))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            "no renderer output".into()
        };
        return Err(WcError::Other(format!(
            "awww apply failed with status {}: {}",
            output.status, detail
        )));
    }
    Ok(())
}

fn apply_mpvpaper(
    s: &StorageApi,
    path: &str,
    output: &str,
    runtime: &mut dyn BackendRuntime,
) -> Result<(), ApplyBackendFailure> {
    let previous_pids = runtime.mpvpaper_pids().map_err(ApplyBackendFailure::from)?;
    let opts_raw = s.config_get("mpvpaper_options", "--loop-file=inf --panscan=1.0");
    let opts = normalize_mpvpaper_options(&opts_raw);
    let mut cmd = build_mpvpaper_launch_command_for_output(opts, output, path)
        .map_err(ApplyBackendFailure::from)?;
    let status = runtime.command_status(&mut cmd).map_err(|e| {
        ApplyBackendFailure::from(WcError::Other(format!("mpvpaper failed: {}", e)))
    })?;
    if !status.success() {
        return Err(WcError::Other("mpvpaper failed to apply wallpaper".into()).into());
    }
    let pid = match runtime.wait_for_mpvpaper_ready(&previous_pids) {
        Ok(pid) => pid,
        Err(error) => {
            return Err(mpvpaper_cleanup_failure(runtime, error));
        }
    };
    match runtime.mpvpaper_pid_running(pid) {
        Ok(true) => Ok(()),
        Ok(false) => Err(mpvpaper_cleanup_failure(
            runtime,
            WcError::Other("mpvpaper renderer exited before startup settled".into()),
        )),
        Err(error) => Err(mpvpaper_cleanup_failure(runtime, error)),
    }
}

fn mpvpaper_cleanup_failure(
    runtime: &mut dyn BackendRuntime,
    original_error: WcError,
) -> ApplyBackendFailure {
    match runtime.stop_mpvpaper_checked() {
        Ok(()) => ApplyBackendFailure {
            error: original_error,
            completed_stops: vec![CompletedStop {
                backend: Backend::Mpvpaper,
                scope: ExecutionScope::AllDisplays,
                destructive: true,
            }],
            uncertain_stop: None,
        },
        Err(cleanup_error) => ApplyBackendFailure {
            error: WcError::Other(format!(
                "{original_error}; mpvpaper cleanup could not be verified: {cleanup_error}"
            )),
            completed_stops: Vec::new(),
            uncertain_stop: Some(Box::new(CompletedStop {
                backend: Backend::Mpvpaper,
                scope: ExecutionScope::AllDisplays,
                destructive: true,
            })),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apply_stage::NoopReporter;
    use crate::runtime::AwwwReadiness;
    use std::cell::RefCell;
    use std::process::Command;
    use wc_core::config::ConfigDir;

    #[derive(Default)]
    struct FakeRuntime {
        missing_backend: Option<Backend>,
        stop_awww_count: usize,
        stop_mpvpaper_count: usize,
        stop_lwe_count: usize,
        command_output_success: bool,
        command_status_success: bool,
        command_output_args: Vec<Vec<String>>,
        command_status_args: Vec<Vec<String>>,
        mpvpaper_ready_pid: Option<u32>,
        running_mpvpaper_pids: Vec<u32>,
        mpvpaper_readiness_error: Option<String>,
        dead_mpvpaper_pids: Vec<u32>,
        awww_readiness_sequence: RefCell<Vec<AwwwReadiness>>,
        lwe_apply_calls: Vec<(String, Vec<String>)>,
        lwe_apply_error: Option<String>,
    }

    impl BackendRuntime for FakeRuntime {
        fn ensure_backend_available(
            &mut self,
            backend: Backend,
            _storage: &StorageApi,
        ) -> Result<(), WcError> {
            if self.missing_backend == Some(backend) {
                Err(WcError::BackendNotFound(backend.as_str().into()))
            } else {
                Ok(())
            }
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
            std::process::Command::new(program)
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
            let program = if self.command_status_success {
                "true"
            } else {
                "false"
            };
            std::process::Command::new(program)
                .status()
                .map_err(|e| WcError::Other(format!("fake command failed: {e}")))
        }

        fn mpvpaper_pids(&mut self) -> Result<Vec<u32>, WcError> {
            Ok(self.running_mpvpaper_pids.clone())
        }

        fn wait_for_mpvpaper_ready(&mut self, _previous_pids: &[u32]) -> Result<u32, WcError> {
            match &self.mpvpaper_readiness_error {
                Some(message) => Err(WcError::Other(message.clone())),
                None => Ok(self.mpvpaper_ready_pid.unwrap_or(42)),
            }
        }

        fn mpvpaper_pid_running(&mut self, pid: u32) -> Result<bool, WcError> {
            Ok(!self.dead_mpvpaper_pids.contains(&pid))
        }

        fn stop_awww(&mut self) {
            self.stop_awww_count += 1;
        }

        fn stop_mpvpaper(&mut self) {
            self.stop_mpvpaper_count += 1;
            self.running_mpvpaper_pids.clear();
        }

        fn stop_lwe(&mut self, _s: Option<&StorageApi>) {
            self.stop_lwe_count += 1;
        }

        fn apply_lwe_to_outputs(
            &mut self,
            _s: &StorageApi,
            project: &crate::linux_wallpaperengine::LinuxWallpaperEngineProject,
            outputs: &[String],
        ) -> Result<(), WcError> {
            self.lwe_apply_calls
                .push((project.project_path.clone(), outputs.to_vec()));
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

    fn temp_storage() -> (tempfile::TempDir, StorageApi) {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        wc_core::config::write_config_value(&cd.path, "storage_backend", "sqlite").unwrap();
        let s = StorageApi::new(cd);
        (tmp, s)
    }

    fn ctx<'a>(known: &'a [String]) -> DisplayExecContext<'a> {
        DisplayExecContext {
            known_outputs: known,
        }
    }

    #[test]
    fn stop_only_runs_when_present_in_actions() {
        let (_tmp, s) = temp_storage();
        let img = _tmp.path().join("a.jpg");
        std::fs::write(&img, b"jpg").unwrap();
        let known = vec!["eDP-1".into()];

        let mut rt = FakeRuntime {
            command_output_success: true,
            ..Default::default()
        };
        let mut reporter = NoopReporter;
        let report = execute_display_actions(
            &s,
            &[DisplayExecAction::Apply {
                backend: Backend::Awww,
                path: img.to_string_lossy().to_string(),
                scope: ExecutionScope::named(vec!["eDP-1".into()]).unwrap(),
                use_instant: true,
            }],
            &ctx(&known),
            &mut rt,
            &mut reporter,
            None,
        )
        .unwrap();

        assert!(report.completed_stops.is_empty());
        assert_eq!(report.completed_applies.len(), 1);
        assert_eq!(rt.stop_awww_count, 0);
        assert_eq!(rt.stop_mpvpaper_count, 0);
        assert_eq!(rt.stop_lwe_count, 0);
    }

    #[test]
    fn missing_renderer_is_rejected_before_any_destructive_stop() {
        let (tmp, storage) = temp_storage();
        let video = tmp.path().join("missing-renderer.mp4");
        std::fs::write(&video, b"mp4").unwrap();
        let known = vec!["eDP-1".into()];
        let actions = vec![
            DisplayExecAction::Stop {
                backend: Backend::Awww,
                scope: ExecutionScope::AllDisplays,
            },
            DisplayExecAction::Apply {
                backend: Backend::Mpvpaper,
                path: video.to_string_lossy().into(),
                scope: ExecutionScope::named(known.clone()).unwrap(),
                use_instant: true,
            },
        ];
        let mut runtime = FakeRuntime {
            missing_backend: Some(Backend::Mpvpaper),
            command_status_success: true,
            ..Default::default()
        };
        let mut reporter = NoopReporter;

        let error = execute_display_actions(
            &storage,
            &actions,
            &ctx(&known),
            &mut runtime,
            &mut reporter,
            None,
        )
        .unwrap_err();

        assert!(matches!(
            error.error,
            WcError::BackendNotFound(ref backend) if backend == "mpvpaper"
        ));
        assert_eq!(runtime.stop_awww_count, 0);
        assert_eq!(runtime.stop_mpvpaper_count, 0);
        assert_eq!(runtime.stop_lwe_count, 0);
    }

    #[test]
    fn stop_action_targets_only_listed_backend() {
        let (_tmp, s) = temp_storage();
        let known = vec!["eDP-1".into()];
        let mut rt = FakeRuntime::default();
        let mut reporter = NoopReporter;
        let report = execute_display_actions(
            &s,
            &[DisplayExecAction::Stop {
                backend: Backend::Mpvpaper,
                scope: ExecutionScope::named(vec!["eDP-1".into()]).unwrap(),
            }],
            &ctx(&known),
            &mut rt,
            &mut reporter,
            None,
        )
        .unwrap();
        assert_eq!(rt.stop_mpvpaper_count, 1);
        assert_eq!(rt.stop_awww_count, 0);
        assert_eq!(rt.stop_lwe_count, 0);
        assert!(report.completed_stops[0].destructive);
    }

    #[test]
    fn partial_named_stop_is_rejected_without_running_global_stop() {
        let (_tmp, s) = temp_storage();
        let known = vec!["eDP-1".into(), "HDMI-1".into()];
        let mut rt = FakeRuntime::default();
        let mut reporter = NoopReporter;
        let err = execute_display_actions(
            &s,
            &[DisplayExecAction::Stop {
                backend: Backend::Mpvpaper,
                scope: ExecutionScope::named(vec!["eDP-1".into()]).unwrap(),
            }],
            &ctx(&known),
            &mut rt,
            &mut reporter,
            None,
        )
        .unwrap_err();
        assert!(err.error.to_string().contains("refusing to broaden"));
        assert_eq!(rt.stop_mpvpaper_count, 0);
        assert!(!err.after_destructive_stop());
    }

    #[test]
    fn all_displays_stop_is_allowed_with_multi_output_known() {
        let (_tmp, s) = temp_storage();
        let known = vec!["eDP-1".into(), "HDMI-1".into()];
        let mut rt = FakeRuntime::default();
        let mut reporter = NoopReporter;
        execute_display_actions(
            &s,
            &[DisplayExecAction::Stop {
                backend: Backend::Awww,
                scope: ExecutionScope::AllDisplays,
            }],
            &ctx(&known),
            &mut rt,
            &mut reporter,
            None,
        )
        .unwrap();
        assert_eq!(rt.stop_awww_count, 1);
    }

    #[test]
    fn awww_named_apply_passes_outputs_flag() {
        let (_tmp, s) = temp_storage();
        let img = _tmp.path().join("a.jpg");
        std::fs::write(&img, b"jpg").unwrap();
        let known = vec!["eDP-1".into()];
        let mut rt = FakeRuntime {
            command_output_success: true,
            ..Default::default()
        };
        let mut reporter = NoopReporter;
        execute_display_actions(
            &s,
            &[DisplayExecAction::Apply {
                backend: Backend::Awww,
                path: img.to_string_lossy().to_string(),
                scope: ExecutionScope::named(vec!["eDP-1".into()]).unwrap(),
                use_instant: true,
            }],
            &ctx(&known),
            &mut rt,
            &mut reporter,
            None,
        )
        .unwrap();
        let args = &rt.command_output_args[0];
        let idx = args
            .iter()
            .position(|a| a == "--outputs")
            .expect("--outputs");
        assert_eq!(args.get(idx + 1).map(String::as_str), Some("eDP-1"));
    }

    #[test]
    fn awww_all_displays_apply_omits_outputs_flag() {
        let (_tmp, s) = temp_storage();
        let img = _tmp.path().join("a.jpg");
        std::fs::write(&img, b"jpg").unwrap();
        let known = vec!["eDP-1".into(), "HDMI-1".into()];
        let mut rt = FakeRuntime {
            command_output_success: true,
            ..Default::default()
        };
        let mut reporter = NoopReporter;
        execute_display_actions(
            &s,
            &[DisplayExecAction::Apply {
                backend: Backend::Awww,
                path: img.to_string_lossy().to_string(),
                scope: ExecutionScope::AllDisplays,
                use_instant: true,
            }],
            &ctx(&known),
            &mut rt,
            &mut reporter,
            None,
        )
        .unwrap();
        let args = &rt.command_output_args[0];
        assert!(
            !args.iter().any(|a| a == "--outputs"),
            "AllDisplays must omit --outputs, got {args:?}"
        );
    }

    #[test]
    fn mpvpaper_apply_uses_planned_output_not_config_wildcard() {
        let (_tmp, s) = temp_storage();
        let video = _tmp.path().join("v.mp4");
        std::fs::write(&video, b"mp4").unwrap();
        s.config_set("mpvpaper_output", "*").unwrap();
        let known = vec!["HDMI-1".into()];

        let mut rt = FakeRuntime {
            command_status_success: true,
            mpvpaper_ready_pid: Some(9),
            ..Default::default()
        };
        let mut reporter = NoopReporter;
        execute_display_actions(
            &s,
            &[DisplayExecAction::Apply {
                backend: Backend::Mpvpaper,
                path: video.to_string_lossy().to_string(),
                scope: ExecutionScope::named(vec!["HDMI-1".into()]).unwrap(),
                use_instant: false,
            }],
            &ctx(&known),
            &mut rt,
            &mut reporter,
            None,
        )
        .unwrap();
        let args = &rt.command_status_args[0];
        assert!(args.iter().any(|a| a == "HDMI-1"), "args={args:?}");
        assert!(!args.iter().any(|a| a == "*"));
    }

    #[test]
    fn command_failure_does_not_run_later_actions_and_reports_progress() {
        let (_tmp, s) = temp_storage();
        let img = _tmp.path().join("a.jpg");
        std::fs::write(&img, b"jpg").unwrap();
        let known = vec!["eDP-1".into()];
        let mut rt = FakeRuntime {
            command_output_success: false,
            ..Default::default()
        };
        let mut reporter = NoopReporter;
        let err = execute_display_actions(
            &s,
            &[
                DisplayExecAction::Apply {
                    backend: Backend::Awww,
                    path: img.to_string_lossy().to_string(),
                    scope: ExecutionScope::named(vec!["eDP-1".into()]).unwrap(),
                    use_instant: false,
                },
                DisplayExecAction::Stop {
                    backend: Backend::Mpvpaper,
                    scope: ExecutionScope::named(vec!["eDP-1".into()]).unwrap(),
                },
            ],
            &ctx(&known),
            &mut rt,
            &mut reporter,
            None,
        )
        .unwrap_err();
        assert!(err.error.to_string().contains("awww"));
        assert_eq!(rt.stop_mpvpaper_count, 0);
        assert!(err.report.completed_applies.is_empty());
        assert!(!err.after_destructive_stop());
    }

    #[test]
    fn stop_success_then_apply_failure_marks_destructive_progress() {
        let (_tmp, s) = temp_storage();
        let img = _tmp.path().join("a.jpg");
        std::fs::write(&img, b"jpg").unwrap();
        let known = vec!["eDP-1".into()];
        let mut rt = FakeRuntime {
            command_output_success: false,
            ..Default::default()
        };
        let mut reporter = NoopReporter;
        let err = execute_display_actions(
            &s,
            &[
                DisplayExecAction::Stop {
                    backend: Backend::Mpvpaper,
                    scope: ExecutionScope::named(vec!["eDP-1".into()]).unwrap(),
                },
                DisplayExecAction::Apply {
                    backend: Backend::Awww,
                    path: img.to_string_lossy().to_string(),
                    scope: ExecutionScope::named(vec!["eDP-1".into()]).unwrap(),
                    use_instant: true,
                },
            ],
            &ctx(&known),
            &mut rt,
            &mut reporter,
            None,
        )
        .unwrap_err();
        assert_eq!(rt.stop_mpvpaper_count, 1);
        assert!(err.after_destructive_stop());
        assert_eq!(err.report.completed_stops.len(), 1);
        assert!(err.report.completed_applies.is_empty());
    }

    #[test]
    fn partial_multi_apply_failure_keeps_successful_apply_in_report() {
        let (_tmp, s) = temp_storage();
        let video = _tmp.path().join("v.mp4");
        std::fs::write(&video, b"mp4").unwrap();
        let known = vec!["eDP-1".into(), "HDMI-1".into()];
        let rt = FakeRuntime {
            command_status_success: true,
            mpvpaper_ready_pid: Some(3),
            ..Default::default()
        };

        // Custom: make second command_status fail by toggling after first call.
        struct SeqRuntime {
            inner: FakeRuntime,
            status_calls: usize,
        }
        impl BackendRuntime for SeqRuntime {
            fn command_output(&mut self, c: &mut Command) -> Result<std::process::Output, WcError> {
                self.inner.command_output(c)
            }
            fn command_status(
                &mut self,
                c: &mut Command,
            ) -> Result<std::process::ExitStatus, WcError> {
                self.status_calls += 1;
                self.inner.command_status_success = self.status_calls < 2;
                self.inner.command_status(c)
            }
            fn mpvpaper_pids(&mut self) -> Result<Vec<u32>, WcError> {
                self.inner.mpvpaper_pids()
            }
            fn wait_for_mpvpaper_ready(&mut self, p: &[u32]) -> Result<u32, WcError> {
                self.inner.wait_for_mpvpaper_ready(p)
            }
            fn mpvpaper_pid_running(&mut self, pid: u32) -> Result<bool, WcError> {
                self.inner.mpvpaper_pid_running(pid)
            }
            fn stop_awww(&mut self) {
                self.inner.stop_awww();
            }
            fn stop_mpvpaper(&mut self) {
                self.inner.stop_mpvpaper();
            }
            fn stop_lwe(&mut self, s: Option<&StorageApi>) {
                self.inner.stop_lwe(s);
            }
            fn apply_lwe_to_outputs(
                &mut self,
                s: &StorageApi,
                project: &crate::linux_wallpaperengine::LinuxWallpaperEngineProject,
                outputs: &[String],
            ) -> Result<(), WcError> {
                self.inner.apply_lwe_to_outputs(s, project, outputs)
            }
            fn awww_socket_ready(&mut self) -> AwwwReadiness {
                self.inner.awww_socket_ready()
            }
            fn ensure_awww_daemon_running(&mut self) -> Result<(), WcError> {
                self.inner.ensure_awww_daemon_running()
            }
            fn clear_awww_state_hint(&mut self) {
                self.inner.clear_awww_state_hint();
            }
        }

        let mut rt = SeqRuntime {
            inner: rt,
            status_calls: 0,
        };
        let mut reporter = NoopReporter;
        let err = execute_display_actions(
            &s,
            &[
                DisplayExecAction::Stop {
                    backend: Backend::Awww,
                    scope: ExecutionScope::AllDisplays,
                },
                DisplayExecAction::Apply {
                    backend: Backend::Mpvpaper,
                    path: video.to_string_lossy().to_string(),
                    scope: ExecutionScope::named(vec!["eDP-1".into()]).unwrap(),
                    use_instant: false,
                },
                DisplayExecAction::Apply {
                    backend: Backend::Mpvpaper,
                    path: video.to_string_lossy().to_string(),
                    scope: ExecutionScope::named(vec!["HDMI-1".into()]).unwrap(),
                    use_instant: false,
                },
            ],
            &ctx(&known),
            &mut rt,
            &mut reporter,
            None,
        )
        .unwrap_err();
        assert!(err.after_destructive_stop());
        assert_eq!(err.report.completed_applies.len(), 1);
        assert_eq!(
            err.report.completed_applies[0].scope,
            ExecutionScope::named(vec!["eDP-1".into()]).unwrap()
        );
    }

    #[test]
    fn mpvpaper_readiness_failure_reports_implicit_global_cleanup_stop() {
        let (_tmp, s) = temp_storage();
        let video = _tmp.path().join("v.mp4");
        std::fs::write(&video, b"mp4").unwrap();
        let known = vec!["eDP-1".into(), "HDMI-1".into()];

        struct SeqRuntime {
            inner: FakeRuntime,
            status_calls: usize,
        }
        impl BackendRuntime for SeqRuntime {
            fn command_output(&mut self, c: &mut Command) -> Result<std::process::Output, WcError> {
                self.inner.command_output(c)
            }
            fn command_status(
                &mut self,
                c: &mut Command,
            ) -> Result<std::process::ExitStatus, WcError> {
                self.status_calls += 1;
                self.inner.command_status(c)
            }
            fn mpvpaper_pids(&mut self) -> Result<Vec<u32>, WcError> {
                self.inner.mpvpaper_pids()
            }
            fn wait_for_mpvpaper_ready(&mut self, p: &[u32]) -> Result<u32, WcError> {
                if self.status_calls >= 2 {
                    return Err(WcError::Other("second instance not ready".into()));
                }
                self.inner.wait_for_mpvpaper_ready(p)
            }
            fn mpvpaper_pid_running(&mut self, pid: u32) -> Result<bool, WcError> {
                self.inner.mpvpaper_pid_running(pid)
            }
            fn stop_awww(&mut self) {
                self.inner.stop_awww();
            }
            fn stop_mpvpaper(&mut self) {
                self.inner.stop_mpvpaper();
            }
            fn stop_lwe(&mut self, s: Option<&StorageApi>) {
                self.inner.stop_lwe(s);
            }
            fn apply_lwe_to_outputs(
                &mut self,
                s: &StorageApi,
                project: &crate::linux_wallpaperengine::LinuxWallpaperEngineProject,
                outputs: &[String],
            ) -> Result<(), WcError> {
                self.inner.apply_lwe_to_outputs(s, project, outputs)
            }
            fn awww_socket_ready(&mut self) -> AwwwReadiness {
                self.inner.awww_socket_ready()
            }
            fn ensure_awww_daemon_running(&mut self) -> Result<(), WcError> {
                self.inner.ensure_awww_daemon_running()
            }
            fn clear_awww_state_hint(&mut self) {
                self.inner.clear_awww_state_hint();
            }
        }

        let mut rt = SeqRuntime {
            inner: FakeRuntime {
                command_status_success: true,
                mpvpaper_ready_pid: Some(3),
                ..Default::default()
            },
            status_calls: 0,
        };
        let mut reporter = NoopReporter;
        let err = execute_display_actions(
            &s,
            &[
                DisplayExecAction::Stop {
                    backend: Backend::Awww,
                    scope: ExecutionScope::AllDisplays,
                },
                DisplayExecAction::Apply {
                    backend: Backend::Mpvpaper,
                    path: video.to_string_lossy().to_string(),
                    scope: ExecutionScope::named(vec!["eDP-1".into()]).unwrap(),
                    use_instant: false,
                },
                DisplayExecAction::Apply {
                    backend: Backend::Mpvpaper,
                    path: video.to_string_lossy().to_string(),
                    scope: ExecutionScope::named(vec!["HDMI-1".into()]).unwrap(),
                    use_instant: false,
                },
            ],
            &ctx(&known),
            &mut rt,
            &mut reporter,
            None,
        )
        .unwrap_err();
        assert_eq!(rt.inner.stop_mpvpaper_count, 1);
        assert_eq!(err.report.completed_applies.len(), 1);
        assert!(
            err.report
                .completed_stops
                .iter()
                .any(|stop| { stop.backend == Backend::Mpvpaper && stop.destructive }),
            "implicit mpvpaper cleanup must be reported for reconcile: {:?}",
            err.report.completed_stops
        );
    }

    #[test]
    fn checked_stop_failure_does_not_record_completed_stop_or_run_apply() {
        let (_tmp, s) = temp_storage();
        let img = _tmp.path().join("a.jpg");
        std::fs::write(&img, b"jpg").unwrap();
        let known = vec!["eDP-1".into()];

        struct FailStopRuntime {
            inner: FakeRuntime,
            stop_checked_calls: usize,
        }
        impl BackendRuntime for FailStopRuntime {
            fn command_output(&mut self, c: &mut Command) -> Result<std::process::Output, WcError> {
                self.inner.command_output(c)
            }
            fn command_status(
                &mut self,
                c: &mut Command,
            ) -> Result<std::process::ExitStatus, WcError> {
                self.inner.command_status(c)
            }
            fn mpvpaper_pids(&mut self) -> Result<Vec<u32>, WcError> {
                self.inner.mpvpaper_pids()
            }
            fn wait_for_mpvpaper_ready(&mut self, p: &[u32]) -> Result<u32, WcError> {
                self.inner.wait_for_mpvpaper_ready(p)
            }
            fn mpvpaper_pid_running(&mut self, pid: u32) -> Result<bool, WcError> {
                self.inner.mpvpaper_pid_running(pid)
            }
            fn stop_awww(&mut self) {
                self.inner.stop_awww();
            }
            fn stop_mpvpaper(&mut self) {
                self.inner.stop_mpvpaper();
            }
            fn stop_mpvpaper_checked(&mut self) -> Result<(), WcError> {
                self.stop_checked_calls += 1;
                Err(WcError::Other("mpvpaper still running after stop".into()))
            }
            fn stop_lwe(&mut self, s: Option<&StorageApi>) {
                self.inner.stop_lwe(s);
            }
            fn apply_lwe_to_outputs(
                &mut self,
                s: &StorageApi,
                project: &crate::linux_wallpaperengine::LinuxWallpaperEngineProject,
                outputs: &[String],
            ) -> Result<(), WcError> {
                self.inner.apply_lwe_to_outputs(s, project, outputs)
            }
            fn awww_socket_ready(&mut self) -> AwwwReadiness {
                self.inner.awww_socket_ready()
            }
            fn ensure_awww_daemon_running(&mut self) -> Result<(), WcError> {
                self.inner.ensure_awww_daemon_running()
            }
            fn clear_awww_state_hint(&mut self) {
                self.inner.clear_awww_state_hint();
            }
        }

        let mut rt = FailStopRuntime {
            inner: FakeRuntime {
                command_output_success: true,
                ..Default::default()
            },
            stop_checked_calls: 0,
        };
        let mut reporter = NoopReporter;
        let err = execute_display_actions(
            &s,
            &[
                DisplayExecAction::Stop {
                    backend: Backend::Mpvpaper,
                    scope: ExecutionScope::named(vec!["eDP-1".into()]).unwrap(),
                },
                DisplayExecAction::Apply {
                    backend: Backend::Awww,
                    path: img.to_string_lossy().to_string(),
                    scope: ExecutionScope::named(vec!["eDP-1".into()]).unwrap(),
                    use_instant: true,
                },
            ],
            &ctx(&known),
            &mut rt,
            &mut reporter,
            None,
        )
        .unwrap_err();
        assert_eq!(rt.stop_checked_calls, 1);
        assert!(err.error.to_string().contains("mpvpaper still running"));
        assert!(err.report.completed_stops.is_empty());
        assert!(err.report.completed_applies.is_empty());
        assert!(rt.inner.command_output_args.is_empty());
        assert!(!err.after_destructive_stop());
    }

    #[test]
    fn mpvpaper_readiness_failure_stops_and_errors() {
        let (_tmp, s) = temp_storage();
        let video = _tmp.path().join("v.mp4");
        std::fs::write(&video, b"mp4").unwrap();
        let known = vec!["eDP-1".into()];
        let mut rt = FakeRuntime {
            command_status_success: true,
            mpvpaper_readiness_error: Some("not ready".into()),
            ..Default::default()
        };
        let mut reporter = NoopReporter;
        let err = execute_display_actions(
            &s,
            &[DisplayExecAction::Apply {
                backend: Backend::Mpvpaper,
                path: video.to_string_lossy().to_string(),
                scope: ExecutionScope::named(vec!["eDP-1".into()]).unwrap(),
                use_instant: false,
            }],
            &ctx(&known),
            &mut rt,
            &mut reporter,
            None,
        )
        .unwrap_err();
        assert!(err.error.to_string().contains("not ready"));
        assert_eq!(rt.stop_mpvpaper_count, 1);
    }

    #[test]
    fn lwe_apply_goes_through_runtime_never_real_process_in_tests() {
        let (_tmp, s) = temp_storage();
        let scene = _tmp.path().join("scene");
        std::fs::create_dir_all(&scene).unwrap();
        std::fs::write(
            scene.join("project.json"),
            r#"{"type":"scene","file":"scene.pkg","workshopid":"9"}"#,
        )
        .unwrap();
        let known = vec!["eDP-1".into()];
        let mut rt = FakeRuntime::default();
        let mut reporter = crate::apply_stage::test_support::CapturingReporter::new();
        let report = execute_display_actions(
            &s,
            &[DisplayExecAction::Apply {
                backend: Backend::LinuxWallpaperEngine,
                path: scene.to_string_lossy().to_string(),
                scope: ExecutionScope::named(vec!["eDP-1".into()]).unwrap(),
                use_instant: false,
            }],
            &ctx(&known),
            &mut rt,
            &mut reporter,
            Some("lwe"),
        )
        .unwrap();
        assert_eq!(rt.lwe_apply_calls.len(), 1);
        assert_eq!(rt.lwe_apply_calls[0].1, vec!["eDP-1".to_string()]);
        assert_eq!(report.completed_applies.len(), 1);
        let stages = reporter.stages();
        assert!(stages.contains(&apply_stage::ApplyStage::StartLwe));
        assert!(stages.contains(&apply_stage::ApplyStage::WaitRendererAlive));
        let start = stages
            .iter()
            .position(|s| *s == apply_stage::ApplyStage::StartLwe)
            .unwrap();
        let wait = stages
            .iter()
            .position(|s| *s == apply_stage::ApplyStage::WaitRendererAlive)
            .unwrap();
        assert!(start < wait, "StartLwe must precede WaitRendererAlive");
    }
}
