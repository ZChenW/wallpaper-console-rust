//! Execute a display-scoped Stop/Apply action list via BackendRuntime.
//!
//! Does not read or write `display_state` — callers commit intended mappings
//! only after every action succeeds, and reconcile after destructive stops
//! when a later action fails. Stop runs only when present in the list.
//!
//! Cross-backend visual transition (instant fallback / settle) is owned by
//! [`crate::apply_transition`]. This executor only runs the Stop/Apply skeleton.

use std::collections::HashSet;

#[cfg(test)]
use wc_config::ConfigDirExt;
use wc_core::error::WcError;
use wc_core::types::Backend;
use wc_storage::StorageApi;

use crate::apply_stage::{self, ApplyStageReporter};
use crate::driver;
use crate::runtime::BackendRuntime;
use crate::target_commands::ExecutionScope;

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
    /// Global stops (AllDisplays, or Named covering every known connected
    /// output) use process/daemon-wide APIs. Partial Named stops are allowed
    /// only when the backend supports output-scoped stop
    /// (`StopScope::TrackedProcessPerOutput`); otherwise they are rejected
    /// rather than silently broadened.
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletedEvent {
    Stop(CompletedStop),
    Apply(CompletedApply),
}

impl DisplayExecReport {
    pub fn record_stop(&mut self, stop: CompletedStop) {
        self.events.push(CompletedEvent::Stop(stop));
    }
    fn record_apply(&mut self, apply: CompletedApply) {
        self.events.push(CompletedEvent::Apply(apply));
    }
    pub fn completed_stops(&self) -> impl Iterator<Item = &CompletedStop> {
        self.events.iter().filter_map(|event| match event {
            CompletedEvent::Stop(stop) => Some(stop),
            _ => None,
        })
    }
    pub fn completed_applies(&self) -> impl Iterator<Item = &CompletedApply> {
        self.events.iter().filter_map(|event| match event {
            CompletedEvent::Apply(apply) => Some(apply),
            _ => None,
        })
    }
    pub fn had_destructive_stop(&self) -> bool {
        self.completed_stops().any(|stop| stop.destructive)
    }
    pub fn stopped_backends(&self) -> Vec<Backend> {
        self.completed_stops().map(|stop| stop.backend).collect()
    }
    pub(crate) fn append(&mut self, other: Self) {
        self.events.extend(other.events);
    }
}

pub(crate) enum PreparedDisplayAction {
    Stop {
        backend: Backend,
        scope: ExecutionScope,
        execution: StopExecution,
    },
    Apply(Box<driver::PreparedApply>),
}
pub(crate) type PreparedDisplayActions = Vec<PreparedDisplayAction>;

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
    /// Failure cleanup could not prove the requested renderer state is absent.
    pub cleanup_uncertain: bool,
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
#[allow(clippy::result_large_err)]
pub fn execute_display_actions(
    s: &StorageApi,
    actions: &[DisplayExecAction],
    ctx: &DisplayExecContext<'_>,
    runtime: &mut dyn BackendRuntime,
    reporter: &mut dyn ApplyStageReporter,
    request_id: Option<&str>,
) -> Result<DisplayExecReport, DisplayExecFailure> {
    let prepared = prepare_display_actions(s, actions, ctx, runtime, request_id, &mut Vec::new())?;
    execute_prepared_display_actions(s, prepared, runtime, reporter, request_id)
}

#[allow(clippy::result_large_err)]
pub(crate) fn execute_prepared_display_actions(
    s: &StorageApi,
    prepared: PreparedDisplayActions,
    runtime: &mut dyn BackendRuntime,
    reporter: &mut dyn ApplyStageReporter,
    request_id: Option<&str>,
) -> Result<DisplayExecReport, DisplayExecFailure> {
    let mut report = DisplayExecReport::default();
    for action in prepared {
        match action {
            PreparedDisplayAction::Stop {
                backend,
                scope,
                execution,
            } => {
                // Prepared scopes are stable; live selectors/daemon facts are not.
                if let StopExecution::Scoped(outputs) = &execution {
                    if let Some(driver) = driver::driver_for(backend) {
                        if let Err(error) =
                            driver.preflight_stop(&ExecutionScope::Named(outputs.clone()), runtime)
                        {
                            return Err(DisplayExecFailure {
                                report,
                                error,
                                uncertain_stop: None,
                                cleanup_uncertain: false,
                            });
                        }
                    }
                }
                if let Err(error) = stop_backend(s, backend, &execution, runtime) {
                    return Err(DisplayExecFailure {
                        report,
                        error,
                        uncertain_stop: Some(Box::new(CompletedStop {
                            backend,
                            scope: scope.clone(),
                            destructive: true,
                        })),
                        cleanup_uncertain: true,
                    });
                }
                report.record_stop(CompletedStop {
                    backend,
                    scope: scope.clone(),
                    destructive: true,
                });
            }
            PreparedDisplayAction::Apply(mut operation) => {
                if let Err(failure) = operation.execute(s, runtime, reporter) {
                    let mut uncertain_stop = None;
                    let mut cleanup_uncertain = false;
                    match failure.cleanup {
                        driver::CleanupOutcome::NotRequired => {}
                        driver::CleanupOutcome::VerifiedTargetedStop { backend, outputs } => {
                            let scope = ExecutionScope::named(outputs)
                                .unwrap_or(ExecutionScope::AllDisplays);
                            report.record_stop(CompletedStop {
                                backend,
                                scope,
                                destructive: true,
                            });
                        }
                        driver::CleanupOutcome::VerifiedGlobalStop(backend) => {
                            report.record_stop(CompletedStop {
                                backend,
                                scope: ExecutionScope::AllDisplays,
                                destructive: true,
                            });
                        }
                        driver::CleanupOutcome::UncertainGlobalStop(backend) => {
                            uncertain_stop = Some(Box::new(CompletedStop {
                                backend,
                                scope: ExecutionScope::AllDisplays,
                                destructive: true,
                            }));
                            cleanup_uncertain = true;
                        }
                        driver::CleanupOutcome::UncertainTarget => {
                            cleanup_uncertain = true;
                        }
                    }
                    return Err(DisplayExecFailure {
                        report,
                        error: failure.error,
                        uncertain_stop,
                        cleanup_uncertain,
                    });
                }
                report.record_apply(CompletedApply {
                    backend: operation.backend(),
                    scope: operation.scope().clone(),
                    path: operation.path().to_string(),
                });
            }
        }
    }
    apply_stage::report_stage(reporter, apply_stage::ApplyStage::RefreshStatus, request_id);
    Ok(report)
}

#[allow(clippy::result_large_err)]
pub(crate) fn prepare_display_actions(
    s: &StorageApi,
    actions: &[DisplayExecAction],
    ctx: &DisplayExecContext<'_>,
    runtime: &mut dyn BackendRuntime,
    request_id: Option<&str>,
    stopped_backends: &mut Vec<Backend>,
) -> Result<PreparedDisplayActions, DisplayExecFailure> {
    let mut prepared = Vec::with_capacity(actions.len());
    let mut preceding_stop = false;
    for action in actions {
        match action {
            DisplayExecAction::Stop { backend, scope } => {
                let execution = match classify_stop_scope(*backend, scope, ctx.known_outputs) {
                    Ok(execution) => execution,
                    Err(error) => {
                        return Err(DisplayExecFailure {
                            report: DisplayExecReport::default(),
                            error,
                            uncertain_stop: None,
                            cleanup_uncertain: false,
                        });
                    }
                };
                if matches!(execution, StopExecution::Scoped(_))
                    && !stopped_backends.contains(backend)
                {
                    let Some(backend_driver) = driver::driver_for(*backend) else {
                        return Err(DisplayExecFailure {
                            report: DisplayExecReport::default(),
                            error: WcError::Other(format!(
                                "no driver registered for backend {}",
                                backend.as_str()
                            )),
                            uncertain_stop: None,
                            cleanup_uncertain: false,
                        });
                    };
                    if let Err(error) = backend_driver.preflight_stop(scope, runtime) {
                        return Err(DisplayExecFailure {
                            report: DisplayExecReport::default(),
                            error,
                            uncertain_stop: None,
                            cleanup_uncertain: false,
                        });
                    }
                }
                if matches!(execution, StopExecution::Global) {
                    stopped_backends.push(*backend);
                }
                preceding_stop = true;
                prepared.push(PreparedDisplayAction::Stop {
                    backend: *backend,
                    scope: scope.clone(),
                    execution,
                });
            }
            DisplayExecAction::Apply {
                backend,
                path,
                scope,
                use_instant,
            } => {
                let Some(backend_driver) = driver::driver_for(*backend) else {
                    return Err(DisplayExecFailure {
                        report: DisplayExecReport::default(),
                        error: WcError::UnsupportedFileType(path.clone()),
                        uncertain_stop: None,
                        cleanup_uncertain: false,
                    });
                };
                let operation = match backend_driver.prepare(
                    s,
                    &driver::PrepareApplyRequest {
                        path,
                        scope,
                        after_stop: *use_instant || preceding_stop,
                        stopped_backends,
                        clear_state_hint: false,
                        request_id,
                    },
                    runtime,
                ) {
                    Ok(operation) => operation,
                    Err(error) => {
                        return Err(DisplayExecFailure {
                            report: DisplayExecReport::default(),
                            error,
                            uncertain_stop: None,
                            cleanup_uncertain: false,
                        });
                    }
                };
                prepared.push(PreparedDisplayAction::Apply(Box::new(operation)));
            }
        }
    }
    Ok(prepared)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StopExecution {
    Global,
    Scoped(Vec<String>),
}

/// Validate that a stop scope is executable for `backend`.
///
/// Named scopes covering every known output are accepted as global. Partial
/// named scopes are accepted only when the backend supports output-scoped stop.
pub fn validate_stop_scope(
    backend: Backend,
    scope: &ExecutionScope,
    known_outputs: &[String],
) -> Result<(), WcError> {
    classify_stop_scope(backend, scope, known_outputs).map(|_| ())
}

fn classify_stop_scope(
    backend: Backend,
    scope: &ExecutionScope,
    known_outputs: &[String],
) -> Result<StopExecution, WcError> {
    scope.validate()?;
    match scope {
        ExecutionScope::AllDisplays => Ok(StopExecution::Global),
        ExecutionScope::Named(outputs) => {
            let known: HashSet<&str> = known_outputs.iter().map(String::as_str).collect();
            let named: HashSet<&str> = outputs.iter().map(String::as_str).collect();
            if named == known && !known.is_empty() {
                return Ok(StopExecution::Global);
            }
            let supports_scoped = crate::driver::driver_for(backend)
                .is_some_and(|driver| driver.supports_output_scoped_stop());
            if supports_scoped {
                Ok(StopExecution::Scoped(outputs.clone()))
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
    execution: &StopExecution,
    runtime: &mut dyn BackendRuntime,
) -> Result<(), WcError> {
    let Some(driver) = crate::driver::driver_for(backend) else {
        return Ok(());
    };
    match execution {
        StopExecution::Global => driver.stop_checked(runtime, Some(s)),
        StopExecution::Scoped(outputs) => {
            let scope = ExecutionScope::named(outputs.clone())
                .map_err(|error| WcError::Other(format!("invalid scoped stop outputs: {error}")))?;
            driver.stop_scoped_checked(runtime, Some(s), &scope)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apply_stage::NoopReporter;
    use crate::runtime::{AwwwReadiness, MpvpaperOutputSelector, MpvpaperProcess, ProcessIo};
    use crate::test_support::FakeRuntime;
    use std::process::Command;
    use wc_core::config::ConfigDir;

    fn temp_storage() -> (tempfile::TempDir, StorageApi) {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        wc_config::write_config_value(&cd.path, "storage_backend", "sqlite").unwrap();
        let s = StorageApi::new(cd);
        (tmp, s)
    }

    fn ctx<'a>(known: &'a [String]) -> DisplayExecContext<'a> {
        DisplayExecContext {
            known_outputs: known,
        }
    }

    #[test]
    fn failed_new_daemon_release_cleans_up_and_never_kills_sibling_video() {
        for query_failed in [false, true] {
            let (tmp, storage) = temp_storage();
            let image = tmp.path().join("image.png");
            std::fs::write(&image, b"image").unwrap();
            let known = vec!["eDP-1".into(), "DP-8".into()];
            let mut runtime = FakeRuntime {
                command_status_success: true,
                command_output_success: false,
                stop_awww_query_failed: query_failed,
                awww_readiness_sequence: std::cell::RefCell::new(vec![
                    AwwwReadiness::SocketMissing,
                    AwwwReadiness::SocketMissing,
                    AwwwReadiness::Ready,
                ]),
                mpvpaper_process_table: vec![MpvpaperProcess {
                    pid: 42,
                    path: "/video.mp4".into(),
                    selector: MpvpaperOutputSelector::Single("DP-8".into()),
                }],
                ..Default::default()
            };
            let failure = execute_display_actions(
                &storage,
                &[DisplayExecAction::Apply {
                    backend: Backend::Awww,
                    path: image.to_string_lossy().into(),
                    scope: ExecutionScope::named(vec!["eDP-1".into()]).unwrap(),
                    use_instant: true,
                }],
                &ctx(&known),
                &mut runtime,
                &mut NoopReporter,
                None,
            )
            .unwrap_err();
            assert_eq!(runtime.stop_awww_count, 1);
            assert_eq!(runtime.stop_mpvpaper_count, 0);
            assert!(runtime.stop_mpvpaper_outputs_calls.is_empty());
            assert_eq!(failure.cleanup_uncertain, query_failed);
        }
    }

    #[test]
    fn global_retirement_does_not_require_mixed_environment() {
        let (tmp, storage) = temp_storage();
        let video = tmp.path().join("video.mp4");
        std::fs::write(&video, b"video").unwrap();
        let known = vec!["eDP-1".into(), "DP-8".into()];
        let mut runtime = FakeRuntime {
            awww_version: Some("awww 0.11.0".into()),
            ..Default::default()
        };
        let apply = DisplayExecAction::Apply {
            backend: Backend::Mpvpaper,
            path: video.to_string_lossy().into(),
            scope: ExecutionScope::named(vec!["DP-8".into()]).unwrap(),
            use_instant: true,
        };
        assert!(prepare_display_actions(
            &storage,
            &[apply.clone()],
            &ctx(&known),
            &mut runtime,
            None,
            &mut Vec::new()
        )
        .is_err());
        prepare_display_actions(
            &storage,
            &[
                DisplayExecAction::Stop {
                    backend: Backend::Awww,
                    scope: ExecutionScope::AllDisplays,
                },
                apply,
            ],
            &ctx(&known),
            &mut runtime,
            None,
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(runtime.stop_awww_count, 0);
    }

    #[test]
    fn named_awww_release_keeps_shared_daemon_and_records_target_only() {
        let (_tmp, storage) = temp_storage();
        let known = vec!["eDP-1".into(), "DP-8".into()];
        let mut runtime = FakeRuntime {
            command_output_success: true,
            ..Default::default()
        };
        let scope = ExecutionScope::named(vec!["eDP-1".into()]).unwrap();
        let report = execute_display_actions(
            &storage,
            &[DisplayExecAction::Stop {
                backend: Backend::Awww,
                scope: scope.clone(),
            }],
            &ctx(&known),
            &mut runtime,
            &mut NoopReporter,
            None,
        )
        .unwrap();
        assert_eq!(runtime.stop_awww_count, 0);
        assert_eq!(report.completed_stops().next().unwrap().scope, scope);
        assert!(runtime
            .command_output_args
            .iter()
            .any(|args| args == &["clear", "--outputs", "eDP-1", "00000000"]));
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

        assert!(report.completed_stops().next().is_none());
        assert_eq!(report.completed_applies().count(), 1);
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
    fn later_invalid_apply_is_rejected_before_an_earlier_stop() {
        let (tmp, storage) = temp_storage();
        let known = vec!["eDP-1".into()];
        let actions = vec![
            DisplayExecAction::Stop {
                backend: Backend::Mpvpaper,
                scope: ExecutionScope::AllDisplays,
            },
            DisplayExecAction::Apply {
                backend: Backend::Awww,
                path: tmp
                    .path()
                    .join("missing.jpg")
                    .to_string_lossy()
                    .into_owned(),
                scope: ExecutionScope::named(known.clone()).unwrap(),
                use_instant: true,
            },
        ];
        let mut runtime = FakeRuntime::default();
        let mut reporter = NoopReporter;

        let failure = execute_display_actions(
            &storage,
            &actions,
            &ctx(&known),
            &mut runtime,
            &mut reporter,
            None,
        )
        .unwrap_err();

        assert!(matches!(failure.error, WcError::NotRegularFile(_)));
        assert_eq!(runtime.stop_mpvpaper_count, 0);
        assert!(failure.report.events.is_empty());
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
        assert!(report.completed_stops().next().unwrap().destructive);
    }

    #[test]
    fn partial_named_stop_is_rejected_without_running_global_stop() {
        let (_tmp, s) = temp_storage();
        let known = vec!["eDP-1".into(), "HDMI-1".into()];
        let mut rt = FakeRuntime {
            extra_command_lines: vec![vec![
                "linux-wallpaperengine",
                "--screen-root",
                "eDP-1",
                "--bg",
                "42",
                "--screen-root",
                "HDMI-1",
                "--bg",
                "42",
            ]
            .into_iter()
            .map(str::to_string)
            .collect()],
            ..Default::default()
        };
        let mut reporter = NoopReporter;
        let err = execute_display_actions(
            &s,
            &[DisplayExecAction::Stop {
                backend: Backend::LinuxWallpaperEngine,
                scope: ExecutionScope::named(vec!["eDP-1".into()]).unwrap(),
            }],
            &ctx(&known),
            &mut rt,
            &mut reporter,
            None,
        )
        .unwrap_err();
        assert!(err.error.to_string().contains("non-target display HDMI-1"));
        assert_eq!(rt.stop_lwe_count, 0);
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
        assert!(err.report.completed_applies().next().is_none());
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
        assert_eq!(err.report.completed_stops().count(), 1);
        assert!(err.report.completed_applies().next().is_none());
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
        impl ProcessIo for SeqRuntime {
            fn awww_query_json(&mut self) -> Result<String, WcError> {
                self.inner.awww_query_json()
            }
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
            fn mpvpaper_processes(
                &mut self,
            ) -> Result<Vec<crate::runtime::MpvpaperProcess>, WcError> {
                self.inner.mpvpaper_processes()
            }
            fn wait_for_mpvpaper_ready(
                &mut self,
                previous_pids: &[u32],
                output: &str,
                path: &str,
            ) -> Result<u32, WcError> {
                self.inner
                    .wait_for_mpvpaper_ready(previous_pids, output, path)
            }
            fn mpvpaper_pid_running(&mut self, pid: u32) -> Result<bool, WcError> {
                self.inner.mpvpaper_pid_running(pid)
            }
            fn cleanup_failed_mpvpaper_launch(
                &mut self,
                previous_pids: &[u32],
                output: &str,
                path: &str,
            ) -> Result<(), WcError> {
                self.inner
                    .cleanup_failed_mpvpaper_launch(previous_pids, output, path)
            }
            fn awww_socket_ready(&mut self) -> AwwwReadiness {
                self.inner.awww_socket_ready()
            }
        }
        impl BackendRuntime for SeqRuntime {
            fn stop_awww(&mut self) {
                self.inner.stop_awww();
            }
            fn stop_mpvpaper(&mut self) {
                self.inner.stop_mpvpaper();
            }
            fn stop_mpvpaper_outputs(&mut self, outputs: &[String]) -> Result<(), WcError> {
                self.inner.stop_mpvpaper_outputs(outputs)
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
        assert_eq!(err.report.completed_applies().count(), 1);
        assert_eq!(
            err.report.completed_applies().next().unwrap().scope,
            ExecutionScope::named(vec!["eDP-1".into()]).unwrap()
        );
    }

    #[test]
    fn mpvpaper_readiness_failure_runs_targeted_cleanup_without_global_stop() {
        let (_tmp, s) = temp_storage();
        let video = _tmp.path().join("v.mp4");
        std::fs::write(&video, b"mp4").unwrap();
        let known = vec!["eDP-1".into(), "HDMI-1".into()];

        struct SeqRuntime {
            inner: FakeRuntime,
            status_calls: usize,
        }
        impl ProcessIo for SeqRuntime {
            fn awww_query_json(&mut self) -> Result<String, WcError> {
                self.inner.awww_query_json()
            }
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
            fn mpvpaper_processes(
                &mut self,
            ) -> Result<Vec<crate::runtime::MpvpaperProcess>, WcError> {
                self.inner.mpvpaper_processes()
            }
            fn wait_for_mpvpaper_ready(
                &mut self,
                previous_pids: &[u32],
                output: &str,
                path: &str,
            ) -> Result<u32, WcError> {
                if self.status_calls >= 2 {
                    return Err(WcError::Other("second instance not ready".into()));
                }
                self.inner
                    .wait_for_mpvpaper_ready(previous_pids, output, path)
            }
            fn mpvpaper_pid_running(&mut self, pid: u32) -> Result<bool, WcError> {
                self.inner.mpvpaper_pid_running(pid)
            }
            fn cleanup_failed_mpvpaper_launch(
                &mut self,
                previous_pids: &[u32],
                output: &str,
                path: &str,
            ) -> Result<(), WcError> {
                self.inner
                    .cleanup_failed_mpvpaper_launch(previous_pids, output, path)
            }
            fn awww_socket_ready(&mut self) -> AwwwReadiness {
                self.inner.awww_socket_ready()
            }
        }
        impl BackendRuntime for SeqRuntime {
            fn stop_awww(&mut self) {
                self.inner.stop_awww();
            }
            fn stop_mpvpaper(&mut self) {
                self.inner.stop_mpvpaper();
            }
            fn stop_mpvpaper_outputs(&mut self, outputs: &[String]) -> Result<(), WcError> {
                self.inner.stop_mpvpaper_outputs(outputs)
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
        assert_eq!(rt.inner.stop_mpvpaper_count, 0);
        assert_eq!(rt.inner.failed_mpvpaper_launch_cleanup_count, 1);
        assert_eq!(err.report.completed_applies().count(), 1);
        let mpvpaper_stops: Vec<_> = err
            .report
            .completed_stops()
            .filter(|stop| stop.backend == Backend::Mpvpaper)
            .collect();
        assert_eq!(
            mpvpaper_stops.len(),
            1,
            "readiness cleanup must record one named mpvpaper stop: {:?}",
            err.report.completed_stops().collect::<Vec<_>>()
        );
        assert_eq!(
            mpvpaper_stops[0].scope,
            ExecutionScope::named(vec!["HDMI-1".into()]).unwrap()
        );
        assert!(
            !matches!(mpvpaper_stops[0].scope, ExecutionScope::AllDisplays),
            "targeted cleanup must not report a global mpvpaper stop: {:?}",
            err.report.completed_stops().collect::<Vec<_>>()
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
        impl ProcessIo for FailStopRuntime {
            fn awww_query_json(&mut self) -> Result<String, WcError> {
                self.inner.awww_query_json()
            }
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
            fn mpvpaper_processes(
                &mut self,
            ) -> Result<Vec<crate::runtime::MpvpaperProcess>, WcError> {
                self.inner.mpvpaper_processes()
            }
            fn wait_for_mpvpaper_ready(
                &mut self,
                previous_pids: &[u32],
                output: &str,
                path: &str,
            ) -> Result<u32, WcError> {
                self.inner
                    .wait_for_mpvpaper_ready(previous_pids, output, path)
            }
            fn mpvpaper_pid_running(&mut self, pid: u32) -> Result<bool, WcError> {
                self.inner.mpvpaper_pid_running(pid)
            }
            fn cleanup_failed_mpvpaper_launch(
                &mut self,
                previous_pids: &[u32],
                output: &str,
                path: &str,
            ) -> Result<(), WcError> {
                self.inner
                    .cleanup_failed_mpvpaper_launch(previous_pids, output, path)
            }
            fn awww_socket_ready(&mut self) -> AwwwReadiness {
                self.inner.awww_socket_ready()
            }
        }
        impl BackendRuntime for FailStopRuntime {
            fn stop_awww(&mut self) {
                self.inner.stop_awww();
            }
            fn stop_mpvpaper(&mut self) {
                self.stop_checked_calls += 1;
                // Force driver stop_checked's pid probe to fail immediately.
                self.inner.mpvpaper_pids_error = Some("mpvpaper still running after stop".into());
            }
            fn stop_mpvpaper_outputs(&mut self, outputs: &[String]) -> Result<(), WcError> {
                self.inner.stop_mpvpaper_outputs(outputs)
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
        assert!(err.report.completed_stops().next().is_none());
        assert!(err.report.completed_applies().next().is_none());
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
        assert_eq!(rt.stop_mpvpaper_count, 0);
        assert_eq!(rt.failed_mpvpaper_launch_cleanup_count, 1);
        let mpvpaper_stops: Vec<_> = err
            .report
            .completed_stops()
            .filter(|stop| stop.backend == Backend::Mpvpaper)
            .collect();
        assert_eq!(mpvpaper_stops.len(), 1);
        assert_eq!(
            mpvpaper_stops[0].scope,
            ExecutionScope::named(vec!["eDP-1".into()]).unwrap()
        );
    }

    #[test]
    fn mpvpaper_launcher_failure_also_cleans_up_a_possible_detached_renderer() {
        let (tmp, storage) = temp_storage();
        let video = tmp.path().join("v.mp4");
        std::fs::write(&video, b"mp4").unwrap();
        let known = vec!["eDP-1".into()];
        let mut runtime = FakeRuntime {
            command_status_success: false,
            ..Default::default()
        };
        let mut reporter = NoopReporter;

        let failure = execute_display_actions(
            &storage,
            &[DisplayExecAction::Apply {
                backend: Backend::Mpvpaper,
                path: video.to_string_lossy().into_owned(),
                scope: ExecutionScope::named(known.clone()).unwrap(),
                use_instant: false,
            }],
            &ctx(&known),
            &mut runtime,
            &mut reporter,
            None,
        )
        .unwrap_err();

        assert!(failure.error.to_string().contains("mpvpaper"));
        assert_eq!(runtime.failed_mpvpaper_launch_cleanup_count, 1);
        assert_eq!(
            runtime.failed_mpvpaper_launch_cleanup_calls,
            vec![(
                Vec::new(),
                "eDP-1".to_string(),
                video.to_string_lossy().into_owned()
            )]
        );
        assert_eq!(runtime.stop_mpvpaper_count, 0);
        assert!(failure.report.completed_stops().next().is_none());
    }

    #[test]
    fn unverifiable_targeted_cleanup_is_explicitly_uncertain() {
        let (tmp, storage) = temp_storage();
        let video = tmp.path().join("v.mp4");
        std::fs::write(&video, b"mp4").unwrap();
        let known = vec!["eDP-1".into()];
        let mut runtime = FakeRuntime {
            command_status_success: false,
            failed_mpvpaper_launch_cleanup_error: Some("probe failed".into()),
            ..Default::default()
        };
        let mut reporter = NoopReporter;

        let failure = execute_display_actions(
            &storage,
            &[DisplayExecAction::Apply {
                backend: Backend::Mpvpaper,
                path: video.to_string_lossy().into_owned(),
                scope: ExecutionScope::named(known.clone()).unwrap(),
                use_instant: false,
            }],
            &ctx(&known),
            &mut runtime,
            &mut reporter,
            None,
        )
        .unwrap_err();

        assert!(failure.cleanup_uncertain);
        assert!(failure.uncertain_stop.is_none());
        assert!(failure.error.to_string().contains("could not be verified"));
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
        assert_eq!(report.completed_applies().count(), 1);
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

    #[test]
    fn mpvpaper_partial_named_stop_runs_scoped_stop_only() {
        let (_tmp, s) = temp_storage();
        let known = vec!["eDP-1".into(), "HDMI-1".into()];
        let mut rt = FakeRuntime {
            mpvpaper_process_table: vec![
                MpvpaperProcess::for_output(10, "eDP-1", "/a.mp4"),
                MpvpaperProcess::for_output(20, "HDMI-1", "/b.mp4"),
            ],
            ..Default::default()
        };
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
        assert_eq!(rt.stop_mpvpaper_count, 0);
        assert_eq!(
            rt.stop_mpvpaper_outputs_calls,
            vec![vec!["eDP-1".to_string()]]
        );
        assert_eq!(report.completed_stops().count(), 1);
        assert_eq!(
            report.completed_stops().next().unwrap().scope,
            ExecutionScope::named(vec!["eDP-1".into()]).unwrap()
        );
        assert!(
            rt.mpvpaper_process_table
                .iter()
                .any(|process| process.pid == 20
                    && process.selector == MpvpaperOutputSelector::Single("HDMI-1".into())),
            "sibling process must remain: {:?}",
            rt.mpvpaper_process_table
        );
    }

    #[test]
    fn mpvpaper_partial_named_stop_refused_when_wildcard_process_exists() {
        let (_tmp, s) = temp_storage();
        let known = vec!["eDP-1".into(), "HDMI-1".into()];
        let mut rt = FakeRuntime {
            mpvpaper_process_table: vec![
                MpvpaperProcess::for_output(10, "eDP-1", "/a.mp4"),
                MpvpaperProcess::for_output(99, "*", "/wild.mp4"),
            ],
            ..Default::default()
        };
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
        assert!(err.error.to_string().contains("own multiple/all outputs"));
        assert_eq!(rt.stop_mpvpaper_count, 0);
        assert!(rt.stop_mpvpaper_outputs_calls.is_empty());
        assert!(err.report.events.is_empty());
        assert!(!err.after_destructive_stop());
        assert_eq!(rt.mpvpaper_process_table.len(), 2);
    }

    #[test]
    fn mpvpaper_partial_named_stop_refused_when_multi_output_process_exists() {
        let (_tmp, s) = temp_storage();
        let known = vec!["eDP-1".into(), "HDMI-1".into()];
        let mut rt = FakeRuntime {
            mpvpaper_process_table: vec![
                MpvpaperProcess::for_output(10, "eDP-1", "/a.mp4"),
                MpvpaperProcess::for_output(99, "eDP-1 HDMI-1", "/multi.mp4"),
            ],
            ..Default::default()
        };
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
        assert!(err.error.to_string().contains("own multiple/all outputs"));
        assert_eq!(rt.stop_mpvpaper_count, 0);
        assert!(rt.stop_mpvpaper_outputs_calls.is_empty());
        assert!(!err.after_destructive_stop());
        assert_eq!(rt.mpvpaper_process_table.len(), 2);
    }

    #[test]
    fn mpvpaper_partial_named_stop_refused_when_unparseable_process_exists() {
        let (_tmp, s) = temp_storage();
        let known = vec!["eDP-1".into(), "HDMI-1".into()];
        let mut rt = FakeRuntime {
            mpvpaper_process_table: vec![
                MpvpaperProcess::for_output(10, "eDP-1", "/a.mp4"),
                MpvpaperProcess {
                    pid: 99,
                    selector: MpvpaperOutputSelector::Unparseable,
                    path: "/mystery.mp4".into(),
                },
            ],
            ..Default::default()
        };
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
        assert!(err.error.to_string().contains("own multiple/all outputs"));
        assert_eq!(rt.stop_mpvpaper_count, 0);
        assert!(rt.stop_mpvpaper_outputs_calls.is_empty());
        assert!(!err.after_destructive_stop());
        assert_eq!(rt.mpvpaper_process_table.len(), 2);
    }

    #[test]
    fn full_coverage_named_stop_still_global() {
        let (_tmp, s) = temp_storage();
        let known = vec!["eDP-1".into(), "HDMI-1".into()];
        let mut rt = FakeRuntime {
            mpvpaper_process_table: vec![
                MpvpaperProcess::for_output(10, "eDP-1", "/a.mp4"),
                MpvpaperProcess::for_output(20, "HDMI-1", "/b.mp4"),
            ],
            ..Default::default()
        };
        let mut reporter = NoopReporter;
        execute_display_actions(
            &s,
            &[DisplayExecAction::Stop {
                backend: Backend::Mpvpaper,
                scope: ExecutionScope::named(known.clone()).unwrap(),
            }],
            &ctx(&known),
            &mut rt,
            &mut reporter,
            None,
        )
        .unwrap();
        assert_eq!(rt.stop_mpvpaper_count, 1);
        assert!(rt.stop_mpvpaper_outputs_calls.is_empty());
    }

    #[test]
    fn failed_awww_release_is_uncertain_without_killing_daemon() {
        let (_tmp, s) = temp_storage();
        let known = vec!["eDP-1".into(), "HDMI-1".into()];
        let mut rt = FakeRuntime::default();
        let mut reporter = NoopReporter;
        let err = execute_display_actions(
            &s,
            &[DisplayExecAction::Stop {
                backend: Backend::Awww,
                scope: ExecutionScope::named(vec!["eDP-1".into()]).unwrap(),
            }],
            &ctx(&known),
            &mut rt,
            &mut reporter,
            None,
        )
        .unwrap_err();
        assert!(err.error.to_string().contains("transparent release failed"));
        assert!(err.cleanup_uncertain);
        assert!(err.report.completed_stops().next().is_none());
        assert_eq!(rt.stop_awww_count, 0);
    }

    #[test]
    fn d3_mpvpaper_prepare_fails_closed_when_awww_socket_query_fails() {
        let (tmp, s) = temp_storage();
        let video = tmp.path().join("video.mp4");
        std::fs::write(&video, b"video").unwrap();
        let known = vec!["eDP-1".into(), "DP-8".into()];
        let mut rt = FakeRuntime {
            command_status_success: true,
            awww_readiness_sequence: std::cell::RefCell::new(vec![
                AwwwReadiness::SocketPresentQueryFailed {
                    stderr: "daemon busy".into(),
                },
            ]),
            ..Default::default()
        };
        let mut reporter = NoopReporter;
        let err = execute_display_actions(
            &s,
            &[DisplayExecAction::Apply {
                backend: Backend::Mpvpaper,
                path: video.to_string_lossy().into(),
                scope: ExecutionScope::named(vec!["DP-8".into()]).unwrap(),
                use_instant: true,
            }],
            &ctx(&known),
            &mut rt,
            &mut reporter,
            None,
        )
        .unwrap_err();
        assert!(
            err.error.to_string().contains("cannot verify awww state"),
            "unexpected error: {}",
            err.error
        );
        assert!(
            err.report.completed_applies().next().is_none(),
            "a video must never report success while awww ownership is unknown"
        );
        assert!(rt.command_status_args.is_empty(), "no launch attempted");
        assert_eq!(rt.stop_awww_count, 0);
        assert_eq!(rt.stop_mpvpaper_count, 0);
        assert!(!err.cleanup_uncertain);
    }

    #[test]
    fn d3_mpvpaper_execute_rechecks_awww_and_keeps_prior_stops_in_failure_report() {
        let (tmp, s) = temp_storage();
        let video = tmp.path().join("video.mp4");
        std::fs::write(&video, b"video").unwrap();
        let known = vec!["eDP-1".into(), "DP-8".into()];
        let mut rt = FakeRuntime {
            command_status_success: true,
            // Prepare sees a ready daemon; by execution the socket stops answering.
            awww_readiness_sequence: std::cell::RefCell::new(vec![
                AwwwReadiness::Ready,
                AwwwReadiness::SocketPresentQueryFailed {
                    stderr: "query timed out".into(),
                },
            ]),
            ..Default::default()
        };
        let mut reporter = NoopReporter;
        let err = execute_display_actions(
            &s,
            &[
                DisplayExecAction::Stop {
                    backend: Backend::Mpvpaper,
                    scope: ExecutionScope::AllDisplays,
                },
                DisplayExecAction::Apply {
                    backend: Backend::Mpvpaper,
                    path: video.to_string_lossy().into(),
                    scope: ExecutionScope::named(vec!["DP-8".into()]).unwrap(),
                    use_instant: true,
                },
            ],
            &ctx(&known),
            &mut rt,
            &mut reporter,
            None,
        )
        .unwrap_err();
        assert!(
            err.error
                .to_string()
                .contains("cannot verify awww released output DP-8"),
            "unexpected error: {}",
            err.error
        );
        assert_eq!(
            err.report.completed_stops().count(),
            1,
            "the confirmed stop must stay in the failure report"
        );
        assert!(err.report.completed_applies().next().is_none());
        assert!(rt.command_status_args.is_empty(), "no launch attempted");
        assert!(!err.cleanup_uncertain, "nothing was launched or cleaned up");
    }

    #[test]
    fn d3_failed_transparent_release_blocks_video_launch() {
        let (tmp, s) = temp_storage();
        let video = tmp.path().join("video.mp4");
        std::fs::write(&video, b"video").unwrap();
        let known = vec!["eDP-1".into(), "DP-8".into()];
        let mut rt = FakeRuntime {
            // Daemon ready and still displays an image on the target, but the
            // release command fails: the video must not start underneath it.
            command_output_success: false,
            command_status_success: true,
            awww_displayed: vec![("DP-8".into(), "/walls/still.jpg".into())],
            ..Default::default()
        };
        let mut reporter = NoopReporter;
        let err = execute_display_actions(
            &s,
            &[DisplayExecAction::Apply {
                backend: Backend::Mpvpaper,
                path: video.to_string_lossy().into(),
                scope: ExecutionScope::named(vec!["DP-8".into()]).unwrap(),
                use_instant: true,
            }],
            &ctx(&known),
            &mut rt,
            &mut reporter,
            None,
        )
        .unwrap_err();
        assert!(
            err.error.to_string().contains("transparent release"),
            "unexpected error: {}",
            err.error
        );
        assert!(err.report.completed_applies().next().is_none());
        assert!(rt.command_status_args.is_empty(), "no launch attempted");
    }

    #[test]
    fn d3_missing_socket_and_released_target_still_launch_video() {
        for readiness in [
            AwwwReadiness::SocketMissing,
            AwwwReadiness::Ready, // ready daemon, target has no surface
        ] {
            let (tmp, s) = temp_storage();
            let video = tmp.path().join("video.mp4");
            std::fs::write(&video, b"video").unwrap();
            let known = vec!["eDP-1".into(), "DP-8".into()];
            let mut rt = FakeRuntime {
                command_status_success: true,
                awww_readiness_sequence: std::cell::RefCell::new(vec![readiness]),
                ..Default::default()
            };
            let mut reporter = NoopReporter;
            let report = execute_display_actions(
                &s,
                &[DisplayExecAction::Apply {
                    backend: Backend::Mpvpaper,
                    path: video.to_string_lossy().into(),
                    scope: ExecutionScope::named(vec!["DP-8".into()]).unwrap(),
                    use_instant: true,
                }],
                &ctx(&known),
                &mut rt,
                &mut reporter,
                None,
            )
            .unwrap();
            assert_eq!(report.completed_applies().count(), 1);
            assert_eq!(rt.command_status_args.len(), 1);
        }
    }
}
