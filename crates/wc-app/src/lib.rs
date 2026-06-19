pub mod apply_execution;
pub mod apply_plan;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use wc_core::error::WcError;
use wc_core::types::{Backend, FileType, WallpaperEntry};
use wc_storage::StorageApi;

pub use apply_execution::{ApplyExecutionResult, ApplyRequest, ApplyRequestKind};
pub use apply_plan::{
    ApplyAction, ApplyActionKind, ApplyAvailability, ApplyPlan, CompatibilityKind,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppError {
    pub code: String,
    pub message: String,
    pub detail: Option<String>,
    pub recoverable: bool,
    pub suggestion: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyTarget {
    pub input_path: String,
    pub resolved_path: String,
    pub file_type: FileType,
    pub backend: Backend,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectResult {
    pub input_path: String,
    pub resolved_path: Option<String>,
    pub entry: Option<WallpaperEntry>,
    pub backend: Option<Backend>,
    pub error: Option<AppError>,
}

pub struct AppService {
    storage: StorageApi,
}

impl AppService {
    #[cfg(test)]
    pub(crate) fn storage_for_tests(&self) -> &StorageApi {
        &self.storage
    }

    pub fn from_config_dir(cd: wc_core::ConfigDir) -> Self {
        AppService {
            storage: StorageApi::try_new(cd).expect("storage initialization failed"),
        }
    }

    pub fn apply(&self, path: &str) -> Result<ApplyTarget, AppError> {
        let result = self.execute_apply_request(ApplyRequest {
            kind: ApplyRequestKind::Apply,
            path: path.to_string(),
            request_id: None,
        })?;

        Ok(ApplyTarget {
            input_path: path.to_string(),
            resolved_path: result.applied_path,
            file_type: result.file_type,
            backend: result.backend,
        })
    }

    pub fn execute_apply_request(
        &self,
        request: ApplyRequest,
    ) -> Result<ApplyExecutionResult, AppError> {
        let target = self.resolve_apply_request_target(&request)?;
        let result = wc_backend::apply_wallpaper(
            &self.storage,
            &target.resolved_path,
            target.backend,
            target.fallback_path.as_deref(),
        );
        match result {
            Ok(()) => {
                if target.file_type == FileType::WeScene {
                    let _ = wc_storage::we_compat::clear_failure(&target.state_path);
                }
                Ok(ApplyExecutionResult {
                    request_id: request.request_id,
                    applied_path: target.resolved_path,
                    state_path: target.state_path,
                    backend: target.backend,
                    file_type: target.file_type,
                    preview: target.preview,
                })
            }
            Err(WcError::LinuxWallpaperEngine { kind, detail }) => {
                if target.file_type == FileType::WeScene {
                    let backend_status = if kind == wc_core::error::BackendErrorKind::RendererLimitation {
                        "renderer_limitation"
                    } else {
                        "failed"
                    };
                    let app_err = AppError::from_wc_error(WcError::LinuxWallpaperEngine {
                        kind: kind.clone(),
                        detail: detail.clone(),
                    });
                    let _ = wc_storage::we_compat::record_failure(
                        &target.state_path,
                        backend_status,
                        &app_err.code,
                        &app_err.message,
                        Some(detail.clone()),
                    );
                }
                Err(AppError::from_wc_error(WcError::LinuxWallpaperEngine { kind, detail }))
            }
            Err(e) => Err(AppError::from_wc_error(e)),
        }
    }

    pub fn inspect_path(&self, path: &str) -> Result<InspectResult, AppError> {
        match self.resolve_apply_target(path) {
            Ok(target) => Ok(InspectResult {
                input_path: path.to_string(),
                resolved_path: Some(target.resolved_path.clone()),
                entry: wc_scan::make_entry(&target.resolved_path),
                backend: Some(target.backend),
                error: None,
            }),
            Err(error) => Ok(InspectResult {
                input_path: path.to_string(),
                resolved_path: None,
                entry: None,
                backend: None,
                error: Some(error),
            }),
        }
    }

    pub fn resolve_apply_target(&self, path: &str) -> Result<ApplyTarget, AppError> {
        let resolved_path = resolve_wallpaper_path(path).map_err(AppError::from_wc_error)?;
        let entry = wc_scan::make_entry(&resolved_path)
            .ok_or_else(|| AppError::unsupported_path(&resolved_path))?;
        let backend = self.backend_for_entry(&entry)?;
        if backend == Backend::Unsupported {
            return Err(AppError::unsupported_backend(
                entry.file_type,
                &resolved_path,
            ));
        }
        Ok(ApplyTarget {
            input_path: path.to_string(),
            resolved_path,
            file_type: entry.file_type,
            backend,
        })
    }

    pub fn resolve_apply_request_target(
        &self,
        request: &ApplyRequest,
    ) -> Result<apply_execution::ApplyExecutionTarget, AppError> {
        match request.kind {
            ApplyRequestKind::Apply | ApplyRequestKind::RetryBackendApply => {
                let target = self.resolve_apply_target(&request.path)?;
                let fallback_path = resolve_fallback(&target);
                Ok(apply_execution::ApplyExecutionTarget {
                    input_path: request.path.clone(),
                    resolved_path: target.resolved_path.clone(),
                    state_path: target.resolved_path,
                    file_type: target.file_type,
                    backend: target.backend,
                    preview: false,
                    fallback_path,
                })
            }
            ApplyRequestKind::ApplyPreview => {
                let project_path =
                    resolve_wallpaper_path(&request.path).map_err(AppError::from_wc_error)?;
                let project = std::path::Path::new(&project_path);
                let info = wc_scan::read_we_project_info(project)
                    .ok_or_else(|| AppError::preview_missing(&request.path))?;
                let preview = info
                    .preview_path
                    .ok_or_else(|| AppError::preview_missing(&request.path))?;
                let preview_entry = wc_scan::make_entry(&preview)
                    .ok_or_else(|| AppError::unsupported_path(&preview))?;
                let backend = self.backend_for_entry(&preview_entry)?;
                Ok(apply_execution::ApplyExecutionTarget {
                    input_path: request.path.clone(),
                    resolved_path: preview.to_string(),
                    state_path: preview.to_string(),
                    file_type: preview_entry.file_type,
                    backend,
                    preview: true,
                    fallback_path: Some(preview.to_string()),
                })
            }
        }
    }

    fn backend_for_entry(&self, entry: &WallpaperEntry) -> Result<Backend, AppError> {
        match entry.backend {
            Backend::Unsupported => Err(AppError::unsupported_backend(
                entry.file_type,
                entry.path.as_str(),
            )),
            Backend::Awww | Backend::Mpvpaper | Backend::LinuxWallpaperEngine => Ok(entry.backend),
        }
    }
}

fn resolve_fallback(target: &ApplyTarget) -> Option<String> {
    match target.file_type {
        FileType::Image | FileType::Gif => Some(target.resolved_path.clone()),
        FileType::Video | FileType::WeScene | FileType::WeWeb | FileType::WeApplication => None,
    }
}

pub fn resolve_wallpaper_path(path: &str) -> Result<String, WcError> {
    let p = Path::new(path);
    if p.is_dir() {
        if let Some(info) = wc_scan::read_we_project_info(p) {
            return match info.entry_type {
                FileType::Image | FileType::Gif | FileType::Video => {
                    let file = info
                        .file
                        .ok_or_else(|| WcError::UnsupportedFileType(path.to_string()))?;
                    let media = wc_scan::safe_join(p, &file).map_err(WcError::Other)?;
                    Ok(media.to_string_lossy().to_string())
                }
                FileType::WeScene | FileType::WeWeb | FileType::WeApplication => {
                    Ok(p.to_string_lossy().to_string())
                }
            };
        }
        return Err(WcError::UnsupportedFileType(path.to_string()));
    }
    if p.is_file() {
        return Ok(p.to_string_lossy().to_string());
    }
    Err(WcError::WallpaperMissing(PathBuf::from(path)))
}

impl AppError {
    pub fn from_wc_error(err: WcError) -> Self {
        match &err {
            WcError::LinuxWallpaperEngine { kind, detail } => {
                let (code, message, suggestion) = match kind {
                    wc_core::error::BackendErrorKind::RendererLimitation => (
                        "renderer_limitation",
                        "This Wallpaper Engine scene is not compatible with \
                         linux-wallpaperengine."
                            .to_string(),
                        Some(
                            "Use the preview GIF or choose another Wallpaper Engine scene."
                                .to_string(),
                        ),
                    ),
                    wc_core::error::BackendErrorKind::TargetConfig => (
                        "target_config_error",
                        "linux-wallpaperengine could not find the correct display output."
                            .to_string(),
                        Some(
                            "Set target_mode=screen-root and target=<output name> in Settings \
                             (e.g. eDP-1)."
                                .to_string(),
                        ),
                    ),
                    wc_core::error::BackendErrorKind::WorkshopDirectory => (
                        "workshop_directory_missing",
                        "Wallpaper Engine workshop directory not found.".to_string(),
                        Some(
                            "Check the workshop content path in your Wallpaper Engine sources."
                                .to_string(),
                        ),
                    ),
                    wc_core::error::BackendErrorKind::Generic => (
                        "linux_wallpaperengine_failed",
                        "Wallpaper Engine scene support is not ready.".to_string(),
                        Some(
                            "Use the preview GIF or choose another Wallpaper Engine scene."
                                .to_string(),
                        ),
                    ),
                };
                AppError {
                    code: code.into(),
                    message,
                    detail: Some(detail.clone()),
                    recoverable: true,
                    suggestion,
                }
            }
            _ => {
                let text = err.to_string();
                AppError {
                    code: "command_failed".into(),
                    message: text,
                    detail: None,
                    recoverable: true,
                    suggestion: None,
                }
            }
        }
    }

    fn unsupported_path(path: &str) -> Self {
        AppError {
            code: "unsupported_file".into(),
            message: format!("Unsupported wallpaper file: {}", path),
            detail: None,
            recoverable: true,
            suggestion: Some(
                "Choose an image, gif, video, or supported Wallpaper Engine project.".into(),
            ),
        }
    }

    fn unsupported_backend(file_type: FileType, path: &str) -> Self {
        if file_type == FileType::WeWeb {
            return AppError::we_web_unsupported();
        }
        AppError {
            code: "unsupported_backend".into(),
            message: format!("No backend is available for this wallpaper: {}", path),
            detail: Some(format!("file_type={:?}", file_type)),
            recoverable: true,
            suggestion: None,
        }
    }

    fn we_web_unsupported() -> Self {
        AppError {
            code: "we_web_unsupported".into(),
            message: "Wallpaper Engine Web wallpapers are unsupported.".into(),
            detail: Some("WE Web projects are kept in the library for browsing, preview thumbnails, project-folder access, and workshop ID lookup only.".into()),
            recoverable: true,
            suggestion: Some(
                "Use Apply preview GIF if the project has one, or choose a WE Scene/image/video wallpaper.".into(),
            ),
        }
    }

    fn preview_missing(path: &str) -> Self {
        AppError {
            code: "preview_missing".into(),
            message: "This wallpaper has no preview file to apply.".into(),
            detail: Some(format!("project={}", path)),
            recoverable: true,
            suggestion: Some("Open the project folder or choose another wallpaper.".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wc_core::config::ConfigDir;

    fn temp_service() -> (tempfile::TempDir, AppService) {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("config"),
        };
        cd.init().unwrap();
        wc_core::config::write_config_value(&cd.path, "storage_backend", "sqlite").unwrap();
        (tmp, AppService::from_config_dir(cd))
    }

    fn web_project(root: &Path) -> PathBuf {
        let project = root.join("steamapps/workshop/content/431960/3650880224");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("index.html"), "<html></html>").unwrap();
        std::fs::write(
            project.join("project.json"),
            r#"{"type":"web","file":"index.html"}"#,
        )
        .unwrap();
        project
    }

    #[test]
    fn we_web_apply_returns_unsupported() {
        let (tmp, service) = temp_service();
        let project = web_project(tmp.path());
        let err = service
            .resolve_apply_target(&project.to_string_lossy())
            .unwrap_err();
        assert_eq!(err.code, "we_web_unsupported");
    }
}
