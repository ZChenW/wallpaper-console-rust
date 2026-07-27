//! wc-backend — wallpaper backend process management.

#[cfg(test)]
use std::process::{Command, Stdio};
#[cfg(test)]
use wc_config::ConfigDirExt;
use wc_core::error::WcError;
use wc_core::types::Backend;
use wc_storage::StorageApi;

pub mod apply_stage;
pub mod apply_transition;
pub mod capability;
pub mod display_executor;
pub mod lifecycle;
pub mod linux_wallpaperengine;
pub mod process_control;
pub mod runtime;
pub mod runtime_observation;
pub mod target_commands;
pub mod visual_handoff;

pub(crate) mod driver;
#[cfg(test)]
pub(crate) mod test_support;

mod awww;
mod deadline_command;
mod debug_log;
mod mpvpaper;
mod restore;
mod swaybg;

pub use apply_transition::{
    execute_apply_transition, plan_apply_transition, preflight_apply_transition,
    ApplyTransitionFailure, ApplyTransitionPlan, ApplyTransitionReport, ApplyTransitionRequest,
};
pub use display_executor::{
    execute_display_actions, DisplayExecAction, DisplayExecContext, DisplayExecFailure,
    DisplayExecReport,
};
pub use restore::restore_clean;
pub use target_commands::ExecutionScope;

use awww::stop_awww;
use debug_log::{write_apply_stage_timings, write_debug_handoff_log};

/// Stop all wallpaper backends.
pub fn stop_all_backends(s: Option<&StorageApi>) -> Result<(), WcError> {
    linux_wallpaperengine::stop(s);
    mpvpaper::stop_mpvpaper();
    swaybg::stop_swaybg();
    stop_awww();
    // Fallback cleanup: kill residual scene renderer processes that may not have been
    // recorded in config (e.g. setsid forked and parent PID was recorded, or a crash
    // left the process behind).
    linux_wallpaperengine::stop_tracked_processes();
    Ok(())
}

/// Backend name constant used for LWE state tracking.
pub const LWE_BACKEND_NAME: &str = "linux-wallpaperengine";

use lifecycle::StopPlan;

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
/// [`restore::restore_clean`].
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
        if matches!(
            failure.cleanup,
            driver::CleanupOutcome::UncertainGlobalStop(_)
                | driver::CleanupOutcome::UncertainTarget
        ) {
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

#[cfg(test)]
pub(crate) fn whoami() -> String {
    match current_process_user() {
        ProcessUserScope::Name(name) => name,
        ProcessUserScope::Uid(uid) => uid.to_string(),
    }
}

/// Scope for pgrep/pkill user filtering. Prefer login name when available;
/// fall back to numeric uid so process queries still work without USER set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ProcessUserScope {
    Name(String),
    Uid(u32),
}

pub(crate) fn current_process_user() -> ProcessUserScope {
    static RESOLVED: std::sync::OnceLock<ProcessUserScope> = std::sync::OnceLock::new();
    RESOLVED.get_or_init(resolve_process_user).clone()
}

pub(crate) fn append_pgrep_user_scope(cmd: &mut std::process::Command, scope: &ProcessUserScope) {
    match scope {
        ProcessUserScope::Name(name) => {
            cmd.arg("-u").arg(name);
        }
        ProcessUserScope::Uid(uid) => {
            cmd.arg("-U").arg(uid.to_string());
        }
    }
}

fn resolve_process_user() -> ProcessUserScope {
    for key in ["USER", "LOGNAME"] {
        if let Ok(user) = std::env::var(key) {
            let trimmed = user.trim();
            if !trimmed.is_empty() {
                return ProcessUserScope::Name(trimmed.to_string());
            }
        }
    }

    let uid = unsafe { libc::getuid() };
    if let Some(name) = passwd_name_for_uid(uid) {
        return ProcessUserScope::Name(name);
    }
    if let Some(name) = passwd_name_from_proc_status() {
        return ProcessUserScope::Name(name);
    }

    log::error!(
        "wc-backend: could not resolve login name for uid {uid}; using uid scope for process queries"
    );
    ProcessUserScope::Uid(uid)
}

fn passwd_name_for_uid(uid: u32) -> Option<String> {
    use std::ffi::CStr;
    use std::ptr;

    let mut buf = vec![0u8; 16_384];
    let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result: *mut libc::passwd = ptr::null_mut();
    let rc = unsafe {
        libc::getpwuid_r(
            uid as libc::uid_t,
            &mut pwd,
            buf.as_mut_ptr() as *mut libc::c_char,
            buf.len(),
            &mut result,
        )
    };
    if rc != 0 || result.is_null() {
        return None;
    }
    unsafe {
        if pwd.pw_name.is_null() {
            return None;
        }
        CStr::from_ptr(pwd.pw_name)
            .to_str()
            .ok()
            .map(|name| name.to_owned())
    }
}

fn passwd_name_from_proc_status() -> Option<String> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        if let Some(name) = line.strip_prefix("Name:\t") {
            let trimmed = name.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::awww::{build_awww_img_command, build_awww_instant_command};
    use crate::test_support::FakeRuntime;
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

    #[test]
    fn whoami_returns_nonempty_login_or_uid_string() {
        let name = whoami();
        assert!(!name.is_empty());
        match current_process_user() {
            ProcessUserScope::Name(resolved) => assert_eq!(name, resolved),
            ProcessUserScope::Uid(uid) => assert_eq!(name, uid.to_string()),
        }
    }

    #[test]
    fn append_pgrep_user_scope_uses_name_or_uid_flag() {
        let mut name_cmd = std::process::Command::new("pgrep");
        append_pgrep_user_scope(&mut name_cmd, &ProcessUserScope::Name("alice".to_string()));
        let name_args: Vec<String> = name_cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(name_args.contains(&"-u".to_string()));
        assert!(name_args.contains(&"alice".to_string()));

        let mut uid_cmd = std::process::Command::new("pgrep");
        append_pgrep_user_scope(&mut uid_cmd, &ProcessUserScope::Uid(1000));
        let uid_args: Vec<String> = uid_cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(uid_args.contains(&"-U".to_string()));
        assert!(uid_args.contains(&"1000".to_string()));
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

        let err = restore_clean(&s).unwrap_err();
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
            img.to_string_lossy().as_ref(),
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

        let preview_path = preview.to_string_lossy();
        let err = apply_wallpaper(
            &s,
            &scene.to_string_lossy(),
            Backend::LinuxWallpaperEngine,
            Some(preview_path.as_ref()),
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

        let preview_path = preview.to_string_lossy();
        let err = apply_wallpaper(
            &s,
            &scene.to_string_lossy(),
            Backend::LinuxWallpaperEngine,
            Some(preview_path.as_ref()),
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

        let preview_path = preview.to_string_lossy();
        let err = apply_wallpaper(
            &s,
            &scene.to_string_lossy(),
            Backend::LinuxWallpaperEngine,
            Some(preview_path.as_ref()),
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
            stages.contains(&apply_stage::ApplyStage::EnsureAwwwDaemon),
            "fallback instant path must emit EnsureAwwwDaemon"
        );
        assert!(
            stages.contains(&apply_stage::ApplyStage::AwwwSocketReady),
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
        assert_eq!(
            rt.mpvpaper_wait_targets,
            vec![("*".to_string(), next.to_string_lossy().into_owned())]
        );
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
        assert!(crate::driver::ensure_awww_daemon_running(&mut rt).is_ok());
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
        let err = crate::driver::ensure_awww_daemon_running(&mut rt).unwrap_err();
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
        let result = crate::runtime::wait_for_awww_socket_ready(
            &mut rt,
            &crate::ProcessUserScope::Name("testuser".to_string()),
        );
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
        assert!(crate::driver::ensure_awww_daemon_running(&mut rt).is_ok());
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
        // which uses runtime.command_output. driver::ensure_awww_daemon_running
        // returns Ok(()) via FakeRuntime socket Ready so the path doesn't depend
        // on a real compositor daemon.
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
    fn swaybg_apply_launches_and_confirms_the_renderer_before_writing_state() {
        let (tmp, s) = temp_storage();
        s.last_backend_write("").unwrap();
        let image = tmp.path().join("swaybg.png");
        std::fs::write(&image, b"image").unwrap();
        let mut runtime = FakeRuntime {
            command_status_success: true,
            swaybg_ready_pid: Some(52),
            ..Default::default()
        };

        apply_with_fake_runtime(
            &s,
            &image.to_string_lossy(),
            Backend::Swaybg,
            None,
            &mut runtime,
        )
        .unwrap();

        assert_eq!(runtime.command_status_programs, ["setsid"]);
        assert_eq!(runtime.swaybg_wait_count, 1);
        assert_eq!(runtime.swaybg_pid_running_checks, [52]);
        assert_eq!(s.last_backend_read().unwrap().as_deref(), Some("swaybg"));
    }

    #[test]
    fn feh_apply_runs_once_and_writes_state_only_after_success() {
        let (tmp, s) = temp_storage();
        s.last_backend_write("").unwrap();
        let image = tmp.path().join("feh.png");
        std::fs::write(&image, b"image").unwrap();
        let mut runtime = FakeRuntime {
            command_output_success: true,
            ..Default::default()
        };

        apply_with_fake_runtime(
            &s,
            &image.to_string_lossy(),
            Backend::Feh,
            None,
            &mut runtime,
        )
        .unwrap();

        assert_eq!(runtime.command_output_programs, ["feh"]);
        assert_eq!(
            runtime.command_output_args,
            [vec![
                "--no-fehbg".to_string(),
                "--bg-fill".to_string(),
                image.to_string_lossy().to_string(),
            ]]
        );
        assert_eq!(s.last_backend_read().unwrap().as_deref(), Some("feh"));
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
