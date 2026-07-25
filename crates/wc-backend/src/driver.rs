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

    fn prepare(
        &self,
        storage: &StorageApi,
        request: &PrepareApplyRequest<'_>,
        runtime: &mut dyn BackendRuntime,
    ) -> Result<PreparedApply, WcError>;

    fn execute(
        &self,
        storage: &StorageApi,
        prepared: &mut PreparedApply,
        runtime: &mut dyn BackendRuntime,
        reporter: &mut dyn ApplyStageReporter,
    ) -> Result<(), DriverApplyFailure>;

    /// Best-effort stop (legacy / lifecycle plans).
    fn stop(&self, runtime: &mut dyn BackendRuntime, storage: Option<&StorageApi>);

    /// Stop with post-conditions checked (display executor).
    fn stop_checked(
        &self,
        runtime: &mut dyn BackendRuntime,
        storage: Option<&StorageApi>,
    ) -> Result<(), WcError>;
}

pub(crate) struct PrepareApplyRequest<'a> {
    pub path: &'a str,
    pub scope: &'a ExecutionScope,
    pub after_stop: bool,
    pub clear_state_hint: bool,
    pub request_id: Option<&'a str>,
}

/// Fully validated renderer operation. Its variant and command details stay
/// private so orchestration can only execute it through its registered driver.
pub(crate) struct PreparedApply {
    backend: Backend,
    path: String,
    scope: ExecutionScope,
    request_id: Option<String>,
    operation: PreparedOperation,
}

enum PreparedOperation {
    Awww {
        command: Command,
        clear_state_hint: bool,
    },
    Mpvpaper {
        command: Command,
        output: String,
        previous_pids: Vec<u32>,
    },
    LinuxWallpaperEngine {
        project: crate::linux_wallpaperengine::LinuxWallpaperEngineProject,
        outputs: Vec<String>,
    },
    LinuxWallpaperEngineLegacy {
        project: crate::linux_wallpaperengine::LinuxWallpaperEngineProject,
    },
}

impl PreparedApply {
    pub(crate) fn backend(&self) -> Backend {
        self.backend
    }

    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    pub(crate) fn scope(&self) -> &ExecutionScope {
        &self.scope
    }

    pub(crate) fn execute(
        &mut self,
        storage: &StorageApi,
        runtime: &mut dyn BackendRuntime,
        reporter: &mut dyn ApplyStageReporter,
    ) -> Result<(), DriverApplyFailure> {
        driver_for(self.backend)
            .expect("prepared applies always retain a registered backend")
            .execute(storage, self, runtime, reporter)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CleanupOutcome {
    NotRequired,
    VerifiedGlobalStop(Backend),
    UncertainGlobalStop(Backend),
    UncertainTarget,
}

#[derive(Debug)]
pub(crate) struct DriverApplyFailure {
    pub error: WcError,
    pub cleanup: CleanupOutcome,
}

impl DriverApplyFailure {
    fn new(error: WcError) -> Self {
        Self {
            error,
            cleanup: CleanupOutcome::NotRequired,
        }
    }
}

impl From<WcError> for DriverApplyFailure {
    fn from(error: WcError) -> Self {
        Self::new(error)
    }
}

pub(crate) fn prepare_legacy_apply(
    storage: &StorageApi,
    backend: Backend,
    path: &str,
    after_stop: bool,
    clear_state_hint: bool,
    request_id: Option<&str>,
    runtime: &mut dyn BackendRuntime,
) -> Result<PreparedApply, WcError> {
    let Some(backend_driver) = driver_for(backend) else {
        return Err(WcError::UnsupportedFileType(path.to_string()));
    };
    if backend == Backend::LinuxWallpaperEngine {
        runtime.ensure_backend_available(backend, storage)?;
        return Ok(PreparedApply {
            backend,
            path: path.to_string(),
            scope: ExecutionScope::AllDisplays,
            request_id: request_id.map(str::to_string),
            operation: PreparedOperation::LinuxWallpaperEngineLegacy {
                project: crate::linux_wallpaperengine::project_from_path(path)?,
            },
        });
    }
    let scope = if backend == Backend::Mpvpaper {
        ExecutionScope::named(vec![storage.config_get("mpvpaper_output", "*")])?
    } else {
        ExecutionScope::AllDisplays
    };
    backend_driver.prepare(
        storage,
        &PrepareApplyRequest {
            path,
            scope: &scope,
            after_stop,
            clear_state_hint,
            request_id,
        },
        runtime,
    )
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

fn prepare_awww(
    storage: &StorageApi,
    request: &PrepareApplyRequest<'_>,
) -> Result<PreparedOperation, WcError> {
    let path = std::path::Path::new(request.path);
    if !path.is_file() {
        return Err(WcError::NotRegularFile(path.to_path_buf()));
    }
    request.scope.validate()?;
    let fps_raw = storage.config_get("wallpaper_transition_fps", "60");
    let fps = wc_core::config_normalizer::normalize_awww_transition_fps(&fps_raw);
    let resize_raw = storage.config_get("awww_resize", "crop");
    let resize = normalize_awww_resize(&resize_raw);
    let mut command = if request.after_stop {
        build_awww_instant_command_for_scope(request.path, resize, &fps, request.scope)?
    } else {
        let transition_raw = storage.config_get("awww_transition_type", "fade");
        let transition_type = normalize_awww_transition_type(&transition_raw);
        let duration_raw = storage.config_get("awww_transition_duration", "1");
        let duration =
            wc_core::config_normalizer::normalize_awww_transition_duration(&duration_raw);
        build_awww_img_command_for_scope(
            request.path,
            resize,
            transition_type,
            &duration,
            &fps,
            request.scope,
        )?
    };
    if !request.after_stop {
        command.arg("--filter").arg("Lanczos3");
    }
    Ok(PreparedOperation::Awww {
        command,
        clear_state_hint: request.clear_state_hint,
    })
}

fn prepare_mpvpaper(
    storage: &StorageApi,
    request: &PrepareApplyRequest<'_>,
    runtime: &mut dyn BackendRuntime,
) -> Result<PreparedOperation, WcError> {
    let path = std::path::Path::new(request.path);
    if !path.is_file() {
        return Err(WcError::NotRegularFile(path.to_path_buf()));
    }
    request.scope.validate()?;
    let outputs = request.scope.named_outputs().ok_or_else(|| {
        WcError::Other("mpvpaper apply requires a named single-output execution scope".into())
    })?;
    if outputs.len() != 1 {
        return Err(WcError::Other(format!(
            "mpvpaper apply expects exactly one output per invocation, got {}",
            outputs.len()
        )));
    }
    let output = outputs[0].clone();
    let options_raw = storage.config_get("mpvpaper_options", "--loop-file=inf --panscan=1.0");
    let options = normalize_mpvpaper_options(&options_raw);
    let command = build_mpvpaper_launch_command_for_output(options, &output, request.path)?;
    let previous_pids = runtime.mpvpaper_pids()?;
    Ok(PreparedOperation::Mpvpaper {
        command,
        output,
        previous_pids,
    })
}

fn prepare_lwe(request: &PrepareApplyRequest<'_>) -> Result<PreparedOperation, WcError> {
    request.scope.validate()?;
    let outputs = match request.scope {
        ExecutionScope::AllDisplays => {
            return Err(WcError::Other(
                "linux-wallpaperengine apply requires explicit named outputs".into(),
            ));
        }
        ExecutionScope::Named(outputs) => outputs.clone(),
    };
    let project = crate::linux_wallpaperengine::project_from_path(request.path)?;
    Ok(PreparedOperation::LinuxWallpaperEngine { project, outputs })
}

fn execute_mpvpaper(
    prepared: &mut PreparedApply,
    runtime: &mut dyn BackendRuntime,
) -> Result<(), DriverApplyFailure> {
    let path = prepared.path.clone();
    let PreparedOperation::Mpvpaper {
        command,
        output,
        previous_pids,
    } = &mut prepared.operation
    else {
        return Err(
            WcError::Other("mpvpaper driver received another backend's apply".into()).into(),
        );
    };
    let status = match runtime.command_status(command) {
        Ok(status) if status.success() => status,
        Ok(_) => {
            return Err(cleanup_failed_mpvpaper_start(
                runtime,
                previous_pids,
                output,
                &path,
                WcError::Other("mpvpaper failed to apply wallpaper".into()),
            ));
        }
        Err(error) => {
            return Err(cleanup_failed_mpvpaper_start(
                runtime,
                previous_pids,
                output,
                &path,
                WcError::Other(format!("mpvpaper failed: {error}")),
            ));
        }
    };
    let _ = status;
    let pid = match runtime.wait_for_mpvpaper_ready(previous_pids, output, &path) {
        Ok(pid) => pid,
        Err(error) => return Err(cleanup_started_mpvpaper(runtime, error)),
    };
    match runtime.mpvpaper_pid_running(pid) {
        Ok(true) => Ok(()),
        Ok(false) => Err(cleanup_started_mpvpaper(
            runtime,
            WcError::Other("mpvpaper renderer exited before startup settled".into()),
        )),
        Err(error) => Err(cleanup_started_mpvpaper(runtime, error)),
    }
}

fn cleanup_failed_mpvpaper_start(
    runtime: &mut dyn BackendRuntime,
    previous_pids: &[u32],
    output: &str,
    path: &str,
    original_error: WcError,
) -> DriverApplyFailure {
    match runtime.cleanup_failed_mpvpaper_launch(previous_pids, output, path) {
        Ok(()) => DriverApplyFailure::new(original_error),
        Err(cleanup_error) => DriverApplyFailure {
            error: WcError::Other(format!(
                "{original_error}; failed-launch mpvpaper cleanup could not be verified: \
                 {cleanup_error}"
            )),
            cleanup: CleanupOutcome::UncertainTarget,
        },
    }
}

fn cleanup_started_mpvpaper(
    runtime: &mut dyn BackendRuntime,
    original_error: WcError,
) -> DriverApplyFailure {
    match MPVPAPER_DRIVER.stop_checked(runtime, None) {
        Ok(()) => DriverApplyFailure {
            error: original_error,
            cleanup: CleanupOutcome::VerifiedGlobalStop(Backend::Mpvpaper),
        },
        Err(cleanup_error) => DriverApplyFailure {
            error: WcError::Other(format!(
                "{original_error}; mpvpaper cleanup could not be verified: {cleanup_error}"
            )),
            cleanup: CleanupOutcome::UncertainGlobalStop(Backend::Mpvpaper),
        },
    }
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

    fn prepare(
        &self,
        storage: &StorageApi,
        request: &PrepareApplyRequest<'_>,
        runtime: &mut dyn BackendRuntime,
    ) -> Result<PreparedApply, WcError> {
        runtime.ensure_backend_available(self.backend(), storage)?;
        Ok(PreparedApply {
            backend: self.backend(),
            path: request.path.to_string(),
            scope: request.scope.clone(),
            request_id: request.request_id.map(str::to_string),
            operation: prepare_awww(storage, request)?,
        })
    }

    fn execute(
        &self,
        _storage: &StorageApi,
        prepared: &mut PreparedApply,
        runtime: &mut dyn BackendRuntime,
        reporter: &mut dyn ApplyStageReporter,
    ) -> Result<(), DriverApplyFailure> {
        let request_id = prepared.request_id.as_deref();
        let PreparedOperation::Awww {
            command,
            clear_state_hint,
        } = &mut prepared.operation
        else {
            return Err(
                WcError::Other("awww driver received another backend's apply".into()).into(),
            );
        };
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
        if *clear_state_hint {
            clear_awww_state_hint(runtime);
        }
        let output = runtime
            .command_output(command)
            .map_err(|error| WcError::Other(format!("awww failed: {error}")))?;
        if !output.status.success() {
            return Err(awww_apply_status_error(&output).into());
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

    fn prepare(
        &self,
        storage: &StorageApi,
        request: &PrepareApplyRequest<'_>,
        runtime: &mut dyn BackendRuntime,
    ) -> Result<PreparedApply, WcError> {
        runtime.ensure_backend_available(self.backend(), storage)?;
        Ok(PreparedApply {
            backend: self.backend(),
            path: request.path.to_string(),
            scope: request.scope.clone(),
            request_id: request.request_id.map(str::to_string),
            operation: prepare_mpvpaper(storage, request, runtime)?,
        })
    }

    fn execute(
        &self,
        _storage: &StorageApi,
        prepared: &mut PreparedApply,
        runtime: &mut dyn BackendRuntime,
        _reporter: &mut dyn ApplyStageReporter,
    ) -> Result<(), DriverApplyFailure> {
        execute_mpvpaper(prepared, runtime)
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

    fn prepare(
        &self,
        storage: &StorageApi,
        request: &PrepareApplyRequest<'_>,
        runtime: &mut dyn BackendRuntime,
    ) -> Result<PreparedApply, WcError> {
        runtime.ensure_backend_available(self.backend(), storage)?;
        Ok(PreparedApply {
            backend: self.backend(),
            path: request.path.to_string(),
            scope: request.scope.clone(),
            request_id: request.request_id.map(str::to_string),
            operation: prepare_lwe(request)?,
        })
    }

    fn execute(
        &self,
        storage: &StorageApi,
        prepared: &mut PreparedApply,
        runtime: &mut dyn BackendRuntime,
        reporter: &mut dyn ApplyStageReporter,
    ) -> Result<(), DriverApplyFailure> {
        let request_id = prepared.request_id.as_deref();
        apply_stage::report_stage(reporter, apply_stage::ApplyStage::StartLwe, request_id);
        match &prepared.operation {
            PreparedOperation::LinuxWallpaperEngine { project, outputs } => {
                runtime.apply_lwe_to_outputs(storage, project, outputs)?;
                apply_stage::report_stage(
                    reporter,
                    apply_stage::ApplyStage::WaitRendererAlive,
                    request_id,
                );
            }
            PreparedOperation::LinuxWallpaperEngineLegacy { project } => {
                apply_stage::report_stage(
                    reporter,
                    apply_stage::ApplyStage::WaitRendererAlive,
                    request_id,
                );
                crate::linux_wallpaperengine::apply(storage, project.clone())?;
            }
            _ => {
                return Err(
                    WcError::Other("LWE driver received another backend's apply".into()).into(),
                );
            }
        }
        Ok(())
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
