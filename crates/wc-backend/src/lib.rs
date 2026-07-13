//! wc-backend — wallpaper backend process management.

use std::process::{Command, Stdio};
use wc_core::error::WcError;
use wc_core::types::Backend;
use wc_storage::StorageApi;

pub mod apply_stage;
pub mod capability;
pub mod display_executor;
pub mod lifecycle;
pub mod linux_wallpaperengine;
pub mod process_control;
pub mod runtime;
pub mod target_commands;
pub mod visual_handoff;

mod awww;
mod debug_log;
mod mpvpaper;
mod restore;

pub use display_executor::{
    execute_display_actions, DisplayExecAction, DisplayExecContext, DisplayExecFailure,
    DisplayExecReport,
};
pub use restore::{restore, restore_clean};
pub use target_commands::ExecutionScope;

use awww::{
    build_awww_img_command, build_awww_instant_command, normalize_awww_resize,
    normalize_awww_transition_type, stop_awww,
};
use debug_log::{write_apply_stage_timings, write_debug_handoff_log};
use mpvpaper::{build_launch_command, normalize_mpvpaper_options};

/// Stop all wallpaper backends via pkill.
pub fn stop_all_backends(s: Option<&StorageApi>) -> Result<(), WcError> {
    let user = whoami();
    linux_wallpaperengine::stop(s);
    let _ = Command::new("pkill")
        .args(["-u", &user, "-f", r"(^|/)mpvpaper\b"])
        .status();
    stop_awww();
    // Fallback cleanup: kill residual scene renderer processes that may not have been
    // recorded in config (e.g. setsid forked and parent PID was recorded, or a crash
    // left the process behind).
    let _ = Command::new("pkill")
        .args(["-u", &user, "-f", r"(^|/)linux-wallpaperengine\b"])
        .status();
    Ok(())
}

/// Backend name constant used for LWE state tracking.
pub const LWE_BACKEND_NAME: &str = "linux-wallpaperengine";

/// Stop only non-LWE wallpaper backends (mpvpaper, awww-daemon).
pub fn stop_non_lwe_backends(_s: &StorageApi) {
    let user = whoami();
    let _ = Command::new("pkill")
        .args(["-u", &user, "-f", r"(^|/)mpvpaper\b"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    stop_awww();
}

use lifecycle::StopPlan;

pub(crate) fn execute_stop_plan_with_runtime(
    s: &StorageApi,
    plan: lifecycle::StopPlan,
    runtime: &mut dyn runtime::BackendRuntime,
) -> Result<(), WcError> {
    match plan {
        lifecycle::StopPlan::All => {
            runtime.stop_awww();
            runtime.stop_mpvpaper();
            runtime.stop_lwe(Some(s));
            Ok(())
        }
        lifecycle::StopPlan::AwwwDaemonOnly => {
            runtime.stop_awww();
            Ok(())
        }
        lifecycle::StopPlan::MpvpaperOnly => {
            runtime.stop_mpvpaper();
            Ok(())
        }
        lifecycle::StopPlan::LweOnly => {
            runtime.stop_lwe(Some(s));
            Ok(())
        }
        lifecycle::StopPlan::NonLwe => {
            runtime.stop_awww();
            runtime.stop_mpvpaper();
            Ok(())
        }
        lifecycle::StopPlan::None => Ok(()),
    }
}

fn write_success_state(s: &StorageApi, state_path: &str, backend: Backend) -> Result<(), WcError> {
    s.current_write(state_path)?;
    s.last_backend_write(backend.as_str())?;
    Ok(())
}

/// Apply a wallpaper via the appropriate backend process.
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
    let p = std::path::Path::new(path);
    if backend == Backend::Unsupported {
        return Err(WcError::UnsupportedFileType(path.to_string()));
    }
    if backend != Backend::LinuxWallpaperEngine && !p.is_file() {
        return Err(WcError::NotRegularFile(p.to_path_buf()));
    }

    let previous_backend_raw = s.last_backend_read()?.unwrap_or_default();
    let lifecycle = lifecycle::plan_apply_lifecycle(&previous_backend_raw, backend);
    let visual = visual_handoff::plan_visual_handoff(lifecycle.previous, backend, fallback_path);
    let previous_mpvpaper_pids = if backend == Backend::Mpvpaper {
        runtime.mpvpaper_pids()?
    } else {
        Vec::new()
    };

    let timing_start = std::time::Instant::now();
    execute_stop_plan_with_runtime(s, lifecycle.pre_stop, runtime)?;
    let pre_stop_elapsed = timing_start.elapsed();

    let fallback_ok = match visual.fallback_stage {
        visual_handoff::FallbackStage::TargetImageInstant => {
            if let Some(fb) = fallback_path {
                match apply_awww_instant_with_runtime(s, fb, runtime, Some(reporter), request_id) {
                    Ok(()) => {
                        std::thread::sleep(std::time::Duration::from_millis(
                            visual_handoff::AWWW_FALLBACK_SETTLE_MS,
                        ));
                        true
                    }
                    Err(e) => {
                        let fb_name = std::path::Path::new(fb)
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| "<unknown>".to_string());
                        let msg = format!("instant awww fallback {} failed: {}", fb_name, e);
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

    let mut mpvpaper_launcher_succeeded = false;
    let mut launched_mpvpaper_pid = None;
    let target_result = if backend == Backend::Awww && fallback_ok {
        Ok(())
    } else {
        match backend {
            Backend::Awww => (|| -> Result<(), WcError> {
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
                if matches!(
                    lifecycle.previous,
                    lifecycle::RunningBackend::Mpvpaper
                        | lifecycle::RunningBackend::LinuxWallpaperEngine
                        | lifecycle::RunningBackend::Unknown
                ) {
                    runtime.clear_awww_state_hint();
                }
                let fps_raw = s.config_get("wallpaper_transition_fps", "60");
                let fps = wc_core::config_normalizer::normalize_awww_transition_fps(&fps_raw);
                let resize_raw = s.config_get("awww_resize", "crop");
                let resize = normalize_awww_resize(&resize_raw);
                let mut cmd = if lifecycle.previous == lifecycle::RunningBackend::None {
                    build_awww_instant_command(path, resize, &fps)
                } else {
                    let transition_raw = s.config_get("awww_transition_type", "fade");
                    let transition_type = normalize_awww_transition_type(&transition_raw);
                    let duration_raw = s.config_get("awww_transition_duration", "1");
                    let duration = wc_core::config_normalizer::normalize_awww_transition_duration(
                        &duration_raw,
                    );
                    let mut cmd =
                        build_awww_img_command(path, resize, transition_type, &duration, &fps);
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
            })(),
            Backend::Mpvpaper => {
                let opts_raw = s.config_get("mpvpaper_options", "--loop-file=inf --panscan=1.0");
                let opts = normalize_mpvpaper_options(&opts_raw);
                let output = s.config_get("mpvpaper_output", "*");
                let mut cmd = build_launch_command(opts, &output, path);
                let status = runtime
                    .command_status(&mut cmd)
                    .map_err(|e| WcError::Other(format!("mpvpaper failed: {}", e)))?;
                if !status.success() {
                    Err(WcError::Other("mpvpaper failed to apply wallpaper".into()))
                } else {
                    mpvpaper_launcher_succeeded = true;
                    runtime
                        .wait_for_mpvpaper_ready(&previous_mpvpaper_pids)
                        .map(|pid| launched_mpvpaper_pid = Some(pid))
                }
            }
            Backend::LinuxWallpaperEngine => {
                let project = linux_wallpaperengine::project_from_path(path)?;
                apply_stage::report_stage(reporter, apply_stage::ApplyStage::StartLwe, request_id);
                apply_stage::report_stage(
                    reporter,
                    apply_stage::ApplyStage::WaitRendererAlive,
                    request_id,
                );
                match linux_wallpaperengine::apply(s, project) {
                    Ok(()) => Ok(()),
                    Err(e) => Err(e),
                }
            }
            Backend::Unsupported => unreachable!(),
        }
    };
    let target_elapsed = timing_start.elapsed();

    if let Err(e) = target_result {
        if mpvpaper_launcher_succeeded {
            runtime.stop_mpvpaper();
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
        return Err(e);
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

    if let Some(pid) = launched_mpvpaper_pid {
        let readiness_error = match runtime.mpvpaper_pid_running(pid) {
            Ok(true) => None,
            Ok(false) => Some(WcError::Other(
                "mpvpaper renderer exited before startup settled".into(),
            )),
            Err(error) => Some(error),
        };
        if let Some(error) = readiness_error {
            runtime.stop_mpvpaper();
            let rollback_msg = rollback_visual_fallback_after_target_failure_with_runtime(
                s,
                lifecycle.previous,
                fallback_ok,
                runtime,
            );
            if let Some(msg) = rollback_msg {
                write_debug_handoff_log(s, &lifecycle, backend, fallback_path, &visual, &msg, path);
            }
            return Err(error);
        }
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
    match reporter {
        Some(reporter) => {
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
        }
        None => runtime.ensure_awww_daemon_running()?,
    }
    let resize_raw = s.config_get("awww_resize", "crop");
    let resize = normalize_awww_resize(&resize_raw);
    let fps_raw = s.config_get("wallpaper_transition_fps", "60");
    let fps = wc_core::config_normalizer::normalize_awww_transition_fps(&fps_raw);
    let mut cmd = build_awww_instant_command(path, resize, &fps);
    let output = runtime
        .command_output(&mut cmd)
        .map_err(|e| WcError::Other(format!("awww instant failed: {}", e)))?;
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
            "awww instant apply failed with status {}: {}",
            output.status, detail
        )));
    }
    Ok(())
}

fn rollback_visual_fallback_after_target_failure_with_runtime(
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

pub(crate) fn whoami() -> String {
    std::env::var("USER").unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::BackendRuntime;
    use wc_core::config::ConfigDir;

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

    fn insert_history(s: &StorageApi, path: &str, backend: &str) {
        let conn = wc_storage::sqlite::open_runtime_connection(&s.cd).unwrap();
        conn.execute(
            "INSERT INTO history (path, backend) VALUES (?1, ?2)",
            [path, backend],
        )
        .unwrap();
    }

    fn history_rows(s: &StorageApi) -> Vec<(String, String)> {
        let conn = wc_storage::sqlite::open_runtime_connection(&s.cd).unwrap();
        let mut stmt = conn
            .prepare("SELECT path, backend FROM history ORDER BY id")
            .unwrap();
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    }

    fn apply_with_fake_runtime(
        s: &StorageApi,
        path: &str,
        backend: Backend,
        fallback_path: Option<&str>,
        rt: &mut FakeRuntime,
    ) -> Result<(), WcError> {
        let mut reporter = apply_stage::NoopReporter;
        apply_wallpaper_with_runtime(s, path, backend, fallback_path, rt, &mut reporter, None)
    }

    #[test]
    fn restore_we_web_rejects_as_unsupported() {
        let (tmp, s) = temp_storage();

        let project = tmp
            .path()
            .join("steamapps/workshop/content/431960/3650880224");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("index.html"), b"<html></html>").unwrap();
        std::fs::write(
            project.join("project.json"),
            r#"{"type":"web","file":"index.html","title":"Test Web"}"#,
        )
        .unwrap();

        // Simulate a previous session having written a WE Web project as current.
        s.current_write(&project.to_string_lossy()).unwrap();
        s.last_backend_write("unsupported").unwrap();
        insert_history(&s, &project.to_string_lossy(), "unsupported");

        let history_before = history_rows(&s);

        let err = restore(&s).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("unsupported") || msg.contains("Unsupported"),
            "error should explain that WE Web restore is unsupported, got: {}",
            msg
        );

        // Old state should remain — restore doesn't clear on error.
        assert_eq!(
            s.current_read().unwrap().as_deref(),
            Some(project.to_string_lossy().as_ref())
        );
        assert_eq!(
            s.last_backend_read().unwrap().as_deref(),
            Some("unsupported")
        );
        // No new history entry added by the failed restore.
        assert_eq!(
            history_rows(&s),
            history_before,
            "failed restore should not add history"
        );
    }

    #[test]
    fn apply_wallpaper_rejects_unsupported_backend() {
        let (_tmp, s) = temp_storage();

        let img = _tmp.path().join("test.png");
        std::fs::write(&img, b"").unwrap();

        let err = apply_wallpaper(
            &s,
            &img.to_string_lossy().to_string(),
            Backend::Unsupported,
            None,
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("unsupported") || msg.contains("Unsupported"),
            "apply_wallpaper should reject Unsupported backend, got: {}",
            msg
        );
    }

    fn assert_command_arg(args: &[String], name: &str, value: &str) {
        let actual = args
            .windows(2)
            .find_map(|pair| (pair[0] == name).then_some(pair[1].as_str()));
        assert_eq!(actual, Some(value), "missing or wrong {name} in {args:?}");
    }

    #[test]
    fn stop_plan_awww_after_lwe_stops_lwe_after_success() {
        let plan = lifecycle::plan_apply_lifecycle(LWE_BACKEND_NAME, Backend::Awww);
        assert_eq!(plan.pre_stop, StopPlan::None);
        assert_eq!(plan.post_success_stop, StopPlan::LweOnly);
    }

    #[test]
    fn stop_plan_awww_after_awww_preserves_awww_daemon() {
        let plan = lifecycle::plan_apply_lifecycle("awww", Backend::Awww);
        assert_eq!(plan.pre_stop, StopPlan::None);
        assert_eq!(plan.post_success_stop, StopPlan::MpvpaperOnly);

        let plan_swww = lifecycle::plan_apply_lifecycle("swww", Backend::Awww);
        assert_eq!(plan_swww.pre_stop, StopPlan::None);
        assert_eq!(plan_swww.post_success_stop, StopPlan::MpvpaperOnly);
    }

    #[test]
    fn stop_plan_mpvpaper_handoff_depends_on_previous_backend() {
        let plan_awww = lifecycle::plan_apply_lifecycle("awww", Backend::Mpvpaper);
        assert_eq!(plan_awww.pre_stop, StopPlan::AwwwDaemonOnly);
        assert_eq!(plan_awww.post_success_stop, StopPlan::None);

        let plan_mpv = lifecycle::plan_apply_lifecycle("mpvpaper", Backend::Mpvpaper);
        assert_eq!(plan_mpv.pre_stop, StopPlan::MpvpaperOnly);
        assert_eq!(plan_mpv.post_success_stop, StopPlan::None);
    }

    #[test]
    fn failed_regular_apply_preserves_current_state() {
        let (tmp, s) = temp_storage();
        let current = tmp.path().join("old.jpg");
        std::fs::write(&current, b"old").unwrap();
        s.current_write(&current.to_string_lossy()).unwrap();
        s.last_backend_write("awww").unwrap();

        let missing = tmp.path().join("missing.jpg");
        let err = apply_wallpaper(&s, &missing.to_string_lossy(), Backend::Awww, None).unwrap_err();
        assert!(
            err.to_string().contains("missing") || err.to_string().contains("not"),
            "expected missing-file error"
        );
        assert_eq!(
            s.current_read().unwrap().as_deref(),
            Some(current.to_string_lossy().as_ref())
        );
        assert_eq!(s.last_backend_read().unwrap().as_deref(), Some("awww"));
    }

    #[test]
    fn post_apply_settle_same_backend_zero() {
        let plan = lifecycle::plan_apply_lifecycle("awww", Backend::Awww);
        assert_eq!(plan.post_success_settle_ms, 0);
        let plan = lifecycle::plan_apply_lifecycle("mpvpaper", Backend::Mpvpaper);
        assert_eq!(plan.post_success_settle_ms, 0);
    }

    #[test]
    fn post_apply_settle_cross_to_awww_positive() {
        let plan = lifecycle::plan_apply_lifecycle("mpvpaper", Backend::Awww);
        assert!(plan.post_success_settle_ms > 0);
        let plan = lifecycle::plan_apply_lifecycle(LWE_BACKEND_NAME, Backend::Awww);
        assert!(plan.post_success_settle_ms > 0);
    }

    #[test]
    fn post_apply_settle_empty_previous_zero() {
        let plan = lifecycle::plan_apply_lifecycle("", Backend::Awww);
        assert_eq!(plan.post_success_settle_ms, 0);
        let plan = lifecycle::plan_apply_lifecycle("", Backend::Mpvpaper);
        assert_eq!(plan.post_success_settle_ms, 0);
    }

    #[test]
    fn post_apply_settle_to_unsupported_zero() {
        let plan = lifecycle::plan_apply_lifecycle("awww", Backend::Unsupported);
        assert_eq!(plan.post_success_settle_ms, 0);
    }

    #[test]
    fn post_apply_settle_cross_to_lwe_non_zero_for_image_video_sources() {
        let plan = lifecycle::plan_apply_lifecycle("mpvpaper", Backend::LinuxWallpaperEngine);
        assert!(plan.post_success_settle_ms > 0);
    }

    #[test]
    fn post_apply_settle_swww_is_awww_legacy_zero() {
        let plan = lifecycle::plan_apply_lifecycle("swww", Backend::Awww);
        assert_eq!(plan.post_success_settle_ms, 0);
    }

    #[test]
    fn post_stop_unknown_previous_backend_returns_none_after_awww() {
        let plan = lifecycle::plan_apply_lifecycle("unknown-backend", Backend::Awww);
        assert_eq!(plan.post_success_stop, StopPlan::None);
    }

    #[test]
    fn post_stop_unknown_previous_backend_returns_none_after_mpvpaper() {
        let plan = lifecycle::plan_apply_lifecycle("unknown-backend", Backend::Mpvpaper);
        assert_eq!(plan.post_success_stop, StopPlan::None);
    }

    #[test]
    fn post_stop_unknown_previous_backend_returns_none_after_lwe() {
        let plan =
            lifecycle::plan_apply_lifecycle("unknown-backend", Backend::LinuxWallpaperEngine);
        assert_eq!(plan.post_success_stop, StopPlan::None);
    }

    #[cfg(unix)]
    #[test]
    fn apply_wallpaper_lwe_updates_state_without_appending_history() {
        use std::os::unix::fs::PermissionsExt;

        let (tmp, s) = temp_storage();
        s.last_backend_write("awww").unwrap();
        insert_history(&s, "legacy.jpg", "awww");
        let history_before = history_rows(&s);

        let bin = tmp.path().join("test-lwe-state-write");
        std::fs::write(&bin, "#!/bin/sh\nsleep 5\n").unwrap();
        let mut perms = std::fs::metadata(&bin).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&bin, perms).unwrap();
        s.config_set("linux_wallpaperengine_path", &bin.to_string_lossy())
            .unwrap();

        let scene = tmp.path().join("steamapps/workshop/content/431960/123456");
        std::fs::create_dir_all(&scene).unwrap();
        std::fs::write(scene.join("scene.pkg"), b"scene").unwrap();
        std::fs::write(
            scene.join("project.json"),
            r#"{"type":"scene","file":"scene.pkg","workshopid":"123456"}"#,
        )
        .unwrap();

        apply_wallpaper(
            &s,
            &scene.to_string_lossy(),
            Backend::LinuxWallpaperEngine,
            None,
        )
        .unwrap();

        assert_eq!(
            s.current_read().unwrap().as_deref(),
            Some(scene.to_string_lossy().as_ref())
        );
        assert_eq!(
            s.last_backend_read().unwrap().as_deref(),
            Some(LWE_BACKEND_NAME)
        );
        assert_eq!(history_rows(&s), history_before);

        let pid = s.config_get("linux_wallpaperengine_pid", "");
        if let Ok(pid) = pid.parse::<i32>() {
            let _ = Command::new("kill")
                .args(["-TERM", &format!("-{}", pid)])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }

    #[cfg(unix)]
    #[test]
    fn apply_wallpaper_lwe_failure_preserves_old_state() {
        use std::os::unix::fs::PermissionsExt;

        let (tmp, s) = temp_storage();
        let old = tmp.path().join("old.jpg");
        std::fs::write(&old, b"old").unwrap();
        s.current_write(&old.to_string_lossy()).unwrap();
        s.last_backend_write("awww").unwrap();
        insert_history(&s, &old.to_string_lossy(), "awww");
        let history_before = history_rows(&s);

        let bin = tmp.path().join("test-lwe-fail-state");
        std::fs::write(&bin, "#!/bin/sh\nexit 1\n").unwrap();
        let mut perms = std::fs::metadata(&bin).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&bin, perms).unwrap();
        s.config_set("linux_wallpaperengine_path", &bin.to_string_lossy())
            .unwrap();

        let scene = tmp.path().join("steamapps/workshop/content/431960/987654");
        std::fs::create_dir_all(&scene).unwrap();
        std::fs::write(scene.join("scene.pkg"), b"scene").unwrap();
        std::fs::write(
            scene.join("project.json"),
            r#"{"type":"scene","file":"scene.pkg","workshopid":"987654"}"#,
        )
        .unwrap();

        let err = apply_wallpaper(
            &s,
            &scene.to_string_lossy(),
            Backend::LinuxWallpaperEngine,
            None,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("linux-wallpaperengine") || err.to_string().contains("exited")
        );
        assert_eq!(
            s.current_read().unwrap().as_deref(),
            Some(old.to_string_lossy().as_ref())
        );
        assert_eq!(s.last_backend_read().unwrap().as_deref(), Some("awww"));
        assert_eq!(history_rows(&s), history_before);
    }

    #[cfg(unix)]
    #[test]
    fn apply_wallpaper_fallback_ok_target_lwe_fail_previous_mpvpaper_preserves_state() {
        use std::os::unix::fs::PermissionsExt;

        let (tmp, s) = temp_storage();
        let old = tmp.path().join("old.jpg");
        std::fs::write(&old, b"old").unwrap();
        s.current_write(&old.to_string_lossy()).unwrap();
        s.last_backend_write("mpvpaper").unwrap();
        insert_history(&s, &old.to_string_lossy(), "mpvpaper");
        let history_before = history_rows(&s);

        let bin = tmp.path().join("test-lwe-fallback-fail-state");
        std::fs::write(&bin, "#!/bin/sh\nexit 1\n").unwrap();
        let mut perms = std::fs::metadata(&bin).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&bin, perms).unwrap();
        s.config_set("linux_wallpaperengine_path", &bin.to_string_lossy())
            .unwrap();

        let scene = tmp.path().join("steamapps/workshop/content/431960/555555");
        std::fs::create_dir_all(&scene).unwrap();
        std::fs::write(scene.join("scene.pkg"), b"scene").unwrap();
        let preview = scene.join("preview.gif");
        std::fs::write(&preview, b"gif").unwrap();
        std::fs::write(
            scene.join("project.json"),
            r#"{"type":"scene","file":"scene.pkg","preview":"preview.gif","workshopid":"555555"}"#,
        )
        .unwrap();

        let err = apply_wallpaper(
            &s,
            &scene.to_string_lossy(),
            Backend::LinuxWallpaperEngine,
            Some(&preview.to_string_lossy().to_string()),
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("linux-wallpaperengine") || err.to_string().contains("exited")
        );
        assert_eq!(
            s.current_read().unwrap().as_deref(),
            Some(old.to_string_lossy().as_ref())
        );
        assert_eq!(s.last_backend_read().unwrap().as_deref(), Some("mpvpaper"));
        assert_eq!(history_rows(&s), history_before);
    }

    #[cfg(unix)]
    #[test]
    fn apply_wallpaper_fallback_ok_target_lwe_fail_restores_previous_awww_state() {
        use std::os::unix::fs::PermissionsExt;

        let (tmp, s) = temp_storage();
        let old = tmp.path().join("old.jpg");
        std::fs::write(&old, b"old").unwrap();
        s.current_write(&old.to_string_lossy()).unwrap();
        s.last_backend_write("awww").unwrap();
        insert_history(&s, &old.to_string_lossy(), "awww");
        let history_before = history_rows(&s);

        let bin = tmp.path().join("test-lwe-fallback-awww-previous");
        std::fs::write(&bin, "#!/bin/sh\nexit 1\n").unwrap();
        let mut perms = std::fs::metadata(&bin).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&bin, perms).unwrap();
        s.config_set("linux_wallpaperengine_path", &bin.to_string_lossy())
            .unwrap();

        let scene = tmp.path().join("steamapps/workshop/content/431960/666666");
        std::fs::create_dir_all(&scene).unwrap();
        std::fs::write(scene.join("scene.pkg"), b"scene").unwrap();
        let preview = scene.join("preview.gif");
        std::fs::write(&preview, b"gif").unwrap();
        std::fs::write(
            scene.join("project.json"),
            r#"{"type":"scene","file":"scene.pkg","preview":"preview.gif","workshopid":"666666"}"#,
        )
        .unwrap();

        let err = apply_wallpaper(
            &s,
            &scene.to_string_lossy(),
            Backend::LinuxWallpaperEngine,
            Some(&preview.to_string_lossy().to_string()),
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("linux-wallpaperengine") || err.to_string().contains("exited")
        );
        assert_eq!(
            s.current_read().unwrap().as_deref(),
            Some(old.to_string_lossy().as_ref())
        );
        assert_eq!(s.last_backend_read().unwrap().as_deref(), Some("awww"));
        assert_eq!(history_rows(&s), history_before);
    }

    #[cfg(unix)]
    #[test]
    fn apply_wallpaper_previous_awww_missing_path_lwe_fail_still_preserves_state() {
        use std::os::unix::fs::PermissionsExt;

        let (tmp, s) = temp_storage();
        let missing = tmp.path().join("missing.jpg");
        s.current_write(&missing.to_string_lossy()).unwrap();
        s.last_backend_write("awww").unwrap();
        insert_history(&s, &missing.to_string_lossy(), "awww");
        let history_before = history_rows(&s);

        let bin = tmp.path().join("test-lwe-missing-awww-state");
        std::fs::write(&bin, "#!/bin/sh\nexit 1\n").unwrap();
        let mut perms = std::fs::metadata(&bin).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&bin, perms).unwrap();
        s.config_set("linux_wallpaperengine_path", &bin.to_string_lossy())
            .unwrap();

        let scene = tmp.path().join("steamapps/workshop/content/431960/777777");
        std::fs::create_dir_all(&scene).unwrap();
        std::fs::write(scene.join("scene.pkg"), b"scene").unwrap();
        let preview = scene.join("preview.gif");
        std::fs::write(&preview, b"gif").unwrap();
        std::fs::write(
            scene.join("project.json"),
            r#"{"type":"scene","file":"scene.pkg","preview":"preview.gif","workshopid":"777777"}"#,
        )
        .unwrap();

        let err = apply_wallpaper(
            &s,
            &scene.to_string_lossy(),
            Backend::LinuxWallpaperEngine,
            Some(&preview.to_string_lossy().to_string()),
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("linux-wallpaperengine") || err.to_string().contains("exited")
        );
        assert_eq!(
            s.current_read().unwrap().as_deref(),
            Some(missing.to_string_lossy().as_ref())
        );
        assert_eq!(s.last_backend_read().unwrap().as_deref(), Some("awww"));
        assert_eq!(history_rows(&s), history_before);
    }

    #[test]
    fn awww_instant_command_uses_minimal_transition_duration() {
        let cmd = build_awww_instant_command("/tmp/test.jpg", "crop", "60");
        let args: Vec<String> = cmd
            .get_args()
            .map(|s| s.to_string_lossy().to_string())
            .collect();
        assert!(args.contains(&"--transition-type".to_string()));
        assert!(args.contains(&"simple".to_string()));
        assert!(args.contains(&"--transition-duration".to_string()));
        assert!(args.contains(&"0".to_string()));
        assert!(!args.contains(&"fade".to_string()));
        assert!(!args.contains(&"none".to_string()));
    }

    #[test]
    fn normal_awww_command_keeps_user_transition() {
        let cmd = build_awww_img_command("/tmp/test.jpg", "crop", "fade", "2.5", "60");
        let args: Vec<String> = cmd
            .get_args()
            .map(|s| s.to_string_lossy().to_string())
            .collect();
        assert!(args.contains(&"--transition-type".to_string()));
        assert!(args.contains(&"fade".to_string()));
        assert!(args.contains(&"--transition-duration".to_string()));
        assert!(args.contains(&"2.5".to_string()));
        assert!(!args.contains(&"simple".to_string()));
        assert!(!args.contains(&"0".to_string()));
    }

    #[test]
    fn instant_command_keeps_resize_and_fps() {
        let cmd = build_awww_instant_command("/tmp/test.jpg", "fit", "30");
        let args: Vec<String> = cmd
            .get_args()
            .map(|s| s.to_string_lossy().to_string())
            .collect();
        assert!(args.contains(&"--resize".to_string()));
        assert!(args.contains(&"fit".to_string()));
        assert!(args.contains(&"--transition-fps".to_string()));
        assert!(args.contains(&"30".to_string()));
        assert!(args.contains(&"--filter".to_string()));
        assert!(args.contains(&"Lanczos3".to_string()));
    }

    #[test]
    fn cross_backend_to_awww_has_instant_fallback_stage() {
        use visual_handoff::FallbackStage;
        let plan = visual_handoff::plan_visual_handoff(
            lifecycle::RunningBackend::Mpvpaper,
            Backend::Awww,
            Some("/tmp/img.jpg"),
        );
        assert_eq!(plan.fallback_stage, FallbackStage::TargetImageInstant);
    }

    #[test]
    fn cross_backend_image_fallback_emits_awww_readiness_stages() {
        let (tmp, s) = temp_storage();
        let img = tmp.path().join("fallback.jpg");
        std::fs::write(&img, b"jpg").unwrap();
        s.last_backend_write("mpvpaper").unwrap();

        let mut rt = FakeRuntime {
            command_output_success: true,
            command_status_success: true,
            ..Default::default()
        };
        let mut reporter = apply_stage::test_support::CapturingReporter::new();
        apply_wallpaper_with_runtime(
            &s,
            &img.to_string_lossy(),
            Backend::Awww,
            Some(&img.to_string_lossy()),
            &mut rt,
            &mut reporter,
            Some("req-fallback"),
        )
        .unwrap();

        let stages = reporter.stages();
        assert!(
            stages
                .iter()
                .any(|stage| *stage == apply_stage::ApplyStage::EnsureAwwwDaemon),
            "fallback instant path must emit EnsureAwwwDaemon"
        );
        assert!(
            stages
                .iter()
                .any(|stage| *stage == apply_stage::ApplyStage::AwwwSocketReady),
            "fallback instant path must emit AwwwSocketReady"
        );
    }

    #[test]
    fn cross_backend_image_uses_instant_target_without_second_awww_apply() {
        let (tmp, s) = temp_storage();
        let img = tmp.path().join("target.jpg");
        std::fs::write(&img, b"jpg").unwrap();
        s.last_backend_write("mpvpaper").unwrap();

        let mut rt = FakeRuntime {
            command_output_success: true,
            command_status_success: true,
            ..Default::default()
        };
        apply_with_fake_runtime(
            &s,
            &img.to_string_lossy(),
            Backend::Awww,
            Some(&img.to_string_lossy()),
            &mut rt,
        )
        .unwrap();

        assert_eq!(
            rt.command_output_count, 1,
            "instant target image should not be followed by a second awww apply"
        );
        assert_eq!(
            rt.clear_awww_state_hint_count, 0,
            "clearing awww after instant target causes image -> black -> image flicker"
        );
        assert_eq!(rt.stop_mpvpaper_count, 1);
    }

    #[test]
    fn same_backend_awww_to_awww_does_not_stop_awww_daemon() {
        let (tmp, s) = temp_storage();
        let img = tmp.path().join("target.jpg");
        std::fs::write(&img, b"jpg").unwrap();
        s.last_backend_write("awww").unwrap();

        let mut rt = FakeRuntime {
            command_output_success: true,
            command_status_success: true,
            ..Default::default()
        };
        apply_with_fake_runtime(
            &s,
            &img.to_string_lossy(),
            Backend::Awww,
            Some(&img.to_string_lossy()),
            &mut rt,
        )
        .unwrap();

        assert_eq!(
            rt.stop_awww_count, 0,
            "image->image awww fast path must not stop the awww daemon"
        );
        // awww->awww still cleans up a residual mpvpaper via post_success_stop.
        assert_eq!(rt.stop_mpvpaper_count, 1);
        assert_eq!(rt.stop_lwe_count, 0);
    }

    #[test]
    fn same_backend_mpvpaper_to_mpvpaper_only_stops_mpvpaper() {
        let (tmp, s) = temp_storage();
        let video = tmp.path().join("target.mp4");
        std::fs::write(&video, b"mp4").unwrap();
        s.last_backend_write("mpvpaper").unwrap();

        let mut rt = FakeRuntime {
            command_output_success: true,
            command_status_success: true,
            ..Default::default()
        };
        apply_with_fake_runtime(
            &s,
            &video.to_string_lossy(),
            Backend::Mpvpaper,
            None,
            &mut rt,
        )
        .unwrap();

        assert_eq!(
            rt.stop_mpvpaper_count, 1,
            "video->video stops old mpvpaper before starting the new one"
        );
        assert_eq!(
            rt.stop_awww_count, 0,
            "video->video fast path must not stop unrelated awww daemon"
        );
        assert_eq!(rt.stop_lwe_count, 0);
    }

    #[test]
    fn awww_from_none_applies_without_clear_to_avoid_old_image_black_flash() {
        let (tmp, s) = temp_storage();
        let img = tmp.path().join("target.jpg");
        std::fs::write(&img, b"jpg").unwrap();

        let mut rt = FakeRuntime {
            command_output_success: true,
            command_status_success: true,
            ..Default::default()
        };
        apply_with_fake_runtime(
            &s,
            &img.to_string_lossy(),
            Backend::Awww,
            Some(&img.to_string_lossy()),
            &mut rt,
        )
        .unwrap();

        assert_eq!(
            rt.clear_awww_state_hint_count, 0,
            "after Stop Backends, awww daemon startup may restore its old layer; clearing after startup causes old image -> black -> new image"
        );
        assert_eq!(rt.command_output_count, 1);
        assert_command_arg(&rt.command_output_args[0], "--transition-type", "simple");
        assert_command_arg(&rt.command_output_args[0], "--transition-duration", "0");
    }

    #[test]
    fn ensure_awww_daemon_starts_with_no_cache_to_avoid_restoring_old_wallpaper() {
        let cmd = runtime::build_awww_daemon_command();
        let args: Vec<String> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();
        assert!(
            args.iter().any(|arg| arg == "--no-cache"),
            "daemon startup args must include --no-cache, got {args:?}"
        );
    }

    // -----------------------------------------------------------------
    // FakeRuntime for testing the backend runtime seam
    // -----------------------------------------------------------------

    #[derive(Default)]
    struct FakeRuntime {
        stop_awww_count: usize,
        stop_mpvpaper_count: usize,
        stop_lwe_count: usize,
        command_output_count: usize,
        command_status_count: usize,
        clear_awww_state_hint_count: usize,
        command_output_success: bool,
        command_status_success: bool,
        command_output_programs: Vec<String>,
        command_status_programs: Vec<String>,
        command_output_args: Vec<Vec<String>>,
        command_status_args: Vec<Vec<String>>,
        awww_readiness_sequence: std::cell::RefCell<Vec<crate::runtime::AwwwReadiness>>,
        running_mpvpaper_pids: Vec<u32>,
        mpvpaper_pids_error: Option<String>,
        mpvpaper_pids_count: usize,
        mpvpaper_readiness_error: Option<String>,
        mpvpaper_wait_count: usize,
        mpvpaper_wait_previous_pids: Vec<Vec<u32>>,
        mpvpaper_ready_pid: Option<u32>,
        dead_mpvpaper_pids: Vec<u32>,
        mpvpaper_pid_running_error: Option<String>,
        mpvpaper_pid_running_checks: Vec<u32>,
    }

    impl crate::runtime::BackendRuntime for FakeRuntime {
        fn command_output(
            &mut self,
            command: &mut std::process::Command,
        ) -> Result<std::process::Output, WcError> {
            self.command_output_count += 1;
            self.command_output_programs
                .push(command.get_program().to_string_lossy().to_string());
            self.command_output_args.push(
                command
                    .get_args()
                    .map(|arg| arg.to_string_lossy().to_string())
                    .collect(),
            );
            let program = if self.command_output_success {
                "true"
            } else {
                "false"
            };
            std::process::Command::new(program)
                .output()
                .map_err(|e| WcError::Other(format!("fake command failed: {}", e)))
        }

        fn command_status(
            &mut self,
            command: &mut std::process::Command,
        ) -> Result<std::process::ExitStatus, WcError> {
            self.command_status_count += 1;
            self.command_status_programs
                .push(command.get_program().to_string_lossy().to_string());
            self.command_status_args.push(
                command
                    .get_args()
                    .map(|arg| arg.to_string_lossy().to_string())
                    .collect(),
            );
            let program = if self.command_status_success {
                "true"
            } else {
                "false"
            };
            std::process::Command::new(program)
                .status()
                .map_err(|e| WcError::Other(format!("fake command failed: {}", e)))
        }

        fn stop_awww(&mut self) {
            self.stop_awww_count += 1;
        }

        fn stop_mpvpaper(&mut self) {
            self.stop_mpvpaper_count += 1;
            self.running_mpvpaper_pids.clear();
        }

        fn stop_lwe(&mut self, _s: Option<&wc_storage::StorageApi>) {
            self.stop_lwe_count += 1;
        }

        fn apply_lwe_to_outputs(
            &mut self,
            _s: &wc_storage::StorageApi,
            _project: &crate::linux_wallpaperengine::LinuxWallpaperEngineProject,
            _outputs: &[String],
        ) -> Result<(), WcError> {
            Ok(())
        }

        fn awww_socket_ready(&mut self) -> crate::runtime::AwwwReadiness {
            let mut seq = self.awww_readiness_sequence.borrow_mut();
            if seq.len() > 1 {
                seq.remove(0)
            } else if !seq.is_empty() {
                seq[0].clone()
            } else {
                crate::runtime::AwwwReadiness::Ready
            }
        }

        fn mpvpaper_pids(&mut self) -> Result<Vec<u32>, WcError> {
            self.mpvpaper_pids_count += 1;
            match &self.mpvpaper_pids_error {
                Some(message) => Err(WcError::Other(message.clone())),
                None => Ok(self.running_mpvpaper_pids.clone()),
            }
        }

        fn wait_for_mpvpaper_ready(&mut self, previous_pids: &[u32]) -> Result<u32, WcError> {
            self.mpvpaper_wait_count += 1;
            self.mpvpaper_wait_previous_pids
                .push(previous_pids.to_vec());
            match &self.mpvpaper_readiness_error {
                Some(message) => Err(WcError::Other(message.clone())),
                None => Ok(self.mpvpaper_ready_pid.unwrap_or(1)),
            }
        }

        fn mpvpaper_pid_running(&mut self, pid: u32) -> Result<bool, WcError> {
            self.mpvpaper_pid_running_checks.push(pid);
            match &self.mpvpaper_pid_running_error {
                Some(message) => Err(WcError::Other(message.clone())),
                None => Ok(!self.dead_mpvpaper_pids.contains(&pid)),
            }
        }

        fn ensure_awww_daemon_running(&mut self) -> Result<(), WcError> {
            if matches!(
                self.awww_socket_ready(),
                crate::runtime::AwwwReadiness::Ready
            ) {
                return Ok(());
            }
            let user = crate::whoami();
            if !crate::awww::is_awww_daemon_running(&user) {
                let mut cmd = crate::runtime::build_awww_daemon_command();
                let _ = self.command_status(&mut cmd);
            }
            crate::runtime::wait_for_awww_socket_ready(self, &user)
        }

        fn clear_awww_state_hint(&mut self) {
            self.clear_awww_state_hint_count += 1;
        }
    }

    #[test]
    fn mpvpaper_baseline_pid_probe_failure_happens_before_pre_stop() {
        let (tmp, s) = temp_storage();
        let old = tmp.path().join("old.png");
        let next = tmp.path().join("private-next.mp4");
        std::fs::write(&old, b"old").unwrap();
        std::fs::write(&next, b"next").unwrap();
        s.current_write(&old.to_string_lossy()).unwrap();
        s.last_backend_write("awww").unwrap();
        insert_history(&s, &old.to_string_lossy(), "awww");
        let history_before = history_rows(&s);

        let mut rt = FakeRuntime {
            command_status_success: true,
            mpvpaper_pids_error: Some("mpvpaper baseline PID probe failed".into()),
            mpvpaper_ready_pid: Some(808),
            ..Default::default()
        };
        let error = apply_with_fake_runtime(
            &s,
            &next.to_string_lossy(),
            Backend::Mpvpaper,
            None,
            &mut rt,
        )
        .unwrap_err();

        assert!(error.to_string().contains("baseline PID probe failed"));
        assert!(!error.to_string().contains(next.to_string_lossy().as_ref()));
        assert_eq!(rt.mpvpaper_pids_count, 1);
        assert_eq!(rt.stop_awww_count, 0);
        assert_eq!(rt.stop_mpvpaper_count, 0);
        assert_eq!(rt.stop_lwe_count, 0);
        assert_eq!(rt.command_status_count, 0);
        assert_eq!(rt.mpvpaper_wait_count, 0);
        assert_eq!(
            s.current_read().unwrap().as_deref(),
            Some(old.to_string_lossy().as_ref())
        );
        assert_eq!(s.last_backend_read().unwrap().as_deref(), Some("awww"));
        assert_eq!(history_rows(&s), history_before);
    }

    #[test]
    fn mpvpaper_launcher_failure_preserves_persisted_state() {
        let (tmp, s) = temp_storage();
        let old = tmp.path().join("old.mp4");
        let next = tmp.path().join("private-next.mp4");
        std::fs::write(&old, b"old").unwrap();
        std::fs::write(&next, b"next").unwrap();
        s.current_write(&old.to_string_lossy()).unwrap();
        s.last_backend_write("mpvpaper").unwrap();
        insert_history(&s, &old.to_string_lossy(), "mpvpaper");
        let history_before = history_rows(&s);

        let mut rt = FakeRuntime {
            command_status_success: false,
            running_mpvpaper_pids: vec![101],
            ..Default::default()
        };
        let err = apply_with_fake_runtime(
            &s,
            &next.to_string_lossy(),
            Backend::Mpvpaper,
            None,
            &mut rt,
        )
        .unwrap_err();

        assert!(err.to_string().contains("mpvpaper"));
        assert!(!err.to_string().contains(next.to_string_lossy().as_ref()));
        assert_eq!(
            s.current_read().unwrap().as_deref(),
            Some(old.to_string_lossy().as_ref())
        );
        assert_eq!(s.last_backend_read().unwrap().as_deref(), Some("mpvpaper"));
        assert_eq!(history_rows(&s), history_before);
        assert_eq!(rt.mpvpaper_wait_count, 0);
        assert_eq!(rt.stop_mpvpaper_count, 1);
    }

    #[test]
    fn mpvpaper_readiness_failure_preserves_persisted_state() {
        let (tmp, s) = temp_storage();
        let old = tmp.path().join("old.mp4");
        let next = tmp.path().join("private-next.mp4");
        std::fs::write(&old, b"old").unwrap();
        std::fs::write(&next, b"next").unwrap();
        s.current_write(&old.to_string_lossy()).unwrap();
        s.last_backend_write("mpvpaper").unwrap();
        insert_history(&s, &old.to_string_lossy(), "mpvpaper");
        let history_before = history_rows(&s);

        let mut rt = FakeRuntime {
            command_status_success: true,
            running_mpvpaper_pids: vec![202],
            mpvpaper_readiness_error: Some("mpvpaper did not become ready".into()),
            ..Default::default()
        };
        let err = apply_with_fake_runtime(
            &s,
            &next.to_string_lossy(),
            Backend::Mpvpaper,
            None,
            &mut rt,
        )
        .unwrap_err();

        assert!(err.to_string().contains("did not become ready"));
        assert!(!err.to_string().contains(next.to_string_lossy().as_ref()));
        assert_eq!(
            s.current_read().unwrap().as_deref(),
            Some(old.to_string_lossy().as_ref())
        );
        assert_eq!(s.last_backend_read().unwrap().as_deref(), Some("mpvpaper"));
        assert_eq!(history_rows(&s), history_before);
        assert_eq!(rt.mpvpaper_wait_count, 1);
        assert_eq!(rt.mpvpaper_wait_previous_pids, vec![vec![202]]);
        assert_eq!(rt.stop_mpvpaper_count, 2);
    }

    #[test]
    fn mpvpaper_readiness_success_updates_state_without_appending_history() {
        let (tmp, s) = temp_storage();
        let old = tmp.path().join("old.mp4");
        let next = tmp.path().join("next.mp4");
        std::fs::write(&old, b"old").unwrap();
        std::fs::write(&next, b"next").unwrap();
        s.current_write(&old.to_string_lossy()).unwrap();
        s.last_backend_write("mpvpaper").unwrap();
        insert_history(&s, &old.to_string_lossy(), "mpvpaper");
        let history_before = history_rows(&s);

        let mut rt = FakeRuntime {
            command_status_success: true,
            running_mpvpaper_pids: vec![303],
            mpvpaper_ready_pid: Some(404),
            ..Default::default()
        };
        apply_with_fake_runtime(
            &s,
            &next.to_string_lossy(),
            Backend::Mpvpaper,
            None,
            &mut rt,
        )
        .unwrap();

        assert_eq!(
            s.current_read().unwrap().as_deref(),
            Some(next.to_string_lossy().as_ref())
        );
        assert_eq!(s.last_backend_read().unwrap().as_deref(), Some("mpvpaper"));
        assert_eq!(history_rows(&s), history_before);
        assert_eq!(rt.mpvpaper_wait_count, 1);
        assert_eq!(rt.mpvpaper_wait_previous_pids, vec![vec![303]]);
        assert_eq!(rt.mpvpaper_pid_running_checks, vec![404]);
    }

    #[test]
    fn mpvpaper_exit_after_readiness_before_commit_preserves_persisted_state() {
        let (tmp, s) = temp_storage();
        let old = tmp.path().join("old.mp4");
        let next = tmp.path().join("private-next.mp4");
        std::fs::write(&old, b"old").unwrap();
        std::fs::write(&next, b"next").unwrap();
        s.current_write(&old.to_string_lossy()).unwrap();
        s.last_backend_write("mpvpaper").unwrap();
        insert_history(&s, &old.to_string_lossy(), "mpvpaper");
        let history_before = history_rows(&s);

        let mut rt = FakeRuntime {
            command_status_success: true,
            running_mpvpaper_pids: vec![404],
            mpvpaper_ready_pid: Some(505),
            dead_mpvpaper_pids: vec![505],
            ..Default::default()
        };
        let err = apply_with_fake_runtime(
            &s,
            &next.to_string_lossy(),
            Backend::Mpvpaper,
            None,
            &mut rt,
        )
        .unwrap_err();

        assert!(err.to_string().contains("mpvpaper"));
        assert!(!err.to_string().contains(next.to_string_lossy().as_ref()));
        assert_eq!(
            s.current_read().unwrap().as_deref(),
            Some(old.to_string_lossy().as_ref())
        );
        assert_eq!(s.last_backend_read().unwrap().as_deref(), Some("mpvpaper"));
        assert_eq!(history_rows(&s), history_before);
        assert_eq!(rt.mpvpaper_pid_running_checks, vec![505]);
        assert_eq!(rt.stop_mpvpaper_count, 2);
    }

    #[test]
    fn mpvpaper_post_settle_probe_failure_preserves_persisted_state() {
        let (tmp, s) = temp_storage();
        let old = tmp.path().join("old.mp4");
        let next = tmp.path().join("private-next.mp4");
        std::fs::write(&old, b"old").unwrap();
        std::fs::write(&next, b"next").unwrap();
        s.current_write(&old.to_string_lossy()).unwrap();
        s.last_backend_write("mpvpaper").unwrap();
        insert_history(&s, &old.to_string_lossy(), "mpvpaper");
        let history_before = history_rows(&s);

        let mut rt = FakeRuntime {
            command_status_success: true,
            running_mpvpaper_pids: vec![606],
            mpvpaper_ready_pid: Some(707),
            mpvpaper_pid_running_error: Some("mpvpaper PID probe failed".into()),
            ..Default::default()
        };
        let err = apply_with_fake_runtime(
            &s,
            &next.to_string_lossy(),
            Backend::Mpvpaper,
            None,
            &mut rt,
        )
        .unwrap_err();

        assert!(err.to_string().contains("probe failed"));
        assert!(!err.to_string().contains(next.to_string_lossy().as_ref()));
        assert_eq!(
            s.current_read().unwrap().as_deref(),
            Some(old.to_string_lossy().as_ref())
        );
        assert_eq!(s.last_backend_read().unwrap().as_deref(), Some("mpvpaper"));
        assert_eq!(history_rows(&s), history_before);
        assert_eq!(rt.mpvpaper_pid_running_checks, vec![707]);
        assert_eq!(rt.stop_mpvpaper_count, 2);
    }

    #[test]
    fn ensure_daemon_ok_fast_path_when_socket_ready() {
        let mut rt = FakeRuntime {
            command_status_success: true,
            awww_readiness_sequence: std::cell::RefCell::new(vec![
                crate::runtime::AwwwReadiness::Ready,
            ]),
            ..Default::default()
        };
        assert!(rt.ensure_awww_daemon_running().is_ok());
        assert_eq!(
            rt.command_status_count, 0,
            "fast path must not spawn daemon"
        );
    }

    #[test]
    fn ensure_daemon_err_when_socket_never_ready_and_process_absent() {
        let mut rt = FakeRuntime {
            command_status_success: true,
            awww_readiness_sequence: std::cell::RefCell::new(vec![
                crate::runtime::AwwwReadiness::SocketMissing;
                40
            ]),
            ..Default::default()
        };
        let err = rt.ensure_awww_daemon_running().unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("failed to start") || msg.contains("socket is not ready"),
            "should give a clear readiness error, got: {msg}"
        );
    }

    #[test]
    fn wait_for_socket_ok_when_ready_after_polls() {
        let mut rt = FakeRuntime {
            command_status_success: true,
            awww_readiness_sequence: std::cell::RefCell::new({
                let mut v = vec![
                    crate::runtime::AwwwReadiness::SocketMissing,
                    crate::runtime::AwwwReadiness::SocketMissing,
                    crate::runtime::AwwwReadiness::SocketMissing,
                ];
                v.push(crate::runtime::AwwwReadiness::Ready);
                v
            }),
            ..Default::default()
        };
        let result = crate::runtime::wait_for_awww_socket_ready(&mut rt, "testuser");
        assert!(result.is_ok(), "should become ready after polls");
    }

    #[test]
    fn ensure_daemon_spawns_when_socket_missing_and_no_process() {
        let mut rt = FakeRuntime {
            command_status_success: true,
            awww_readiness_sequence: std::cell::RefCell::new({
                let mut v = vec![crate::runtime::AwwwReadiness::SocketMissing];
                v.push(crate::runtime::AwwwReadiness::Ready);
                v
            }),
            ..Default::default()
        };
        assert!(rt.ensure_awww_daemon_running().is_ok());
        assert_eq!(
            rt.command_status_count, 1,
            "should spawn daemon when socket missing and no process"
        );
    }

    #[test]
    fn execute_stop_plan_all_calls_all_stops() {
        let (_tmp, s) = temp_storage();
        let mut rt = FakeRuntime::default();
        execute_stop_plan_with_runtime(&s, StopPlan::All, &mut rt).unwrap();
        assert_eq!(rt.stop_awww_count, 1);
        assert_eq!(rt.stop_mpvpaper_count, 1);
        assert_eq!(rt.stop_lwe_count, 1);
    }

    #[test]
    fn execute_stop_plan_awww_daemon_only() {
        let (_tmp, s) = temp_storage();
        let mut rt = FakeRuntime::default();
        execute_stop_plan_with_runtime(&s, StopPlan::AwwwDaemonOnly, &mut rt).unwrap();
        assert_eq!(rt.stop_awww_count, 1);
        assert_eq!(rt.stop_mpvpaper_count, 0);
        assert_eq!(rt.stop_lwe_count, 0);
    }

    #[test]
    fn execute_stop_plan_mpvpaper_only() {
        let (_tmp, s) = temp_storage();
        let mut rt = FakeRuntime::default();
        execute_stop_plan_with_runtime(&s, StopPlan::MpvpaperOnly, &mut rt).unwrap();
        assert_eq!(rt.stop_awww_count, 0);
        assert_eq!(rt.stop_mpvpaper_count, 1);
        assert_eq!(rt.stop_lwe_count, 0);
    }

    #[test]
    fn execute_stop_plan_lwe_only() {
        let (_tmp, s) = temp_storage();
        let mut rt = FakeRuntime::default();
        execute_stop_plan_with_runtime(&s, StopPlan::LweOnly, &mut rt).unwrap();
        assert_eq!(rt.stop_awww_count, 0);
        assert_eq!(rt.stop_mpvpaper_count, 0);
        assert_eq!(rt.stop_lwe_count, 1);
    }

    #[test]
    fn execute_stop_plan_non_lwe() {
        let (_tmp, s) = temp_storage();
        let mut rt = FakeRuntime::default();
        execute_stop_plan_with_runtime(&s, StopPlan::NonLwe, &mut rt).unwrap();
        assert_eq!(rt.stop_awww_count, 1);
        assert_eq!(rt.stop_mpvpaper_count, 1);
        assert_eq!(rt.stop_lwe_count, 0);
    }

    #[test]
    fn execute_stop_plan_none_is_noop() {
        let (_tmp, s) = temp_storage();
        let mut rt = FakeRuntime::default();
        execute_stop_plan_with_runtime(&s, StopPlan::None, &mut rt).unwrap();
        assert_eq!(rt.stop_awww_count, 0);
        assert_eq!(rt.stop_mpvpaper_count, 0);
        assert_eq!(rt.stop_lwe_count, 0);
    }

    #[test]
    fn apply_with_runtime_pre_stop_all_counts_calls() {
        // When previous backend is None and target is Mpvpaper,
        // the pre_stop plan is All, which calls all stop methods
        // via the injected runtime. The apply path also uses
        // runtime.command_status for mpvpaper (setsid).
        let (tmp, s) = temp_storage();
        s.last_backend_write("").unwrap();

        let img = tmp.path().join("test.png");
        std::fs::write(&img, b"").unwrap();

        let mut rt = FakeRuntime {
            command_output_success: true,
            command_status_success: true,
            ..Default::default()
        };
        let _result =
            apply_with_fake_runtime(&s, &img.to_string_lossy(), Backend::Mpvpaper, None, &mut rt);

        assert_eq!(rt.stop_awww_count, 1);
        assert_eq!(rt.stop_mpvpaper_count, 1);
        assert_eq!(rt.stop_lwe_count, 1);
        assert_eq!(
            rt.command_status_count, 1,
            "mpvpaper target must use runtime.command_status"
        );
    }

    #[test]
    fn apply_with_runtime_awww_target_uses_command_output() {
        // Awww TargetImageInstant fallback runs apply_awww_instant_with_runtime
        // which uses runtime.command_output. FakeRuntime.ensure_awww_daemon_running
        // returns Ok(()) so the path doesn't depend on a real compositor daemon.
        let (tmp, s) = temp_storage();
        s.last_backend_write("").unwrap();

        let img = tmp.path().join("test.png");
        std::fs::write(&img, b"").unwrap();

        let mut rt = FakeRuntime {
            command_output_success: true,
            command_status_success: true,
            ..Default::default()
        };
        let _result = apply_with_fake_runtime(
            &s,
            &img.to_string_lossy(),
            Backend::Awww,
            Some(&img.to_string_lossy()),
            &mut rt,
        );

        assert!(
            rt.command_output_count >= 1,
            "awww instant fallback must use runtime.command_output"
        );
    }

    #[test]
    fn rollback_missing_previous_awww_path_calls_stop_awww() {
        let (tmp, s) = temp_storage();
        let missing = tmp.path().join("missing.jpg");
        s.current_write(&missing.to_string_lossy()).unwrap();

        let mut rt = FakeRuntime::default();
        let msg = rollback_visual_fallback_after_target_failure_with_runtime(
            &s,
            lifecycle::RunningBackend::Awww,
            true,
            &mut rt,
        );

        assert!(msg.is_some());
        assert!(msg.unwrap().contains("not found"));
        assert_eq!(rt.stop_awww_count, 1);
    }

    #[test]
    fn rollback_no_previous_awww_state_calls_stop_awww() {
        let (_tmp, s) = temp_storage();
        // No current written — s.current_read() returns None.

        let mut rt = FakeRuntime::default();
        let msg = rollback_visual_fallback_after_target_failure_with_runtime(
            &s,
            lifecycle::RunningBackend::Awww,
            true,
            &mut rt,
        );

        assert!(msg.is_some());
        assert!(msg.unwrap().contains("no previous awww state"));
        assert_eq!(rt.stop_awww_count, 1);
    }

    #[test]
    fn rollback_non_awww_previous_calls_stop_awww() {
        let (_tmp, s) = temp_storage();

        let mut rt = FakeRuntime::default();
        let msg = rollback_visual_fallback_after_target_failure_with_runtime(
            &s,
            lifecycle::RunningBackend::Mpvpaper,
            true,
            &mut rt,
        );

        assert!(msg.is_some());
        assert!(msg.unwrap().contains("non-awww"));
        assert_eq!(rt.stop_awww_count, 1);
    }

    #[test]
    fn rollback_fallback_not_ok_is_noop() {
        let (_tmp, s) = temp_storage();

        let mut rt = FakeRuntime::default();
        let msg = rollback_visual_fallback_after_target_failure_with_runtime(
            &s,
            lifecycle::RunningBackend::Awww,
            false,
            &mut rt,
        );

        assert!(msg.is_none());
        assert_eq!(rt.stop_awww_count, 0);
    }

    #[test]
    fn restore_clean_stops_all_before_reapplying_current_image() {
        let (tmp, s) = temp_storage();
        let img = tmp.path().join("current.png");
        std::fs::write(&img, b"png").unwrap();
        s.current_write(&img.to_string_lossy()).unwrap();
        s.last_backend_write("mpvpaper").unwrap();

        let mut rt = FakeRuntime {
            command_output_success: true,
            command_status_success: true,
            ..Default::default()
        };

        crate::restore::restore_clean_with_runtime(&s, &mut rt).unwrap();

        assert_eq!(rt.stop_awww_count, 1, "clean restore must stop awww");
        assert!(
            rt.stop_mpvpaper_count >= 1,
            "clean restore must stop mpvpaper"
        );
        assert_eq!(rt.stop_lwe_count, 1, "clean restore must stop LWE");
        assert!(
            rt.command_output_programs.iter().any(|p| p == "awww"),
            "restore should apply image through awww"
        );
        assert_eq!(
            s.current_read().unwrap().as_deref(),
            Some(img.to_string_lossy().as_ref())
        );
        assert_eq!(s.last_backend_read().unwrap().as_deref(), Some("awww"));
    }

    #[test]
    fn restore_clean_failure_preserves_previous_state() {
        let (tmp, s) = temp_storage();
        let img = tmp.path().join("current.png");
        std::fs::write(&img, b"png").unwrap();
        s.current_write(&img.to_string_lossy()).unwrap();
        s.last_backend_write("mpvpaper").unwrap();
        insert_history(&s, &img.to_string_lossy(), "mpvpaper");
        let history_before = history_rows(&s);

        let mut rt = FakeRuntime {
            command_output_success: false,
            command_status_success: true,
            ..Default::default()
        };

        let err = crate::restore::restore_clean_with_runtime(&s, &mut rt).unwrap_err();
        assert!(
            err.to_string().contains("awww") || err.to_string().contains("false"),
            "error should mention awww failure: {}",
            err
        );
        assert_eq!(
            s.current_read().unwrap().as_deref(),
            Some(img.to_string_lossy().as_ref())
        );
        assert_eq!(s.last_backend_read().unwrap().as_deref(), Some("mpvpaper"));
        assert_eq!(history_rows(&s), history_before);
    }

    #[test]
    fn video_after_image_stops_awww_before_starting_mpvpaper_without_fallback() {
        let (tmp, s) = temp_storage();
        let video = tmp.path().join("next.mp4");
        std::fs::write(&video, b"mp4").unwrap();
        s.last_backend_write("awww").unwrap();

        let mut rt = FakeRuntime {
            command_output_success: true,
            command_status_success: true,
            ..Default::default()
        };

        apply_with_fake_runtime(
            &s,
            &video.to_string_lossy(),
            Backend::Mpvpaper,
            Some("/tmp/should-not-be-used.gif"),
            &mut rt,
        )
        .unwrap();

        assert_eq!(rt.stop_awww_count, 1);
        assert_eq!(rt.stop_mpvpaper_count, 0);
        assert_eq!(
            rt.command_output_count, 0,
            "video target must not use awww fallback command_output"
        );
        assert_eq!(s.last_backend_read().unwrap().as_deref(), Some("mpvpaper"));
    }

    #[cfg(unix)]
    #[test]
    fn scene_after_video_stops_mpvpaper_before_lwe_without_preview_fallback() {
        use std::os::unix::fs::PermissionsExt;

        let (tmp, s) = temp_storage();
        let scene = tmp
            .path()
            .join("steamapps/workshop/content/431960/2651567796");
        std::fs::create_dir_all(&scene).unwrap();
        std::fs::write(scene.join("scene.pkg"), b"scene").unwrap();
        std::fs::write(
            scene.join("project.json"),
            r#"{"type":"scene","file":"scene.pkg","workshopid":"2651567796"}"#,
        )
        .unwrap();
        s.last_backend_write("mpvpaper").unwrap();

        let bin = tmp.path().join("fake-linux-wallpaperengine");
        std::fs::write(&bin, "#!/bin/sh\nsleep 5\n").unwrap();
        let mut perms = std::fs::metadata(&bin).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&bin, perms).unwrap();
        s.config_set("linux_wallpaperengine_path", &bin.to_string_lossy())
            .unwrap();

        let mut rt = FakeRuntime {
            command_output_success: true,
            command_status_success: true,
            ..Default::default()
        };
        apply_with_fake_runtime(
            &s,
            &scene.to_string_lossy(),
            Backend::LinuxWallpaperEngine,
            Some("/tmp/should-not-be-used.gif"),
            &mut rt,
        )
        .unwrap();

        assert_eq!(rt.stop_mpvpaper_count, 1);
        assert_eq!(
            rt.command_output_count, 0,
            "scene target must not use preview fallback command_output"
        );
        assert_eq!(
            s.current_read().unwrap().as_deref(),
            Some(scene.to_string_lossy().as_ref())
        );
        assert_eq!(
            s.last_backend_read().unwrap().as_deref(),
            Some(crate::LWE_BACKEND_NAME)
        );

        let pid = s.config_get("linux_wallpaperengine_pid", "");
        if let Ok(pid) = pid.parse::<i32>() {
            let _ = std::process::Command::new("kill")
                .args(["-TERM", &format!("-{}", pid)])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
    }

    #[test]
    fn apply_awww_success_emits_expected_stages() {
        let (tmp, s) = temp_storage();
        let img = tmp.path().join("test.png");
        std::fs::write(&img, b"png").unwrap();
        s.last_backend_write("awww").unwrap();

        let mut rt = FakeRuntime {
            command_output_success: true,
            command_status_success: true,
            ..Default::default()
        };
        let mut reporter = apply_stage::test_support::CapturingReporter::new();
        apply_wallpaper_with_runtime(
            &s,
            &img.to_string_lossy(),
            Backend::Awww,
            Some(&img.to_string_lossy()),
            &mut rt,
            &mut reporter,
            Some("req-awww"),
        )
        .unwrap();

        assert_eq!(
            reporter.stages(),
            vec![
                apply_stage::ApplyStage::EnsureAwwwDaemon,
                apply_stage::ApplyStage::AwwwSocketReady,
                apply_stage::ApplyStage::CleanupPrevious,
                apply_stage::ApplyStage::RefreshStatus,
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn apply_lwe_success_emits_expected_stages() {
        use std::os::unix::fs::PermissionsExt;

        let (tmp, s) = temp_storage();
        s.last_backend_write("awww").unwrap();

        let bin = tmp.path().join("test-lwe-stages");
        std::fs::write(&bin, "#!/bin/sh\nsleep 5\n").unwrap();
        let mut perms = std::fs::metadata(&bin).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&bin, perms).unwrap();
        s.config_set("linux_wallpaperengine_path", &bin.to_string_lossy())
            .unwrap();

        let scene = tmp.path().join("steamapps/workshop/content/431960/123456");
        std::fs::create_dir_all(&scene).unwrap();
        std::fs::write(scene.join("scene.pkg"), b"scene").unwrap();
        std::fs::write(
            scene.join("project.json"),
            r#"{"type":"scene","file":"scene.pkg","workshopid":"123456"}"#,
        )
        .unwrap();

        let mut rt = FakeRuntime::default();
        let mut reporter = apply_stage::test_support::CapturingReporter::new();
        apply_wallpaper_with_runtime(
            &s,
            &scene.to_string_lossy(),
            Backend::LinuxWallpaperEngine,
            None,
            &mut rt,
            &mut reporter,
            Some("req-lwe"),
        )
        .unwrap();

        assert_eq!(
            reporter.stages(),
            vec![
                apply_stage::ApplyStage::StartLwe,
                apply_stage::ApplyStage::WaitRendererAlive,
                apply_stage::ApplyStage::CleanupPrevious,
                apply_stage::ApplyStage::RefreshStatus,
            ]
        );

        let pid = s.config_get("linux_wallpaperengine_pid", "");
        if let Ok(pid) = pid.parse::<i32>() {
            let _ = Command::new("kill")
                .args(["-TERM", &format!("-{}", pid)])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }

    #[test]
    fn apply_awww_socket_timeout_stops_at_ensure_daemon() {
        let (tmp, s) = temp_storage();
        let img = tmp.path().join("test.png");
        std::fs::write(&img, b"png").unwrap();
        s.last_backend_write("awww").unwrap();

        let mut rt = FakeRuntime {
            command_status_success: true,
            awww_readiness_sequence: std::cell::RefCell::new(vec![
                crate::runtime::AwwwReadiness::SocketMissing;
                40
            ]),
            ..Default::default()
        };
        let mut reporter = apply_stage::test_support::CapturingReporter::new();
        let err = apply_wallpaper_with_runtime(
            &s,
            &img.to_string_lossy(),
            Backend::Awww,
            Some(&img.to_string_lossy()),
            &mut rt,
            &mut reporter,
            Some("req-timeout"),
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("socket") || err.to_string().contains("failed to start"),
            "expected readiness error, got: {err}"
        );
        assert_eq!(
            reporter.stages(),
            vec![apply_stage::ApplyStage::EnsureAwwwDaemon]
        );
    }

    #[cfg(unix)]
    #[test]
    fn apply_lwe_crash_reaches_wait_renderer_alive() {
        use std::os::unix::fs::PermissionsExt;

        let (tmp, s) = temp_storage();
        s.last_backend_write("awww").unwrap();

        let bin = tmp.path().join("test-lwe-crash-stages");
        std::fs::write(&bin, "#!/bin/sh\nexit 1\n").unwrap();
        let mut perms = std::fs::metadata(&bin).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&bin, perms).unwrap();
        s.config_set("linux_wallpaperengine_path", &bin.to_string_lossy())
            .unwrap();

        let scene = tmp.path().join("steamapps/workshop/content/431960/999999");
        std::fs::create_dir_all(&scene).unwrap();
        std::fs::write(scene.join("scene.pkg"), b"scene").unwrap();
        std::fs::write(
            scene.join("project.json"),
            r#"{"type":"scene","file":"scene.pkg","workshopid":"999999"}"#,
        )
        .unwrap();

        let mut rt = FakeRuntime::default();
        let mut reporter = apply_stage::test_support::CapturingReporter::new();
        let err = apply_wallpaper_with_runtime(
            &s,
            &scene.to_string_lossy(),
            Backend::LinuxWallpaperEngine,
            None,
            &mut rt,
            &mut reporter,
            Some("req-crash"),
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("linux-wallpaperengine") || err.to_string().contains("exited"),
            "expected LWE failure, got: {err}"
        );
        assert_eq!(
            reporter.stages(),
            vec![
                apply_stage::ApplyStage::StartLwe,
                apply_stage::ApplyStage::WaitRendererAlive,
            ]
        );
    }
}
