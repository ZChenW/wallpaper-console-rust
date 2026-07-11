use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use wc_core::types::{FileType, WallpaperEntry};
use wc_storage::StorageApi;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandErrorDto {
    pub kind: String,
    pub message: String,
    pub detail: Option<String>,
    pub recoverable: bool,
    pub suggestion: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandResultDto {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub error: Option<CommandErrorDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusDto {
    pub config_dir: String,
    pub current: String,
    pub last_backend: String,
    pub source_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendStatusDto {
    pub available: bool,
    pub path: Option<String>,
    pub message: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceDto {
    pub path: String,
    pub exists: bool,
    pub is_we: bool,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyActionDto {
    pub kind: String,
    pub label: String,
    pub enabled: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WallpaperDto {
    pub path: String,
    #[serde(rename = "type")]
    pub file_type: String,
    pub ext: String,
    pub backend: String,
    pub size: u64,
    pub mtime: u64,
    pub resolution: String,
    pub project_type: Option<String>,
    pub preview_path: Option<String>,
    pub workshop_id: Option<String>,
    pub title: Option<String>,
    pub we_file: Option<String>,
    pub unsupported_reason: Option<String>,
    pub backend_status: Option<String>,
    pub backend_error_kind: Option<String>,
    pub backend_error_message: Option<String>,
    pub backend_error_detail: Option<String>,
    pub backend_failed_at: Option<String>,
    pub apply_availability: Option<String>,
    pub apply_backend: Option<String>,
    pub apply_reason: Option<String>,
    pub apply_actions: Option<Vec<ApplyActionDto>>,
    pub renderer_compatibility: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyRequestDto {
    pub kind: String,
    pub path: String,
    pub request_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResultDto {
    pub request_id: Option<String>,
    pub applied_path: String,
    pub state_path: String,
    pub backend: String,
    pub file_type: String,
    pub preview: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryCountDto {
    pub total: usize,
    pub images: usize,
    pub gifs: usize,
    pub videos: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryPageDto {
    pub total: usize,
    pub items: Vec<WallpaperDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbnailDto {
    pub path: String,
    pub thumbnail: Option<String>,
    pub cache_hit: bool,
    pub failure_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbnailCacheDto {
    pub dir: String,
    pub size: String,
    pub entries: usize,
    pub oldest_mtime: Option<u64>,
    pub newest_mtime: Option<u64>,
    pub failure_entries: usize,
    pub cleanup_days: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgressDto {
    pub running: bool,
    pub stage: String,
    pub scanned: usize,
    pub total_hint: Option<usize>,
    pub reused_metadata: usize,
    pub probed_metadata: usize,
    pub inserted_sqlite: usize,
    pub staged: usize,
    pub skipped: usize,
    /// Always 0 for now; `make_entry_cached` does not expose a distinguishable
    /// error path, so unsupported/skipped paths are counted as `skipped` instead.
    pub metadata_errors: usize,
    pub current_path: Option<String>,
    pub cancel_requested: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySourceStatusDto {
    pub configured: String,
    pub effective: String,
    pub sqlite_ready: bool,
    pub sqlite_rows: usize,
    pub tsv_rows: usize,
    pub source_count: usize,
    pub stale: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WeDebugInfoDto {
    pub last_command_line: String,
    pub last_target_config: String,
    pub last_stderr: String,
    pub last_exit_status: String,
    pub log_path: String,
}

pub type CommandResult = CommandResultDto;

struct StorageCell {
    storage: OnceLock<StorageApi>,
    initialization_lock: Mutex<()>,
}

impl StorageCell {
    const fn new() -> Self {
        Self {
            storage: OnceLock::new(),
            initialization_lock: Mutex::new(()),
        }
    }

    fn get_or_init_with(
        &self,
        initializer: impl FnOnce() -> Result<StorageApi, String>,
    ) -> Result<&StorageApi, String> {
        if let Some(storage) = self.storage.get() {
            return Ok(storage);
        }

        let _initialization_guard = self
            .initialization_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(storage) = self.storage.get() {
            return Ok(storage);
        }

        let storage = initializer()?;
        Ok(self.storage.get_or_init(|| storage))
    }
}

static STORAGE: StorageCell = StorageCell::new();

pub fn storage() -> Result<&'static StorageApi, String> {
    STORAGE.get_or_init_with(|| {
        let cd = wc_core::ConfigDir::new().map_err(|e| e.to_string())?;
        StorageApi::try_new(cd).map_err(|e| e.to_string())
    })
}

pub fn ok(stdout: impl Into<String>) -> CommandResultDto {
    CommandResultDto {
        success: true,
        stdout: stdout.into(),
        stderr: String::new(),
        exit_code: 0,
        error: None,
    }
}

pub fn fail(err: impl Into<String>) -> CommandResultDto {
    let msg = err.into();
    CommandResultDto {
        success: false,
        stdout: String::new(),
        stderr: msg.clone(),
        exit_code: 1,
        error: Some(error_dto(&msg)),
    }
}

/// Classify backend errors from their text message into structured error codes.
///
/// **Order matters**: more-specific checks (e.g. WE Web unsupported or scene
/// projection errors) must precede generic command-failure checks.
/// Reordering these blocks may cause misclassification.
pub fn error_dto(msg: &str) -> CommandErrorDto {
    let lower = msg.to_lowercase();
    if lower.contains("web wallpapers are unsupported")
        || lower.contains("wallpaper engine web wallpapers are unsupported")
        || lower.contains("we web")
    {
        return CommandErrorDto {
            kind: "we_web_unsupported".into(),
            message: "Wallpaper Engine Web wallpapers are unsupported.".into(),
            detail: Some(msg.to_string()),
            recoverable: true,
            suggestion: Some(
                "Use Apply preview GIF if available, or choose a WE Scene/image/video wallpaper."
                    .into(),
            ),
        };
    }
    if lower.contains("could not create a window")
        || lower.contains("no suitable output")
        || lower.contains("no display")
    {
        return CommandErrorDto {
            kind: "target_config_error".into(),
            message: "linux-wallpaperengine could not find the correct display output.".into(),
            detail: Some(msg.to_string()),
            recoverable: true,
            suggestion: Some(
                "Set target_mode=screen-root and target=<output name> in Settings (e.g. eDP-1)."
                    .into(),
            ),
        };
    }
    if lower.contains("linux-wallpaperengine") || lower.contains("projection must have a width") {
        return CommandErrorDto {
            kind: if lower.contains("projection must have a width") {
                "scene_projection_unsupported".into()
            } else if lower.contains("cannot find workshop") {
                "workshop_directory_missing".into()
            } else {
                "linux_wallpaperengine_failed".into()
            },
            message: if lower.contains("projection must have a width") {
                "This Wallpaper Engine scene is not compatible with linux-wallpaperengine.".into()
            } else if lower.contains("cannot find workshop") {
                "Wallpaper Engine workshop directory not found.".into()
            } else {
                "Wallpaper Engine scene support is not ready.".into()
            },
            detail: Some(msg.to_string()),
            recoverable: true,
            suggestion: Some(if lower.contains("cannot find workshop") {
                "Check the workshop content path in your Wallpaper Engine sources.".to_string()
            } else {
                "Use the preview GIF or choose another Wallpaper Engine scene.".to_string()
            }),
        };
    }
    CommandErrorDto {
        kind: "command_failed".into(),
        message: msg.to_string(),
        detail: None,
        recoverable: true,
        suggestion: None,
    }
}

pub fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.1} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{} B", bytes)
    }
}

pub fn source_label(path: &str) -> String {
    let p = Path::new(path);
    if wc_scan::is_wallpaper_engine_source(path) {
        let has_workshop_id = path
            .find("/steamapps/workshop/content/431960/")
            .map(|pos| {
                let after = &path[pos + "/steamapps/workshop/content/431960/".len()..];
                let first_seg = after.split('/').next().unwrap_or("");
                !first_seg.is_empty() && first_seg.chars().all(|c| c.is_ascii_digit())
            })
            .unwrap_or(false);
        if has_workshop_id {
            return p
                .file_name()
                .map(|s| format!("Wallpaper Engine {}", s.to_string_lossy()))
                .unwrap_or_else(|| "Wallpaper Engine".into());
        }
        return "Wallpaper Engine Workshop".into();
    }
    p.file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

pub fn dto_from_entry(entry: WallpaperEntry) -> WallpaperDto {
    let project = entry.project.clone();

    let cached_failure = if entry.file_type == FileType::WeScene {
        wc_storage::we_compat::lookup_failure(entry.path.as_ref())
            .ok()
            .flatten()
    } else {
        None
    };

    let backend_failed = cached_failure.is_some();
    let error_kind = cached_failure.as_ref().map(|f| f.error_kind.as_str());
    let plan = wc_app::apply_plan::plan_for_entry_with_kind(&entry, backend_failed, error_kind);
    let renderer_compatibility = plan.compatibility.as_ref().map(|c| match c {
        wc_app::apply_plan::CompatibilityKind::NativeScene { disclaimer } => disclaimer.clone(),
    });

    let mut dto = WallpaperDto {
        path: entry.path.to_string(),
        file_type: entry.file_type.as_str().to_string(),
        ext: entry.ext,
        backend: entry.backend.as_str().to_string(),
        size: entry.size,
        mtime: entry.mtime,
        resolution: entry.resolution,
        project_type: project.as_ref().map(|p| p.project_type.clone()),
        preview_path: project.as_ref().and_then(|p| p.preview_path.clone()),
        workshop_id: project.as_ref().and_then(|p| p.workshop_id.clone()),
        title: project.as_ref().and_then(|p| p.title.clone()),
        we_file: project.as_ref().and_then(|p| p.we_file.clone()),
        unsupported_reason: project.as_ref().and_then(|p| p.unsupported_reason.clone()),
        backend_status: None,
        backend_error_kind: None,
        backend_error_message: None,
        backend_error_detail: None,
        backend_failed_at: None,
        apply_availability: Some(plan.availability.as_str().to_string()),
        apply_backend: plan.backend.map(|b| b.as_str().to_string()),
        apply_reason: plan.reason.clone(),
        apply_actions: Some(
            plan.actions
                .into_iter()
                .map(|a| ApplyActionDto {
                    kind: a.kind.as_str().to_string(),
                    label: a.label,
                    enabled: a.enabled,
                    reason: a.reason,
                })
                .collect(),
        ),
        renderer_compatibility,
    };
    if let Some(cached) = cached_failure {
        dto.backend_status = Some(cached.backend_status);
        dto.backend_error_kind = Some(cached.error_kind);
        dto.backend_error_message = Some(cached.error_message);
        dto.backend_error_detail = cached.error_detail;
        dto.backend_failed_at = Some(cached.failed_at);
    }
    dto
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Barrier,
    };
    use wc_core::types::{Backend, FileType, WallpaperEntry, WallpaperProject};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn storage_cell_retries_after_initialization_error() {
        let tmp = tempfile::tempdir().unwrap();
        let cell = StorageCell::new();
        let initializer_calls = AtomicUsize::new(0);

        let first = cell.get_or_init_with(|| {
            initializer_calls.fetch_add(1, Ordering::SeqCst);
            Err("transient initialization error".to_string())
        });
        assert_eq!(
            first.err().as_deref(),
            Some("transient initialization error")
        );

        let second = cell
            .get_or_init_with(|| {
                initializer_calls.fetch_add(1, Ordering::SeqCst);
                StorageApi::try_new(wc_core::ConfigDir {
                    path: tmp.path().join("config"),
                })
                .map_err(|e| e.to_string())
            })
            .unwrap();
        let third = cell
            .get_or_init_with(|| panic!("successful initialization should be reused"))
            .unwrap();

        assert_eq!(initializer_calls.load(Ordering::SeqCst), 2);
        assert!(std::ptr::eq(second, third));
    }

    #[test]
    fn storage_cell_initializes_once() {
        let tmp = tempfile::tempdir().unwrap();
        let cell = StorageCell::new();
        let initializer_calls = AtomicUsize::new(0);
        let barrier = Barrier::new(16);

        let handles = std::thread::scope(|scope| {
            let mut threads = Vec::with_capacity(16);
            for _ in 0..16 {
                let config_path = tmp.path().join("config");
                let cell = &cell;
                let initializer_calls = &initializer_calls;
                let barrier = &barrier;
                threads.push(scope.spawn(move || {
                    barrier.wait();
                    let storage = cell
                        .get_or_init_with(|| {
                            initializer_calls.fetch_add(1, Ordering::SeqCst);
                            StorageApi::try_new(wc_core::ConfigDir { path: config_path })
                                .map_err(|e| e.to_string())
                        })
                        .unwrap();
                    storage as *const StorageApi as usize
                }));
            }
            threads
                .into_iter()
                .map(|thread| thread.join().unwrap())
                .collect::<Vec<_>>()
        });

        assert_eq!(initializer_calls.load(Ordering::SeqCst), 1);
        assert!(handles.windows(2).all(|pair| pair[0] == pair[1]));
    }

    #[test]
    fn dto_from_entry_maps_renderer_limitation_from_we_compat() {
        let _guard = ENV_LOCK.lock().unwrap();
        let previous = std::env::var_os("XDG_CONFIG_HOME");
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_CONFIG_HOME", tmp.path());

        let project_path = tmp.path().join("431960/3589454154");
        std::fs::create_dir_all(&project_path).unwrap();
        std::fs::write(project_path.join("project.json"), "{}").unwrap();
        let project_path_str = project_path.to_string_lossy().to_string();

        wc_storage::we_compat::record_failure(
            &project_path_str,
            "renderer_limitation",
            "renderer_limitation",
            "Scene is not compatible with linux-wallpaperengine.",
            None,
        )
        .unwrap();

        let entry = WallpaperEntry {
            path: Utf8PathBuf::from(&project_path_str),
            file_type: FileType::WeScene,
            ext: "scene".into(),
            backend: Backend::LinuxWallpaperEngine,
            size: 1,
            mtime: 1,
            resolution: "WE".into(),
            project: Some(WallpaperProject {
                project_type: "we_scene".into(),
                preview_path: None,
                workshop_id: Some("3589454154".into()),
                title: Some("Test scene".into()),
                we_file: Some("scene.json".into()),
                backend: Some("linux-wallpaperengine".into()),
                unsupported_reason: None,
            }),
        };

        let dto = dto_from_entry(entry);
        assert_eq!(dto.backend_status.as_deref(), Some("renderer_limitation"));
        assert_eq!(
            dto.backend_error_kind.as_deref(),
            Some("renderer_limitation")
        );
        assert_eq!(dto.apply_availability.as_deref(), Some("retryable_failure"));
        assert!(
            dto.apply_actions
                .as_ref()
                .is_some_and(|actions| actions.iter().any(|a| a.kind == "retry_backend_apply")),
            "renderer limitation should expose retry backend apply"
        );

        match previous {
            Some(value) => std::env::set_var("XDG_CONFIG_HOME", value),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
    }
}
