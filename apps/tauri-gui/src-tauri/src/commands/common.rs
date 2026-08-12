use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex, OnceLock};

use wc_config::ConfigDirExt;
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
    pub id: i64,
    pub path: String,
    pub display_name: String,
    pub kind: String,
    pub recursive: bool,
    pub availability: String,
    pub added_at: String,
    pub exists: bool,
    #[serde(rename = "isWE")]
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub applied_outputs: Option<Vec<String>>,
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

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryBrowserQueryDto {
    pub source_id: Option<i64>,
    pub type_filter: String,
    pub favorites_only: bool,
    pub search: String,
    pub sort: String,
    #[serde(default)]
    pub cursor: Option<String>,
    pub limit: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryBrowserSourceDto {
    pub id: i64,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryBrowserItemDto {
    #[serde(flatten)]
    pub wallpaper: WallpaperDto,
    pub wallpaper_id: i64,
    pub favorite: bool,
    pub user_unsupported: bool,
    pub author: Option<String>,
    pub added_at: String,
    pub sources: Vec<LibraryBrowserSourceDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryBrowserPageDto {
    pub revision: u64,
    pub next_cursor: Option<String>,
    pub total: Option<usize>,
    pub items: Vec<LibraryBrowserItemDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryBrowserTotalDto {
    pub revision: u64,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryQueryErrorDto {
    pub kind: &'static str,
    pub message: String,
}

impl From<wc_core::error::WcError> for LibraryQueryErrorDto {
    fn from(error: wc_core::error::WcError) -> Self {
        let kind = match &error {
            wc_core::error::WcError::RevisionChanged { .. } => "revision_changed",
            wc_core::error::WcError::InvalidCursor { .. } => "invalid_cursor",
            _ => "storage_error",
        };
        Self {
            kind,
            message: error.to_string(),
        }
    }
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

/// Single-flight initialization cell.
///
/// Each initialization attempt is represented by an `Arc<Attempt>`. The
/// leader creates a fresh `Attempt`, transitions the global state to
/// `InProgress(arc)`, releases the lock, and runs the initializer.
///
/// Concurrent callers see `InProgress(arc)`, clone the `Arc`, drop the
/// global lock, and wait on that **specific** attempt's condvar. When the
/// leader finishes it writes the outcome into the `Attempt`, notifies
/// waiters on the attempt's own condvar, and clears the global state back
/// to `Empty` (only if it still points to the same `Arc`).
///
/// This per-attempt isolation guarantees that old waiters always receive
/// their batch's outcome, even if a fresh caller starts a new attempt
/// before all old waiters have woken up.
struct StorageCell {
    storage: OnceLock<StorageApi>,
    state: std::sync::Mutex<AttemptState>,
}

struct Attempt {
    /// `None` while in progress, `Some(Ok(()))` on success,
    /// `Some(Err(msg))` on failure.
    outcome: Mutex<Option<Result<(), String>>>,
    condvar: std::sync::Condvar,
}

impl Attempt {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            outcome: Mutex::new(None),
            condvar: std::sync::Condvar::new(),
        })
    }
}

enum AttemptState {
    Empty,
    InProgress(Arc<Attempt>),
}

impl StorageCell {
    const fn new() -> Self {
        Self {
            storage: OnceLock::new(),
            state: Mutex::new(AttemptState::Empty),
        }
    }

    /// Shared implementation. Hooks are no-ops in production and may be
    /// overridden in `#[cfg(test)]` builds via `get_or_init_with_test_hooks`.
    fn get_or_init_inner(
        &self,
        initializer: impl FnOnce() -> Result<StorageApi, String>,
        on_wait_joined: &(dyn Fn() + Send + Sync),
        on_before_read_outcome: &(dyn Fn() + Send + Sync),
    ) -> Result<&StorageApi, String> {
        // ── Fast path ──────────────────────────────────────────────────
        if let Some(storage) = self.storage.get() {
            return Ok(storage);
        }

        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());

        loop {
            match &*state {
                AttemptState::Empty => {
                    let attempt = Attempt::new();
                    *state = AttemptState::InProgress(Arc::clone(&attempt));
                    drop(state); // release the global mutex before I/O

                    match initializer() {
                        Ok(storage) => {
                            let stored = self.storage.get_or_init(|| storage);
                            {
                                let mut outcome =
                                    attempt.outcome.lock().unwrap_or_else(|e| e.into_inner());
                                *outcome = Some(Ok(()));
                            }
                            attempt.condvar.notify_all();
                            let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
                            if let AttemptState::InProgress(ref a) = *s {
                                if Arc::ptr_eq(a, &attempt) {
                                    *s = AttemptState::Empty;
                                }
                            }
                            return Ok(stored);
                        }
                        Err(error) => {
                            {
                                let mut outcome =
                                    attempt.outcome.lock().unwrap_or_else(|e| e.into_inner());
                                *outcome = Some(Err(error.clone()));
                            }
                            attempt.condvar.notify_all();
                            let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
                            if let AttemptState::InProgress(ref a) = *s {
                                if Arc::ptr_eq(a, &attempt) {
                                    *s = AttemptState::Empty;
                                }
                            }
                            return Err(error);
                        }
                    }
                }
                AttemptState::InProgress(attempt) => {
                    // Clone the Arc so we wait on THIS attempt's condvar.
                    let attempt = Arc::clone(attempt);
                    drop(state);

                    // Test seam: the waiter has cloned the attempt and released
                    // the global lock — it is now joined to this batch.
                    on_wait_joined();

                    // Test seam: pause before the waiter reads the outcome, so
                    // tests can interleave a second attempt before old waiters
                    // observe the result.
                    on_before_read_outcome();

                    let mut outcome = attempt.outcome.lock().unwrap_or_else(|e| e.into_inner());
                    while outcome.is_none() {
                        outcome = attempt
                            .condvar
                            .wait(outcome)
                            .unwrap_or_else(|e| e.into_inner());
                    }
                    match outcome.as_ref().unwrap() {
                        Ok(()) => {
                            if let Some(storage) = self.storage.get() {
                                return Ok(storage);
                            }
                            state = self.state.lock().unwrap_or_else(|e| e.into_inner());
                        }
                        Err(msg) => {
                            return Err(msg.clone());
                        }
                    }
                }
            }
        }
    }

    fn get_or_init_with(
        &self,
        initializer: impl FnOnce() -> Result<StorageApi, String>,
    ) -> Result<&StorageApi, String> {
        self.get_or_init_inner(initializer, &|| {}, &|| {})
    }

    #[cfg(test)]
    fn get_or_init_with_test_hooks(
        &self,
        initializer: impl FnOnce() -> Result<StorageApi, String>,
        on_wait_joined: &(dyn Fn() + Send + Sync),
        on_before_read_outcome: &(dyn Fn() + Send + Sync),
    ) -> Result<&StorageApi, String> {
        self.get_or_init_inner(initializer, on_wait_joined, on_before_read_outcome)
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

#[cfg(test)]
pub fn dto_from_entry(entry: WallpaperEntry) -> WallpaperDto {
    dto_from_entry_with_routing(
        entry,
        &wc_core::backend_routing::BackendRouting::default(),
        None,
    )
}

pub fn dto_from_entry_with_routing(
    entry: WallpaperEntry,
    routing: &wc_core::backend_routing::BackendRouting,
    we_compat: Option<&mut wc_storage::we_compat::WeCompatCache>,
) -> WallpaperDto {
    let project = entry.project.clone();

    let cached_failure = if entry.file_type == FileType::WeScene {
        match we_compat {
            Some(cache) => cache.lookup_failure(entry.path.as_ref()).ok().flatten(),
            None => wc_storage::we_compat::lookup_failure(entry.path.as_ref())
                .ok()
                .flatten(),
        }
    } else {
        None
    };

    let backend_failed = cached_failure.is_some();
    let error_kind = cached_failure.as_ref().map(|f| f.error_kind.as_str());
    let plan = wc_app::apply_plan::plan_for_entry_with_kind_and_routing(
        &entry,
        backend_failed,
        error_kind,
        routing,
    );
    let renderer_compatibility = plan.compatibility.as_ref().map(|c| match c {
        wc_app::apply_plan::CompatibilityKind::NativeScene { disclaimer } => disclaimer.clone(),
    });
    let effective_backend = plan.backend.unwrap_or(entry.backend);

    let mut dto = WallpaperDto {
        path: entry.path.to_string(),
        file_type: entry.file_type.as_str().to_string(),
        ext: entry.ext,
        backend: effective_backend.as_str().to_string(),
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
        mpsc, Barrier,
    };
    use wc_core::types::{Backend, FileType, WallpaperEntry, WallpaperProject};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn dto_apply_plan_uses_normalized_runtime_routing() {
        let entry = WallpaperEntry {
            path: Utf8PathBuf::from("/walls/still.jpg"),
            file_type: FileType::Image,
            ext: "jpg".into(),
            backend: Backend::Awww,
            size: 1,
            mtime: 1,
            resolution: "1920x1080".into(),
            project: None,
        };
        let routing =
            wc_core::backend_routing::BackendRouting::from_raw("mpvpaper", "awww", "mpvpaper");

        let dto = dto_from_entry_with_routing(entry, &routing, None);

        assert_eq!(dto.backend, "mpvpaper");
        assert_eq!(dto.apply_backend.as_deref(), Some("mpvpaper"));
    }

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

    /// 16 threads simultaneously enter a **failing** initializer.
    /// The initializer must be called exactly **once** and all 16 threads must
    /// receive the **same** error.
    ///
    /// The `waited` flag inside `get_or_init_with` ensures that callers who
    /// blocked on the condvar (same batch) receive the cached error while a
    /// fresh caller clears the failure and retries.
    #[test]
    fn storage_cell_single_flight_failure_16_threads() {
        let cell = StorageCell::new();
        let initializer_calls = AtomicUsize::new(0);
        let barrier = Barrier::new(16);

        let errors: Vec<String> = std::thread::scope(|scope| {
            let mut threads = Vec::with_capacity(16);
            for _ in 0..16 {
                let cell = &cell;
                let initializer_calls = &initializer_calls;
                let barrier = &barrier;
                threads.push(scope.spawn(move || {
                    barrier.wait();
                    match cell.get_or_init_with(|| {
                        // Brief sleep gives other threads time to enter
                        // condvar.wait before the leader finishes.
                        std::thread::sleep(std::time::Duration::from_millis(50));
                        initializer_calls.fetch_add(1, Ordering::SeqCst);
                        Err::<StorageApi, _>("shared initialization failure".to_string())
                    }) {
                        Err(e) => e,
                        Ok(_) => panic!("expected error"),
                    }
                }));
            }
            threads.into_iter().map(|t| t.join().unwrap()).collect()
        });

        // The initializer must have been called exactly once.
        assert_eq!(initializer_calls.load(Ordering::SeqCst), 1);
        // All 16 threads must receive the same error message.
        assert!(errors.iter().all(|e| e == "shared initialization failure"));
        assert_eq!(errors.len(), 16);
    }

    /// After a failed batch, the next explicit attempt (new `get_or_init_with`
    /// call) clears the cached failure and can succeed.  Once it succeeds the
    /// result is reused.
    #[test]
    fn storage_cell_recovers_after_failed_batch() {
        let tmp = tempfile::tempdir().unwrap();
        let cell = StorageCell::new();
        let initializer_calls = AtomicUsize::new(0);

        // First: failing batch (single-flight failure).
        let barrier = Barrier::new(8);
        let errors: Vec<String> = std::thread::scope(|scope| {
            let mut threads = Vec::with_capacity(8);
            for _ in 0..8 {
                let cell = &cell;
                let initializer_calls = &initializer_calls;
                let barrier = &barrier;
                threads.push(scope.spawn(move || {
                    barrier.wait();
                    match cell.get_or_init_with(|| {
                        std::thread::sleep(std::time::Duration::from_millis(50));
                        initializer_calls.fetch_add(1, Ordering::SeqCst);
                        Err::<StorageApi, _>("batch 1 failure".to_string())
                    }) {
                        Err(e) => e,
                        Ok(_) => panic!("expected error"),
                    }
                }));
            }
            threads.into_iter().map(|t| t.join().unwrap()).collect()
        });
        assert_eq!(initializer_calls.load(Ordering::SeqCst), 1);
        assert_eq!(errors.len(), 8);
        assert!(errors.iter().all(|e| e == "batch 1 failure"));

        // Second: new explicit attempt — must succeed.
        let config_path = tmp.path().join("config");
        let second = cell
            .get_or_init_with(|| {
                initializer_calls.fetch_add(1, Ordering::SeqCst);
                StorageApi::try_new(wc_core::ConfigDir {
                    path: config_path.clone(),
                })
                .map_err(|e| e.to_string())
            })
            .unwrap();

        // Third: result is reused without calling the initializer.
        let third = cell
            .get_or_init_with(|| panic!("success value must be reused"))
            .unwrap();

        // Total calls: 1 (failed batch) + 1 (recovery) = 2.
        assert_eq!(initializer_calls.load(Ordering::SeqCst), 2);
        assert!(std::ptr::eq(second, third));
    }

    /// Deterministic test: after the first (failing) leader finishes, a second
    /// (succeeding) attempt starts **before** old waiters have read their
    /// outcome. With per-attempt `Arc<Attempt>` isolation the old waiters still
    /// observe the **first** batch's error (not the second batch's success),
    /// and each batch's initializer fires exactly once.
    ///
    /// ## Deterministic interleaving
    ///
    /// 1. All 8 threads enter `get_or_init_with_test_hooks`. One becomes leader,
    ///    7 become waiters.
    /// 2. Each waiter clones the old `Arc<Attempt>`, drops the global lock, and
    ///    fires `on_wait_joined` (sends to a channel). The leader's initializer
    ///    blocks until all 7 `on_wait_joined` signals arrive — this guarantees
    ///    every waiter has been joined to the **first** batch.
    /// 3. Each waiter then fires `on_before_read_outcome` (pauses on a barrier)
    ///    — they are suspended *before* they lock the attempt's outcome mutex,
    ///    so the leader can still write the outcome.
    /// 4. The leader's initializer unblocks after all 7 joins, returns `Err`,
    ///    writes the outcome, notifies the condvar, and clears global state.
    /// 5. The main thread now starts a **second** (successful) attempt — new
    ///    `Arc<Attempt>`, new initializer — while the old waiters are still
    ///    paused at the barrier.
    /// 6. The main thread releases the barrier. Old waiters unpause, lock the
    ///    *old* attempt's outcome, see `Some(Err(…))`, and return the error
    ///    without ever touching the condvar.
    ///
    /// No sleeps — the interleaving is purely enforced by the channel + barrier.
    /// `recv_timeout` on every channel receive protects against permanent hangs.
    #[test]
    fn storage_cell_per_attempt_isolation_old_waiters_not_affected_by_new_attempt() {
        let cell = StorageCell::new();
        let initializer_calls = AtomicUsize::new(0);
        const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

        // Channel: waiters signal they have joined the batch.
        let (joined_tx, joined_rx) = mpsc::channel::<()>();
        let joined_rx = std::sync::Arc::new(Mutex::new(joined_rx));

        // Barrier: olds waiters pause here before reading outcome.
        // 7 waiters + 1 main thread = 8 parties.
        let pause_before_read = std::sync::Arc::new(Barrier::new(8));

        // Thread-safe outcome collector.
        let waiter_outcomes = Mutex::new(Vec::<Result<(), String>>::new());

        // All 8 threads rendezvous here before calling get_or_init_with_test_hooks.
        let enter_barrier = Barrier::new(8);

        // Channel to signal "leader has fully finished (outcome written, state
        // cleared)" so the main thread can safely start the second attempt.
        // Unbounded so sends never block; only the leader reaches the send point
        // before the pause barrier is released.
        let (leader_done_tx, leader_done_rx) = mpsc::channel::<()>();

        std::thread::scope(|scope| {
            let mut handles = Vec::with_capacity(8);
            for _ in 0..8 {
                let cell = &cell;
                let initializer_calls = &initializer_calls;
                let enter_barrier = &enter_barrier;
                let pause_before_read = Arc::clone(&pause_before_read);
                let joined_tx = joined_tx.clone();
                let waiter_outcomes = &waiter_outcomes;
                let joined_rx = Arc::clone(&joined_rx);
                let leader_done_tx = leader_done_tx.clone();

                handles.push(scope.spawn(move || {
                    enter_barrier.wait();

                    let on_wait_joined: &(dyn Fn() + Send + Sync) = &|| {
                        joined_tx.send(()).unwrap();
                    };
                    let on_before_read: &(dyn Fn() + Send + Sync) = &|| {
                        pause_before_read.wait();
                    };

                    let outcome = cell.get_or_init_with_test_hooks(
                        || {
                            initializer_calls.fetch_add(1, Ordering::SeqCst);
                            // Wait until all 7 waiters have joined the batch.
                            for _ in 0..7 {
                                joined_rx
                                    .lock()
                                    .unwrap()
                                    .recv_timeout(TIMEOUT)
                                    .expect("timed out waiting for waiters to join");
                            }
                            Err::<StorageApi, _>("batch1_fatal_error".to_string())
                        },
                        on_wait_joined,
                        on_before_read,
                    );

                    // Only the leader reaches this point before the pause
                    // barrier is released — all waiters are blocked inside
                    // on_before_read_outcome. At this point the leader has
                    // written the outcome, notified condvar, and cleared
                    // global state back to Empty.
                    let _ = leader_done_tx.send(());

                    let mapped = match outcome {
                        Ok(_) => Ok(()),
                        Err(msg) => Err(msg),
                    };
                    waiter_outcomes.lock().unwrap().push(mapped);
                }));
            }

            // Drop the main thread's sender so the channel can eventually close.
            drop(leader_done_tx);

            // ── Wait for the leader to finish completely ────────────────
            leader_done_rx
                .recv_timeout(TIMEOUT)
                .expect("timed out waiting for leader to finish batch 1");

            // ── Start second (successful) attempt while old waiters ─────
            // are still paused at on_before_read_outcome. Global state is
            // now Empty so this creates a brand-new Arc<Attempt>.
            let tmp = tempfile::tempdir().unwrap();
            let config_path = tmp.path().join("config");
            let _second = cell.get_or_init_with(|| {
                initializer_calls.fetch_add(1, Ordering::SeqCst);
                StorageApi::try_new(wc_core::ConfigDir { path: config_path })
                    .map_err(|e| e.to_string())
            });

            // ── Release the barrier — old waiters now read old outcome ──
            pause_before_read.wait();

            for h in handles {
                h.join().unwrap();
            }
        });

        // ── Assertions ──────────────────────────────────────────────────
        let outcomes = waiter_outcomes.lock().unwrap();
        assert_eq!(outcomes.len(), 8, "all 8 threads must record an outcome");

        // ALL waiters (including those that paused) must see batch1_fatal_error.
        for (i, outcome) in outcomes.iter().enumerate() {
            assert!(
                outcome.is_err(),
                "waiter {i} should see error, got {outcome:?}"
            );
            assert_eq!(
                outcome.as_ref().unwrap_err(),
                "batch1_fatal_error",
                "waiter {i} should see batch1 error"
            );
        }

        // Initializer called exactly twice: once for batch1, once for batch2.
        assert_eq!(initializer_calls.load(Ordering::SeqCst), 2);
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
