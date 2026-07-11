use super::common::{
    fail, ok, storage, BackendStatusDto, CommandErrorDto, CommandResult, StatusDto, WeDebugInfoDto,
};
use std::sync::{Arc, Mutex};
use tauri::Emitter;
use wc_storage::StorageApi;

static APPLY_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static APPLY_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
            let dto = super::common::ApplyResultDto {
                request_id: result.request_id,
                applied_path: result.applied_path.clone(),
                state_path: result.state_path,
                backend: result.backend.as_str().to_string(),
                file_type: result.file_type.as_str().to_string(),
                preview: result.preview,
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
    let kind = match dto.kind.as_str() {
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
        path: dto.path,
        request_id: dto.request_id,
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

#[cfg(test)]
mod tests {
    use super::*;
    use wc_backend::apply_stage::ApplyStage;

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
        storage.history_add("/walls/current.jpg", "awww").unwrap();

        let result = stop_with_storage(&storage);

        assert!(result.success, "stop failed: {}", result.stderr);
        assert_eq!(storage.current_read().unwrap(), None);
        assert_eq!(storage.last_backend_read().unwrap(), None);
        assert_eq!(
            storage.history_list().unwrap(),
            vec!["/walls/current.jpg".to_string()]
        );
    }
}
