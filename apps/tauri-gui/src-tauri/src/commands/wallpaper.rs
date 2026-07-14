use super::common::{
    fail, ok, storage, BackendStatusDto, CommandErrorDto, CommandResult, StatusDto, WeDebugInfoDto,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use tauri::Emitter;
use wc_backend::runtime::BackendRuntime;
use wc_core::types::Backend;
use wc_storage::sqlite::{DisplayStateRow, DisplayStateTarget, ALL_DISPLAYS_TARGET_KEY};
use wc_storage::StorageApi;

/// Connected Wayland/X output discovered from the compositor/daemon.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DisplayDto {
    pub name: String,
}

/// Typed display-list bridge payload.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DisplayListDto {
    pub outputs: Vec<DisplayDto>,
}

/// Typed display-state row for the bridge.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DisplayStateDto {
    pub target_key: String,
    pub kind: String,
    pub output: Option<String>,
    pub wallpaper_path: String,
    pub backend: String,
    pub updated_at: String,
}

/// Read-only renderer evidence for one connected output.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeWallpaperObservationDto {
    pub output: String,
    pub wallpaper_path: Option<String>,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Read-only installation/readiness snapshot for every supported renderer.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RendererStatusesDto {
    pub awww: BackendStatusDto,
    pub mpvpaper: BackendStatusDto,
    pub linux_wallpaper_engine: BackendStatusDto,
}

/// Targeted apply request. Omitting `target` means All Displays.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TargetedApplyRequestDto {
    #[serde(default = "default_targeted_apply_kind")]
    pub kind: String,
    pub path: String,
    pub target: Option<String>,
    pub request_id: Option<String>,
}

fn default_targeted_apply_kind() -> String {
    "apply".into()
}

/// Targeted restore request. Omitting `outputs` discovers connected displays.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct TargetedRestoreRequestDto {
    pub outputs: Option<Vec<String>>,
}

static APPLY_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static APPLY_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn with_renderer_state_lock<T>(
    lock: &Mutex<()>,
    operation: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let _guard = lock
        .lock()
        .map_err(|_| "Renderer state lock is poisoned.".to_string())?;
    operation()
}

struct TauriStageReporter {
    app: tauri::AppHandle,
    context: Arc<Mutex<wc_app::ApplyStageContext>>,
}

impl TauriStageReporter {
    fn new(app: tauri::AppHandle) -> Self {
        Self {
            app,
            context: Arc::new(Mutex::new(wc_app::ApplyStageContext::default())),
        }
    }

    fn context_handle(&self) -> Arc<Mutex<wc_app::ApplyStageContext>> {
        self.context.clone()
    }
}

impl wc_backend::apply_stage::ApplyStageReporter for TauriStageReporter {
    fn emit(&mut self, event: wc_backend::apply_stage::ApplyStageEvent) {
        let ctx = self.context.lock().unwrap().clone();
        let _ = self.app.emit(
            "wc-apply-stage",
            serde_json::json!({
                "requestId": event.request_id,
                "stage": format!("{:?}", event.stage),
                "label": wc_app::apply_stage_label(&event.stage),
                "detail": wc_app::apply_stage_detail(&event.stage, &ctx),
            }),
        );
    }
}

#[tauri::command]
pub async fn status() -> Result<StatusDto, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let s = storage()?;
        Ok(StatusDto {
            config_dir: s.cd.path.to_string_lossy().to_string(),
            current: s
                .current_read()
                .map_err(|e| e.to_string())?
                .unwrap_or_default(),
            last_backend: s
                .last_backend_read()
                .map_err(|e| e.to_string())?
                .unwrap_or_default(),
            source_count: s.sources_list().map_err(|e| e.to_string())?.len(),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn linux_wallpaperengine_status() -> Result<BackendStatusDto, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let s = storage()?;
        let config =
            wc_backend::linux_wallpaperengine::LinuxWallpaperEngineConfig::from_storage(s);
        let st = wc_backend::linux_wallpaperengine::status(&config);
        let mut detail = st.detail;
        if config.target_mode == "auto" {
            let wayland = std::env::var("WAYLAND_DISPLAY").is_ok()
                || std::env::var("XDG_SESSION_TYPE")
                    .map(|v| v == "wayland")
                    .unwrap_or(false);
            if wayland {
                let warning = "Warning: Wayland detected. Recommend setting target_mode=screen-root and target=<your output name> for stable scene rendering.";
                detail = Some(match detail {
                    Some(d) => format!("{}\n{}", d, warning),
                    None => warning.to_string(),
                });
            }
        }
        Ok(BackendStatusDto {
            available: st.available,
            path: st.path,
            message: st.message,
            detail,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn renderer_statuses() -> Result<RendererStatusesDto, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let s = storage()?;
        let mut runtime = wc_backend::runtime::SystemBackendRuntime;
        Ok(renderer_statuses_with(|backend| {
            runtime.ensure_backend_available(backend, s)
        }))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn apply(app: tauri::AppHandle, path: String) -> CommandResult {
    let seq = APPLY_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    tauri::async_runtime::spawn_blocking(move || match storage() {
        Ok(s) => {
            let service = wc_app::AppService::from_config_dir(wc_core::ConfigDir {
                path: s.cd.path.clone(),
            });
            let request = wc_app::ApplyRequest {
                kind: wc_app::ApplyRequestKind::Apply,
                path: path.clone(),
                request_id: None,
            };
            execute_and_format_result(&app, &service, request, seq)
        }
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn apply_action(
    app: tauri::AppHandle,
    request: super::common::ApplyRequestDto,
) -> CommandResult {
    let seq = APPLY_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    tauri::async_runtime::spawn_blocking(move || match storage() {
        Ok(s) => {
            let service = wc_app::AppService::from_config_dir(wc_core::ConfigDir {
                path: s.cd.path.clone(),
            });
            let request = match apply_request_from_dto(request) {
                Ok(r) => r,
                Err(err) => return command_error_from_app_error(err),
            };
            execute_and_format_result(&app, &service, request, seq)
        }
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

/// Returns a stale-apply result when the request sequence has been superseded,
/// otherwise `None` so the caller proceeds with the real apply side effects.
/// Checking this after acquiring the apply lock ensures a request that became
/// stale while waiting for the lock is still skipped before backend stop/start.
fn apply_stale_guard(seq: u64, current_seq: u64) -> Option<CommandResult> {
    if seq != current_seq {
        Some(stale_apply_result())
    } else {
        None
    }
}

fn execute_and_format_result(
    app: &tauri::AppHandle,
    service: &wc_app::AppService,
    request: wc_app::ApplyRequest,
    seq: u64,
) -> CommandResult {
    let _guard = match APPLY_LOCK.lock() {
        Ok(guard) => guard,
        Err(_) => {
            return CommandResult {
                success: false,
                stdout: String::new(),
                stderr: "Apply lock is poisoned.".into(),
                exit_code: 1,
                error: Some(CommandErrorDto {
                    kind: "apply_lock_poisoned".into(),
                    message: "Wallpaper apply is temporarily unavailable because the apply lock is poisoned.".into(),
                    detail: None,
                    recoverable: true,
                    suggestion: Some("Restart Wallpaper Console and try again.".into()),
                }),
            };
        }
    };

    let current_seq = APPLY_SEQUENCE.load(std::sync::atomic::Ordering::SeqCst);
    if let Some(stale) = apply_stale_guard(seq, current_seq) {
        return stale;
    }

    let reporter = TauriStageReporter::new(app.clone());
    let context = reporter.context_handle();
    let options = wc_app::ApplyExecutionOptions {
        stage_reporter: Some(Box::new(reporter)),
        on_target_resolved: Some(Box::new(move |ctx| {
            *context.lock().unwrap() = ctx;
        })),
    };

    match service.execute_apply_request_with_options(request, options) {
        Ok(result) => {
            if let Err(err) =
                service.commit_legacy_apply_display_state(&result.applied_path, result.backend)
            {
                return command_error_from_app_error(err);
            }
            let dto = super::common::ApplyResultDto {
                request_id: result.request_id,
                applied_path: result.applied_path.clone(),
                state_path: result.state_path,
                backend: result.backend.as_str().to_string(),
                file_type: result.file_type.as_str().to_string(),
                preview: result.preview,
                applied_outputs: None,
            };
            match serde_json::to_string(&dto) {
                Ok(json) => ok(json),
                Err(e) => fail(e.to_string()),
            }
        }
        Err(err) => command_error_from_app_error(err),
    }
}

fn stale_apply_result() -> CommandResult {
    CommandResult {
        success: false,
        stdout: String::new(),
        stderr: "Apply request was superseded by a newer request.".into(),
        exit_code: 1,
        error: Some(CommandErrorDto {
            kind: "stale_apply_request".into(),
            message: "This apply request was superseded by a newer request.".into(),
            detail: None,
            recoverable: true,
            suggestion: None,
        }),
    }
}

fn apply_request_from_dto(
    dto: super::common::ApplyRequestDto,
) -> Result<wc_app::ApplyRequest, wc_app::AppError> {
    apply_request_from_parts(&dto.kind, dto.path, dto.request_id)
}

fn apply_request_from_parts(
    raw_kind: &str,
    path: String,
    request_id: Option<String>,
) -> Result<wc_app::ApplyRequest, wc_app::AppError> {
    let kind = match raw_kind {
        "apply" => wc_app::ApplyRequestKind::Apply,
        "retry_backend_apply" => wc_app::ApplyRequestKind::RetryBackendApply,
        "apply_preview" => wc_app::ApplyRequestKind::ApplyPreview,
        other => {
            return Err(wc_app::AppError {
                code: "invalid_apply_action".into(),
                message: format!("Unsupported apply action: {}", other),
                detail: None,
                recoverable: true,
                suggestion: None,
            });
        }
    };
    Ok(wc_app::ApplyRequest {
        kind,
        path,
        request_id,
    })
}

fn command_error_from_app_error(err: wc_app::AppError) -> CommandResult {
    CommandResult {
        success: false,
        stdout: String::new(),
        stderr: err.message.clone(),
        exit_code: 1,
        error: Some(CommandErrorDto {
            kind: err.code,
            message: err.message,
            detail: err.detail,
            recoverable: err.recoverable,
            suggestion: err.suggestion,
        }),
    }
}

fn stop_with_storage(s: &StorageApi) -> CommandResult {
    match wc_backend::stop_all_backends(Some(s)) {
        Ok(()) => match s.runtime_state_clear() {
            Ok(()) => ok("Stopped wallpaper backends."),
            Err(e) => fail(e.to_string()),
        },
        Err(e) => fail(e.to_string()),
    }
}

#[tauri::command]
pub async fn stop() -> CommandResult {
    tauri::async_runtime::spawn_blocking(|| match storage() {
        Ok(s) => stop_with_storage(s),
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn restore() -> CommandResult {
    tauri::async_runtime::spawn_blocking(|| match storage() {
        Ok(s) => match wc_backend::restore_clean(s) {
            Ok(()) => ok("Restored wallpaper."),
            Err(e) => fail(e.to_string()),
        },
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn we_clear_backend_error(path: String) -> CommandResult {
    tauri::async_runtime::spawn_blocking(move || {
        match wc_storage::we_compat::clear_failure(&path) {
            Ok(()) => ok("Cleared backend error."),
            Err(e) => fail(e.to_string()),
        }
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn we_debug_info() -> Result<WeDebugInfoDto, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let s = storage()?;
        let log_path = s.cd.path.join("linux-wallpaperengine-last.log");
        Ok(WeDebugInfoDto {
            last_command_line: s.config_get("lwe_last_command_line", ""),
            last_target_config: s.config_get("lwe_last_target_config", ""),
            last_stderr: s.config_get("lwe_last_stderr", ""),
            last_exit_status: s.config_get("lwe_last_exit_status", ""),
            log_path: log_path.to_string_lossy().to_string(),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn displays_list() -> Result<DisplayListDto, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let outputs = discover_connected_outputs()?;
        Ok(DisplayListDto {
            outputs: outputs
                .into_iter()
                .map(|name| DisplayDto { name })
                .collect(),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn display_state_list() -> Result<Vec<DisplayStateDto>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let s = storage()?;
        let rows = s.display_state_list().map_err(|e| e.to_string())?;
        Ok(rows.iter().map(display_state_row_dto).collect())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Reconcile saved display assignments with renderer processes without
/// starting, stopping, or otherwise mutating a renderer.
#[tauri::command]
pub async fn runtime_wallpaper_observations() -> Result<Vec<RuntimeWallpaperObservationDto>, String>
{
    tauri::async_runtime::spawn_blocking(|| -> Result<_, String> {
        // Display discovery may invoke compositor CLIs. Keep it outside the
        // renderer lock so a wedged compositor probe cannot block applies.
        let outputs = discover_connected_outputs()?;
        with_renderer_state_lock(&APPLY_LOCK, || {
            let storage = storage()?;
            let rows = storage
                .display_state_list()
                .map_err(|error| error.to_string())?;
            Ok(runtime_observation_dtos_with(
                &outputs,
                &rows,
                wc_backend::runtime_observation::observe_runtime_wallpapers,
            ))
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn apply_to_display(
    app: tauri::AppHandle,
    request: TargetedApplyRequestDto,
) -> CommandResult {
    let seq = APPLY_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    tauri::async_runtime::spawn_blocking(move || match storage() {
        Ok(s) => {
            let apply_request = match apply_request_from_parts(
                &request.kind,
                request.path.clone(),
                request.request_id.clone(),
            ) {
                Ok(value) => value,
                Err(err) => return command_error_from_app_error(err),
            };
            let target = match parse_display_target(request.target.as_deref()) {
                Ok(t) => t,
                Err(err) => {
                    return command_error_from_app_error(wc_app::AppError {
                        code: "invalid_display_target".into(),
                        message: err,
                        detail: None,
                        recoverable: true,
                        suggestion: Some(
                            "Pass a concrete output name, or omit target for All Displays.".into(),
                        ),
                    });
                }
            };
            let known_outputs = match discover_connected_outputs() {
                Ok(o) => o,
                Err(err) => {
                    return command_error_from_app_error(wc_app::AppError {
                        code: "display_discovery_failed".into(),
                        message: err,
                        detail: None,
                        recoverable: true,
                        suggestion: Some(
                            "Verify that Wallpaper Console can access the active compositor session."
                                .into(),
                        ),
                    });
                }
            };
            let service = wc_app::AppService::from_config_dir(wc_core::ConfigDir {
                path: s.cd.path.clone(),
            });
            execute_display_apply_and_format(
                &app,
                &service,
                apply_request,
                target,
                &known_outputs,
                seq,
            )
        }
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn restore_displays(request: Option<TargetedRestoreRequestDto>) -> CommandResult {
    tauri::async_runtime::spawn_blocking(move || match storage() {
        Ok(s) => {
            let known_outputs = match resolve_restore_outputs(request.as_ref()) {
                Ok(o) => o,
                Err(err) => {
                    return command_error_from_app_error(wc_app::AppError {
                        code: "display_discovery_failed".into(),
                        message: err,
                        detail: None,
                        recoverable: true,
                        suggestion: Some(
                            "Verify that Wallpaper Console can access the active compositor session."
                                .into(),
                        ),
                    });
                }
            };
            let service = wc_app::AppService::from_config_dir(wc_core::ConfigDir {
                path: s.cd.path.clone(),
            });
            match service.restore_displays(&known_outputs) {
                Ok(()) => ok("Restored display wallpapers."),
                Err(err) => command_error_from_app_error(err),
            }
        }
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

fn resolve_restore_outputs(
    request: Option<&TargetedRestoreRequestDto>,
) -> Result<Vec<String>, String> {
    resolve_restore_outputs_with(request, discover_connected_outputs)
}

fn resolve_restore_outputs_with<F>(
    request: Option<&TargetedRestoreRequestDto>,
    discover: F,
) -> Result<Vec<String>, String>
where
    F: FnOnce() -> Result<Vec<String>, String>,
{
    let discovered_outputs = discover()?;
    validate_known_outputs(&discovered_outputs)?;

    if let Some(TargetedRestoreRequestDto {
        outputs: Some(outputs),
    }) = request
    {
        validate_known_outputs(outputs)?;
        let explicit: HashSet<_> = outputs.iter().map(String::as_str).collect();
        let discovered: HashSet<_> = discovered_outputs.iter().map(String::as_str).collect();
        if explicit != discovered {
            return Err(
                "explicit display outputs must exactly match discovered connected outputs".into(),
            );
        }
    }

    Ok(discovered_outputs)
}

fn validate_known_outputs(outputs: &[String]) -> Result<(), String> {
    let mut seen = HashSet::new();
    for output in outputs {
        if output.trim().is_empty() {
            return Err(format!("blank display output: {output:?}"));
        }
        if !seen.insert(output.as_str()) {
            return Err(format!("duplicate display output: {output}"));
        }
    }
    Ok(())
}

fn execute_display_apply_and_format(
    app: &tauri::AppHandle,
    service: &wc_app::AppService,
    request: wc_app::ApplyRequest,
    target: wc_app::DisplayTarget,
    known_outputs: &[String],
    seq: u64,
) -> CommandResult {
    let _guard = match APPLY_LOCK.lock() {
        Ok(guard) => guard,
        Err(_) => {
            return CommandResult {
                success: false,
                stdout: String::new(),
                stderr: "Apply lock is poisoned.".into(),
                exit_code: 1,
                error: Some(CommandErrorDto {
                    kind: "apply_lock_poisoned".into(),
                    message: "Wallpaper apply is temporarily unavailable because the apply lock is poisoned.".into(),
                    detail: None,
                    recoverable: true,
                    suggestion: Some("Restart Wallpaper Console and try again.".into()),
                }),
            };
        }
    };

    let current_seq = APPLY_SEQUENCE.load(std::sync::atomic::Ordering::SeqCst);
    if let Some(stale) = apply_stale_guard(seq, current_seq) {
        return stale;
    }

    let mut stage_reporter = TauriStageReporter::new(app.clone());
    let stage_context = stage_reporter.context_handle();
    let mut runtime = wc_backend::runtime::SystemBackendRuntime;
    let opts = wc_app::display_apply::DisplayApplyRuntimeOpts {
        on_target_resolved: Some(Box::new(move |context| {
            *stage_context.lock().unwrap() = context;
        })),
        ..Default::default()
    };

    match service.execute_apply_request_to_display_with_runtime(
        request,
        target,
        known_outputs,
        &mut runtime,
        &mut stage_reporter,
        opts,
    ) {
        Ok(result) => {
            let dto = super::common::ApplyResultDto {
                request_id: result.request_id,
                applied_path: result.applied_path,
                state_path: result.state_path,
                backend: result.backend.as_str().to_string(),
                file_type: result.file_type.as_str().to_string(),
                preview: result.preview,
                applied_outputs: Some(result.applied_outputs),
            };
            match serde_json::to_string(&dto) {
                Ok(json) => ok(json),
                Err(e) => fail(e.to_string()),
            }
        }
        Err(err) => command_error_from_app_error(err),
    }
}

fn display_state_row_dto(row: &DisplayStateRow) -> DisplayStateDto {
    match &row.target {
        DisplayStateTarget::AllDisplays => DisplayStateDto {
            target_key: ALL_DISPLAYS_TARGET_KEY.to_string(),
            kind: "allDisplays".into(),
            output: None,
            wallpaper_path: row.wallpaper_path.clone(),
            backend: row.backend.clone(),
            updated_at: row.updated_at.clone(),
        },
        DisplayStateTarget::Output(name) => DisplayStateDto {
            target_key: name.clone(),
            kind: "output".into(),
            output: Some(name.clone()),
            wallpaper_path: row.wallpaper_path.clone(),
            backend: row.backend.clone(),
            updated_at: row.updated_at.clone(),
        },
    }
}

fn runtime_observation_dto(
    observation: wc_backend::runtime_observation::RuntimeWallpaperObservation,
) -> RuntimeWallpaperObservationDto {
    let status = match observation.status {
        wc_backend::runtime_observation::RuntimeObservationStatus::Confirmed => "confirmed",
        wc_backend::runtime_observation::RuntimeObservationStatus::Unknown => "unknown",
    };
    RuntimeWallpaperObservationDto {
        output: observation.output,
        wallpaper_path: observation.wallpaper_path,
        status: status.into(),
        reason: observation.reason,
    }
}

fn runtime_observation_dtos_with<F>(
    outputs: &[String],
    rows: &[DisplayStateRow],
    observe: F,
) -> Vec<RuntimeWallpaperObservationDto>
where
    F: FnOnce(
        &[String],
        &[DisplayStateRow],
    ) -> Vec<wc_backend::runtime_observation::RuntimeWallpaperObservation>,
{
    observe(outputs, rows)
        .into_iter()
        .map(runtime_observation_dto)
        .collect()
}

fn renderer_status_from_result(
    backend: Backend,
    result: Result<(), wc_core::error::WcError>,
) -> BackendStatusDto {
    let name = backend.as_str();
    match result {
        Ok(()) => BackendStatusDto {
            available: true,
            path: None,
            message: format!("{name} is installed."),
            detail: None,
        },
        Err(error) => BackendStatusDto {
            available: false,
            path: None,
            message: format!("{name} is unavailable."),
            detail: Some(error.to_string()),
        },
    }
}

fn renderer_statuses_with<F>(mut probe: F) -> RendererStatusesDto
where
    F: FnMut(Backend) -> Result<(), wc_core::error::WcError>,
{
    RendererStatusesDto {
        awww: renderer_status_from_result(Backend::Awww, probe(Backend::Awww)),
        mpvpaper: renderer_status_from_result(Backend::Mpvpaper, probe(Backend::Mpvpaper)),
        linux_wallpaper_engine: renderer_status_from_result(
            Backend::LinuxWallpaperEngine,
            probe(Backend::LinuxWallpaperEngine),
        ),
    }
}

fn parse_display_target(raw: Option<&str>) -> Result<wc_app::DisplayTarget, String> {
    let Some(raw) = raw else {
        return Ok(wc_app::DisplayTarget::AllDisplays);
    };
    let value = raw.trim();
    if value.is_empty() {
        return Err("display target must not be blank".into());
    }
    if value.eq_ignore_ascii_case("all")
        || value.eq_ignore_ascii_case("all displays")
        || value == ALL_DISPLAYS_TARGET_KEY
    {
        return Ok(wc_app::DisplayTarget::AllDisplays);
    }
    Ok(wc_app::DisplayTarget::Output(value.to_string()))
}

/// Discover connected outputs via the active compositor, with awww retained
/// only as a compatibility fallback. Never invents names.
fn discover_connected_outputs() -> Result<Vec<String>, String> {
    wc_app::discover_connected_outputs().map_err(|error| {
        error
            .detail
            .map(|detail| format!("{} ({detail})", error.message))
            .unwrap_or(error.message)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wc_backend::apply_stage::ApplyStage;

    fn insert_history(storage: &StorageApi, path: &str, backend: &str) {
        let conn = wc_storage::sqlite::open_runtime_connection(&storage.cd).unwrap();
        conn.execute(
            "INSERT INTO history (path, backend) VALUES (?1, ?2)",
            [path, backend],
        )
        .unwrap();
    }

    fn history_rows(storage: &StorageApi) -> Vec<(String, String)> {
        let conn = wc_storage::sqlite::open_runtime_connection(&storage.cd).unwrap();
        let mut stmt = conn
            .prepare("SELECT path, backend FROM history ORDER BY id")
            .unwrap();
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    }

    #[test]
    fn stale_apply_result_returns_structured_error() {
        let r = stale_apply_result();
        assert!(!r.success);
        assert_eq!(r.error.unwrap().kind, "stale_apply_request");
    }

    #[test]
    fn apply_lock_poison_error_is_structured() {
        let err = CommandResult {
            success: false,
            stdout: String::new(),
            stderr: "Apply lock is poisoned.".into(),
            exit_code: 1,
            error: Some(CommandErrorDto {
                kind: "apply_lock_poisoned".into(),
                message:
                    "Wallpaper apply is temporarily unavailable because the apply lock is poisoned."
                        .into(),
                detail: None,
                recoverable: true,
                suggestion: Some("Restart Wallpaper Console and try again.".into()),
            }),
        };
        assert!(!err.success);
        assert_eq!(err.error.unwrap().kind, "apply_lock_poisoned");
    }

    #[test]
    fn apply_stale_guard_skips_superseded_request_without_side_effects() {
        let stale = apply_stale_guard(9, 10);
        assert!(stale.is_some(), "superseded seq should be guarded");
        assert_eq!(stale.unwrap().error.unwrap().kind, "stale_apply_request");

        let fresh = apply_stale_guard(10, 10);
        assert!(fresh.is_none(), "current seq should not be guarded");
    }

    #[test]
    fn apply_stage_payload_includes_request_id_and_labels() {
        let ctx = wc_app::ApplyStageContext {
            preview: true,
            backend: wc_core::types::Backend::Awww,
        };
        let event = wc_backend::apply_stage::ApplyStageEvent {
            stage: ApplyStage::WaitRendererAlive,
            request_id: Some("req-1".into()),
        };
        let payload = serde_json::json!({
            "requestId": event.request_id,
            "stage": format!("{:?}", event.stage),
            "label": wc_app::apply_stage_label(&event.stage),
            "detail": wc_app::apply_stage_detail(&event.stage, &ctx),
        });
        assert_eq!(payload["requestId"], "req-1");
        assert_eq!(payload["stage"], "WaitRendererAlive");
        assert_eq!(payload["label"], "Waiting for renderer");
        assert!(payload["detail"].as_str().unwrap().contains("preview"));
    }

    #[test]
    fn stop_with_storage_clears_runtime_state_and_preserves_history() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = wc_core::ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        wc_core::config::write_config_value(&cd.path, "storage_backend", "sqlite").unwrap();
        let storage = wc_storage::StorageApi::new(cd);
        storage.current_write("/walls/current.jpg").unwrap();
        storage.last_backend_write("awww").unwrap();
        insert_history(&storage, "/walls/current.jpg", "awww");

        let result = stop_with_storage(&storage);

        assert!(result.success, "stop failed: {}", result.stderr);
        assert_eq!(storage.current_read().unwrap(), None);
        assert_eq!(storage.last_backend_read().unwrap(), None);
        assert_eq!(
            history_rows(&storage),
            vec![("/walls/current.jpg".to_string(), "awww".to_string())]
        );
    }

    #[test]
    fn legacy_apply_state_commit_replaces_named_overrides_and_syncs_legacy_keys() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = wc_core::ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        wc_core::config::write_config_value(&cd.path, "storage_backend", "sqlite").unwrap();
        let storage = wc_storage::StorageApi::new(cd);
        storage
            .display_state_upsert(
                &wc_storage::sqlite::DisplayStateTarget::Output("eDP-1".into()),
                "/walls/old.jpg",
                "awww",
            )
            .unwrap();
        storage.current_write("/walls/old.jpg").unwrap();
        storage.last_backend_write("awww").unwrap();

        let service = wc_app::AppService::from_config_dir(wc_core::ConfigDir {
            path: storage.cd.path.clone(),
        });
        service
            .commit_legacy_apply_display_state("/walls/new.mp4", wc_core::types::Backend::Mpvpaper)
            .unwrap();

        let rows = storage.display_state_list().unwrap();
        assert_eq!(rows.len(), 1, "named overrides must be cleared");
        assert_eq!(
            rows[0].target,
            wc_storage::sqlite::DisplayStateTarget::AllDisplays
        );
        assert_eq!(rows[0].wallpaper_path, "/walls/new.mp4");
        assert_eq!(rows[0].backend, "mpvpaper");
        assert_eq!(
            storage.current_read().unwrap().as_deref(),
            Some("/walls/new.mp4")
        );
        assert_eq!(
            storage.last_backend_read().unwrap().as_deref(),
            Some("mpvpaper")
        );
    }

    #[test]
    fn legacy_apply_state_commit_failure_reconciles_post_renderer_state() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = wc_core::ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        wc_core::config::write_config_value(&cd.path, "storage_backend", "sqlite").unwrap();
        let storage = wc_storage::StorageApi::new(cd);
        storage
            .display_state_upsert(
                &wc_storage::sqlite::DisplayStateTarget::Output("eDP-1".into()),
                "/walls/old.jpg",
                "awww",
            )
            .unwrap();
        // `execute_apply_request_with_options` has already updated these legacy
        // keys by the time the All Displays commit runs.
        storage.current_write("/walls/new.mp4").unwrap();
        storage.last_backend_write("mpvpaper").unwrap();
        let service = wc_app::AppService::from_config_dir(wc_core::ConfigDir {
            path: storage.cd.path.clone(),
        });
        let mut fail_commit = || {
            Err(wc_core::error::WcError::Other(
                "injected legacy apply commit failure".into(),
            ))
        };

        let err = service
            .commit_legacy_apply_display_state_with_seam(
                "/walls/new.mp4",
                wc_core::types::Backend::Mpvpaper,
                &mut fail_commit,
            )
            .unwrap_err();

        assert_eq!(err.code, "display_state_commit_failed");
        assert!(err.message.contains("injected legacy apply commit failure"));
        let rows = storage.display_state_list().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].target,
            wc_storage::sqlite::DisplayStateTarget::AllDisplays
        );
        assert_eq!(rows[0].wallpaper_path, "/walls/new.mp4");
        assert_eq!(rows[0].backend, "mpvpaper");
        assert_eq!(storage.current_read().unwrap(), None);
        assert_eq!(storage.last_backend_read().unwrap(), None);
    }

    #[test]
    fn parse_display_target_defaults_to_all_displays() {
        assert_eq!(
            parse_display_target(None).unwrap(),
            wc_app::DisplayTarget::AllDisplays
        );
        assert_eq!(
            parse_display_target(Some("all")).unwrap(),
            wc_app::DisplayTarget::AllDisplays
        );
        assert_eq!(
            parse_display_target(Some("__all_displays__")).unwrap(),
            wc_app::DisplayTarget::AllDisplays
        );
        assert_eq!(
            parse_display_target(Some("eDP-1")).unwrap(),
            wc_app::DisplayTarget::Output("eDP-1".into())
        );
        let err = parse_display_target(Some("   ")).unwrap_err();
        assert!(err.contains("blank"), "{err}");
    }

    #[test]
    fn display_state_and_list_dtos_serialize_camel_case() {
        let list = DisplayListDto {
            outputs: vec![
                DisplayDto {
                    name: "eDP-1".into(),
                },
                DisplayDto {
                    name: "HDMI-A-1".into(),
                },
            ],
        };
        let list_json = serde_json::to_value(&list).unwrap();
        assert_eq!(list_json["outputs"][0]["name"], "eDP-1");
        assert!(list_json.get("Outputs").is_none());

        let row = display_state_row_dto(&wc_storage::sqlite::DisplayStateRow {
            target: wc_storage::sqlite::DisplayStateTarget::AllDisplays,
            wallpaper_path: "/walls/a.jpg".into(),
            backend: "awww".into(),
            updated_at: "2026-07-11 12:00:00".into(),
        });
        let row_json = serde_json::to_value(&row).unwrap();
        assert_eq!(row_json["kind"], "allDisplays");
        assert_eq!(row_json["targetKey"], "__all_displays__");
        assert_eq!(row_json["wallpaperPath"], "/walls/a.jpg");
        assert_eq!(row_json["updatedAt"], "2026-07-11 12:00:00");
        assert!(row_json["output"].is_null());

        let named = display_state_row_dto(&wc_storage::sqlite::DisplayStateRow {
            target: wc_storage::sqlite::DisplayStateTarget::Output("eDP-1".into()),
            wallpaper_path: "/walls/b.jpg".into(),
            backend: "mpvpaper".into(),
            updated_at: "2026-07-11 13:00:00".into(),
        });
        let named_json = serde_json::to_value(&named).unwrap();
        assert_eq!(named_json["kind"], "output");
        assert_eq!(named_json["targetKey"], "eDP-1");
        assert_eq!(named_json["output"], "eDP-1");
    }

    #[test]
    fn runtime_observation_dtos_serialize_positive_evidence_and_unknown_reason() {
        let confirmed = runtime_observation_dto(
            wc_backend::runtime_observation::RuntimeWallpaperObservation {
                output: "eDP-1".into(),
                wallpaper_path: Some("/walls/current.jpg".into()),
                status: wc_backend::runtime_observation::RuntimeObservationStatus::Confirmed,
                reason: None,
            },
        );
        let unknown = runtime_observation_dto(
            wc_backend::runtime_observation::RuntimeWallpaperObservation {
                output: "HDMI-A-1".into(),
                wallpaper_path: None,
                status: wc_backend::runtime_observation::RuntimeObservationStatus::Unknown,
                reason: Some("No renderer evidence.".into()),
            },
        );

        let confirmed = serde_json::to_value(confirmed).unwrap();
        assert_eq!(confirmed["output"], "eDP-1");
        assert_eq!(confirmed["wallpaperPath"], "/walls/current.jpg");
        assert_eq!(confirmed["status"], "confirmed");
        assert!(confirmed.get("reason").is_none());

        let unknown = serde_json::to_value(unknown).unwrap();
        assert_eq!(unknown["output"], "HDMI-A-1");
        assert!(unknown["wallpaperPath"].is_null());
        assert_eq!(unknown["status"], "unknown");
        assert_eq!(unknown["reason"], "No renderer evidence.");
    }

    #[test]
    fn runtime_observation_coordinator_passes_connected_outputs_and_saved_rows_to_probe() {
        let outputs = vec!["eDP-1".to_string()];
        let rows = vec![wc_storage::sqlite::DisplayStateRow {
            target: wc_storage::sqlite::DisplayStateTarget::Output("eDP-1".into()),
            wallpaper_path: "/walls/current.jpg".into(),
            backend: "awww".into(),
            updated_at: "2026-07-14 00:00:00".into(),
        }];

        let dtos = runtime_observation_dtos_with(&outputs, &rows, |actual_outputs, actual_rows| {
            assert_eq!(actual_outputs, outputs);
            assert_eq!(actual_rows, rows);
            vec![
                wc_backend::runtime_observation::RuntimeWallpaperObservation {
                    output: "eDP-1".into(),
                    wallpaper_path: Some("/walls/current.jpg".into()),
                    status: wc_backend::runtime_observation::RuntimeObservationStatus::Confirmed,
                    reason: None,
                },
            ]
        });

        assert_eq!(dtos.len(), 1);
        assert_eq!(dtos[0].status, "confirmed");
        assert_eq!(
            dtos[0].wallpaper_path.as_deref(),
            Some("/walls/current.jpg")
        );
    }

    #[test]
    fn renderer_statuses_probe_each_supported_backend_without_starting_one() {
        let seen = std::cell::RefCell::new(Vec::new());
        let statuses = renderer_statuses_with(|backend| {
            seen.borrow_mut().push(backend);
            match backend {
                wc_core::types::Backend::Mpvpaper => {
                    Err(wc_core::error::WcError::BackendNotFound("mpvpaper".into()))
                }
                _ => Ok(()),
            }
        });

        assert_eq!(
            seen.into_inner(),
            [
                wc_core::types::Backend::Awww,
                wc_core::types::Backend::Mpvpaper,
                wc_core::types::Backend::LinuxWallpaperEngine,
            ]
        );
        assert!(statuses.awww.available);
        assert!(!statuses.mpvpaper.available);
        assert_eq!(
            statuses.mpvpaper.detail.as_deref(),
            Some("backend not found: mpvpaper")
        );
        assert!(statuses.linux_wallpaper_engine.available);

        let json = serde_json::to_value(statuses).unwrap();
        assert!(json.get("awww").is_some());
        assert!(json.get("mpvpaper").is_some());
        assert!(json.get("linuxWallpaperEngine").is_some());
        assert!(json.get("linux_wallpaper_engine").is_none());
    }

    #[test]
    fn runtime_observation_waits_for_an_in_progress_apply() {
        let lock = Arc::new(std::sync::Mutex::new(()));
        let apply_guard = lock.lock().unwrap();
        let worker_lock = Arc::clone(&lock);
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (observed_tx, observed_rx) = std::sync::mpsc::channel();

        let worker = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            with_renderer_state_lock(&worker_lock, || {
                observed_tx.send(()).unwrap();
                Ok(())
            })
            .unwrap();
        });

        started_rx.recv().unwrap();
        assert!(
            observed_rx.try_recv().is_err(),
            "runtime inspection must not run while apply owns the renderer lock"
        );
        drop(apply_guard);
        observed_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        worker.join().unwrap();
    }

    #[test]
    fn targeted_apply_request_deserializes_camel_case() {
        let dto: TargetedApplyRequestDto = serde_json::from_value(serde_json::json!({
            "kind": "apply_preview",
            "path": "/walls/a.jpg",
            "target": "eDP-1",
            "requestId": "req-9"
        }))
        .unwrap();
        assert_eq!(dto.path, "/walls/a.jpg");
        assert_eq!(dto.target.as_deref(), Some("eDP-1"));
        assert_eq!(dto.request_id.as_deref(), Some("req-9"));
        assert_eq!(dto.kind, "apply_preview");

        let all: TargetedApplyRequestDto = serde_json::from_value(serde_json::json!({
            "path": "/walls/a.jpg"
        }))
        .unwrap();
        assert!(all.target.is_none());
        assert_eq!(all.kind, "apply");
    }

    #[test]
    fn targeted_apply_kind_maps_every_supported_action_and_rejects_unknown() {
        let cases = [
            ("apply", wc_app::ApplyRequestKind::Apply),
            (
                "retry_backend_apply",
                wc_app::ApplyRequestKind::RetryBackendApply,
            ),
            ("apply_preview", wc_app::ApplyRequestKind::ApplyPreview),
        ];
        for (raw, expected) in cases {
            let request =
                apply_request_from_parts(raw, "/walls/a.jpg".into(), Some("req".into())).unwrap();
            assert_eq!(request.kind, expected);
            assert_eq!(request.path, "/walls/a.jpg");
            assert_eq!(request.request_id.as_deref(), Some("req"));
        }

        let error =
            apply_request_from_parts("open_folder", "/walls/a.jpg".into(), None).unwrap_err();
        assert_eq!(error.code, "invalid_apply_action");
    }

    #[test]
    fn targeted_apply_result_serializes_applied_outputs() {
        let dto = super::super::common::ApplyResultDto {
            request_id: Some("req-outputs".into()),
            applied_path: "/walls/a.jpg".into(),
            state_path: "/walls/a.jpg".into(),
            backend: "awww".into(),
            file_type: "image".into(),
            preview: false,
            applied_outputs: Some(vec!["eDP-1".into(), "HDMI-A-1".into()]),
        };

        let json = serde_json::to_value(dto).unwrap();
        assert_eq!(
            json["appliedOutputs"],
            serde_json::json!(["eDP-1", "HDMI-A-1"])
        );
    }

    #[test]
    fn targeted_restore_request_deserializes_optional_outputs() {
        let dto: TargetedRestoreRequestDto = serde_json::from_value(serde_json::json!({
            "outputs": ["eDP-1", "HDMI-A-1"]
        }))
        .unwrap();
        assert_eq!(
            dto.outputs.as_ref().unwrap(),
            &vec!["eDP-1".to_string(), "HDMI-A-1".to_string()]
        );

        let empty: TargetedRestoreRequestDto =
            serde_json::from_value(serde_json::json!({})).unwrap();
        assert!(empty.outputs.is_none());
    }

    #[test]
    fn explicit_restore_outputs_must_match_discovery_order_independently() {
        let request = TargetedRestoreRequestDto {
            outputs: Some(vec!["eDP-1".into(), "HDMI-A-1".into()]),
        };

        let resolved = resolve_restore_outputs_with(Some(&request), || {
            Ok(vec!["HDMI-A-1".into(), "eDP-1".into()])
        })
        .unwrap();

        assert_eq!(resolved, ["HDMI-A-1", "eDP-1"]);
    }

    #[test]
    fn explicit_restore_outputs_reject_incomplete_or_extra_sets() {
        let incomplete = TargetedRestoreRequestDto {
            outputs: Some(vec!["eDP-1".into()]),
        };
        let err = resolve_restore_outputs_with(Some(&incomplete), || {
            Ok(vec!["eDP-1".into(), "HDMI-A-1".into()])
        })
        .unwrap_err();
        assert!(err.contains("match"), "{err}");

        let extra = TargetedRestoreRequestDto {
            outputs: Some(vec!["eDP-1".into(), "HDMI-A-1".into()]),
        };
        let err =
            resolve_restore_outputs_with(Some(&extra), || Ok(vec!["eDP-1".into()])).unwrap_err();
        assert!(err.contains("match"), "{err}");
    }

    #[test]
    fn explicit_restore_outputs_reject_discovery_failure() {
        let request = TargetedRestoreRequestDto {
            outputs: Some(vec!["eDP-1".into()]),
        };

        let err = resolve_restore_outputs_with(Some(&request), || {
            Err("real display discovery failed".into())
        })
        .unwrap_err();

        assert!(err.contains("discovery failed"), "{err}");
    }
}
