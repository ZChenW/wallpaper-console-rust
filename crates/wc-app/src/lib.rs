use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use wc_core::error::WcError;
use wc_core::types::{Backend, FileType, WallpaperEntry};
use wc_storage::StorageApi;

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
    pub fn from_config_dir(cd: wc_core::ConfigDir) -> Self {
        AppService {
            storage: StorageApi::new(cd),
        }
    }

    pub fn apply(&self, path: &str) -> Result<ApplyTarget, AppError> {
        let target = self.resolve_apply_target(path)?;
        wc_backend::apply_wallpaper(&self.storage, &target.resolved_path, target.backend)
            .map_err(AppError::from_wc_error)?;
        Ok(target)
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

    fn backend_for_entry(&self, entry: &WallpaperEntry) -> Result<Backend, AppError> {
        match entry.backend {
            Backend::ChromiumWeb => Err(AppError::web_renderer_unavailable()),
            Backend::WebKitLayerShell => {
                if wc_backend::web_renderer::is_available(&self.storage) {
                    Ok(Backend::WebKitLayerShell)
                } else {
                    Err(AppError::web_renderer_unavailable())
                }
            }
            Backend::Unsupported => Err(AppError::unsupported_backend(
                entry.file_type,
                entry.path.as_str(),
            )),
            Backend::Awww | Backend::Mpvpaper | Backend::LinuxWallpaperEngine => Ok(entry.backend),
        }
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
        let text = err.to_string();
        let lower = text.to_lowercase();
        if lower.contains("web renderer")
            || lower.contains("wallpaper-console-web-renderer")
            || lower.contains("webkit")
        {
            return AppError {
                code: "web_renderer_failed".into(),
                message: "The native Web wallpaper renderer failed.".into(),
                detail: Some(text),
                recoverable: true,
                suggestion: Some(
                    "Check that wallpaper-console-web-renderer is installed and your Wayland compositor supports layer-shell."
                        .into(),
                ),
            };
        }
        AppError {
            code: "command_failed".into(),
            message: text,
            detail: None,
            recoverable: true,
            suggestion: None,
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
            return AppError::web_renderer_unavailable();
        }
        AppError {
            code: "unsupported_backend".into(),
            message: format!("No backend is available for this wallpaper: {}", path),
            detail: Some(format!("file_type={:?}", file_type)),
            recoverable: true,
            suggestion: None,
        }
    }

    fn web_renderer_unavailable() -> Self {
        AppError {
            code: "web_renderer_unavailable".into(),
            message: "Web wallpapers require the native Web renderer.".into(),
            detail: Some(
                "Chromium preview opens a normal window; real Web wallpaper backgrounds require wallpaper-console-web-renderer."
                    .into(),
            ),
            recoverable: true,
            suggestion: Some(
                "Build and install wallpaper-console-web-renderer, or use Apply preview GIF / Open experimental Chromium preview."
                    .into(),
            ),
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
    fn we_web_apply_returns_renderer_unavailable_when_renderer_missing() {
        let (tmp, service) = temp_service();
        let project = web_project(tmp.path());
        let err = service
            .resolve_apply_target(&project.to_string_lossy())
            .unwrap_err();
        assert_eq!(err.code, "web_renderer_unavailable");
    }

    #[cfg(unix)]
    #[test]
    fn we_web_resolves_to_webkit_layer_shell_when_renderer_is_configured() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("config"),
        };
        cd.init().unwrap();
        wc_core::config::write_config_value(&cd.path, "storage_backend", "sqlite").unwrap();
        let storage = StorageApi::new(ConfigDir {
            path: cd.path.clone(),
        });
        let bin = tmp.path().join("renderer");
        std::fs::write(&bin, "#!/bin/sh\nsleep 5\n").unwrap();
        let mut perms = std::fs::metadata(&bin).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&bin, perms).unwrap();
        storage
            .config_set("web_renderer_path", &bin.to_string_lossy())
            .unwrap();
        let service = AppService::from_config_dir(ConfigDir { path: cd.path });
        let project = web_project(tmp.path());
        let target = service
            .resolve_apply_target(&project.to_string_lossy())
            .unwrap();
        assert_eq!(target.backend, Backend::WebKitLayerShell);
    }
}
