use super::common::{
    fail, ok, storage, BackendStatusDto, CommandErrorDto, CommandResult, StatusDto, WeDebugInfoDto,
};
use wc_core::types::FileType;

static APPLY_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static APPLY_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
            wc_backend::linux_wallpaperengine::LinuxWallpaperEngineConfig::from_storage(&s);
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
pub async fn apply(path: String) -> CommandResult {
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
            execute_and_format_result(&service, request, seq)
        }
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn apply_action(request: super::common::ApplyRequestDto) -> CommandResult {
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
            execute_and_format_result(&service, request, seq)
        }
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

fn is_stale_apply(seq: u64) -> bool {
    seq != APPLY_SEQUENCE.load(std::sync::atomic::Ordering::SeqCst)
}

fn execute_and_format_result(
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

    if is_stale_apply(seq) {
        return stale_apply_result();
    }

    match service.execute_apply_request(request) {
        Ok(result) => {
            if result.file_type == FileType::WeScene {
                wc_storage::we_compat::clear_failure(&result.state_path).ok();
            }
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

#[tauri::command]
pub async fn stop() -> CommandResult {
    tauri::async_runtime::spawn_blocking(|| match storage() {
        Ok(s) => match wc_backend::stop_all_backends(Some(&s)) {
            Ok(()) => ok("Stopped wallpaper backends."),
            Err(e) => fail(e.to_string()),
        },
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn restore() -> CommandResult {
    tauri::async_runtime::spawn_blocking(|| match storage() {
        Ok(s) => match wc_backend::restore(&s) {
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
    fn stale_apply_helper_detects_superseded_sequence() {
        let previous = APPLY_SEQUENCE.load(std::sync::atomic::Ordering::SeqCst);
        APPLY_SEQUENCE.store(10, std::sync::atomic::Ordering::SeqCst);

        struct RestoreSeq(u64);
        impl Drop for RestoreSeq {
            fn drop(&mut self) {
                APPLY_SEQUENCE.store(self.0, std::sync::atomic::Ordering::SeqCst);
            }
        }
        let _guard = RestoreSeq(previous);

        assert!(is_stale_apply(9));
        assert!(!is_stale_apply(10));
    }
}
