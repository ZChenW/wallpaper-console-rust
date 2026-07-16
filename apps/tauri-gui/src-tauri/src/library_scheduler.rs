use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use wc_core::config::ConfigDir;

const DEBOUNCE_MILLIS: u64 = 1_500;
const RATE_LIMIT_MILLIS: u64 = 10_000;

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
    Changed(i64),
    Overflow,
    Failed(i64),
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
    stop: Arc<AtomicBool>,
}

pub fn start_library_scheduler(
    service: crate::library_service::LibraryService,
    cd: ConfigDir,
) -> LibrarySchedulerHandle {
    let (control_tx, control_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let clock = SystemClock(Instant::now());
        let scheduler = Arc::new(Mutex::new(LibraryScheduler::new(clock, [])));
        let (watch_tx, watch_rx) = mpsc::channel();
        let mut watchers = BTreeMap::<i64, WatchHandle>::new();
        let mut last_catalog_refresh = Instant::now() - Duration::from_secs(5);

        loop {
            while let Ok(control) = control_rx.try_recv() {
                let mut scheduler = scheduler.lock().unwrap_or_else(|p| p.into_inner());
                match control {
                    SchedulerControl::Manual(id) => scheduler.manual_requested(id),
                    SchedulerControl::ManualAll => {
                        for id in watchers.keys().copied().collect::<Vec<_>>() {
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
                            let replace = watchers
                                .get(&source.id)
                                .map_or(true, |watch| watch.signature != signature);
                            if replace {
                                if let Some(old) = watchers.remove(&source.id) {
                                    old.stop.store(true, Ordering::Release);
                                }
                                let stop = Arc::new(AtomicBool::new(false));
                                spawn_recursive_watcher(
                                    source.id,
                                    std::path::PathBuf::from(&source.path),
                                    source.recursive,
                                    stop.clone(),
                                    watch_tx.clone(),
                                );
                                watchers.insert(source.id, WatchHandle { signature, stop });
                            }
                        }
                        watchers.retain(|id, watch| {
                            let keep = live_ids.contains(id);
                            if !keep {
                                watch.stop.store(true, Ordering::Release);
                            }
                            keep
                        });
                    }
                }
                last_catalog_refresh = Instant::now();
            }

            while let Ok(message) = watch_rx.try_recv() {
                let mut scheduler = scheduler.lock().unwrap_or_else(|p| p.into_inner());
                match message {
                    WatchMessage::Changed(id) | WatchMessage::Failed(id) => {
                        let _ = wc_storage::sqlite::mark_source_refresh_dirty(&cd, id);
                        scheduler.source_changed(id);
                    }
                    WatchMessage::Overflow => {
                        for id in scheduler.watcher_overflow() {
                            let _ = wc_storage::sqlite::mark_source_refresh_dirty(&cd, id);
                        }
                    }
                }
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
    root: std::path::PathBuf,
    recursive: bool,
    stop: Arc<AtomicBool>,
    sender: mpsc::Sender<WatchMessage>,
) {
    std::thread::spawn(move || {
        let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
        if fd < 0 {
            let _ = sender.send(WatchMessage::Failed(source_id));
            return;
        }
        if add_recursive_watches(fd, &root, recursive).is_err() {
            let _ = sender.send(WatchMessage::Failed(source_id));
        }
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
                let _ = sender.send(if overflow {
                    WatchMessage::Overflow
                } else {
                    let _ = add_recursive_watches(fd, &root, recursive);
                    WatchMessage::Changed(source_id)
                });
            } else {
                let error = std::io::Error::last_os_error();
                if error.kind() != std::io::ErrorKind::WouldBlock {
                    let _ = sender.send(WatchMessage::Failed(source_id));
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        unsafe { libc::close(fd) };
    });
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
    _root: std::path::PathBuf,
    _recursive: bool,
    _stop: Arc<AtomicBool>,
    sender: mpsc::Sender<WatchMessage>,
) {
    let _ = sender.send(WatchMessage::Failed(source_id));
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
}
