use std::sync::{Mutex, OnceLock};
#[cfg(test)]
use wc_config::ConfigDirExt;

use wc_app::library_refresh::{refresh_library_source, LibraryRefreshError, LibraryRefreshReport};
use wc_app::library_rescan::{run_library_rescan, LibraryRescanError, LibraryRescanReport};
use wc_scan::{ScanControl, ScanStats, SourceScanEvent};
use wc_storage::SourceRecord;

use super::common::{fail, ok, storage, CommandResult, ScanProgressDto};

static SCAN_STATE: OnceLock<Mutex<ScanProgressDto>> = OnceLock::new();

#[cfg(test)]
pub(crate) static TEST_SCAN_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IndexSourcesResult {
    pub indexed: usize,
    pub library_total: usize,
    pub removed: usize,
    pub removed_we_workshop_ids: Vec<String>,
    pub offline_sources: usize,
    pub incomplete_sources: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StartupRefreshOutcome {
    NoConfiguredSources,
    ScanAlreadyRunning { stage: String },
    Refreshed(IndexSourcesResult),
}

pub(crate) fn format_index_sources_message(result: &IndexSourcesResult) -> String {
    let mut msg = format!(
        "Scan complete. Library contains {} wallpaper(s).",
        result.library_total
    );
    if result.removed == 1 && result.removed_we_workshop_ids.len() == 1 {
        msg.push_str(&format!(
            " Removed {} missing workshop item ({}).",
            result.removed, result.removed_we_workshop_ids[0]
        ));
    } else if result.removed > 0 && !result.removed_we_workshop_ids.is_empty() {
        msg.push_str(&format!(
            " Removed {} missing item(s), including workshop ID(s): {}.",
            result.removed,
            result.removed_we_workshop_ids.join(", ")
        ));
    } else if result.removed > 0 {
        msg.push_str(&format!(" Removed {} missing item(s).", result.removed));
    }
    if result.offline_sources > 0 {
        msg.push_str(&format!(
            " Preserved {} offline source snapshot(s).",
            result.offline_sources
        ));
    }
    if result.incomplete_sources > 0 {
        msg.push_str(&format!(
            " Preserved {} incomplete source snapshot(s).",
            result.incomplete_sources
        ));
    }
    msg
}

fn scan_state() -> &'static Mutex<ScanProgressDto> {
    SCAN_STATE.get_or_init(|| {
        Mutex::new(ScanProgressDto {
            running: false,
            stage: "idle".into(),
            scanned: 0,
            total_hint: None,
            reused_metadata: 0,
            probed_metadata: 0,
            inserted_sqlite: 0,
            staged: 0,
            skipped: 0,
            metadata_errors: 0,
            current_path: None,
            cancel_requested: false,
            error: None,
        })
    })
}

#[cfg(test)]
pub(crate) fn reset_scan_state_for_test() {
    if let Ok(mut state) = scan_state().lock() {
        *state = ScanProgressDto {
            running: false,
            stage: "idle".into(),
            scanned: 0,
            total_hint: None,
            reused_metadata: 0,
            probed_metadata: 0,
            inserted_sqlite: 0,
            staged: 0,
            skipped: 0,
            metadata_errors: 0,
            current_path: None,
            cancel_requested: false,
            error: None,
        };
    }
}

pub(crate) fn current_scan_progress_snapshot() -> ScanProgressDto {
    match scan_state().lock() {
        Ok(state) => state.clone(),
        Err(_) => ScanProgressDto {
            running: false,
            stage: "idle".into(),
            scanned: 0,
            total_hint: None,
            reused_metadata: 0,
            probed_metadata: 0,
            inserted_sqlite: 0,
            staged: 0,
            skipped: 0,
            metadata_errors: 0,
            current_path: None,
            cancel_requested: false,
            error: None,
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ScanStartOutcome {
    Started,
    AlreadyRunning { stage: String },
}

fn try_mark_scan_started(stage: &str) -> Result<ScanStartOutcome, String> {
    let mut state = scan_state().lock().map_err(|e| e.to_string())?;
    if state.running {
        return Ok(ScanStartOutcome::AlreadyRunning {
            stage: state.stage.clone(),
        });
    }
    *state = ScanProgressDto {
        running: true,
        stage: stage.into(),
        scanned: 0,
        total_hint: None,
        reused_metadata: 0,
        probed_metadata: 0,
        inserted_sqlite: 0,
        staged: 0,
        skipped: 0,
        metadata_errors: 0,
        current_path: None,
        cancel_requested: false,
        error: None,
    };
    Ok(ScanStartOutcome::Started)
}

pub(crate) fn mark_scan_started(stage: &str) -> Result<(), String> {
    match try_mark_scan_started(stage)? {
        ScanStartOutcome::Started => Ok(()),
        ScanStartOutcome::AlreadyRunning { stage } => Err(format!("Scan already running: {stage}")),
    }
}

pub(crate) fn with_scan_idle_operation<T>(operation: impl FnOnce() -> T) -> Result<T, String> {
    let state = scan_state().lock().map_err(|error| error.to_string())?;
    if state.running {
        return Err(format!(
            "Cannot modify sources while a scan is running: {}",
            state.stage
        ));
    }
    Ok(operation())
}

pub(crate) fn update_scan_stage(stage: &str) {
    if let Ok(mut state) = scan_state().lock() {
        state.stage = stage.into();
    }
}

fn scan_cancelled() -> Result<bool, String> {
    scan_state()
        .lock()
        .map(|state| state.cancel_requested)
        .map_err(|e| e.to_string())
}

pub(crate) fn finish_scan_success() {
    if let Ok(mut state) = scan_state().lock() {
        state.running = false;
        state.stage = "idle".into();
        state.current_path = None;
        state.cancel_requested = false;
    }
}

pub(crate) fn finish_scan_error(err: &str) {
    if let Ok(mut state) = scan_state().lock() {
        state.running = false;
        state.error = Some(err.to_string());
        state.current_path = None;
    }
}

#[derive(Debug, Default)]
struct LiveScanProgress {
    current_source_id: Option<i64>,
    completed: ScanStats,
    current: ScanStats,
}

pub(crate) struct BackgroundScanPresentation {
    owned: bool,
    progress: LiveScanProgress,
}

pub(crate) fn begin_background_scan() -> BackgroundScanPresentation {
    BackgroundScanPresentation {
        owned: matches!(
            try_mark_scan_started("building library"),
            Ok(ScanStartOutcome::Started)
        ),
        progress: LiveScanProgress::default(),
    }
}

impl BackgroundScanPresentation {
    pub(crate) fn observe(
        &mut self,
        source: &SourceRecord,
        event: &SourceScanEvent,
    ) -> ScanControl {
        if self.owned {
            update_scan_progress(&mut self.progress, source, event)
        } else {
            ScanControl::Continue
        }
    }

    pub(crate) fn finish(
        self,
        result: &Result<LibraryRefreshReport, LibraryRefreshError>,
        storage: &wc_storage::StorageApi,
    ) {
        if !self.owned {
            return;
        }
        match result {
            Ok(report) => {
                let total = wc_storage::sqlite::source_backed_library_counts_sqlite(&storage.cd)
                    .map(|counts| counts.total)
                    .unwrap_or(0);
                apply_refresh_report_to_progress(report, total);
                finish_scan_success();
            }
            Err(error) => finish_scan_error(&error.to_string()),
        }
    }
}

impl LiveScanProgress {
    fn observe(&mut self, source: &SourceRecord, event: &SourceScanEvent) -> ScanStats {
        if self.current_source_id != Some(source.id) {
            if self.current_source_id.is_some() {
                self.completed.entries_visited = self
                    .completed
                    .entries_visited
                    .saturating_add(self.current.entries_visited);
                self.completed.candidates_found = self
                    .completed
                    .candidates_found
                    .saturating_add(self.current.candidates_found);
                self.completed.entries_indexed = self
                    .completed
                    .entries_indexed
                    .saturating_add(self.current.entries_indexed);
                self.completed.metadata_reused = self
                    .completed
                    .metadata_reused
                    .saturating_add(self.current.metadata_reused);
            }
            self.current_source_id = Some(source.id);
            self.current = ScanStats::default();
        }
        if let SourceScanEvent::EntryVisited { stats, .. }
        | SourceScanEvent::CandidateFound { stats, .. } = event
        {
            self.current = *stats;
        }
        ScanStats {
            entries_visited: self
                .completed
                .entries_visited
                .saturating_add(self.current.entries_visited),
            candidates_found: self
                .completed
                .candidates_found
                .saturating_add(self.current.candidates_found),
            entries_indexed: self
                .completed
                .entries_indexed
                .saturating_add(self.current.entries_indexed),
            metadata_reused: self
                .completed
                .metadata_reused
                .saturating_add(self.current.metadata_reused),
        }
    }
}

fn update_scan_progress(
    progress: &mut LiveScanProgress,
    source: &SourceRecord,
    event: &SourceScanEvent,
) -> ScanControl {
    let totals = progress.observe(source, event);
    let Ok(mut state) = scan_state().lock() else {
        return ScanControl::Cancel;
    };
    if state.cancel_requested {
        return ScanControl::Cancel;
    }

    match event {
        SourceScanEvent::SourceStarted { path } => {
            state.stage = "walking files".into();
            state.current_path = Some(path.to_string_lossy().into_owned());
        }
        SourceScanEvent::EntryVisited { path, .. } => {
            state.stage = "walking files".into();
            state.current_path = Some(path.to_string_lossy().into_owned());
        }
        SourceScanEvent::CandidateFound { path, .. } => {
            state.stage = "reading metadata".into();
            state.current_path = Some(path.to_string_lossy().into_owned());
        }
    }
    state.scanned = totals.entries_visited;
    state.total_hint = None;
    state.reused_metadata = totals.metadata_reused;
    state.probed_metadata = totals
        .entries_indexed
        .saturating_sub(totals.metadata_reused);
    state.staged = totals.entries_indexed;
    ScanControl::Continue
}

fn apply_refresh_report_to_progress(report: &LibraryRefreshReport, unique_library_count: usize) {
    if let Ok(mut state) = scan_state().lock() {
        state.scanned = report.metadata.entries_visited;
        state.total_hint = Some(report.metadata.entries_visited);
        state.reused_metadata = report.metadata.metadata_reused;
        state.probed_metadata = report
            .metadata
            .entries_indexed
            .saturating_sub(report.metadata.metadata_reused);
        state.inserted_sqlite = unique_library_count;
        state.staged = report.indexed;
        state.skipped = report
            .metadata
            .candidates_found
            .saturating_sub(report.metadata.entries_indexed);
        state.metadata_errors = report.incomplete_sources;
        state.current_path = None;
    }
}

fn index_result_from_report(
    report: &LibraryRefreshReport,
    unique_library_count: usize,
) -> IndexSourcesResult {
    IndexSourcesResult {
        indexed: report.indexed,
        library_total: unique_library_count,
        removed: report.wallpapers_removed,
        removed_we_workshop_ids: report.removed_we_workshop_ids.clone(),
        offline_sources: report.offline_sources,
        incomplete_sources: report.incomplete_sources,
    }
}

fn index_sources_with_event_control<F>(
    storage: &wc_storage::StorageApi,
    mut on_event: F,
) -> Result<IndexSourcesResult, String>
where
    F: FnMut(&SourceRecord, &SourceScanEvent) -> ScanControl,
{
    finish_rescan(
        storage,
        run_library_rescan(storage, |source, event| on_event(source, event)),
    )
}

fn index_source_with_event_control<F>(
    storage: &wc_storage::StorageApi,
    source_id: i64,
    mut on_event: F,
) -> Result<IndexSourcesResult, String>
where
    F: FnMut(&SourceRecord, &SourceScanEvent) -> ScanControl,
{
    finish_refresh(
        storage,
        refresh_library_source(storage, source_id, |source, event| on_event(source, event)),
    )
}

fn finish_rescan(
    storage: &wc_storage::StorageApi,
    rescan: Result<LibraryRescanReport, LibraryRescanError>,
) -> Result<IndexSourcesResult, String> {
    match rescan {
        Ok(report) => {
            let refresh = report.refresh.unwrap_or_default();
            let unique_library_count = report.snapshot_count;
            apply_refresh_report_to_progress(&refresh, unique_library_count);
            Ok(index_result_from_report(&refresh, unique_library_count))
        }
        Err(LibraryRescanError::Refresh(LibraryRefreshError::Cancelled { report, .. })) => {
            let unique_library_count = wc_storage::sqlite::source_backed_library_count(&storage.cd)
                .unwrap_or(report.indexed);
            apply_refresh_report_to_progress(&report, unique_library_count);
            Err("scan cancelled".to_string())
        }
        Err(LibraryRescanError::Refresh(LibraryRefreshError::Storage {
            report, error, ..
        })) => {
            let unique_library_count = wc_storage::sqlite::source_backed_library_count(&storage.cd)
                .unwrap_or(report.indexed);
            apply_refresh_report_to_progress(&report, unique_library_count);
            Err(error.to_string())
        }
        Err(error) => Err(error.to_string()),
    }
}

fn finish_refresh(
    storage: &wc_storage::StorageApi,
    refresh: Result<LibraryRefreshReport, LibraryRefreshError>,
) -> Result<IndexSourcesResult, String> {
    match refresh {
        Ok(report) => {
            let unique_library_count = wc_storage::sqlite::source_backed_library_count(&storage.cd)
                .map_err(|error| error.to_string())?;
            apply_refresh_report_to_progress(&report, unique_library_count);
            Ok(index_result_from_report(&report, unique_library_count))
        }
        Err(LibraryRefreshError::Cancelled { report, .. }) => {
            let unique_library_count = wc_storage::sqlite::source_backed_library_count(&storage.cd)
                .unwrap_or(report.indexed);
            apply_refresh_report_to_progress(&report, unique_library_count);
            Err("scan cancelled".to_string())
        }
        Err(LibraryRefreshError::Storage { report, error, .. }) => {
            let unique_library_count = wc_storage::sqlite::source_backed_library_count(&storage.cd)
                .unwrap_or(report.indexed);
            apply_refresh_report_to_progress(&report, unique_library_count);
            Err(error.to_string())
        }
        Err(LibraryRefreshError::ScanBusy { source_id, waited }) => Err(format!(
            "source {source_id} is already being scanned (waited {} ms)",
            waited.as_millis()
        )),
    }
}

pub(crate) fn index_current_sources(
    s: &wc_storage::StorageApi,
) -> Result<IndexSourcesResult, String> {
    update_scan_stage("loading sources");
    if scan_cancelled()? {
        return Err("scan cancelled".to_string());
    }
    let mut progress = LiveScanProgress::default();
    index_sources_with_event_control(s, |source, event| {
        update_scan_progress(&mut progress, source, event)
    })
}

pub(crate) fn index_source_by_id(
    storage: &wc_storage::StorageApi,
    source_id: i64,
) -> Result<IndexSourcesResult, String> {
    update_scan_stage("loading source");
    if scan_cancelled()? {
        return Err("scan cancelled".to_string());
    }
    let mut progress = LiveScanProgress::default();
    index_source_with_event_control(storage, source_id, |source, event| {
        update_scan_progress(&mut progress, source, event)
    })
}

/// Discover/add Wallpaper Engine workshop roots, then run a full library rescan with live progress.
pub(crate) fn scan_steam_workshop_and_index(
    storage: &wc_storage::StorageApi,
    home: &std::path::Path,
) -> Result<String, String> {
    update_scan_stage("adding Wallpaper Engine sources");
    if scan_cancelled()? {
        return Err("scan cancelled".to_string());
    }
    let mut progress = LiveScanProgress::default();
    let report =
        wc_app::sources_maintenance::scan_steam_workshop(storage, home, true, |source, event| {
            update_scan_progress(&mut progress, source, event)
        })
        .map_err(|error| error.to_string())?;

    if report.roots.is_empty() {
        return Ok(
            "No Wallpaper Engine workshop directory found. Install or download Wallpaper Engine workshop content in Steam, then scan again."
                .to_string(),
        );
    }

    let rescan = report
        .rescan
        .ok_or_else(|| "steam workshop scan did not produce a library rescan report".to_string())?;
    let index_result = finish_rescan(storage, Ok(rescan))?;
    Ok(format!(
        "Wallpaper Engine scan complete. {} source root(s) found, {} new source(s). {}",
        report.roots.len(),
        report.added_paths.len(),
        format_index_sources_message(&index_result)
    ))
}

pub(crate) fn run_startup_source_refresh_with<SourceCount, Refresh>(
    configured_source_count: SourceCount,
    refresh: Refresh,
) -> Result<StartupRefreshOutcome, String>
where
    SourceCount: FnOnce() -> Result<usize, String>,
    Refresh: FnOnce() -> Result<IndexSourcesResult, String>,
{
    if configured_source_count()? == 0 {
        return Ok(StartupRefreshOutcome::NoConfiguredSources);
    }

    match try_mark_scan_started("starting startup refresh")? {
        ScanStartOutcome::AlreadyRunning { stage } => {
            return Ok(StartupRefreshOutcome::ScanAlreadyRunning { stage });
        }
        ScanStartOutcome::Started => {}
    }

    match refresh() {
        Ok(result) => {
            finish_scan_success();
            Ok(StartupRefreshOutcome::Refreshed(result))
        }
        Err(error) => {
            finish_scan_error(&error);
            Err(error)
        }
    }
}

pub(crate) fn run_configured_source_startup_refresh(
    storage: &wc_storage::StorageApi,
) -> Result<StartupRefreshOutcome, String> {
    run_startup_source_refresh_with(
        || {
            storage
                .source_records()
                .map(|sources| sources.len())
                .map_err(|error| error.to_string())
        },
        || index_current_sources(storage),
    )
}

#[allow(dead_code)]
pub(crate) fn schedule_startup_source_refresh() {
    tauri::async_runtime::spawn(async {
        let worker = tauri::async_runtime::spawn_blocking(|| {
            let storage = storage()?;
            run_configured_source_startup_refresh(storage)
        })
        .await;

        match worker {
            Ok(Ok(StartupRefreshOutcome::NoConfiguredSources)) => {
                log::debug!("Startup library refresh skipped: no configured sources");
            }
            Ok(Ok(StartupRefreshOutcome::ScanAlreadyRunning { stage })) => {
                log::info!(
                    "Startup library refresh skipped because another scan is running: {stage}"
                );
            }
            Ok(Ok(StartupRefreshOutcome::Refreshed(result))) => {
                log::info!(
                    "Startup library refresh finished: {}",
                    format_index_sources_message(&result)
                );
            }
            Ok(Err(error)) => {
                log::warn!("Startup library refresh failed: {error}");
            }
            Err(error) => {
                log::error!("Startup library refresh worker stopped unexpectedly: {error}");
            }
        }
    });
}

#[tauri::command]
pub async fn rescan(
    state: tauri::State<'_, crate::library_service::LibraryService>,
) -> Result<CommandResult, String> {
    let service = state.inner().clone();
    service.manual_refresh_all_requested();
    Ok(tauri::async_runtime::spawn_blocking(move || {
        if let Err(err) = mark_scan_started("starting scan") {
            return fail(err);
        }

        let result: Result<String, String> = (|| {
            let s = storage()?;
            let index_result = index_current_sources(s)?;
            Ok(format_index_sources_message(&index_result))
        })();

        match result {
            Ok(msg) => {
                service.invalidate_local_write();
                finish_scan_success();
                ok(msg)
            }
            Err(err) => {
                finish_scan_error(&err);
                fail(err)
            }
        }
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string())))
}

#[tauri::command]
pub async fn scan_progress() -> Result<ScanProgressDto, String> {
    Ok(scan_state().lock().map_err(|e| e.to_string())?.clone())
}

#[tauri::command]
pub async fn scan_cancel() -> CommandResult {
    request_scan_cancel()
}

pub(crate) fn request_scan_cancel() -> CommandResult {
    match scan_state().lock() {
        Ok(mut state) => {
            if state.running {
                state.cancel_requested = true;
                ok("Cancel requested.")
            } else {
                ok("No scan is running.")
            }
        }
        Err(e) => fail(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn startup_refresh_result(indexed: usize) -> IndexSourcesResult {
        IndexSourcesResult {
            indexed,
            library_total: indexed,
            removed: 0,
            removed_we_workshop_ids: Vec::new(),
            offline_sources: 0,
            incomplete_sources: 0,
        }
    }

    #[test]
    fn startup_refresh_skips_scan_completely_without_configured_sources() {
        let _guard = TEST_SCAN_LOCK.lock().unwrap();
        reset_scan_state_for_test();
        let refresh_called = Cell::new(false);

        let outcome = run_startup_source_refresh_with(
            || Ok(0),
            || {
                refresh_called.set(true);
                Ok(startup_refresh_result(0))
            },
        )
        .unwrap();

        assert_eq!(outcome, StartupRefreshOutcome::NoConfiguredSources);
        assert!(!refresh_called.get(), "zero sources must not enter refresh");
        assert_eq!(current_scan_progress_snapshot().stage, "idle");
        assert!(!current_scan_progress_snapshot().running);
    }

    #[test]
    fn startup_refresh_runs_existing_refresh_path_for_configured_sources() {
        let _guard = TEST_SCAN_LOCK.lock().unwrap();
        reset_scan_state_for_test();
        let refresh_called = Cell::new(0);

        let outcome = run_startup_source_refresh_with(
            || Ok(2),
            || {
                refresh_called.set(refresh_called.get() + 1);
                update_scan_stage("walking files");
                Ok(startup_refresh_result(7))
            },
        )
        .unwrap();

        assert_eq!(
            outcome,
            StartupRefreshOutcome::Refreshed(startup_refresh_result(7))
        );
        assert_eq!(refresh_called.get(), 1);
        let progress = current_scan_progress_snapshot();
        assert!(!progress.running);
        assert_eq!(progress.stage, "idle");
        assert_eq!(progress.error, None);
    }

    #[test]
    fn startup_refresh_does_not_compete_with_an_active_scan() {
        let _guard = TEST_SCAN_LOCK.lock().unwrap();
        reset_scan_state_for_test();
        mark_scan_started("manual refresh").unwrap();
        let refresh_called = Cell::new(false);

        let outcome = run_startup_source_refresh_with(
            || Ok(1),
            || {
                refresh_called.set(true);
                Ok(startup_refresh_result(1))
            },
        )
        .unwrap();

        assert_eq!(
            outcome,
            StartupRefreshOutcome::ScanAlreadyRunning {
                stage: "manual refresh".into()
            }
        );
        assert!(!refresh_called.get());
        let progress = current_scan_progress_snapshot();
        assert!(progress.running);
        assert_eq!(progress.stage, "manual refresh");
        finish_scan_success();
    }

    #[test]
    fn startup_refresh_records_refresh_failure_without_panicking() {
        let _guard = TEST_SCAN_LOCK.lock().unwrap();
        reset_scan_state_for_test();

        let error = run_startup_source_refresh_with(
            || Ok(1),
            || Err("database temporarily unavailable".into()),
        )
        .unwrap_err();

        assert_eq!(error, "database temporarily unavailable");
        let progress = current_scan_progress_snapshot();
        assert!(!progress.running);
        assert_eq!(progress.error.as_deref(), Some(error.as_str()));
    }

    #[test]
    fn configured_source_startup_refresh_reuses_cached_metadata() {
        let _guard = TEST_SCAN_LOCK.lock().unwrap();
        reset_scan_state_for_test();
        let tmp = tempfile::tempdir().unwrap();
        let cd = wc_core::ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        let storage = wc_storage::StorageApi::new(cd);
        let source_path = tmp.path().join("walls");
        std::fs::create_dir_all(&source_path).unwrap();
        std::fs::write(source_path.join("cached.jpg"), b"wallpaper").unwrap();
        storage
            .source_create(&source_path.to_string_lossy())
            .unwrap();
        seed_source_snapshot(&storage);

        let outcome = run_configured_source_startup_refresh(&storage).unwrap();

        assert!(matches!(outcome, StartupRefreshOutcome::Refreshed(_)));
        let progress = current_scan_progress_snapshot();
        assert_eq!(progress.reused_metadata, 1);
        assert_eq!(progress.probed_metadata, 0);
        assert!(!progress.running);
    }

    #[test]
    fn scan_start_rejects_concurrent_scan() {
        let _guard = TEST_SCAN_LOCK.lock().unwrap();
        reset_scan_state_for_test();
        mark_scan_started("first").unwrap();
        let err = mark_scan_started("second").unwrap_err();
        assert!(err.contains("already running"));
        finish_scan_success();
    }

    #[test]
    fn scan_finish_error_records_error() {
        let _guard = TEST_SCAN_LOCK.lock().unwrap();
        reset_scan_state_for_test();
        mark_scan_started("first").unwrap();
        finish_scan_error("boom");
        let state = scan_state().lock().unwrap().clone();
        assert!(!state.running);
        assert_eq!(state.error.as_deref(), Some("boom"));
    }

    #[test]
    fn scan_cancel_sets_cancel_requested_when_running() {
        let _guard = TEST_SCAN_LOCK.lock().unwrap();
        reset_scan_state_for_test();
        mark_scan_started("walking files").unwrap();
        {
            let mut state = scan_state().lock().unwrap();
            state.cancel_requested = true;
        }
        assert!(scan_cancelled().unwrap());
        finish_scan_error("scan cancelled");
    }

    #[test]
    fn scan_state_reset_zeroes_new_counters() {
        let _guard = TEST_SCAN_LOCK.lock().unwrap();
        reset_scan_state_for_test();
        {
            let mut state = scan_state().lock().unwrap();
            state.staged = 5;
            state.skipped = 7;
            state.metadata_errors = 9;
        }
        reset_scan_state_for_test();
        let state = scan_state().lock().unwrap().clone();
        assert_eq!(state.staged, 0);
        assert_eq!(state.skipped, 0);
        assert_eq!(state.metadata_errors, 0);
    }

    #[test]
    fn current_scan_progress_snapshot_returns_idle_state() {
        let _guard = TEST_SCAN_LOCK.lock().unwrap();
        reset_scan_state_for_test();
        let snap = current_scan_progress_snapshot();
        assert!(!snap.running, "idle snapshot should not be running");
        assert_eq!(snap.stage, "idle");
        assert_eq!(snap.scanned, 0);
        assert_eq!(snap.staged, 0);
        assert_eq!(snap.skipped, 0);
    }

    #[test]
    fn scan_progress_dto_serializes_new_fields_camel_case() {
        let dto = ScanProgressDto {
            running: true,
            stage: "reading metadata".into(),
            scanned: 10,
            total_hint: Some(20),
            reused_metadata: 3,
            probed_metadata: 5,
            inserted_sqlite: 8,
            staged: 8,
            skipped: 2,
            metadata_errors: 0,
            current_path: Some("/x/y.jpg".into()),
            cancel_requested: false,
            error: None,
        };
        let json = serde_json::to_string(&dto).unwrap();
        assert!(
            json.contains("\"staged\":8"),
            "staged should serialize as camelCase: {json}"
        );
        assert!(
            json.contains("\"skipped\":2"),
            "skipped should serialize as camelCase: {json}"
        );
        assert!(
            json.contains("\"metadataErrors\":0"),
            "metadataErrors should serialize as camelCase: {json}"
        );
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["staged"], 8);
        assert_eq!(v["skipped"], 2);
        assert_eq!(v["metadataErrors"], 0);
    }

    #[test]
    fn index_current_sources_counts_staged_and_skipped() {
        let _guard = TEST_SCAN_LOCK.lock().unwrap();
        reset_scan_state_for_test();

        let tmp = tempfile::tempdir().unwrap();
        let cd = wc_core::ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        let s = wc_storage::StorageApi::new(cd);

        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("wall.jpg"), b"not a real image").unwrap();
        s.sources_add(&src.to_string_lossy()).unwrap();

        let result = index_current_sources(&s).unwrap();
        assert_eq!(result.library_total, 1);
        assert_eq!(result.removed, 0);

        let state = scan_state().lock().unwrap().clone();
        assert_eq!(
            state.scanned, 1,
            "scanned should count the one visited file"
        );
        assert_eq!(state.staged, 1, "staged should count the one pushed entry");
        assert_eq!(
            state.skipped, 0,
            "skipped should stay 0 for a supported file"
        );
        assert_eq!(
            state.inserted_sqlite, 1,
            "inserted_sqlite should match committed count"
        );
        assert_eq!(
            state.metadata_errors, 0,
            "metadata_errors is always 0 for now"
        );
    }

    fn seed_source_snapshot(storage: &wc_storage::StorageApi) {
        wc_app::library_refresh::refresh_library_sources(storage, |_, _| {
            wc_scan::ScanControl::Continue
        })
        .expect("initial complete refresh should publish the source snapshot");
    }

    fn library_and_membership_counts(storage: &wc_storage::StorageApi) -> (i64, i64) {
        let connection = rusqlite::Connection::open(storage.cd.db_path()).unwrap();
        connection
            .query_row(
                "SELECT
                    (SELECT COUNT(*) FROM wallpapers),
                    (SELECT COUNT(*) FROM wallpaper_sources)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap()
    }

    #[test]
    fn index_current_sources_preserves_offline_source_snapshot() {
        let _guard = TEST_SCAN_LOCK.lock().unwrap();
        reset_scan_state_for_test();

        let tmp = tempfile::tempdir().unwrap();
        let cd = wc_core::ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        let storage = wc_storage::StorageApi::new(cd);
        let source_path = tmp.path().join("offline-source");
        std::fs::create_dir_all(&source_path).unwrap();
        std::fs::write(source_path.join("wall.jpg"), b"wallpaper").unwrap();
        let source = storage
            .source_create(&source_path.to_string_lossy())
            .unwrap();
        seed_source_snapshot(&storage);
        assert_eq!(library_and_membership_counts(&storage), (1, 1));

        std::fs::remove_dir_all(&source_path).unwrap();
        let result = index_current_sources(&storage).unwrap();

        assert_eq!(
            library_and_membership_counts(&storage),
            (1, 1),
            "an offline source must retain both its wallpaper and membership"
        );
        assert_eq!(
            storage.source_records().unwrap()[0].availability,
            wc_storage::SourceAvailability::Offline
        );
        assert!(
            format_index_sources_message(&result).contains("offline"),
            "the successful partial result should explain that offline data was preserved"
        );
        assert_eq!(storage.source_records().unwrap()[0].id, source.id);
    }

    #[test]
    fn index_current_sources_preserves_incomplete_source_snapshot() {
        let _guard = TEST_SCAN_LOCK.lock().unwrap();
        reset_scan_state_for_test();

        let tmp = tempfile::tempdir().unwrap();
        let cd = wc_core::ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        let storage = wc_storage::StorageApi::new(cd);
        let source_path = tmp.path().join("incomplete-source");
        std::fs::create_dir_all(&source_path).unwrap();
        std::fs::write(source_path.join("wall.jpg"), b"wallpaper").unwrap();
        storage
            .source_create(&source_path.to_string_lossy())
            .unwrap();
        seed_source_snapshot(&storage);
        assert_eq!(library_and_membership_counts(&storage), (1, 1));

        std::fs::remove_dir_all(&source_path).unwrap();
        std::fs::write(&source_path, b"not a directory").unwrap();
        let result = index_current_sources(&storage).unwrap();

        assert_eq!(
            library_and_membership_counts(&storage),
            (1, 1),
            "an incomplete source must retain both its wallpaper and membership"
        );
        assert_eq!(
            storage.source_records().unwrap()[0].availability,
            wc_storage::SourceAvailability::Unknown
        );
        assert!(
            format_index_sources_message(&result).contains("incomplete"),
            "the successful partial result should explain that incomplete data was preserved"
        );
    }

    #[test]
    fn cancelling_later_source_keeps_prior_commit_and_current_snapshot() {
        let _guard = TEST_SCAN_LOCK.lock().unwrap();
        reset_scan_state_for_test();

        let tmp = tempfile::tempdir().unwrap();
        let cd = wc_core::ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        let storage = wc_storage::StorageApi::new(cd);
        let first_path = tmp.path().join("a-source");
        let second_path = tmp.path().join("b-source");
        std::fs::create_dir_all(&first_path).unwrap();
        std::fs::create_dir_all(&second_path).unwrap();
        std::fs::write(first_path.join("old-first.jpg"), b"first").unwrap();
        std::fs::write(second_path.join("old-second.jpg"), b"second").unwrap();
        storage
            .source_create(&first_path.to_string_lossy())
            .unwrap();
        let second = storage
            .source_create(&second_path.to_string_lossy())
            .unwrap();
        seed_source_snapshot(&storage);

        let new_first = first_path.join("new-first.jpg");
        let new_second = second_path.join("new-second.jpg");
        std::fs::write(&new_first, b"new first").unwrap();
        std::fs::write(&new_second, b"new second").unwrap();

        let error = index_sources_with_event_control(&storage, |source, event| {
            if source.id == second.id && matches!(event, SourceScanEvent::SourceStarted { .. }) {
                ScanControl::Cancel
            } else {
                ScanControl::Continue
            }
        })
        .unwrap_err();

        assert!(error.contains("cancelled"));
        let connection = rusqlite::Connection::open(storage.cd.db_path()).unwrap();
        let indexed_paths = connection
            .prepare("SELECT path FROM wallpapers ORDER BY path")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(indexed_paths.contains(&new_first.to_string_lossy().to_string()));
        assert!(
            !indexed_paths.contains(&new_second.to_string_lossy().to_string()),
            "the cancelled source must not publish its partial snapshot"
        );
        assert!(
            indexed_paths.contains(
                &second_path
                    .join("old-second.jpg")
                    .to_string_lossy()
                    .to_string()
            ),
            "the cancelled source must retain its previous committed snapshot"
        );
        let membership_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM wallpaper_sources", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(membership_count, 3);
    }

    #[test]
    fn overlapping_sources_report_unique_sqlite_wallpaper_count() {
        let _guard = TEST_SCAN_LOCK.lock().unwrap();
        reset_scan_state_for_test();

        let tmp = tempfile::tempdir().unwrap();
        let cd = wc_core::ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        let storage = wc_storage::StorageApi::new(cd);
        let root = tmp.path().join("walls");
        let nested = root.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("shared.jpg"), b"shared").unwrap();
        storage.source_create(&root.to_string_lossy()).unwrap();
        storage.source_create(&nested.to_string_lossy()).unwrap();

        let result = index_current_sources(&storage).unwrap();

        assert_eq!(
            result.library_total, 1,
            "overlapping sources render one card"
        );
        let state = current_scan_progress_snapshot();
        assert_eq!(
            state.inserted_sqlite, 1,
            "progress must report unique SQLite rows, not source observations"
        );
        assert_eq!(library_and_membership_counts(&storage), (1, 2));
    }

    #[test]
    fn index_current_sources_does_not_count_orphans_after_last_source_is_removed() {
        let _guard = TEST_SCAN_LOCK.lock().unwrap();
        reset_scan_state_for_test();

        let tmp = tempfile::tempdir().unwrap();
        let cd = wc_core::ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        let storage = wc_storage::StorageApi::new(cd);
        let source_path = tmp.path().join("walls");
        std::fs::create_dir(&source_path).unwrap();
        std::fs::write(source_path.join("orphan.jpg"), b"wallpaper").unwrap();
        let source = storage
            .source_create(&source_path.to_string_lossy())
            .unwrap();
        seed_source_snapshot(&storage);
        storage.source_remove_by_id(source.id).unwrap();
        assert_eq!(library_and_membership_counts(&storage), (1, 0));

        let result = index_current_sources(&storage).unwrap();

        assert_eq!(result.library_total, 0);
        assert_eq!(current_scan_progress_snapshot().inserted_sqlite, 0);
    }

    #[test]
    fn format_index_sources_message_reports_removed_workshop_item() {
        let msg = format_index_sources_message(&IndexSourcesResult {
            indexed: 12,
            library_total: 12,
            removed: 1,
            removed_we_workshop_ids: vec!["3589454154".into()],
            offline_sources: 0,
            incomplete_sources: 0,
        });
        assert!(msg.contains("Library contains 12 wallpaper(s)"));
        assert!(msg.contains("Removed 1 missing workshop item (3589454154)"));
    }

    #[test]
    fn format_index_sources_message_uses_plural_wording_for_mixed_removals() {
        let msg = format_index_sources_message(&IndexSourcesResult {
            indexed: 10,
            library_total: 10,
            removed: 3,
            removed_we_workshop_ids: vec!["3589454154".into()],
            offline_sources: 0,
            incomplete_sources: 0,
        });
        assert!(msg.contains("Removed 3 missing item(s), including workshop ID(s): 3589454154"));
        assert!(
            !msg.contains("missing workshop item"),
            "mixed removals must not use singular workshop wording: {msg}"
        );
    }

    #[test]
    fn rescan_removes_deleted_we_workshop_project_from_sqlite() {
        let _guard = TEST_SCAN_LOCK.lock().unwrap();
        reset_scan_state_for_test();

        let tmp = tempfile::tempdir().unwrap();
        let cd = wc_core::ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        let s = wc_storage::StorageApi::new(cd);

        let workshop_root = tmp.path().join("steamapps/workshop/content/431960");
        let project_dir = workshop_root.join("3589454154");
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::write(
            project_dir.join("project.json"),
            r#"{"type":"scene","title":"Deleted Scene Test"}"#,
        )
        .unwrap();
        s.sources_add(&workshop_root.to_string_lossy()).unwrap();

        let first = index_current_sources(&s).unwrap();
        assert_eq!(first.library_total, 1, "scene project should be indexed");
        assert_eq!(first.removed, 0);

        let conn = rusqlite::Connection::open(s.cd.db_path()).unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM wallpapers WHERE workshop_id = '3589454154'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "workshop_id should exist after first rescan");

        std::fs::remove_dir_all(&project_dir).unwrap();

        let second = index_current_sources(&s).unwrap();
        assert_eq!(
            second.library_total, 0,
            "deleted project should not be reindexed"
        );
        assert_eq!(
            second.removed, 1,
            "deleted project should be removed from sqlite"
        );
        assert_eq!(
            second.removed_we_workshop_ids,
            vec!["3589454154".to_string()]
        );

        let count_after: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM wallpapers WHERE workshop_id = '3589454154'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            count_after, 0,
            "workshop_id must be gone from sqlite after rescan"
        );
    }
}
