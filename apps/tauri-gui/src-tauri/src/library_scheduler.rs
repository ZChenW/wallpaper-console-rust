use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use wc_core::config::ConfigDir;

const DEBOUNCE_MILLIS: u64 = 1_500;
const RATE_LIMIT_MILLIS: u64 = 10_000;
const WATCH_RETRY_DELAYS_MILLIS: [u64; 4] = [2_000, 5_000, 15_000, 60_000];

pub trait SchedulerClock: Clone + Send + Sync + 'static {
    fn now_millis(&self) -> u64;
}

#[derive(Debug, Default)]
struct SourceSchedule {
    pending_since: Option<u64>,
    last_started: Option<u64>,
    running: bool,
    dirty_again: bool,
}

/// Deterministic, side-effect-free scheduling core. Watchers and scan workers
/// live outside this type so debounce/rate-limit behavior remains testable.
pub struct LibraryScheduler<C> {
    clock: C,
    sources: BTreeMap<i64, SourceSchedule>,
    round_running: BTreeSet<i64>,
    round_reload_emitted: bool,
}

impl<C: SchedulerClock> LibraryScheduler<C> {
    pub fn new(clock: C, source_ids: impl IntoIterator<Item = i64>) -> Self {
        Self {
            clock,
            sources: source_ids
                .into_iter()
                .map(|id| (id, SourceSchedule::default()))
                .collect(),
            round_running: BTreeSet::new(),
            round_reload_emitted: false,
        }
    }

    pub fn source_changed(&mut self, source_id: i64) {
        let now = self.clock.now_millis();
        let state = self.sources.entry(source_id).or_default();
        if state.running {
            state.dirty_again = true;
        } else {
            state.pending_since.get_or_insert(now);
        }
    }

    pub fn ensure_source(&mut self, source_id: i64) {
        self.sources.entry(source_id).or_default();
    }

    pub fn needs_freshness_check(&self, source_id: i64) -> bool {
        self.sources
            .get(&source_id)
            .is_some_and(|state| !state.running && state.pending_since.is_none())
    }

    pub fn watcher_overflow(&mut self) -> Vec<i64> {
        let ids = self.sources.keys().copied().collect::<Vec<_>>();
        for id in &ids {
            self.source_changed(*id);
        }
        ids
    }

    pub fn manual_requested(&mut self, source_id: i64) {
        let state = self.sources.entry(source_id).or_default();
        state.pending_since = None;
        if state.running {
            state.dirty_again = true;
        }
    }

    pub fn take_due_scans(&mut self) -> Vec<i64> {
        let now = self.clock.now_millis();
        let mut due = Vec::new();
        for (&id, state) in &mut self.sources {
            if state.running {
                continue;
            }
            let Some(pending_since) = state.pending_since else {
                continue;
            };
            let debounce_due = pending_since.saturating_add(DEBOUNCE_MILLIS);
            let rate_due = state
                .last_started
                .map_or(0, |last| last.saturating_add(RATE_LIMIT_MILLIS));
            if now < debounce_due.max(rate_due) {
                continue;
            }
            state.pending_since = None;
            state.last_started = Some(now);
            state.running = true;
            self.round_running.insert(id);
            self.round_reload_emitted = false;
            due.push(id);
        }
        due
    }

    /// Returns true once when a normal background round has fully completed.
    pub fn scan_finished(&mut self, source_id: i64) -> bool {
        let now = self.clock.now_millis();
        let Some(state) = self.sources.get_mut(&source_id) else {
            return false;
        };
        if !state.running {
            return false;
        }
        state.running = false;
        self.round_running.remove(&source_id);
        if state.dirty_again {
            state.dirty_again = false;
            state.pending_since = Some(now);
        }
        if self.round_running.is_empty() && !self.round_reload_emitted {
            self.round_reload_emitted = true;
            true
        } else {
            false
        }
    }
}

#[derive(Clone)]
struct SystemClock(Instant);

impl SchedulerClock for SystemClock {
    fn now_millis(&self) -> u64 {
        u64::try_from(self.0.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}

enum WatchMessage {
    Ready(i64, u64),
    Changed(i64, u64),
    Overflow,
    Failed(i64, u64),
}

enum SchedulerControl {
    Manual(i64),
    ManualAll,
}

#[derive(Clone)]
pub struct LibrarySchedulerHandle {
    sender: mpsc::Sender<SchedulerControl>,
}

impl LibrarySchedulerHandle {
    pub fn manual_requested(&self, source_id: i64) {
        let _ = self.sender.send(SchedulerControl::Manual(source_id));
    }

    pub fn manual_all_requested(&self) {
        let _ = self.sender.send(SchedulerControl::ManualAll);
    }
}

struct WatchHandle {
    signature: (String, bool),
    generation: u64,
    stop: Arc<AtomicBool>,
}

struct WatchRegistration {
    generation: u64,
    stop: Arc<AtomicBool>,
}

struct WatchRetryState {
    signature: (String, bool),
    consecutive_failures: u32,
    next_retry_millis: u64,
}

struct WatchRegistry {
    watchers: BTreeMap<i64, WatchHandle>,
    retry_states: BTreeMap<i64, WatchRetryState>,
    next_generation: u64,
}

impl Default for WatchRegistry {
    fn default() -> Self {
        Self {
            watchers: BTreeMap::new(),
            retry_states: BTreeMap::new(),
            next_generation: 1,
        }
    }
}

enum WatchAction {
    None,
    Dirty(i64),
    FailureStarted(i64),
    Recovered(i64),
    Overflow,
}

fn retire_watcher(watchers: &mut BTreeMap<i64, WatchHandle>, source_id: i64) -> bool {
    let Some(watcher) = watchers.remove(&source_id) else {
        return false;
    };
    watcher.stop.store(true, Ordering::Release);
    true
}

fn watch_retry_delay_millis(consecutive_failures: u32) -> u64 {
    let index = consecutive_failures
        .saturating_sub(1)
        .min((WATCH_RETRY_DELAYS_MILLIS.len() - 1) as u32) as usize;
    WATCH_RETRY_DELAYS_MILLIS[index]
}

impl WatchRegistry {
    fn reconcile(
        &mut self,
        source_id: i64,
        signature: (String, bool),
        now_millis: u64,
    ) -> Option<WatchRegistration> {
        if self
            .watchers
            .get(&source_id)
            .is_some_and(|watcher| watcher.signature == signature)
        {
            return None;
        }
        let _ = retire_watcher(&mut self.watchers, source_id);
        if self
            .retry_states
            .get(&source_id)
            .is_some_and(|state| state.signature != signature)
        {
            self.retry_states.remove(&source_id);
        }
        if self
            .retry_states
            .get(&source_id)
            .is_some_and(|state| now_millis < state.next_retry_millis)
        {
            return None;
        }
        let generation = self.next_generation;
        self.next_generation = self
            .next_generation
            .checked_add(1)
            .expect("watcher generation exhausted");
        let stop = Arc::new(AtomicBool::new(false));
        self.watchers.insert(
            source_id,
            WatchHandle {
                signature,
                generation,
                stop: stop.clone(),
            },
        );
        Some(WatchRegistration { generation, stop })
    }

    fn live_source_ids(&self) -> BTreeSet<i64> {
        self.watchers
            .keys()
            .chain(self.retry_states.keys())
            .copied()
            .collect()
    }

    fn retain_sources(&mut self, live_ids: &BTreeSet<i64>) {
        self.watchers.retain(|id, watcher| {
            let keep = live_ids.contains(id);
            if !keep {
                watcher.stop.store(true, Ordering::Release);
            }
            keep
        });
        self.retry_states.retain(|id, _| live_ids.contains(id));
    }

    fn apply_message(&mut self, message: WatchMessage, now_millis: u64) -> WatchAction {
        match message {
            WatchMessage::Ready(source_id, generation) => {
                if self.is_current(source_id, generation)
                    && self.retry_states.remove(&source_id).is_some()
                {
                    WatchAction::Recovered(source_id)
                } else {
                    WatchAction::None
                }
            }
            WatchMessage::Changed(source_id, generation) => {
                if self.is_current(source_id, generation) {
                    WatchAction::Dirty(source_id)
                } else {
                    WatchAction::None
                }
            }
            WatchMessage::Failed(source_id, generation) => {
                let Some(signature) = self
                    .watchers
                    .get(&source_id)
                    .filter(|watcher| watcher.generation == generation)
                    .map(|watcher| watcher.signature.clone())
                else {
                    return WatchAction::None;
                };
                let _ = retire_watcher(&mut self.watchers, source_id);
                let (consecutive_failures, first_failure) = self
                    .retry_states
                    .get(&source_id)
                    .filter(|state| state.signature == signature)
                    .map_or((1, true), |state| {
                        (state.consecutive_failures.saturating_add(1), false)
                    });
                self.retry_states.insert(
                    source_id,
                    WatchRetryState {
                        signature,
                        consecutive_failures,
                        next_retry_millis: now_millis
                            .saturating_add(watch_retry_delay_millis(consecutive_failures)),
                    },
                );
                if first_failure {
                    WatchAction::FailureStarted(source_id)
                } else {
                    WatchAction::None
                }
            }
            WatchMessage::Overflow => WatchAction::Overflow,
        }
    }

    fn is_current(&self, source_id: i64, generation: u64) -> bool {
        self.watchers
            .get(&source_id)
            .is_some_and(|watcher| watcher.generation == generation)
    }
}

fn process_watch_message<C: SchedulerClock>(
    registry: &mut WatchRegistry,
    scheduler: &mut LibraryScheduler<C>,
    message: WatchMessage,
    now_millis: u64,
    mut mark_dirty: impl FnMut(i64),
    mut mark_recovery_due: impl FnMut(i64),
) {
    match registry.apply_message(message, now_millis) {
        WatchAction::None => {}
        WatchAction::Dirty(source_id) => {
            mark_dirty(source_id);
            scheduler.source_changed(source_id);
        }
        WatchAction::FailureStarted(source_id) => {
            log::warn!("Library watcher failed for source id {source_id}; scheduling recreation");
            mark_dirty(source_id);
            scheduler.source_changed(source_id);
        }
        WatchAction::Recovered(source_id) => {
            mark_recovery_due(source_id);
            log::info!("Library watcher recovered for source id {source_id}; scheduling rescan");
            scheduler.source_changed(source_id);
        }
        WatchAction::Overflow => {
            for source_id in scheduler.watcher_overflow() {
                mark_dirty(source_id);
            }
        }
    }
}

pub fn start_library_scheduler(
    service: crate::library_service::LibraryService,
    cd: ConfigDir,
) -> LibrarySchedulerHandle {
    let (control_tx, control_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let clock = SystemClock(Instant::now());
        let scheduler = Arc::new(Mutex::new(LibraryScheduler::new(clock.clone(), [])));
        let (watch_tx, watch_rx) = mpsc::channel();
        let mut watch_registry = WatchRegistry::default();
        let mut last_catalog_refresh = Instant::now() - Duration::from_secs(5);

        loop {
            while let Ok(control) = control_rx.try_recv() {
                let mut scheduler = scheduler.lock().unwrap_or_else(|p| p.into_inner());
                match control {
                    SchedulerControl::Manual(id) => scheduler.manual_requested(id),
                    SchedulerControl::ManualAll => {
                        for id in watch_registry.live_source_ids() {
                            scheduler.manual_requested(id);
                        }
                    }
                }
            }
            if last_catalog_refresh.elapsed() >= Duration::from_secs(2) {
                if let Ok(storage) = wc_storage::StorageApi::try_new(ConfigDir {
                    path: cd.path.clone(),
                }) {
                    if let Ok(sources) = storage.source_records() {
                        let live_ids = sources
                            .iter()
                            .map(|source| source.id)
                            .collect::<BTreeSet<_>>();
                        for source in sources {
                            let check_freshness = {
                                let mut scheduler =
                                    scheduler.lock().unwrap_or_else(|p| p.into_inner());
                                scheduler.ensure_source(source.id);
                                scheduler.needs_freshness_check(source.id)
                            };
                            if check_freshness {
                                let now = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map(|duration| {
                                        i64::try_from(duration.as_secs()).unwrap_or(i64::MAX)
                                    })
                                    .unwrap_or(0);
                                if matches!(
                                    wc_storage::sqlite::source_refresh_eligibility(
                                        &cd,
                                        source.id,
                                        now,
                                        wc_storage::sqlite::RefreshIntent::Background,
                                    ),
                                    Ok(wc_storage::sqlite::SourceRefreshEligibility::Due)
                                ) {
                                    scheduler
                                        .lock()
                                        .unwrap_or_else(|p| p.into_inner())
                                        .source_changed(source.id);
                                }
                            }
                            let signature = (source.path.clone(), source.recursive);
                            if let Some(registration) =
                                watch_registry.reconcile(source.id, signature, clock.now_millis())
                            {
                                spawn_recursive_watcher(
                                    source.id,
                                    registration.generation,
                                    std::path::PathBuf::from(&source.path),
                                    source.recursive,
                                    registration.stop,
                                    watch_tx.clone(),
                                );
                            }
                        }
                        watch_registry.retain_sources(&live_ids);
                    }
                }
                last_catalog_refresh = Instant::now();
            }

            while let Ok(message) = watch_rx.try_recv() {
                let mut scheduler = scheduler.lock().unwrap_or_else(|p| p.into_inner());
                process_watch_message(
                    &mut watch_registry,
                    &mut scheduler,
                    message,
                    clock.now_millis(),
                    |source_id| {
                        let _ = wc_storage::sqlite::mark_source_refresh_dirty(&cd, source_id);
                    },
                    |source_id| {
                        if wc_storage::sqlite::mark_source_refresh_recovery_due(&cd, source_id)
                            .is_err()
                        {
                            log::warn!(
                                "Failed to mark recovered Library source id {source_id} due"
                            );
                        }
                    },
                );
            }

            let due = if service.maintenance_paused() {
                Vec::new()
            } else {
                scheduler
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .take_due_scans()
            };
            for source_id in due {
                let scheduler = scheduler.clone();
                let service = service.clone();
                let cd = ConfigDir {
                    path: cd.path.clone(),
                };
                std::thread::spawn(move || {
                    let Some(_background_work) = service.begin_background_work() else {
                        let mut scheduler = scheduler.lock().unwrap_or_else(|p| p.into_inner());
                        let _ = scheduler.scan_finished(source_id);
                        scheduler.source_changed(source_id);
                        return;
                    };
                    let generation = service.maintenance_generation();
                    if let Ok(storage) = wc_storage::StorageApi::try_new(ConfigDir {
                        path: cd.path.clone(),
                    }) {
                        let _ = wc_app::library_rescan::establish_library_dirty_marker(&storage);
                        let mut presentation = crate::commands::scan::begin_background_scan();
                        let result = wc_app::library_refresh::refresh_library_source_background(
                            &storage,
                            source_id,
                            |source, event| presentation.observe(source, event),
                        );
                        presentation.finish(&result, &storage);
                    }
                    let reload = scheduler
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .scan_finished(source_id);
                    if reload
                        && !service.maintenance_paused()
                        && service.maintenance_generation() == generation
                    {
                        let published = wc_storage::StorageApi::try_new(ConfigDir {
                            path: cd.path.clone(),
                        })
                        .ok()
                        .is_some_and(|storage| {
                            wc_app::library_rescan::write_legacy_tsv_snapshot(&storage).is_ok()
                        });
                        if published {
                            service.publish_background_round(&cd);
                        } else {
                            let mut scheduler = scheduler.lock().unwrap_or_else(|p| p.into_inner());
                            scheduler.watcher_overflow();
                        }
                    }
                });
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    });
    LibrarySchedulerHandle { sender: control_tx }
}

#[cfg(target_os = "linux")]
fn spawn_recursive_watcher(
    source_id: i64,
    generation: u64,
    root: std::path::PathBuf,
    recursive: bool,
    stop: Arc<AtomicBool>,
    sender: mpsc::Sender<WatchMessage>,
) {
    std::thread::spawn(move || {
        run_recursive_watcher(
            source_id,
            generation,
            &root,
            recursive,
            stop.as_ref(),
            &sender,
        );
    });
}

#[cfg(target_os = "linux")]
fn run_recursive_watcher(
    source_id: i64,
    generation: u64,
    root: &std::path::Path,
    recursive: bool,
    stop: &AtomicBool,
    sender: &mpsc::Sender<WatchMessage>,
) {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

    let raw_fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
    if raw_fd < 0 {
        let _ = sender.send(WatchMessage::Failed(source_id, generation));
        return;
    }
    // SAFETY: `inotify_init1` returned a new descriptor owned by this worker.
    let inotify = unsafe { OwnedFd::from_raw_fd(raw_fd) };
    let fd = inotify.as_raw_fd();
    if add_recursive_watches(fd, root, recursive).is_err() {
        let _ = sender.send(WatchMessage::Failed(source_id, generation));
        return;
    }
    let _ = sender.send(WatchMessage::Ready(source_id, generation));

    let mut buffer = [0u8; 16 * 1024];
    while !stop.load(Ordering::Acquire) {
        let read = unsafe { libc::read(fd, buffer.as_mut_ptr().cast(), buffer.len()) };
        if read > 0 {
            let mut offset = 0usize;
            let mut overflow = false;
            while offset + std::mem::size_of::<libc::inotify_event>() <= read as usize {
                let event =
                    unsafe { &*(buffer.as_ptr().add(offset).cast::<libc::inotify_event>()) };
                overflow |= event.mask & libc::IN_Q_OVERFLOW != 0;
                offset = offset
                    .saturating_add(std::mem::size_of::<libc::inotify_event>())
                    .saturating_add(event.len as usize);
            }
            if overflow {
                let _ = sender.send(WatchMessage::Overflow);
            } else if add_recursive_watches(fd, root, recursive).is_err() {
                let _ = sender.send(WatchMessage::Failed(source_id, generation));
                break;
            } else {
                let _ = sender.send(WatchMessage::Changed(source_id, generation));
            }
        } else {
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::WouldBlock {
                let _ = sender.send(WatchMessage::Failed(source_id, generation));
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(target_os = "linux")]
fn add_recursive_watches(fd: i32, root: &std::path::Path, recursive: bool) -> std::io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let mut directories = vec![root.to_path_buf()];
    let mask = libc::IN_CREATE
        | libc::IN_DELETE
        | libc::IN_MOVED_FROM
        | libc::IN_MOVED_TO
        | libc::IN_CLOSE_WRITE
        | libc::IN_ATTRIB
        | libc::IN_DELETE_SELF
        | libc::IN_MOVE_SELF;
    while let Some(directory) = directories.pop() {
        let path = std::ffi::CString::new(directory.as_os_str().as_bytes())
            .map_err(|_| std::io::Error::other("source path contains a NUL byte"))?;
        if unsafe { libc::inotify_add_watch(fd, path.as_ptr(), mask) } < 0 {
            return Err(std::io::Error::last_os_error());
        }
        if recursive {
            for entry in std::fs::read_dir(&directory)? {
                let entry = entry?;
                if entry.file_type()?.is_dir() {
                    directories.push(entry.path());
                }
            }
        }
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn spawn_recursive_watcher(
    source_id: i64,
    generation: u64,
    _root: std::path::PathBuf,
    _recursive: bool,
    _stop: Arc<AtomicBool>,
    sender: mpsc::Sender<WatchMessage>,
) {
    let _ = sender.send(WatchMessage::Failed(source_id, generation));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    #[derive(Clone, Default)]
    struct ManualClock(Arc<AtomicU64>);

    impl ManualClock {
        fn advance(&self, millis: u64) {
            self.0.fetch_add(millis, Ordering::SeqCst);
        }
    }

    impl SchedulerClock for ManualClock {
        fn now_millis(&self) -> u64 {
            self.0.load(Ordering::SeqCst)
        }
    }

    #[test]
    fn watcher_changes_debounce_for_one_and_a_half_seconds() {
        let clock = ManualClock::default();
        let mut scheduler = LibraryScheduler::new(clock.clone(), [7]);

        scheduler.source_changed(7);
        clock.advance(1_499);
        assert_eq!(scheduler.take_due_scans(), Vec::<i64>::new());
        clock.advance(1);
        assert_eq!(scheduler.take_due_scans(), vec![7]);
    }

    #[test]
    fn source_starts_at_most_once_per_ten_seconds() {
        let clock = ManualClock::default();
        let mut scheduler = LibraryScheduler::new(clock.clone(), [7]);
        scheduler.source_changed(7);
        clock.advance(1_500);
        assert_eq!(scheduler.take_due_scans(), vec![7]);
        assert!(scheduler.scan_finished(7));

        scheduler.source_changed(7);
        clock.advance(9_999);
        assert!(scheduler.take_due_scans().is_empty());
        clock.advance(1);
        assert_eq!(scheduler.take_due_scans(), vec![7]);
    }

    #[test]
    fn events_during_scan_collapse_into_exactly_one_follow_up() {
        let clock = ManualClock::default();
        let mut scheduler = LibraryScheduler::new(clock.clone(), [7]);
        scheduler.source_changed(7);
        clock.advance(1_500);
        assert_eq!(scheduler.take_due_scans(), vec![7]);

        scheduler.source_changed(7);
        scheduler.source_changed(7);
        assert!(scheduler.take_due_scans().is_empty());
        assert!(scheduler.scan_finished(7));
        clock.advance(10_000);
        assert_eq!(scheduler.take_due_scans(), vec![7]);
        assert!(scheduler.take_due_scans().is_empty());
    }

    #[test]
    fn the_same_source_never_runs_in_parallel_but_other_sources_can() {
        let clock = ManualClock::default();
        let mut scheduler = LibraryScheduler::new(clock.clone(), [7, 8]);
        scheduler.source_changed(7);
        scheduler.source_changed(8);
        clock.advance(1_500);
        assert_eq!(scheduler.take_due_scans(), vec![7, 8]);
        scheduler.source_changed(7);
        clock.advance(20_000);
        assert!(scheduler.take_due_scans().is_empty());
    }

    #[test]
    fn overflow_marks_every_source_dirty_and_uses_normal_debounce() {
        let clock = ManualClock::default();
        let mut scheduler = LibraryScheduler::new(clock.clone(), [7, 8]);
        assert_eq!(scheduler.watcher_overflow(), vec![7, 8]);
        clock.advance(1_500);
        assert_eq!(scheduler.take_due_scans(), vec![7, 8]);
    }

    #[test]
    fn manual_request_supersedes_queued_background_work() {
        let clock = ManualClock::default();
        let mut scheduler = LibraryScheduler::new(clock, [7]);
        scheduler.source_changed(7);
        scheduler.manual_requested(7);
        assert!(scheduler.take_due_scans().is_empty());
    }

    #[test]
    fn normal_round_requests_one_reload_when_its_last_scan_finishes() {
        let clock = ManualClock::default();
        let mut scheduler = LibraryScheduler::new(clock.clone(), [7, 8]);
        scheduler.source_changed(7);
        scheduler.source_changed(8);
        clock.advance(1_500);
        assert_eq!(scheduler.take_due_scans(), vec![7, 8]);
        assert!(!scheduler.scan_finished(7));
        assert!(scheduler.scan_finished(8));
        assert!(!scheduler.scan_finished(8));
    }

    #[test]
    fn retire_watcher_stops_and_removes_only_the_requested_source() {
        let retired_stop = Arc::new(AtomicBool::new(false));
        let retained_stop = Arc::new(AtomicBool::new(false));
        let mut watchers = BTreeMap::from([
            (
                7,
                WatchHandle {
                    signature: ("retired".to_owned(), true),
                    generation: 1,
                    stop: retired_stop.clone(),
                },
            ),
            (
                8,
                WatchHandle {
                    signature: ("retained".to_owned(), false),
                    generation: 2,
                    stop: retained_stop.clone(),
                },
            ),
        ]);

        assert!(retire_watcher(&mut watchers, 7));
        assert!(retired_stop.load(Ordering::Acquire));
        assert!(!retained_stop.load(Ordering::Acquire));
        assert!(!watchers.contains_key(&7));
        assert!(watchers.contains_key(&8));
    }

    #[test]
    fn retire_watcher_returns_false_when_the_source_is_missing() {
        let retained_stop = Arc::new(AtomicBool::new(false));
        let mut watchers = BTreeMap::from([(
            8,
            WatchHandle {
                signature: ("retained".to_owned(), false),
                generation: 2,
                stop: retained_stop.clone(),
            },
        )]);

        assert!(!retire_watcher(&mut watchers, 7));
        assert!(!retained_stop.load(Ordering::Acquire));
        assert!(watchers.contains_key(&8));
    }

    #[test]
    fn stale_failure_cannot_retire_or_schedule_for_the_current_watcher() {
        let clock = ManualClock::default();
        let mut scheduler = LibraryScheduler::new(clock.clone(), [7]);
        let mut registry = WatchRegistry::default();
        let first = registry
            .reconcile(7, ("first".to_owned(), true), 0)
            .expect("first watcher");
        let current = registry
            .reconcile(7, ("current".to_owned(), true), 0)
            .expect("replacement watcher");
        let mut dirtied_sources = Vec::new();

        process_watch_message(
            &mut registry,
            &mut scheduler,
            WatchMessage::Failed(7, first.generation),
            0,
            |source_id| dirtied_sources.push(source_id),
            |_| {},
        );

        assert!(!current.stop.load(Ordering::Acquire));
        assert_eq!(
            registry.watchers.get(&7).map(|watcher| watcher.generation),
            Some(current.generation)
        );
        assert!(dirtied_sources.is_empty());
        clock.advance(1_500);
        assert!(scheduler.take_due_scans().is_empty());

        process_watch_message(
            &mut registry,
            &mut scheduler,
            WatchMessage::Failed(7, current.generation),
            0,
            |source_id| {
                assert!(
                    current.stop.load(Ordering::Acquire),
                    "watcher must stop before its source is marked dirty"
                );
                dirtied_sources.push(source_id);
            },
            |_| {},
        );

        assert!(current.stop.load(Ordering::Acquire));
        assert!(!registry.watchers.contains_key(&7));
        assert_eq!(dirtied_sources, vec![7]);
        clock.advance(1_499);
        assert!(scheduler.take_due_scans().is_empty());
        clock.advance(1);
        assert_eq!(scheduler.take_due_scans(), vec![7]);
    }

    #[test]
    fn watcher_retry_delay_grows_and_caps() {
        assert_eq!(watch_retry_delay_millis(1), 2_000);
        assert_eq!(watch_retry_delay_millis(2), 5_000);
        assert_eq!(watch_retry_delay_millis(3), 15_000);
        assert_eq!(watch_retry_delay_millis(4), 60_000);
        assert_eq!(watch_retry_delay_millis(5), 60_000);
        assert_eq!(watch_retry_delay_millis(u32::MAX), 60_000);
    }

    #[test]
    fn same_signature_failures_back_off_and_dirty_only_once_until_ready() {
        let clock = ManualClock::default();
        let mut scheduler = LibraryScheduler::new(clock, [7]);
        let mut registry = WatchRegistry::default();
        let signature = ("same".to_owned(), true);
        let first = registry
            .reconcile(7, signature.clone(), 0)
            .expect("initial watcher");
        let mut dirtied_sources = Vec::new();

        process_watch_message(
            &mut registry,
            &mut scheduler,
            WatchMessage::Failed(7, first.generation),
            0,
            |source_id| dirtied_sources.push(source_id),
            |_| {},
        );
        assert_eq!(dirtied_sources, vec![7]);
        assert!(registry.reconcile(7, signature.clone(), 1_999).is_none());

        let second = registry
            .reconcile(7, signature.clone(), 2_000)
            .expect("first retry");
        process_watch_message(
            &mut registry,
            &mut scheduler,
            WatchMessage::Ready(7, first.generation),
            2_000,
            |source_id| dirtied_sources.push(source_id),
            |_| {},
        );
        process_watch_message(
            &mut registry,
            &mut scheduler,
            WatchMessage::Changed(7, first.generation),
            2_000,
            |source_id| dirtied_sources.push(source_id),
            |_| {},
        );
        assert!(registry.retry_states.contains_key(&7));
        assert_eq!(dirtied_sources, vec![7]);

        process_watch_message(
            &mut registry,
            &mut scheduler,
            WatchMessage::Failed(7, second.generation),
            2_000,
            |source_id| dirtied_sources.push(source_id),
            |_| {},
        );
        assert_eq!(dirtied_sources, vec![7]);
        assert!(registry.reconcile(7, signature.clone(), 6_999).is_none());

        let third = registry
            .reconcile(7, signature.clone(), 7_000)
            .expect("second retry");
        process_watch_message(
            &mut registry,
            &mut scheduler,
            WatchMessage::Ready(7, third.generation),
            7_000,
            |source_id| dirtied_sources.push(source_id),
            |_| {},
        );
        assert!(!registry.retry_states.contains_key(&7));

        process_watch_message(
            &mut registry,
            &mut scheduler,
            WatchMessage::Failed(7, third.generation),
            7_001,
            |source_id| dirtied_sources.push(source_id),
            |_| {},
        );
        assert_eq!(dirtied_sources, vec![7, 7]);
        assert!(registry.reconcile(7, signature.clone(), 9_000).is_none());
        assert!(registry.reconcile(7, signature, 9_001).is_some());
    }

    #[test]
    fn ready_recovery_rescans_exactly_once_and_stale_ready_is_ignored() {
        let clock = ManualClock::default();
        let mut scheduler = LibraryScheduler::new(clock.clone(), [7]);
        let mut registry = WatchRegistry::default();
        let signature = ("recovering".to_owned(), true);
        let first = registry
            .reconcile(7, signature.clone(), 0)
            .expect("initial watcher");
        let mut dirtied_sources = Vec::new();
        let mut recovered_sources = Vec::new();

        process_watch_message(
            &mut registry,
            &mut scheduler,
            WatchMessage::Ready(7, first.generation),
            0,
            |source_id| dirtied_sources.push(source_id),
            |source_id| recovered_sources.push(source_id),
        );
        assert!(dirtied_sources.is_empty());
        assert!(recovered_sources.is_empty());

        process_watch_message(
            &mut registry,
            &mut scheduler,
            WatchMessage::Failed(7, first.generation),
            0,
            |source_id| dirtied_sources.push(source_id),
            |source_id| recovered_sources.push(source_id),
        );
        assert_eq!(dirtied_sources, vec![7]);
        scheduler.manual_requested(7);

        let retry = registry
            .reconcile(7, signature, 2_000)
            .expect("retry watcher");
        process_watch_message(
            &mut registry,
            &mut scheduler,
            WatchMessage::Ready(7, first.generation),
            2_000,
            |source_id| dirtied_sources.push(source_id),
            |source_id| recovered_sources.push(source_id),
        );
        assert!(recovered_sources.is_empty());
        assert!(registry.retry_states.contains_key(&7));

        process_watch_message(
            &mut registry,
            &mut scheduler,
            WatchMessage::Ready(7, retry.generation),
            2_000,
            |source_id| dirtied_sources.push(source_id),
            |source_id| recovered_sources.push(source_id),
        );
        process_watch_message(
            &mut registry,
            &mut scheduler,
            WatchMessage::Ready(7, retry.generation),
            2_000,
            |source_id| dirtied_sources.push(source_id),
            |source_id| recovered_sources.push(source_id),
        );

        assert_eq!(recovered_sources, vec![7]);
        clock.advance(1_499);
        assert!(scheduler.take_due_scans().is_empty());
        clock.advance(1);
        assert_eq!(scheduler.take_due_scans(), vec![7]);
    }

    #[test]
    fn manual_all_includes_retry_only_sources_and_clears_pending_background_scan() {
        let clock = ManualClock::default();
        let mut scheduler = LibraryScheduler::new(clock.clone(), [7]);
        let mut registry = WatchRegistry::default();
        let watcher = registry
            .reconcile(7, ("failed".to_owned(), true), 0)
            .expect("initial watcher");

        process_watch_message(
            &mut registry,
            &mut scheduler,
            WatchMessage::Failed(7, watcher.generation),
            0,
            |_| {},
            |_| {},
        );
        assert!(registry.watchers.is_empty());
        assert!(registry.retry_states.contains_key(&7));

        let live_source_ids = registry.live_source_ids();
        assert_eq!(live_source_ids, BTreeSet::from([7]));
        for source_id in live_source_ids {
            scheduler.manual_requested(source_id);
        }

        clock.advance(1_500);
        assert!(scheduler.take_due_scans().is_empty());
    }

    #[test]
    fn signature_change_bypasses_existing_retry_delay() {
        let clock = ManualClock::default();
        let mut scheduler = LibraryScheduler::new(clock, [7]);
        let mut registry = WatchRegistry::default();
        let first = registry
            .reconcile(7, ("first".to_owned(), true), 0)
            .expect("initial watcher");

        process_watch_message(
            &mut registry,
            &mut scheduler,
            WatchMessage::Failed(7, first.generation),
            0,
            |_| {},
            |_| {},
        );

        assert!(registry
            .reconcile(7, ("first".to_owned(), true), 1_000)
            .is_none());
        assert!(registry
            .reconcile(7, ("changed".to_owned(), true), 1_000)
            .is_some());
    }

    #[test]
    fn removing_source_clears_its_retry_state() {
        let clock = ManualClock::default();
        let mut scheduler = LibraryScheduler::new(clock, [7]);
        let mut registry = WatchRegistry::default();
        let signature = ("source".to_owned(), true);
        let first = registry
            .reconcile(7, signature.clone(), 0)
            .expect("initial watcher");
        process_watch_message(
            &mut registry,
            &mut scheduler,
            WatchMessage::Failed(7, first.generation),
            0,
            |_| {},
            |_| {},
        );

        registry.retain_sources(&BTreeSet::new());

        assert!(!registry.retry_states.contains_key(&7));
        assert!(registry.reconcile(7, signature, 1).is_some());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn watcher_exits_after_initial_recursive_watch_failure() {
        let temporary = tempfile::tempdir().expect("create temporary directory");
        let missing_root = temporary.path().join("missing-source");
        let stop = Arc::new(AtomicBool::new(false));
        let (watch_sender, watch_receiver) = mpsc::channel();
        let (done_sender, done_receiver) = mpsc::channel();
        let worker_stop = stop.clone();
        let worker = std::thread::spawn(move || {
            run_recursive_watcher(
                7,
                11,
                &missing_root,
                false,
                worker_stop.as_ref(),
                &watch_sender,
            );
            let _ = done_sender.send(());
        });

        assert!(matches!(
            watch_receiver.recv_timeout(Duration::from_secs(1)),
            Ok(WatchMessage::Failed(7, 11))
        ));
        let exited = done_receiver.recv_timeout(Duration::from_secs(1)).is_ok();
        stop.store(true, Ordering::Release);
        worker.join().expect("watcher worker should not panic");
        assert!(exited, "watcher worker stayed alive after setup failed");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn watcher_exits_when_refreshing_recursive_watches_fails() {
        let temporary = tempfile::tempdir().expect("create temporary directory");
        let root = temporary.path().join("source");
        std::fs::create_dir(&root).expect("create watched source");
        let stop = Arc::new(AtomicBool::new(false));
        let (watch_sender, watch_receiver) = mpsc::channel();
        let (done_sender, done_receiver) = mpsc::channel();
        let worker_stop = stop.clone();
        let worker_root = root.clone();
        let worker = std::thread::spawn(move || {
            run_recursive_watcher(
                7,
                11,
                &worker_root,
                true,
                worker_stop.as_ref(),
                &watch_sender,
            );
            let _ = done_sender.send(());
        });

        let announced_ready = matches!(
            watch_receiver.recv_timeout(Duration::from_secs(1)),
            Ok(WatchMessage::Ready(7, 11))
        );
        if !announced_ready {
            stop.store(true, Ordering::Release);
            worker.join().expect("watcher worker should not panic");
            assert!(announced_ready, "watcher did not announce readiness");
            return;
        }

        let ready_deadline = Instant::now() + Duration::from_secs(2);
        let mut ready = false;
        let mut sequence = 0;
        while Instant::now() < ready_deadline {
            std::fs::write(root.join(format!("ready-{sequence}")), b"ready")
                .expect("write readiness probe");
            sequence += 1;
            match watch_receiver.recv_timeout(Duration::from_millis(100)) {
                Ok(WatchMessage::Changed(7, 11)) => {
                    ready = true;
                    break;
                }
                Ok(WatchMessage::Ready(7, 11) | WatchMessage::Overflow)
                | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Ok(WatchMessage::Failed(7, 11)) => break,
                Ok(
                    WatchMessage::Ready(id, generation)
                    | WatchMessage::Changed(id, generation)
                    | WatchMessage::Failed(id, generation),
                ) => {
                    panic!("unexpected watcher source id {id}, generation {generation}")
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }

        let mut failed = false;
        if ready {
            std::fs::remove_dir_all(&root).expect("remove watched source");
            let failure_deadline = Instant::now() + Duration::from_secs(2);
            while Instant::now() < failure_deadline {
                match watch_receiver.recv_timeout(Duration::from_millis(100)) {
                    Ok(WatchMessage::Failed(7, 11)) => {
                        failed = true;
                        break;
                    }
                    Ok(
                        WatchMessage::Ready(7, 11)
                        | WatchMessage::Changed(7, 11)
                        | WatchMessage::Overflow,
                    )
                    | Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Ok(
                        WatchMessage::Ready(id, generation)
                        | WatchMessage::Changed(id, generation)
                        | WatchMessage::Failed(id, generation),
                    ) => {
                        panic!("unexpected watcher source id {id}, generation {generation}")
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        }
        let exited = failed && done_receiver.recv_timeout(Duration::from_secs(1)).is_ok();
        stop.store(true, Ordering::Release);
        worker.join().expect("watcher worker should not panic");

        assert!(ready, "watcher never observed the readiness probe");
        assert!(failed, "watcher did not report recursive refresh failure");
        assert!(exited, "watcher worker stayed alive after refresh failed");
    }
}
