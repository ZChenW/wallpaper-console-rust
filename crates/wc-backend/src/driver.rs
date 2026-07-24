//! Backend driver registry — single place to add a new wallpaper backend.
//!
//! # Adding a new backend
//!
//! 1. Add a variant to [`wc_core::types::Backend`] (plus `as_str` / serde rename).
//! 2. Implement [`BackendDriver`] for a unit struct in this module (capability,
//!    ensure_available, stop/stop_checked, and apply helpers as needed).
//! 3. Register a `&'static` instance in [`driver_for`].
//! 4. Wire format → backend routing in `wc-core` / CLI / GUI apply planners.
//! 5. Allow the backend name in storage persistence / display_state whitelist
//!    (and any restore / observation paths that match on backend strings).
//!
//! Lifecycle (`lifecycle.rs`) and visual handoff (`visual_handoff.rs`) stay as
//! cross-backend planners: their decisions depend on *(previous, target)* pairs
//! and do not belong on a single-backend driver. See comments at the top of
//! those modules.

use std::process::{Command, Output};

use wc_core::error::WcError;
use wc_core::types::Backend;
use wc_storage::StorageApi;

use crate::apply_stage::{self, ApplyStageReporter};
use crate::awww::{normalize_awww_resize, normalize_awww_transition_type};
use crate::capability::{
    AllDisplaysTargeting, BackendCapability, CrossOutputCoexistence, Evidence,
    MultiInstanceSupport, OutputTargetMode, SameTargetReplacement, StopScope,
};
use crate::mpvpaper::normalize_mpvpaper_options;
use crate::runtime::{
    build_awww_daemon_command, wait_for_awww_socket_ready, wait_for_mpvpaper_stopped_with,
    AwwwReadiness, BackendRuntime, ProcessIo,
};
use crate::target_commands::{
    build_awww_img_command_for_scope, build_awww_instant_command_for_scope,
    build_mpvpaper_launch_command_for_output, ExecutionScope,
};

/// Per-backend behavior that used to be scattered across `match backend` arms.
pub(crate) trait BackendDriver: Send + Sync {
    fn backend(&self) -> Backend;

    fn capability(&self) -> BackendCapability;

    /// Preflight PATH / config checks before destructive handoff or apply.
    fn ensure_available(&self, storage: &StorageApi) -> Result<(), WcError>;

    /// Best-effort stop (legacy / lifecycle plans).
    fn stop(&self, runtime: &mut dyn BackendRuntime, storage: Option<&StorageApi>);

    /// Stop with post-conditions checked (display executor).
    fn stop_checked(
        &self,
        runtime: &mut dyn BackendRuntime,
        storage: Option<&StorageApi>,
    ) -> Result<(), WcError>;
}

/// Lookup the driver for a supported backend. [`Backend::Unsupported`] → `None`.
pub(crate) fn driver_for(backend: Backend) -> Option<&'static dyn BackendDriver> {
    match backend {
        Backend::Awww => Some(&AWWW_DRIVER),
        Backend::Mpvpaper => Some(&MPVPAPER_DRIVER),
        Backend::LinuxWallpaperEngine => Some(&LWE_DRIVER),
        Backend::Unsupported => None,
    }
}

/// Resolve a persisted backend name (`display_state` / last_backend) to a driver.
pub(crate) fn driver_for_persisted_name(name: &str) -> Option<&'static dyn BackendDriver> {
    let backend = match name {
        "awww" | "swww" => Backend::Awww,
        "mpvpaper" => Backend::Mpvpaper,
        "linux-wallpaperengine" => Backend::LinuxWallpaperEngine,
        _ => return None,
    };
    driver_for(backend)
}

/// Prefer stderr, then stdout, then a stable placeholder — shared by apply paths.
pub(crate) fn command_output_detail(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        "no renderer output".into()
    }
}

/// Format a failed awww apply (`awww apply failed with status …`).
pub(crate) fn awww_apply_status_error(output: &Output) -> WcError {
    WcError::Other(format!(
        "awww apply failed with status {}: {}",
        output.status,
        command_output_detail(output)
    ))
}

/// Format a failed awww instant apply (`awww instant apply failed with status …`).
pub(crate) fn awww_instant_status_error(output: &Output) -> WcError {
    WcError::Other(format!(
        "awww instant apply failed with status {}: {}",
        output.status,
        command_output_detail(output)
    ))
}

/// Parameters for [`apply_awww`] (keeps the call surface under clippy's arg limit).
pub(crate) struct AwwwApplyRequest<'a> {
    pub path: &'a str,
    pub scope: &'a ExecutionScope,
    pub use_instant: bool,
    pub clear_state_hint: bool,
    pub request_id: Option<&'a str>,
}

/// Shared awww apply used by legacy fullscreen and display-scoped paths.
pub(crate) fn apply_awww(
    s: &StorageApi,
    req: &AwwwApplyRequest<'_>,
    runtime: &mut dyn BackendRuntime,
    reporter: Option<&mut dyn ApplyStageReporter>,
) -> Result<(), WcError> {
    if let Some(reporter) = reporter {
        apply_stage::report_stage(
            reporter,
            apply_stage::ApplyStage::EnsureAwwwDaemon,
            req.request_id,
        );
        ensure_awww_daemon_running(runtime)?;
        apply_stage::report_stage(
            reporter,
            apply_stage::ApplyStage::AwwwSocketReady,
            req.request_id,
        );
    } else {
        ensure_awww_daemon_running(runtime)?;
    }
    if req.clear_state_hint {
        clear_awww_state_hint(runtime);
    }

    let fps_raw = s.config_get("wallpaper_transition_fps", "60");
    let fps = wc_core::config_normalizer::normalize_awww_transition_fps(&fps_raw);
    let resize_raw = s.config_get("awww_resize", "crop");
    let resize = normalize_awww_resize(&resize_raw);

    let mut cmd = if req.use_instant {
        build_awww_instant_command_for_scope(req.path, resize, &fps, req.scope)?
    } else {
        let transition_raw = s.config_get("awww_transition_type", "fade");
        let transition_type = normalize_awww_transition_type(&transition_raw);
        let duration_raw = s.config_get("awww_transition_duration", "1");
        let duration =
            wc_core::config_normalizer::normalize_awww_transition_duration(&duration_raw);
        let mut cmd = build_awww_img_command_for_scope(
            req.path,
            resize,
            transition_type,
            &duration,
            &fps,
            req.scope,
        )?;
        cmd.arg("--filter").arg("Lanczos3");
        cmd
    };

    let output = runtime
        .command_output(&mut cmd)
        .map_err(|e| WcError::Other(format!("awww failed: {}", e)))?;
    if !output.status.success() {
        return Err(awww_apply_status_error(&output));
    }
    Ok(())
}

/// Shared awww instant apply (visual fallback / rollback).
pub(crate) fn apply_awww_instant(
    s: &StorageApi,
    path: &str,
    scope: &ExecutionScope,
    runtime: &mut dyn BackendRuntime,
    reporter: Option<&mut dyn ApplyStageReporter>,
    request_id: Option<&str>,
) -> Result<(), WcError> {
    if let Some(reporter) = reporter {
        apply_stage::report_stage(
            reporter,
            apply_stage::ApplyStage::EnsureAwwwDaemon,
            request_id,
        );
        ensure_awww_daemon_running(runtime)?;
        apply_stage::report_stage(
            reporter,
            apply_stage::ApplyStage::AwwwSocketReady,
            request_id,
        );
    } else {
        ensure_awww_daemon_running(runtime)?;
    }
    let resize_raw = s.config_get("awww_resize", "crop");
    let resize = normalize_awww_resize(&resize_raw);
    let fps_raw = s.config_get("wallpaper_transition_fps", "60");
    let fps = wc_core::config_normalizer::normalize_awww_transition_fps(&fps_raw);
    let mut cmd = build_awww_instant_command_for_scope(path, resize, &fps, scope)?;
    let output = runtime
        .command_output(&mut cmd)
        .map_err(|e| WcError::Other(format!("awww instant failed: {}", e)))?;
    if !output.status.success() {
        return Err(awww_instant_status_error(&output));
    }
    Ok(())
}

/// Ensure awww-daemon is running (ProcessIo: socket probe + optional spawn).
pub(crate) fn ensure_awww_daemon_running(runtime: &mut dyn ProcessIo) -> Result<(), WcError> {
    if matches!(runtime.awww_socket_ready(), AwwwReadiness::Ready) {
        return Ok(());
    }
    let user = crate::current_process_user();
    let was_running = crate::awww::is_awww_daemon_running(&user);
    if !was_running {
        let mut cmd = build_awww_daemon_command();
        let status = runtime.command_status(&mut cmd).map_err(|_| {
            WcError::Other(
                "setsid not available — cannot launch awww-daemon. \
                 setsid is part of util-linux; install it with your package manager."
                    .into(),
            )
        })?;
        if !status.success() {
            return Err(WcError::Other(
                "awww-daemon not found. Install awww (pip install awww or AUR).".into(),
            ));
        }
    }
    wait_for_awww_socket_ready(runtime, &user)
}

/// Best-effort `awww clear` after cross-backend handoff (ProcessIo).
pub(crate) fn clear_awww_state_hint(runtime: &mut dyn ProcessIo) {
    let mut cmd = Command::new("awww");
    cmd.arg("clear")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let _ = runtime.command_status(&mut cmd);
}

/// Display-path LWE apply — fakes intercept via [`BackendRuntime::apply_lwe_to_outputs`].
pub(crate) fn apply_lwe_to_outputs(
    s: &StorageApi,
    project: &crate::linux_wallpaperengine::LinuxWallpaperEngineProject,
    outputs: &[String],
    runtime: &mut dyn BackendRuntime,
) -> Result<(), WcError> {
    runtime.apply_lwe_to_outputs(s, project, outputs)
}

/// Error from [`launch_mpvpaper`]: distinguishes launcher vs readiness failure so
/// callers can decide whether a post-failure mpvpaper stop is warranted.
#[derive(Debug)]
pub(crate) enum MpvpaperApplyError {
    /// `setsid`/`mpvpaper` invocation failed before readiness wait.
    Start(WcError),
    /// Process was launched but did not become ready.
    Ready(WcError),
}

impl From<MpvpaperApplyError> for WcError {
    fn from(error: MpvpaperApplyError) -> Self {
        match error {
            MpvpaperApplyError::Start(error) | MpvpaperApplyError::Ready(error) => error,
        }
    }
}

/// Launch mpvpaper and wait until a new PID appears. Caller owns cleanup on failure.
pub(crate) fn launch_mpvpaper(
    s: &StorageApi,
    path: &str,
    output: &str,
    previous_pids: &[u32],
    runtime: &mut dyn BackendRuntime,
) -> Result<u32, MpvpaperApplyError> {
    let opts_raw = s.config_get("mpvpaper_options", "--loop-file=inf --panscan=1.0");
    let opts = normalize_mpvpaper_options(&opts_raw);
    let mut cmd: Command = build_mpvpaper_launch_command_for_output(opts, output, path)
        .map_err(MpvpaperApplyError::Start)?;
    let status = runtime.command_status(&mut cmd).map_err(|e| {
        MpvpaperApplyError::Start(WcError::Other(format!("mpvpaper failed: {}", e)))
    })?;
    if !status.success() {
        return Err(MpvpaperApplyError::Start(WcError::Other(
            "mpvpaper failed to apply wallpaper".into(),
        )));
    }
    runtime
        .wait_for_mpvpaper_ready(previous_pids, output, path)
        .map_err(MpvpaperApplyError::Ready)
}

// --- Drivers ----------------------------------------------------------------

static AWWW_DRIVER: AwwwDriver = AwwwDriver;
static MPVPAPER_DRIVER: MpvpaperDriver = MpvpaperDriver;
static LWE_DRIVER: LweDriver = LweDriver;

struct AwwwDriver;
struct MpvpaperDriver;
struct LweDriver;

impl BackendDriver for AwwwDriver {
    fn backend(&self) -> Backend {
        Backend::Awww
    }

    fn capability(&self) -> BackendCapability {
        BackendCapability {
            backend: Backend::Awww,
            output_target_mode: OutputTargetMode::NamedOutputs,
            output_target_evidence: Evidence::CliVerified,
            all_displays: AllDisplaysTargeting::OmitMeansAll,
            all_displays_evidence: Evidence::CliVerified,
            stop_scope: StopScope::DaemonWide,
            stop_scope_evidence: Evidence::ImplementationLimit,
            multi_instance: MultiInstanceSupport::SharedDaemon,
            // CLI: `awww img` talks to `awww-daemon`; `--outputs` retargets within that daemon.
            multi_instance_evidence: Evidence::CliVerified,
            same_target_replacement: SameTargetReplacement::InPlace,
            same_target_replacement_evidence: Evidence::CliVerified,
            cross_output_coexistence: CrossOutputCoexistence::Unknown,
            cross_output_coexistence_evidence: Evidence::Unknown,
        }
    }

    fn ensure_available(&self, _storage: &StorageApi) -> Result<(), WcError> {
        for command in ["awww", "awww-daemon"] {
            if which::which(command).is_err() {
                return Err(WcError::BackendNotFound(command.to_string()));
            }
        }
        Ok(())
    }

    fn stop(&self, runtime: &mut dyn BackendRuntime, _storage: Option<&StorageApi>) {
        runtime.stop_awww();
    }

    fn stop_checked(
        &self,
        runtime: &mut dyn BackendRuntime,
        _storage: Option<&StorageApi>,
    ) -> Result<(), WcError> {
        self.stop(runtime, None);
        // Verify via ProcessIo socket probe (not live pgrep) so fakes stay hermetic.
        if matches!(runtime.awww_socket_ready(), AwwwReadiness::Ready) {
            return Err(WcError::Other(
                "awww socket still answers query after stop".into(),
            ));
        }
        Ok(())
    }
}

impl BackendDriver for MpvpaperDriver {
    fn backend(&self) -> Backend {
        Backend::Mpvpaper
    }

    fn capability(&self) -> BackendCapability {
        BackendCapability {
            backend: Backend::Mpvpaper,
            output_target_mode: OutputTargetMode::SingleOutputPerProcess,
            output_target_evidence: Evidence::CliVerified,
            all_displays: AllDisplaysTargeting::OneProcessPerOutput,
            all_displays_evidence: Evidence::CliVerified,
            stop_scope: StopScope::AllMatchingProcesses,
            stop_scope_evidence: Evidence::ImplementationLimit,
            multi_instance: MultiInstanceSupport::SeparateProcessesUnverified,
            multi_instance_evidence: Evidence::Unverified,
            // One process per output: replacing without Stop would leave a stale process.
            same_target_replacement: SameTargetReplacement::StopThenApply,
            same_target_replacement_evidence: Evidence::ImplementationLimit,
            cross_output_coexistence: CrossOutputCoexistence::Unknown,
            cross_output_coexistence_evidence: Evidence::Unknown,
        }
    }

    fn ensure_available(&self, _storage: &StorageApi) -> Result<(), WcError> {
        if which::which("mpvpaper").is_err() {
            return Err(WcError::BackendNotFound("mpvpaper".to_string()));
        }
        Ok(())
    }

    fn stop(&self, runtime: &mut dyn BackendRuntime, _storage: Option<&StorageApi>) {
        runtime.stop_mpvpaper();
    }

    fn stop_checked(
        &self,
        runtime: &mut dyn BackendRuntime,
        _storage: Option<&StorageApi>,
    ) -> Result<(), WcError> {
        self.stop(runtime, None);
        wait_for_mpvpaper_stopped_with(|| runtime.mpvpaper_pids(), std::thread::sleep)
    }
}

impl BackendDriver for LweDriver {
    fn backend(&self) -> Backend {
        Backend::LinuxWallpaperEngine
    }

    fn capability(&self) -> BackendCapability {
        BackendCapability {
            backend: Backend::LinuxWallpaperEngine,
            output_target_mode: OutputTargetMode::RepeatedScreenRootPairs,
            output_target_evidence: Evidence::CliVerified,
            all_displays: AllDisplaysTargeting::SingleProcessMultiOutput,
            all_displays_evidence: Evidence::CliVerified,
            // stop() clears the tracked PGID then residual-pkills all matching processes.
            stop_scope: StopScope::AllMatchingProcesses,
            stop_scope_evidence: Evidence::ImplementationLimit,
            multi_instance: MultiInstanceSupport::SingleProcessUnverified,
            multi_instance_evidence: Evidence::Unverified,
            // Apply path replaces the managed tracked process as part of apply.
            same_target_replacement: SameTargetReplacement::ManagedHandoff,
            same_target_replacement_evidence: Evidence::ImplementationLimit,
            cross_output_coexistence: CrossOutputCoexistence::Unknown,
            cross_output_coexistence_evidence: Evidence::Unknown,
        }
    }

    fn ensure_available(&self, storage: &StorageApi) -> Result<(), WcError> {
        let config =
            crate::linux_wallpaperengine::LinuxWallpaperEngineConfig::from_storage(storage);
        if !config.enabled {
            return Err(WcError::Other(
                "linux-wallpaperengine is disabled; enable it in Wallpaper settings before applying a scene"
                    .into(),
            ));
        }
        crate::linux_wallpaperengine::ensure_binary_available(&config)
    }

    fn stop(&self, runtime: &mut dyn BackendRuntime, storage: Option<&StorageApi>) {
        runtime.stop_lwe(storage);
    }

    fn stop_checked(
        &self,
        runtime: &mut dyn BackendRuntime,
        storage: Option<&StorageApi>,
    ) -> Result<(), WcError> {
        self.stop(runtime, storage);
        // Storage pid tracking is the hermetic post-condition for fakes; live
        // process probes stay in SystemBackendRuntime stop for production paths.
        if let Some(storage) = storage {
            let pid = storage.config_get("linux_wallpaperengine_pid", "");
            if !pid.trim().is_empty() {
                return Err(WcError::Other(format!(
                    "linux-wallpaperengine pid tracking not cleared after stop: {pid}"
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn driver_for_covers_supported_backends_only() {
        assert!(driver_for(Backend::Awww).is_some());
        assert!(driver_for(Backend::Mpvpaper).is_some());
        assert!(driver_for(Backend::LinuxWallpaperEngine).is_some());
        assert!(driver_for(Backend::Unsupported).is_none());
    }

    #[test]
    fn persisted_name_maps_legacy_swww_to_awww() {
        let d = driver_for_persisted_name("swww").expect("swww");
        assert_eq!(d.backend(), Backend::Awww);
    }
}
