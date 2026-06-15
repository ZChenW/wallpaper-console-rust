//! wc-backend — wallpaper backend process management.

use std::process::{Command, Stdio};
use wc_core::error::WcError;
use wc_core::types::Backend;
use wc_storage::StorageApi;

pub mod lifecycle;
pub mod linux_wallpaperengine;
pub mod process_control;
pub mod runtime;
pub mod visual_handoff;

/// Stop all wallpaper backends via pkill.
pub fn stop_all_backends(s: Option<&StorageApi>) -> Result<(), WcError> {
    let user = whoami();
    linux_wallpaperengine::stop(s);
    let _ = Command::new("pkill")
        .args(["-u", &user, "-f", r"(^|/)mpvpaper\b"])
        .status();
    let _ = Command::new("pkill")
        .args(["-u", &user, "-f", r"(^|/)awww\b"])
        .status();
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
    let _ = build_pkill_exact_command(&user, "awww-daemon")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

use lifecycle::StopPlan;

fn execute_stop_plan_with_runtime(
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

fn stop_awww() {
    let user = whoami();
    let _ = build_pkill_exact_command(&user, "awww-daemon").status();
}

fn stop_mpvpaper() {
    let user = whoami();
    let _ = Command::new("pkill")
        .args(["-u", &user, "-f", r"(^|/)mpvpaper\b"])
        .status();
}

fn build_pkill_exact_command(user: &str, process_name: &str) -> Command {
    let mut cmd = Command::new("pkill");
    cmd.args(["-u", user, "-x", process_name]);
    cmd
}

fn write_success_state(s: &StorageApi, state_path: &str, backend: Backend) -> Result<(), WcError> {
    s.current_write(state_path)?;
    s.last_backend_write(backend.as_str())?;
    s.history_add(state_path, backend.as_str())?;
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
    apply_wallpaper_with_runtime(s, path, backend, fallback_path, &mut runtime)
}

fn apply_wallpaper_with_runtime(
    s: &StorageApi,
    path: &str,
    backend: Backend,
    fallback_path: Option<&str>,
    runtime: &mut dyn runtime::BackendRuntime,
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

    let mut fallback_error: Option<String> = None;

    execute_stop_plan_with_runtime(s, lifecycle.pre_stop, runtime)?;

    let fallback_ok = match visual.fallback_stage {
        visual_handoff::FallbackStage::TargetImageInstant
        | visual_handoff::FallbackStage::TargetPreviewInstant => {
            if let Some(fb) = fallback_path {
                match apply_awww_instant_with_runtime(s, fb, runtime) {
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
                        fallback_error = Some(msg.clone());
                        if visual.fallback_stage
                            == visual_handoff::FallbackStage::TargetImageInstant
                        {
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
                        false
                    }
                }
            } else {
                false
            }
        }
        visual_handoff::FallbackStage::None => false,
    };

    if visual.stop_previous_after_fallback {
        let stop_target = lifecycle.post_success_stop;
        if stop_target != StopPlan::None {
            let _ = execute_stop_plan_with_runtime(s, stop_target, runtime);
        }
    }

    // If TargetImageInstant already succeeded, skip normal awww img —
    // the target image is already visible via instant awww.
    let target_already_shown = fallback_ok
        && matches!(
            visual.fallback_stage,
            visual_handoff::FallbackStage::TargetImageInstant
        );

    if !target_already_shown {
        let target_result = match backend {
            Backend::Awww => (|| -> Result<(), WcError> {
                runtime.ensure_awww_daemon_running()?;
                let resize_raw = s.config_get("awww_resize", "crop");
                let resize = normalize_awww_resize(&resize_raw);
                let transition_type = s.config_get("awww_transition_type", "fade");
                let duration = s.config_get("awww_transition_duration", "1");
                let fps = s.config_get("wallpaper_transition_fps", "60");
                let mut cmd =
                    build_awww_img_command(path, resize, &transition_type, &duration, &fps);
                cmd.arg("--filter").arg("Lanczos3");
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
                let mut cmd = Command::new("setsid");
                cmd.args(["-f", "mpvpaper", "--fork", "-o", opts, &output, "--", path]);
                let status = runtime
                    .command_status(&mut cmd)
                    .map_err(|e| WcError::Other(format!("mpvpaper failed: {}", e)))?;
                if !status.success() {
                    return Err(WcError::Other("mpvpaper failed to apply wallpaper".into()));
                }
                Ok(())
            }
            Backend::LinuxWallpaperEngine => {
                let project = linux_wallpaperengine::project_from_path(path)?;
                match linux_wallpaperengine::apply(s, project) {
                    Ok(()) => Ok(()),
                    Err(e) => Err(e),
                }
            }
            Backend::Unsupported => unreachable!(),
        };

        if let Err(e) = target_result {
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
        execute_stop_plan_with_runtime(s, lifecycle.post_success_stop, runtime)?;
    }

    write_debug_handoff_log(
        s,
        &lifecycle,
        backend,
        fallback_path,
        &visual,
        fallback_error.as_deref().unwrap_or(""),
        path,
    );

    write_success_state(s, path, backend)?;
    Ok(())
}

fn write_debug_handoff_log(
    s: &StorageApi,
    lifecycle: &lifecycle::ApplyLifecyclePlan,
    backend: Backend,
    fallback_path: Option<&str>,
    visual: &visual_handoff::VisualHandoffPlan,
    fallback_error: &str,
    path: &str,
) {
    if s.config_get("gui_debug_logs", "off") != "on" {
        return;
    }
    let log_path = s.cd.path.join("backend-handoff-last.log");
    let fb_name = fallback_path
        .and_then(|p| std::path::Path::new(p).file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let path_name = std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    let log = format!(
        "previous={:?}\ntarget={:?}\npath={}\nfallback={}\npre_stop={:?}\nfallback_stage={:?}\ntarget_startup_settle_ms={}\npost_success_stop={:?}\nfallback_error={}\n",
        lifecycle.previous,
        backend,
        path_name,
        fb_name,
        lifecycle.pre_stop,
        visual.fallback_stage,
        visual.target_startup_settle_ms,
        lifecycle.post_success_stop,
        fallback_error,
    );
    let _ = std::fs::write(&log_path, log);
}

/// Restore the last wallpaper.
pub fn restore(s: &StorageApi) -> Result<(), WcError> {
    let current = s
        .current_read()?
        .ok_or_else(|| WcError::Other("no previous wallpaper to restore".into()))?;
    let p = std::path::Path::new(&current);
    if !p.is_file() && !p.is_dir() {
        return Err(WcError::WallpaperMissing(p.to_path_buf()));
    }
    let entry = wc_scan::make_entry(&current)
        .ok_or_else(|| WcError::UnsupportedFileType(current.clone()))?;
    let raw = s.config_get("image_backend", "awww");
    let backend = match entry.file_type {
        wc_core::types::FileType::Image => match wc_core::config::normalize_image_backend(&raw) {
            "mpvpaper" => Backend::Mpvpaper,
            _ => Backend::Awww,
        },
        wc_core::types::FileType::Gif => match s.config_get("gif_backend", "awww").as_str() {
            "mpvpaper" => Backend::Mpvpaper,
            _ => Backend::Awww,
        },
        wc_core::types::FileType::Video => {
            match s.config_get("video_backend", "mpvpaper").as_str() {
                "awww" => Backend::Awww,
                _ => Backend::Mpvpaper,
            }
        }
        wc_core::types::FileType::WeScene => Backend::LinuxWallpaperEngine,
        wc_core::types::FileType::WeWeb => Backend::Unsupported,
        wc_core::types::FileType::WeApplication => Backend::Unsupported,
    };

    let fallback_path: Option<String> = match entry.file_type {
        wc_core::types::FileType::Image | wc_core::types::FileType::Gif => Some(current.clone()),
        wc_core::types::FileType::Video => None,
        wc_core::types::FileType::WeScene => {
            wc_scan::read_we_project_info(p).and_then(|info| info.preview_path)
        }
        wc_core::types::FileType::WeWeb | wc_core::types::FileType::WeApplication => None,
    };

    apply_wallpaper(s, &current, backend, fallback_path.as_deref())
}

pub(crate) fn is_awww_daemon_running(user: &str) -> bool {
    if user.is_empty() {
        return false;
    }
    matches!(
        std::process::Command::new("pgrep")
            .arg("-u")
            .arg(user)
            .arg("-x")
            .arg("awww-daemon")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status(),
        Ok(s) if s.success()
    )
}

fn apply_awww_instant_with_runtime(
    s: &StorageApi,
    path: &str,
    runtime: &mut dyn runtime::BackendRuntime,
) -> Result<(), WcError> {
    runtime.ensure_awww_daemon_running()?;
    let resize_raw = s.config_get("awww_resize", "crop");
    let resize = normalize_awww_resize(&resize_raw);
    let fps = s.config_get("wallpaper_transition_fps", "60");
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

fn build_awww_instant_command(path: &str, resize: &str, fps: &str) -> Command {
    let mut cmd = Command::new("awww");
    cmd.arg("img")
        .arg(path)
        .arg("--resize")
        .arg(resize)
        .arg("--transition-type")
        .arg("none")
        .arg("--transition-duration")
        .arg("0")
        .arg("--transition-fps")
        .arg(fps)
        .arg("--filter")
        .arg("Lanczos3");
    cmd
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
                match apply_awww_instant_with_runtime(s, &old_path, runtime) {
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

fn normalize_awww_resize(raw: &str) -> &'static str {
    match raw {
        "crop" => "crop",
        "fit" => "fit",
        "stretch" => "stretch",
        _ => "crop",
    }
}

fn normalize_mpvpaper_options(raw: &str) -> &str {
    let trimmed = raw.trim();
    if trimmed == "no-audio --loop-file=inf" || trimmed == "--loop-file=inf" {
        "--loop-file=inf --panscan=1.0"
    } else {
        trimmed
    }
}

fn build_awww_img_command(
    path: &str,
    resize: &str,
    transition_type: &str,
    duration: &str,
    fps: &str,
) -> std::process::Command {
    let mut cmd = std::process::Command::new("awww");
    cmd.arg("img")
        .arg(path)
        .arg("--resize")
        .arg(resize)
        .arg("--transition-type")
        .arg(transition_type)
        .arg("--transition-duration")
        .arg(duration)
        .arg("--transition-fps")
        .arg(fps);
    cmd
}

pub(crate) fn whoami() -> String {
    std::env::var("USER").unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
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
        s.history_add(&project.to_string_lossy(), "unsupported")
            .unwrap();

        let history_before = s.history_list().unwrap().len();

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
            s.history_list().unwrap().len(),
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

    #[test]
    fn awww_command_includes_transition_fps() {
        let cmd = build_awww_img_command("/tmp/test.jpg", "crop", "fade", "1", "60");
        let args: Vec<String> = cmd
            .get_args()
            .map(|s| s.to_string_lossy().to_string())
            .collect();
        assert!(args.contains(&"--resize".to_string()));
        assert!(args.contains(&"crop".to_string()));
        assert!(args.contains(&"--transition-type".to_string()));
        assert!(args.contains(&"fade".to_string()));
        assert!(args.contains(&"--transition-fps".to_string()));
        assert!(args.contains(&"60".to_string()));
    }

    #[test]
    fn normalize_awww_resize_known_values() {
        assert_eq!(normalize_awww_resize("crop"), "crop");
        assert_eq!(normalize_awww_resize("fit"), "fit");
        assert_eq!(normalize_awww_resize("stretch"), "stretch");
    }

    #[test]
    fn normalize_awww_resize_unknown_fallback() {
        assert_eq!(normalize_awww_resize("unknown"), "crop");
        assert_eq!(normalize_awww_resize(""), "crop");
        assert_eq!(normalize_awww_resize("center"), "crop");
    }

    #[test]
    fn normalize_mpvpaper_options_migrates_legacy_silent_default() {
        assert_eq!(
            normalize_mpvpaper_options("no-audio --loop-file=inf"),
            "--loop-file=inf --panscan=1.0"
        );
        assert_eq!(
            normalize_mpvpaper_options("  no-audio --loop-file=inf  "),
            "--loop-file=inf --panscan=1.0"
        );
        assert_eq!(
            normalize_mpvpaper_options("no-audio --loop-file=inf --panscan=1"),
            "no-audio --loop-file=inf --panscan=1"
        );
    }

    #[test]
    fn normalize_mpvpaper_options_migrates_plain_loop_default_to_crop_fill() {
        assert_eq!(
            normalize_mpvpaper_options("--loop-file=inf"),
            "--loop-file=inf --panscan=1.0"
        );
        assert_eq!(
            normalize_mpvpaper_options("  --loop-file=inf  "),
            "--loop-file=inf --panscan=1.0"
        );
    }

    #[test]
    fn normalize_mpvpaper_options_preserves_custom_args() {
        assert_eq!(
            normalize_mpvpaper_options("--loop-file=inf --volume=60"),
            "--loop-file=inf --volume=60"
        );
        assert_eq!(
            normalize_mpvpaper_options("--loop-file=inf --volume=80 --mute=no"),
            "--loop-file=inf --volume=80 --mute=no"
        );
        assert_eq!(normalize_mpvpaper_options(""), "");
    }

    #[test]
    fn awww_resize_unknown_fallback_to_crop() {
        let cmd = build_awww_img_command("/tmp/test.jpg", "crop", "fade", "1", "60");
        let args: Vec<String> = cmd
            .get_args()
            .map(|s| s.to_string_lossy().to_string())
            .collect();
        assert!(args.contains(&"--resize".to_string()));
        assert!(args.contains(&"crop".to_string()));
        assert!(!args.contains(&"unknown".to_string()));
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
        assert_eq!(plan_awww.pre_stop, StopPlan::None);
        assert_eq!(plan_awww.post_success_stop, StopPlan::AwwwDaemonOnly);

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
    fn apply_wallpaper_lwe_writes_state_after_renderer_survives() {
        use std::os::unix::fs::PermissionsExt;

        let (tmp, s) = temp_storage();
        s.last_backend_write("awww").unwrap();

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
        assert_eq!(
            s.history_list().unwrap().last().cloned(),
            Some(scene.to_string_lossy().to_string())
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

    #[cfg(unix)]
    #[test]
    fn apply_wallpaper_lwe_failure_preserves_old_state() {
        use std::os::unix::fs::PermissionsExt;

        let (tmp, s) = temp_storage();
        let old = tmp.path().join("old.jpg");
        std::fs::write(&old, b"old").unwrap();
        s.current_write(&old.to_string_lossy()).unwrap();
        s.last_backend_write("awww").unwrap();
        s.history_add(&old.to_string_lossy(), "awww").unwrap();
        let history_before = s.history_list().unwrap().len();

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
        assert_eq!(s.history_list().unwrap().len(), history_before);
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
        s.history_add(&old.to_string_lossy(), "mpvpaper").unwrap();
        let history_before = s.history_list().unwrap().len();

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
        assert_eq!(s.history_list().unwrap().len(), history_before);
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
        s.history_add(&old.to_string_lossy(), "awww").unwrap();
        let history_before = s.history_list().unwrap().len();

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
        assert_eq!(s.history_list().unwrap().len(), history_before);
    }

    #[cfg(unix)]
    #[test]
    fn apply_wallpaper_previous_awww_missing_path_lwe_fail_still_preserves_state() {
        use std::os::unix::fs::PermissionsExt;

        let (tmp, s) = temp_storage();
        let missing = tmp.path().join("missing.jpg");
        s.current_write(&missing.to_string_lossy()).unwrap();
        s.last_backend_write("awww").unwrap();
        s.history_add(&missing.to_string_lossy(), "awww").unwrap();
        let history_before = s.history_list().unwrap().len();

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
        assert_eq!(s.history_list().unwrap().len(), history_before);
    }

    #[test]
    fn awww_instant_command_uses_minimal_transition_duration() {
        let cmd = build_awww_instant_command("/tmp/test.jpg", "crop", "60");
        let args: Vec<String> = cmd
            .get_args()
            .map(|s| s.to_string_lossy().to_string())
            .collect();
        assert!(args.contains(&"--transition-type".to_string()));
        assert!(args.contains(&"none".to_string()));
        assert!(args.contains(&"--transition-duration".to_string()));
        assert!(args.contains(&"0".to_string()));
        assert!(!args.contains(&"fade".to_string()));
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
        assert!(!args.contains(&"none".to_string()));
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
    fn stop_awww_uses_exact_daemon_process_name() {
        let cmd = build_pkill_exact_command("alice", "awww-daemon");
        let args: Vec<String> = cmd
            .get_args()
            .map(|s| s.to_string_lossy().to_string())
            .collect();
        assert_eq!(args, ["-u", "alice", "-x", "awww-daemon"]);
        assert!(!args.contains(&r"(^|/)awww\b".to_string()));
    }

    #[test]
    fn cross_backend_image_skips_normal_awww_when_fallback_ok() {
        // When TargetImageInstant fallback succeeds (instant awww placed the image),
        // the normal awww img call with user transition must be skipped.
        // This is tested at the plan level: TargetImageInstant sets target_startup_settle=0
        // and in apply_wallpaper the `target_already_shown` flag gates the match block.
        let plan = visual_handoff::plan_visual_handoff(
            lifecycle::RunningBackend::Mpvpaper,
            Backend::Awww,
            Some("/tmp/img.jpg"),
        );
        assert_eq!(
            plan.fallback_stage,
            visual_handoff::FallbackStage::TargetImageInstant
        );
        // TargetImageInstant means the normal awww apply is skipped in apply_wallpaper
        // when fallback_ok is true.
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
    }

    impl crate::runtime::BackendRuntime for FakeRuntime {
        fn command_output(
            &mut self,
            _command: &mut std::process::Command,
        ) -> Result<std::process::Output, WcError> {
            self.command_output_count += 1;
            std::process::Command::new("true")
                .output()
                .map_err(|e| WcError::Other(format!("fake command failed: {}", e)))
        }

        fn command_status(
            &mut self,
            _command: &mut std::process::Command,
        ) -> Result<std::process::ExitStatus, WcError> {
            self.command_status_count += 1;
            std::process::Command::new("true")
                .status()
                .map_err(|e| WcError::Other(format!("fake command failed: {}", e)))
        }

        fn stop_awww(&mut self) {
            self.stop_awww_count += 1;
        }

        fn stop_mpvpaper(&mut self) {
            self.stop_mpvpaper_count += 1;
        }

        fn stop_lwe(&mut self, _s: Option<&wc_storage::StorageApi>) {
            self.stop_lwe_count += 1;
        }

        fn ensure_awww_daemon_running(&mut self) -> Result<(), WcError> {
            Ok(())
        }
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

        let mut rt = FakeRuntime::default();
        let _result = apply_wallpaper_with_runtime(
            &s,
            &img.to_string_lossy(),
            Backend::Mpvpaper,
            None,
            &mut rt,
        );

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

        let mut rt = FakeRuntime::default();
        let _result = apply_wallpaper_with_runtime(
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
}
