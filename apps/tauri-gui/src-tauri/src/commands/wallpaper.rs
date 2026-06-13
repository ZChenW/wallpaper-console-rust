use super::common::{
    fail, ok, storage, BackendStatusDto, CommandResult, StatusDto, WeDebugInfoDto,
};
use wc_core::types::FileType;

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
                || std::env::var("XDG_SESSION_TYPE").map(|v| v == "wayland").unwrap_or(false);
            if wayland {
                let warning = "⚠ Wayland detected: recommend setting target_mode=screen-root and target=<your output name> for stable scene rendering.";
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
    tauri::async_runtime::spawn_blocking(move || match storage() {
        Ok(s) => {
            let service = wc_app::AppService::from_config_dir(wc_core::ConfigDir {
                path: s.cd.path.clone(),
            });
            match service.apply(&path) {
                Ok(target) => {
                    if target.file_type == FileType::WeScene {
                        wc_storage::we_compat::clear_failure(&target.resolved_path).ok();
                    }
                    ok(format!("Applied: {}", target.resolved_path))
                }
                Err(err) => CommandResult {
                    success: false,
                    stdout: String::new(),
                    stderr: err.message.clone(),
                    exit_code: 1,
                    error: Some(super::common::CommandErrorDto {
                        kind: err.code,
                        message: err.message,
                        detail: err.detail,
                        recoverable: err.recoverable,
                        suggestion: err.suggestion,
                    }),
                },
            }
        }
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
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
