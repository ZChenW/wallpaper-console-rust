//! Legacy fullscreen orchestration over the shared BackendDriver implementations.
use crate::debug_log::{write_apply_stage_timings, write_debug_handoff_log};
use crate::{apply_stage, driver, lifecycle, runtime, visual_handoff, ExecutionScope};
use wc_core::{error::WcError, types::Backend};
use wc_storage::StorageApi;

use crate::lifecycle::StopPlan;

pub(crate) fn execute_stop_plan_with_runtime(
    s: &StorageApi,
    plan: lifecycle::StopPlan,
    runtime: &mut dyn runtime::BackendRuntime,
) -> Result<(), WcError> {
    match plan {
        lifecycle::StopPlan::All => {
            if let Some(d) = driver::driver_for(Backend::Awww) {
                d.stop(runtime, Some(s));
            }
            if let Some(d) = driver::driver_for(Backend::Mpvpaper) {
                d.stop(runtime, Some(s));
            }
            if let Some(d) = driver::driver_for(Backend::Swaybg) {
                d.stop(runtime, Some(s));
            }
            if let Some(d) = driver::driver_for(Backend::LinuxWallpaperEngine) {
                d.stop(runtime, Some(s));
            }
            Ok(())
        }
        lifecycle::StopPlan::AwwwDaemonOnly => {
            if let Some(d) = driver::driver_for(Backend::Awww) {
                d.stop(runtime, Some(s));
            }
            Ok(())
        }
        lifecycle::StopPlan::MpvpaperOnly => {
            if let Some(d) = driver::driver_for(Backend::Mpvpaper) {
                d.stop(runtime, Some(s));
            }
            Ok(())
        }
        lifecycle::StopPlan::SwaybgOnly => {
            if let Some(d) = driver::driver_for(Backend::Swaybg) {
                d.stop(runtime, Some(s));
            }
            Ok(())
        }
        lifecycle::StopPlan::LweOnly => {
            if let Some(d) = driver::driver_for(Backend::LinuxWallpaperEngine) {
                d.stop(runtime, Some(s));
            }
            Ok(())
        }
        lifecycle::StopPlan::NonLwe => {
            if let Some(d) = driver::driver_for(Backend::Awww) {
                d.stop(runtime, Some(s));
            }
            if let Some(d) = driver::driver_for(Backend::Mpvpaper) {
                d.stop(runtime, Some(s));
            }
            if let Some(d) = driver::driver_for(Backend::Swaybg) {
                d.stop(runtime, Some(s));
            }
            Ok(())
        }
        lifecycle::StopPlan::None => Ok(()),
    }
}

fn write_success_state(s: &StorageApi, state_path: &str, backend: Backend) -> Result<(), WcError> {
    s.runtime_state_write_pair(state_path, backend.as_str())
}

/// Apply a wallpaper with the legacy fullscreen orchestrator.
///
/// Prefer display-aware apply via `wc_app` (`apply_to_display` /
/// `execute_apply_request`), which runs [`crate::apply_transition`] around the
/// display_plan Stop/Apply skeleton. Kept for backend unit tests and
/// [`crate::restore_clean`].
///
/// State is written ONLY after successful backend execution.
pub fn apply_wallpaper(
    s: &StorageApi,
    path: &str,
    backend: Backend,
    fallback_path: Option<&str>,
) -> Result<(), WcError> {
    let mut runtime = runtime::SystemBackendRuntime;
    let mut reporter = apply_stage::NoopReporter;
    apply_wallpaper_with_runtime(
        s,
        path,
        backend,
        fallback_path,
        &mut runtime,
        &mut reporter,
        None,
    )
}

/// Apply a wallpaper and emit structured apply stages through `reporter`.
pub fn apply_wallpaper_with_reporter(
    s: &StorageApi,
    path: &str,
    backend: Backend,
    fallback_path: Option<&str>,
    reporter: &mut dyn apply_stage::ApplyStageReporter,
    request_id: Option<&str>,
) -> Result<(), WcError> {
    let mut runtime = runtime::SystemBackendRuntime;
    apply_wallpaper_with_runtime(
        s,
        path,
        backend,
        fallback_path,
        &mut runtime,
        reporter,
        request_id,
    )
}

pub(crate) fn apply_wallpaper_with_runtime(
    s: &StorageApi,
    path: &str,
    backend: Backend,
    fallback_path: Option<&str>,
    runtime: &mut dyn runtime::BackendRuntime,
    reporter: &mut dyn apply_stage::ApplyStageReporter,
    request_id: Option<&str>,
) -> Result<(), WcError> {
    let previous_backend_raw = s.last_backend_read()?.unwrap_or_default();
    let lifecycle = lifecycle::plan_apply_lifecycle(&previous_backend_raw, backend);
    let visual = visual_handoff::plan_visual_handoff(lifecycle.previous, backend, fallback_path);
    let use_instant = lifecycle.previous == lifecycle::RunningBackend::None;
    let clear_state_hint = matches!(
        lifecycle.previous,
        lifecycle::RunningBackend::Mpvpaper
            | lifecycle::RunningBackend::LinuxWallpaperEngine
            | lifecycle::RunningBackend::Unknown
    );
    let mut prepared_target = driver::prepare_legacy_apply(
        s,
        backend,
        path,
        use_instant,
        clear_state_hint,
        request_id,
        runtime,
    )?;
    let mut prepared_fallback =
        if visual.fallback_stage == visual_handoff::FallbackStage::TargetImageInstant {
            fallback_path
                .map(|fallback| {
                    driver::driver_for(Backend::Awww)
                        .expect("awww driver")
                        .prepare(
                            s,
                            &driver::PrepareApplyRequest {
                                path: fallback,
                                scope: &ExecutionScope::AllDisplays,
                                stopped_backends: &[],
                                after_stop: true,
                                clear_state_hint: false,
                                request_id,
                            },
                            runtime,
                        )
                })
                .transpose()?
        } else {
            None
        };

    prepared_target.verify_media()?;
    if let Some(fallback) = &prepared_fallback {
        fallback.verify_media()?;
    }
    let timing_start = std::time::Instant::now();
    execute_stop_plan_with_runtime(s, lifecycle.pre_stop, runtime)?;
    let pre_stop_elapsed = timing_start.elapsed();

    let fallback_ok = match visual.fallback_stage {
        visual_handoff::FallbackStage::TargetImageInstant => {
            if let (Some(fb), Some(prepared)) = (fallback_path, prepared_fallback.as_mut()) {
                match prepared.execute(s, runtime, reporter) {
                    Ok(()) => {
                        std::thread::sleep(std::time::Duration::from_millis(
                            visual_handoff::AWWW_FALLBACK_SETTLE_MS,
                        ));
                        true
                    }
                    Err(failure) => {
                        let fb_name = std::path::Path::new(fb)
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| "<unknown>".to_string());
                        let msg = format!(
                            "instant awww fallback {} failed: {}",
                            fb_name, failure.error
                        );
                        write_debug_handoff_log(
                            s,
                            &lifecycle,
                            backend,
                            fallback_path,
                            &visual,
                            &msg,
                            path,
                        );
                        return Err(WcError::Other(msg));
                    }
                }
            } else {
                false
            }
        }
        visual_handoff::FallbackStage::None => false,
    };
    let fallback_elapsed = timing_start.elapsed();

    if visual.stop_previous_after_fallback {
        let stop_target = lifecycle.post_success_stop;
        if stop_target != StopPlan::None {
            let _ = execute_stop_plan_with_runtime(s, stop_target, runtime);
        }
    }

    let target_result = if backend == Backend::Awww && fallback_ok {
        Ok(())
    } else {
        prepared_target.execute(s, runtime, reporter)
    };
    let target_elapsed = timing_start.elapsed();

    if let Err(failure) = target_result {
        if matches!(failure.cleanup, driver::CleanupOutcome::UncertainTarget) {
            let _ = s.runtime_state_clear();
        }
        let rollback_msg = rollback_visual_fallback_after_target_failure_with_runtime(
            s,
            lifecycle.previous,
            fallback_ok,
            runtime,
        );
        if let Some(msg) = rollback_msg {
            write_debug_handoff_log(s, &lifecycle, backend, fallback_path, &visual, &msg, path);
        }
        return Err(failure.error);
    }

    if visual.target_startup_settle_ms > 0 {
        std::thread::sleep(std::time::Duration::from_millis(
            visual.target_startup_settle_ms,
        ));
    }

    if lifecycle.post_success_settle_ms > 0 {
        std::thread::sleep(std::time::Duration::from_millis(
            lifecycle.post_success_settle_ms,
        ));
    }

    if fallback_ok && visual.stop_fallback_after_target_settle {
        runtime.stop_awww();
    }

    let already_stopped = visual.stop_previous_after_fallback
        && visual.fallback_stage != visual_handoff::FallbackStage::None;
    if !already_stopped {
        apply_stage::report_stage(
            reporter,
            apply_stage::ApplyStage::CleanupPrevious,
            request_id,
        );
        execute_stop_plan_with_runtime(s, lifecycle.post_success_stop, runtime)?;
    }

    write_debug_handoff_log(s, &lifecycle, backend, fallback_path, &visual, "", path);
    write_apply_stage_timings(
        s,
        pre_stop_elapsed,
        fallback_elapsed - pre_stop_elapsed,
        target_elapsed - fallback_elapsed,
        timing_start.elapsed() - target_elapsed,
        backend,
    );

    apply_stage::report_stage(reporter, apply_stage::ApplyStage::RefreshStatus, request_id);
    write_success_state(s, path, backend)?;
    Ok(())
}

fn apply_awww_instant_with_runtime(
    s: &StorageApi,
    path: &str,
    runtime: &mut dyn runtime::BackendRuntime,
    reporter: Option<&mut dyn apply_stage::ApplyStageReporter>,
    request_id: Option<&str>,
) -> Result<(), WcError> {
    driver::apply_awww_instant(
        s,
        path,
        &ExecutionScope::AllDisplays,
        runtime,
        reporter,
        request_id,
    )
}

pub(super) fn rollback_visual_fallback_after_target_failure_with_runtime(
    s: &StorageApi,
    previous: lifecycle::RunningBackend,
    fallback_ok: bool,
    runtime: &mut dyn runtime::BackendRuntime,
) -> Option<String> {
    if !fallback_ok {
        return None;
    }

    if previous == lifecycle::RunningBackend::Awww {
        if let Some(old_path) = s.current_read().ok().flatten() {
            let p = std::path::Path::new(&old_path);
            if p.is_file() {
                match apply_awww_instant_with_runtime(s, &old_path, runtime, None, None) {
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
