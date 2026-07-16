//! Long-lived GUI Library service.
//!
//! SQLite remains durable truth. This module owns only bounded process-local
//! page/total caches, cold-query single-flight, revision observation, and the
//! frontend-ready/maintenance lifecycle.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use wc_core::config::ConfigDir;
use wc_core::error::WcError;
use wc_storage::sqlite::{LibraryBrowserPage, LibraryBrowserQuery, LibraryBrowserTotal};

const MAX_PAGE_COUNT: usize = 128;
const MAX_CACHE_BYTES: usize = 32 * 1024 * 1024;
const FOREGROUND_SQL_DEADLINE: Duration = Duration::from_secs(2);
const OBSERVER_INTERVAL: Duration = Duration::from_millis(500);

type RevisionNotifier = Arc<dyn Fn(u64) + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryServiceError {
    pub kind: &'static str,
    pub message: String,
}

impl From<WcError> for LibraryServiceError {
    fn from(error: WcError) -> Self {
        let kind = match &error {
            WcError::RevisionChanged { .. } => "revision_changed",
            WcError::InvalidCursor { .. } => "invalid_cursor",
            _ => "storage_error",
        };
        Self {
            kind,
            message: error.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct QueryKey(LibraryBrowserQuery);

impl QueryKey {
    fn new(query: &LibraryBrowserQuery) -> Self {
        let mut normalized = query.clone();
        normalized.search = normalize_search(&normalized.search);
        Self(normalized)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PageKey {
    revision: u64,
    query: QueryKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TotalKey {
    revision: u64,
    source_id: Option<i64>,
    type_filter: wc_storage::sqlite::LibraryBrowserType,
    favorites_only: bool,
    search: String,
}

struct CachedPage {
    page: Arc<LibraryBrowserPage>,
    bytes: usize,
}

#[derive(Default)]
struct CacheState {
    pages: HashMap<PageKey, CachedPage>,
    lru: VecDeque<PageKey>,
    bytes: usize,
    totals: HashMap<TotalKey, usize>,
    total_lru: VecDeque<TotalKey>,
    stale: HashMap<QueryKey, CachedPage>,
    stale_lru: VecDeque<QueryKey>,
    stale_bytes: usize,
}

impl CacheState {
    fn page(&mut self, key: &PageKey) -> Option<Arc<LibraryBrowserPage>> {
        let page = self.pages.get(key)?.page.clone();
        promote(&mut self.lru, key);
        Some(page)
    }

    fn current_page_for_query(&mut self, query: &QueryKey) -> Option<Arc<LibraryBrowserPage>> {
        let key = self
            .pages
            .keys()
            .filter(|key| &key.query == query)
            .max_by_key(|key| key.revision)
            .cloned()?;
        self.page(&key)
    }

    fn insert_page(&mut self, key: PageKey, page: Arc<LibraryBrowserPage>) {
        let bytes = estimate_page_bytes(&page);
        if let Some(previous) = self.pages.remove(&key) {
            self.bytes = self.bytes.saturating_sub(previous.bytes);
            remove_key(&mut self.lru, &key);
        }
        if bytes <= MAX_CACHE_BYTES {
            if let Some(previous) = self.stale.remove(&key.query) {
                self.stale_bytes = self.stale_bytes.saturating_sub(previous.bytes);
                remove_key(&mut self.stale_lru, &key.query);
            }
            self.stale_bytes = self.stale_bytes.saturating_add(bytes);
            self.stale_lru.push_back(key.query.clone());
            self.stale.insert(
                key.query.clone(),
                CachedPage {
                    page: page.clone(),
                    bytes,
                },
            );
            while self.stale.len() > MAX_PAGE_COUNT || self.stale_bytes > MAX_CACHE_BYTES {
                let Some(oldest) = self.stale_lru.pop_front() else {
                    break;
                };
                if let Some(evicted) = self.stale.remove(&oldest) {
                    self.stale_bytes = self.stale_bytes.saturating_sub(evicted.bytes);
                }
            }
        }
        self.bytes = self.bytes.saturating_add(bytes);
        self.lru.push_back(key.clone());
        self.pages.insert(key, CachedPage { page, bytes });
        while self.pages.len() > MAX_PAGE_COUNT || self.bytes > MAX_CACHE_BYTES {
            let Some(oldest) = self.lru.pop_front() else {
                break;
            };
            if let Some(evicted) = self.pages.remove(&oldest) {
                self.bytes = self.bytes.saturating_sub(evicted.bytes);
            }
        }
    }

    fn insert_total(&mut self, key: TotalKey, total: usize) {
        remove_key(&mut self.total_lru, &key);
        self.total_lru.push_back(key.clone());
        self.totals.insert(key, total);
        while self.totals.len() > MAX_PAGE_COUNT {
            let Some(oldest) = self.total_lru.pop_front() else {
                break;
            };
            self.totals.remove(&oldest);
        }
    }

    fn total(&mut self, key: &TotalKey) -> Option<usize> {
        let total = *self.totals.get(key)?;
        promote(&mut self.total_lru, key);
        Some(total)
    }

    fn invalidate_current(&mut self) {
        self.pages.clear();
        self.lru.clear();
        self.bytes = 0;
        self.totals.clear();
        self.total_lru.clear();
    }

    fn invalidate_all(&mut self) {
        self.invalidate_current();
        self.stale.clear();
        self.stale_lru.clear();
        self.stale_bytes = 0;
    }
}

fn remove_key<T: PartialEq>(queue: &mut VecDeque<T>, key: &T) {
    if let Some(index) = queue.iter().position(|candidate| candidate == key) {
        queue.remove(index);
    }
}

fn promote<T: Clone + PartialEq>(queue: &mut VecDeque<T>, key: &T) {
    remove_key(queue, key);
    queue.push_back(key.clone());
}

fn estimate_page_bytes(page: &LibraryBrowserPage) -> usize {
    let mut bytes = std::mem::size_of::<LibraryBrowserPage>();
    for item in &page.items {
        bytes = bytes
            .saturating_add(std::mem::size_of_val(item))
            .saturating_add(item.entry.path.as_str().len())
            .saturating_add(item.author.as_deref().map_or(0, str::len))
            .saturating_add(item.added_at.len());
        for source in &item.sources {
            bytes = bytes.saturating_add(source.display_name.len());
        }
    }
    bytes
}

type SharedPageResult = Result<Arc<LibraryBrowserPage>, LibraryServiceError>;
type SharedTotalResult = Result<LibraryBrowserTotal, LibraryServiceError>;

#[derive(Default)]
struct PageFlight {
    result: Mutex<Option<SharedPageResult>>,
    ready: Condvar,
}

impl PageFlight {
    fn wait(&self) -> SharedPageResult {
        let mut result = self.result.lock().unwrap_or_else(|p| p.into_inner());
        while result.is_none() {
            result = self.ready.wait(result).unwrap_or_else(|p| p.into_inner());
        }
        result.as_ref().expect("flight result set").clone()
    }

    fn finish(&self, result: SharedPageResult) {
        *self.result.lock().unwrap_or_else(|p| p.into_inner()) = Some(result);
        self.ready.notify_all();
    }
}

#[derive(Default)]
struct TotalFlight {
    result: Mutex<Option<SharedTotalResult>>,
    ready: Condvar,
}

impl TotalFlight {
    fn wait(&self) -> SharedTotalResult {
        let mut result = self.result.lock().unwrap_or_else(|p| p.into_inner());
        while result.is_none() {
            result = self.ready.wait(result).unwrap_or_else(|p| p.into_inner());
        }
        result.as_ref().expect("total flight result set").clone()
    }

    fn finish(&self, result: SharedTotalResult) {
        *self.result.lock().unwrap_or_else(|p| p.into_inner()) = Some(result);
        self.ready.notify_all();
    }
}

#[derive(Default)]
struct Diagnostics {
    page_hits: AtomicU64,
    page_misses: AtomicU64,
    page_waiters: AtomicU64,
    query_timeouts: AtomicU64,
}

#[derive(Default)]
struct MaintenanceState {
    depth: u32,
    background_active: u32,
}

struct Inner {
    ready: AtomicBool,
    observer_started: AtomicBool,
    scheduler_started: AtomicBool,
    fts_started: AtomicBool,
    scheduler: Mutex<Option<crate::library_scheduler::LibrarySchedulerHandle>>,
    config_dir: Mutex<Option<ConfigDir>>,
    cache: Mutex<CacheState>,
    flights: Mutex<HashMap<PageKey, Arc<PageFlight>>>,
    total_flights: Mutex<HashMap<TotalKey, Arc<TotalFlight>>>,
    maintenance: Mutex<MaintenanceState>,
    background_idle: Condvar,
    maintenance_generation: AtomicU64,
    change_notifier: Mutex<Option<RevisionNotifier>>,
    diagnostics: Diagnostics,
}

#[derive(Clone)]
pub struct LibraryService {
    inner: Arc<Inner>,
}

#[derive(Debug, Clone, Default)]
pub struct LibraryServiceDiagnostics {
    pub page_hits: u64,
    pub page_misses: u64,
    pub page_waiters: u64,
    pub query_timeouts: u64,
    pub cached_pages: usize,
    pub cached_bytes: usize,
    pub cached_totals: usize,
    pub observer_started: bool,
    pub scheduler_started: bool,
    pub fts_status: String,
    pub fts_revision: i64,
    pub fts_next_wallpaper_id: i64,
}

impl Default for LibraryService {
    fn default() -> Self {
        Self::new()
    }
}

impl LibraryService {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                ready: AtomicBool::new(false),
                observer_started: AtomicBool::new(false),
                scheduler_started: AtomicBool::new(false),
                fts_started: AtomicBool::new(false),
                scheduler: Mutex::new(None),
                config_dir: Mutex::new(None),
                cache: Mutex::new(CacheState::default()),
                flights: Mutex::new(HashMap::new()),
                total_flights: Mutex::new(HashMap::new()),
                maintenance: Mutex::new(MaintenanceState::default()),
                background_idle: Condvar::new(),
                maintenance_generation: AtomicU64::new(0),
                change_notifier: Mutex::new(None),
                diagnostics: Diagnostics::default(),
            }),
        }
    }

    pub fn mark_frontend_ready(&self) {
        self.mark_frontend_ready_with(|| {});
    }

    pub fn mark_frontend_ready_with(&self, on_start: impl FnOnce()) {
        if self.inner.ready.swap(true, Ordering::SeqCst) {
            return;
        }
        on_start();
        self.start_background_if_possible();
    }

    pub fn is_ready(&self) -> bool {
        self.inner.ready.load(Ordering::SeqCst)
    }

    pub fn set_change_notifier(&self, notifier: impl Fn(u64) + Send + Sync + 'static) {
        *self
            .inner
            .change_notifier
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Some(Arc::new(notifier));
    }

    pub fn manual_refresh_requested(&self, source_id: i64) {
        if let Some(scheduler) = self
            .inner
            .scheduler
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
        {
            scheduler.manual_requested(source_id);
        }
    }

    pub fn manual_refresh_all_requested(&self) {
        if let Some(scheduler) = self
            .inner
            .scheduler
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
        {
            scheduler.manual_all_requested();
        }
    }

    pub fn diagnostics_snapshot(&self, cd: &ConfigDir) -> LibraryServiceDiagnostics {
        let cache = self.inner.cache.lock().unwrap_or_else(|p| p.into_inner());
        let mut diagnostics = LibraryServiceDiagnostics {
            page_hits: self.inner.diagnostics.page_hits.load(Ordering::Relaxed),
            page_misses: self.inner.diagnostics.page_misses.load(Ordering::Relaxed),
            page_waiters: self.inner.diagnostics.page_waiters.load(Ordering::Relaxed),
            query_timeouts: self
                .inner
                .diagnostics
                .query_timeouts
                .load(Ordering::Relaxed),
            cached_pages: cache.pages.len(),
            cached_bytes: cache.bytes,
            cached_totals: cache.totals.len(),
            observer_started: self.inner.observer_started.load(Ordering::Acquire),
            scheduler_started: self.inner.scheduler_started.load(Ordering::Acquire),
            fts_status: "unavailable".into(),
            fts_revision: -1,
            fts_next_wallpaper_id: 0,
        };
        drop(cache);
        if let Ok(connection) = wc_storage::sqlite::open_runtime_connection(cd) {
            if let Ok((status, revision, next_id)) = connection.query_row(
                "SELECT status, revision, next_wallpaper_id
                 FROM library_fts_state WHERE singleton = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            ) {
                diagnostics.fts_status = status;
                diagnostics.fts_revision = revision;
                diagnostics.fts_next_wallpaper_id = next_id;
            }
        }
        diagnostics
    }

    pub fn page(&self, cd: &ConfigDir, query: &LibraryBrowserQuery) -> SharedPageResult {
        self.remember_config_dir(cd);
        let query_key = QueryKey::new(query);
        if let Some(page) = self
            .inner
            .cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .current_page_for_query(&query_key)
        {
            self.inner
                .diagnostics
                .page_hits
                .fetch_add(1, Ordering::Relaxed);
            return Ok(page);
        }
        if let Some(stale) = stale_page_for_query(&self.inner.cache, &query_key) {
            let service = self.clone();
            let cd = ConfigDir {
                path: cd.path.clone(),
            };
            let query = query.clone();
            std::thread::spawn(move || {
                let started = Instant::now();
                let _ = service.page_with_loader(&cd, &query, move |conn, query| {
                    run_page_with_deadline(conn, query, remaining_budget(started))
                });
            });
            return Ok(stale);
        }
        let started = Instant::now();
        self.page_with_loader(cd, query, move |conn, query| {
            run_page_with_deadline(conn, query, remaining_budget(started))
        })
    }

    fn page_with_loader<F>(
        &self,
        cd: &ConfigDir,
        query: &LibraryBrowserQuery,
        loader: F,
    ) -> SharedPageResult
    where
        F: FnOnce(
            &rusqlite::Connection,
            &LibraryBrowserQuery,
        ) -> Result<LibraryBrowserPage, WcError>,
    {
        self.remember_config_dir(cd);
        let query_key = QueryKey::new(query);
        let conn = match wc_storage::sqlite::open_runtime_connection(cd) {
            Ok(conn) => conn,
            Err(error) => return self.storage_failure_or_stale(&query_key, error),
        };
        let revision = match wc_storage::sqlite::read_library_revision(&conn) {
            Ok(revision) => revision,
            Err(error) => return self.storage_failure_or_stale(&query_key, error),
        };
        let key = PageKey {
            revision,
            query: query_key,
        };

        if let Some(page) = self
            .inner
            .cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .page(&key)
        {
            self.inner
                .diagnostics
                .page_hits
                .fetch_add(1, Ordering::Relaxed);
            return Ok(page);
        }
        self.inner
            .diagnostics
            .page_misses
            .fetch_add(1, Ordering::Relaxed);

        let (flight, leader) = {
            let mut flights = self.inner.flights.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(flight) = flights.get(&key) {
                (flight.clone(), false)
            } else {
                let flight = Arc::new(PageFlight::default());
                flights.insert(key.clone(), flight.clone());
                (flight, true)
            }
        };
        if !leader {
            self.inner
                .diagnostics
                .page_waiters
                .fetch_add(1, Ordering::Relaxed);
            return flight.wait();
        }

        let (loaded, result) = match loader(&conn, query).map(Arc::new) {
            Ok(page) => (Some(page.clone()), Ok(page)),
            Err(error) => {
                let mut service_error = LibraryServiceError::from(error);
                if service_error.message.contains("interrupted") {
                    service_error.kind = "query_timeout";
                    self.inner
                        .diagnostics
                        .query_timeouts
                        .fetch_add(1, Ordering::Relaxed);
                }
                let fallback = matches!(service_error.kind, "storage_error" | "query_timeout")
                    .then(|| stale_page_for_query(&self.inner.cache, &key.query))
                    .flatten();
                (None, fallback.ok_or(service_error))
            }
        };
        if let Some(page) = loaded {
            let actual_key = PageKey {
                revision: page.revision,
                query: key.query.clone(),
            };
            self.inner
                .cache
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert_page(actual_key, page);
        }
        self.inner
            .flights
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&key);
        flight.finish(result.clone());
        result
    }

    fn storage_failure_or_stale(&self, query: &QueryKey, error: WcError) -> SharedPageResult {
        if let Some(stale) = stale_page_for_query(&self.inner.cache, query) {
            return Ok(stale);
        }
        Err(LibraryServiceError::from(error))
    }

    pub fn exact_total(
        &self,
        cd: &ConfigDir,
        query: &LibraryBrowserQuery,
        expected_revision: u64,
    ) -> Result<LibraryBrowserTotal, LibraryServiceError> {
        let started = Instant::now();
        self.exact_total_with_loader(
            cd,
            query,
            expected_revision,
            move |conn, query, revision| {
                run_total_with_deadline(conn, query, revision, remaining_budget(started))
            },
        )
    }

    fn exact_total_with_loader<F>(
        &self,
        cd: &ConfigDir,
        query: &LibraryBrowserQuery,
        expected_revision: u64,
        loader: F,
    ) -> SharedTotalResult
    where
        F: FnOnce(
            &rusqlite::Connection,
            &LibraryBrowserQuery,
            u64,
        ) -> Result<LibraryBrowserTotal, WcError>,
    {
        self.remember_config_dir(cd);
        let mut total_query = query.clone();
        total_query.cursor = None;
        let key = TotalKey {
            revision: expected_revision,
            source_id: total_query.source_id,
            type_filter: total_query.type_filter,
            favorites_only: total_query.favorites_only,
            search: normalize_search(&total_query.search),
        };
        if let Some(total) = self
            .inner
            .cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .total(&key)
        {
            return Ok(LibraryBrowserTotal {
                revision: expected_revision,
                total,
            });
        }
        let (flight, leader) = {
            let mut flights = self
                .inner
                .total_flights
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            if let Some(flight) = flights.get(&key) {
                (flight.clone(), false)
            } else {
                let flight = Arc::new(TotalFlight::default());
                flights.insert(key.clone(), flight.clone());
                (flight, true)
            }
        };
        if !leader {
            return flight.wait();
        }
        let result = wc_storage::sqlite::open_runtime_connection(cd)
            .map_err(LibraryServiceError::from)
            .and_then(|conn| {
                loader(&conn, &total_query, expected_revision).map_err(LibraryServiceError::from)
            });
        if let Ok(total) = &result {
            self.inner
                .cache
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert_total(key.clone(), total.total);
        }
        self.inner
            .total_flights
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&key);
        flight.finish(result.clone());
        result
    }

    pub fn invalidate_local_write(&self) {
        self.inner
            .cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .invalidate_all();
    }

    fn invalidate_external_change(&self) {
        self.inner
            .cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .invalidate_current();
    }

    pub(crate) fn publish_background_round(&self, cd: &ConfigDir) {
        self.invalidate_external_change();
        let revision = wc_storage::sqlite::open_runtime_connection(cd)
            .and_then(|conn| wc_storage::sqlite::read_library_revision(&conn))
            .ok();
        if let Some(revision) = revision {
            if let Some(notifier) = self
                .inner
                .change_notifier
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .clone()
            {
                notifier(revision);
            }
        }
    }

    pub fn pause_for_maintenance(&self) -> MaintenancePause {
        let first = {
            let mut state = self
                .inner
                .maintenance
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            state.depth += 1;
            state.depth == 1
        };
        if first {
            self.inner
                .maintenance_generation
                .fetch_add(1, Ordering::AcqRel);
            self.invalidate_local_write();
            wc_storage::sqlite::invalidate_cached_connections();
            let mut state = self
                .inner
                .maintenance
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            while state.background_active > 0 {
                state = self
                    .inner
                    .background_idle
                    .wait(state)
                    .unwrap_or_else(|p| p.into_inner());
            }
        }
        MaintenancePause {
            service: self.clone(),
        }
    }

    #[cfg(test)]
    pub fn maintenance_depth(&self) -> u32 {
        self.inner
            .maintenance
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .depth
    }

    pub(crate) fn maintenance_paused(&self) -> bool {
        self.inner
            .maintenance
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .depth
            > 0
    }

    pub(crate) fn maintenance_generation(&self) -> u64 {
        self.inner.maintenance_generation.load(Ordering::Acquire)
    }

    pub(crate) fn begin_background_work(&self) -> Option<BackgroundWork> {
        let mut state = self
            .inner
            .maintenance
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if state.depth > 0 {
            return None;
        }
        state.background_active = state.background_active.saturating_add(1);
        Some(BackgroundWork {
            service: self.clone(),
        })
    }

    fn remember_config_dir(&self, cd: &ConfigDir) {
        let mut configured = self
            .inner
            .config_dir
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if configured.is_none() {
            *configured = Some(ConfigDir {
                path: cd.path.clone(),
            });
        }
        drop(configured);
        self.start_background_if_possible();
    }

    fn start_background_if_possible(&self) {
        self.start_observer_if_possible();
        self.start_scheduler_if_possible();
        self.start_fts_builder_if_possible();
    }

    fn start_fts_builder_if_possible(&self) {
        if cfg!(test) || !self.is_ready() || self.inner.fts_started.load(Ordering::Acquire) {
            return;
        }
        let cd = self
            .inner
            .config_dir
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .map(|cd| ConfigDir {
                path: cd.path.clone(),
            });
        let Some(cd) = cd else { return };
        if self.inner.fts_started.swap(true, Ordering::AcqRel) {
            return;
        }
        let inner = Arc::downgrade(&self.inner);
        std::thread::spawn(move || loop {
            let Some(inner) = inner.upgrade() else { break };
            if inner
                .maintenance
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .depth
                == 0
            {
                if let Ok(mut connection) = wc_storage::sqlite::open_runtime_connection(&cd) {
                    let _ = wc_storage::sqlite::build_library_fts_chunk(&mut connection);
                }
            }
            std::thread::sleep(Duration::from_secs(1));
        });
    }

    fn start_scheduler_if_possible(&self) {
        if cfg!(test) {
            return;
        }
        if !self.is_ready() || self.inner.scheduler_started.load(Ordering::Acquire) {
            return;
        }
        let cd = self
            .inner
            .config_dir
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .map(|cd| ConfigDir {
                path: cd.path.clone(),
            });
        let Some(cd) = cd else { return };
        if self.inner.scheduler_started.swap(true, Ordering::AcqRel) {
            return;
        }
        let _ =
            wc_app::scan_worker::cleanup_stale_worker_artifact_dirs(&cd.path.join("scan-workers"));
        let scheduler = crate::library_scheduler::start_library_scheduler(self.clone(), cd);
        *self
            .inner
            .scheduler
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Some(scheduler);
    }

    fn start_observer_if_possible(&self) {
        if !self.is_ready() || self.inner.observer_started.load(Ordering::Acquire) {
            return;
        }
        let cd = self
            .inner
            .config_dir
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .map(|cd| ConfigDir {
                path: cd.path.clone(),
            });
        let Some(cd) = cd else { return };
        if self.inner.observer_started.swap(true, Ordering::AcqRel) {
            return;
        }
        let inner = Arc::downgrade(&self.inner);
        std::thread::spawn(move || {
            let open_observer = || {
                let connection = rusqlite::Connection::open_with_flags(
                    cd.db_path(),
                    rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
                        | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
                )
                .ok()?;
                let observer =
                    wc_storage::sqlite::LibraryRevisionObserver::new(&connection).ok()?;
                Some((connection, observer))
            };
            let mut observed = open_observer();
            let mut observed_generation = inner
                .upgrade()
                .map(|inner| inner.maintenance_generation.load(Ordering::Acquire))
                .unwrap_or(0);
            loop {
                std::thread::sleep(OBSERVER_INTERVAL);
                let Some(inner) = inner.upgrade() else { break };
                if inner
                    .maintenance
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .depth
                    > 0
                {
                    continue;
                }
                let generation = inner.maintenance_generation.load(Ordering::Acquire);
                if generation != observed_generation {
                    observed_generation = generation;
                    observed = open_observer();
                    continue;
                }
                if observed.is_none() {
                    observed = open_observer();
                    continue;
                }
                let Some((connection, observer)) = observed.as_mut() else {
                    continue;
                };
                let Ok(change) = observer.observe(connection) else {
                    observed = None;
                    continue;
                };
                if let Some(revision) = change {
                    LibraryService {
                        inner: inner.clone(),
                    }
                    .invalidate_external_change();
                    if let Some(notifier) = inner
                        .change_notifier
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .clone()
                    {
                        notifier(revision);
                    }
                }
            }
        });
    }

    #[cfg(test)]
    fn cache_stats(&self) -> (usize, usize, usize) {
        let cache = self.inner.cache.lock().unwrap_or_else(|p| p.into_inner());
        (cache.pages.len(), cache.bytes, cache.totals.len())
    }
}

fn stale_page_for_query(
    cache: &Mutex<CacheState>,
    query: &QueryKey,
) -> Option<Arc<LibraryBrowserPage>> {
    let cache = cache.lock().unwrap_or_else(|p| p.into_inner());
    cache.stale.get(query).map(|page| page.page.clone())
}

fn run_page_with_deadline(
    conn: &rusqlite::Connection,
    query: &LibraryBrowserQuery,
    budget: Duration,
) -> Result<LibraryBrowserPage, WcError> {
    with_sql_deadline(conn, budget, |conn| {
        wc_storage::sqlite::browser_library_page_on_connection(conn, query)
    })
}

fn remaining_budget(started: Instant) -> Duration {
    FOREGROUND_SQL_DEADLINE.saturating_sub(started.elapsed())
}

fn normalize_search(search: &str) -> String {
    search.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn run_total_with_deadline(
    conn: &rusqlite::Connection,
    query: &LibraryBrowserQuery,
    expected_revision: u64,
    budget: Duration,
) -> Result<LibraryBrowserTotal, WcError> {
    with_sql_deadline(conn, budget, |conn| {
        wc_storage::sqlite::browser_library_exact_total_on_connection(
            conn,
            query,
            expected_revision,
        )
    })
}

fn with_sql_deadline<T>(
    conn: &rusqlite::Connection,
    budget: Duration,
    operation: impl FnOnce(&rusqlite::Connection) -> Result<T, WcError>,
) -> Result<T, WcError> {
    conn.busy_timeout(budget.min(FOREGROUND_SQL_DEADLINE))
        .map_err(|error| WcError::Sqlite(error.to_string()))?;
    let deadline = Instant::now() + budget;
    conn.progress_handler(1_000, Some(move || Instant::now() >= deadline));
    let result = operation(conn);
    conn.progress_handler(0, None::<fn() -> bool>);
    let _ = conn.busy_timeout(Duration::from_millis(
        wc_storage::sqlite::RUNTIME_BUSY_TIMEOUT_MS,
    ));
    result
}

pub struct MaintenancePause {
    service: LibraryService,
}

pub(crate) struct BackgroundWork {
    service: LibraryService,
}

impl Drop for BackgroundWork {
    fn drop(&mut self) {
        let mut state = self
            .service
            .inner
            .maintenance
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        state.background_active = state.background_active.saturating_sub(1);
        if state.background_active == 0 {
            self.service.inner.background_idle.notify_all();
        }
    }
}

impl Drop for MaintenancePause {
    fn drop(&mut self) {
        let mut state = self
            .service
            .inner
            .maintenance
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        state.depth = state.depth.saturating_sub(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    fn empty_fixture() -> (tempfile::TempDir, ConfigDir) {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        wc_config::ConfigDirExt::init(&cd).unwrap();
        wc_storage::sqlite::ensure_sqlite_db(&cd);
        (tmp, cd)
    }

    fn query() -> LibraryBrowserQuery {
        LibraryBrowserQuery {
            source_id: None,
            type_filter: wc_storage::sqlite::LibraryBrowserType::Usable,
            favorites_only: false,
            search: String::new(),
            sort: wc_storage::sqlite::LibraryBrowserSort::RecentlyAdded,
            cursor: None,
            limit: 10,
        }
    }

    #[test]
    fn readiness_is_idempotent_and_concurrent() {
        let service = Arc::new(LibraryService::new());
        let starts = Arc::new(AtomicUsize::new(0));
        let threads = (0..32)
            .map(|_| {
                let service = service.clone();
                let starts = starts.clone();
                std::thread::spawn(move || {
                    service.mark_frontend_ready_with(|| {
                        starts.fetch_add(1, Ordering::SeqCst);
                    });
                })
            })
            .collect::<Vec<_>>();
        for thread in threads {
            thread.join().unwrap();
        }
        assert!(service.is_ready());
        assert_eq!(starts.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn page_cache_is_bounded_and_local_invalidation_is_immediate() {
        let (_tmp, cd) = empty_fixture();
        let service = LibraryService::new();
        let first = service.page(&cd, &query()).unwrap();
        let second = service.page(&cd, &query()).unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(service.cache_stats().0, 1);
        assert!(service.cache_stats().1 <= MAX_CACHE_BYTES);
        service.invalidate_local_write();
        assert_eq!(service.cache_stats(), (0, 0, 0));
        assert!(service
            .page_with_loader(&cd, &query(), |_, _| Err(WcError::Sqlite("busy".into())))
            .is_err());
    }

    #[test]
    fn pause_for_maintenance_nests() {
        let service = LibraryService::new();
        let outer = service.pause_for_maintenance();
        assert_eq!(service.maintenance_depth(), 1);
        {
            let _inner = service.pause_for_maintenance();
            assert_eq!(service.maintenance_depth(), 2);
        }
        assert_eq!(service.maintenance_depth(), 1);
        drop(outer);
        assert_eq!(service.maintenance_depth(), 0);
    }

    #[test]
    fn maintenance_waits_for_active_background_work_and_blocks_new_work() {
        let service = Arc::new(LibraryService::new());
        let work = service.begin_background_work().unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        let pausing = service.clone();
        let thread = std::thread::spawn(move || {
            let pause = pausing.pause_for_maintenance();
            sender.send(()).unwrap();
            std::thread::sleep(Duration::from_millis(20));
            drop(pause);
        });
        std::thread::sleep(Duration::from_millis(20));
        assert!(receiver.try_recv().is_err());
        assert!(service.begin_background_work().is_none());
        drop(work);
        receiver.recv_timeout(Duration::from_secs(1)).unwrap();
        thread.join().unwrap();
        assert!(service.begin_background_work().is_some());
    }

    #[test]
    fn observer_does_not_hold_the_maintenance_lock_between_polls() {
        let (_tmp, cd) = empty_fixture();
        let service = LibraryService::new();
        service.page(&cd, &query()).unwrap();
        service.mark_frontend_ready();
        std::thread::sleep(OBSERVER_INTERVAL + Duration::from_millis(50));

        let started = Instant::now();
        let _pause = service.pause_for_maintenance();
        wc_storage::sqlite::repair(&cd).unwrap();
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn exact_total_is_cached_by_revision_and_criteria() {
        let (_tmp, cd) = empty_fixture();
        let service = LibraryService::new();
        assert_eq!(service.exact_total(&cd, &query(), 0).unwrap().total, 0);
        assert_eq!(service.exact_total(&cd, &query(), 0).unwrap().total, 0);
        assert_eq!(service.cache_stats().2, 1);
    }

    #[test]
    fn exact_total_normalizes_whitespace_and_ignores_limit_and_sort() {
        let (_tmp, cd) = empty_fixture();
        let service = LibraryService::new();
        let calls = AtomicUsize::new(0);
        let mut first = query();
        first.search = "alpha   beta".into();
        first.limit = 10;
        let mut equivalent = first.clone();
        equivalent.search = " alpha beta ".into();
        equivalent.limit = 250;
        equivalent.sort = wc_storage::sqlite::LibraryBrowserSort::NameAsc;
        let loader = |_: &rusqlite::Connection, _: &LibraryBrowserQuery, revision: u64| {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(LibraryBrowserTotal { revision, total: 7 })
        };
        assert_eq!(
            service
                .exact_total_with_loader(&cd, &first, 0, loader)
                .unwrap()
                .total,
            7
        );
        assert_eq!(
            service
                .exact_total_with_loader(&cd, &equivalent, 0, loader)
                .unwrap()
                .total,
            7
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn page_cache_normalizes_equivalent_search_whitespace() {
        let (_tmp, cd) = empty_fixture();
        let service = LibraryService::new();
        let calls = AtomicUsize::new(0);
        let mut first = query();
        first.search = " alpha   beta ".into();
        let mut equivalent = first.clone();
        equivalent.search = "alpha beta".into();
        let loader = |_: &rusqlite::Connection, _: &LibraryBrowserQuery| {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(LibraryBrowserPage {
                revision: 0,
                next_cursor: None,
                total: None,
                items: Vec::new(),
            })
        };

        service.page_with_loader(&cd, &first, loader).unwrap();
        service.page_with_loader(&cd, &equivalent, loader).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn concurrent_cold_page_is_single_flight() {
        let (_tmp, cd) = empty_fixture();
        let service = Arc::new(LibraryService::new());
        let calls = Arc::new(AtomicUsize::new(0));
        let barrier = Arc::new(std::sync::Barrier::new(16));
        let threads = (0..16)
            .map(|_| {
                let service = service.clone();
                let calls = calls.clone();
                let barrier = barrier.clone();
                let cd = ConfigDir {
                    path: cd.path.clone(),
                };
                std::thread::spawn(move || {
                    barrier.wait();
                    service
                        .page_with_loader(&cd, &query(), |_, _| {
                            calls.fetch_add(1, Ordering::SeqCst);
                            std::thread::sleep(Duration::from_millis(50));
                            Ok(LibraryBrowserPage {
                                revision: 0,
                                next_cursor: None,
                                total: None,
                                items: Vec::new(),
                            })
                        })
                        .unwrap()
                })
            })
            .collect::<Vec<_>>();
        let pages = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(pages.iter().all(|page| Arc::ptr_eq(page, &pages[0])));
    }

    #[test]
    fn concurrent_cold_total_is_single_flight() {
        let (_tmp, cd) = empty_fixture();
        let service = Arc::new(LibraryService::new());
        let calls = Arc::new(AtomicUsize::new(0));
        let barrier = Arc::new(std::sync::Barrier::new(12));
        let threads = (0..12)
            .map(|_| {
                let service = service.clone();
                let calls = calls.clone();
                let barrier = barrier.clone();
                let cd = ConfigDir {
                    path: cd.path.clone(),
                };
                std::thread::spawn(move || {
                    barrier.wait();
                    service
                        .exact_total_with_loader(&cd, &query(), 0, |_, _, revision| {
                            calls.fetch_add(1, Ordering::SeqCst);
                            std::thread::sleep(Duration::from_millis(40));
                            Ok(LibraryBrowserTotal { revision, total: 7 })
                        })
                        .unwrap()
                })
            })
            .collect::<Vec<_>>();
        for thread in threads {
            assert_eq!(thread.join().unwrap().total, 7);
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn cache_enforces_page_and_byte_limits() {
        let mut cache = CacheState::default();
        for revision in 0..=MAX_PAGE_COUNT as u64 {
            cache.insert_page(
                PageKey {
                    revision,
                    query: QueryKey(query()),
                },
                Arc::new(LibraryBrowserPage {
                    revision,
                    next_cursor: None,
                    total: None,
                    items: Vec::new(),
                }),
            );
        }
        assert_eq!(cache.pages.len(), MAX_PAGE_COUNT);

        let huge_path = format!("/{}", "x".repeat(MAX_CACHE_BYTES + 1));
        let entry = wc_core::types::WallpaperEntry {
            path: camino::Utf8PathBuf::from(huge_path),
            file_type: wc_core::types::FileType::Image,
            ext: "jpg".into(),
            backend: wc_core::types::Backend::Awww,
            size: 0,
            mtime: 0,
            resolution: String::new(),
            project: None,
        };
        cache.insert_page(
            PageKey {
                revision: 999,
                query: QueryKey(query()),
            },
            Arc::new(LibraryBrowserPage {
                revision: 999,
                next_cursor: None,
                total: None,
                items: vec![wc_storage::sqlite::LibraryBrowserItem {
                    wallpaper_id: 1,
                    entry,
                    favorite: false,
                    author: None,
                    added_at: String::new(),
                    sources: Vec::new(),
                }],
            }),
        );
        assert!(cache.bytes <= MAX_CACHE_BYTES);
        assert!(cache.pages.len() <= MAX_PAGE_COUNT);
    }

    #[test]
    fn transient_storage_failure_returns_last_stale_page() {
        let (_tmp, cd) = empty_fixture();
        let service = LibraryService::new();
        let first = service
            .page_with_loader(&cd, &query(), |_, _| {
                Ok(LibraryBrowserPage {
                    revision: 0,
                    next_cursor: None,
                    total: None,
                    items: Vec::new(),
                })
            })
            .unwrap();
        service.invalidate_external_change();
        let fallback = service
            .page_with_loader(&cd, &query(), |_, _| {
                Err(WcError::Sqlite("temporary busy".into()))
            })
            .unwrap();
        assert!(Arc::ptr_eq(&first, &fallback));
    }

    #[test]
    fn stale_fallback_keeps_first_page_after_later_cursor_pages() {
        let (_tmp, cd) = empty_fixture();
        let service = LibraryService::new();
        let first_query = query();
        let first = service
            .page_with_loader(&cd, &first_query, |_, _| {
                Ok(LibraryBrowserPage {
                    revision: 0,
                    next_cursor: Some("next".into()),
                    total: None,
                    items: Vec::new(),
                })
            })
            .unwrap();
        let mut next_query = first_query.clone();
        next_query.cursor = Some("next".into());
        service
            .page_with_loader(&cd, &next_query, |_, _| {
                Ok(LibraryBrowserPage {
                    revision: 0,
                    next_cursor: None,
                    total: None,
                    items: Vec::new(),
                })
            })
            .unwrap();
        service.invalidate_external_change();

        let fallback = service
            .page_with_loader(&cd, &first_query, |_, _| {
                Err(WcError::Sqlite("temporary busy".into()))
            })
            .unwrap();
        assert!(Arc::ptr_eq(&first, &fallback));
    }

    #[test]
    fn observer_invalidates_external_revision_within_one_second() {
        let (_tmp, cd) = empty_fixture();
        let service = LibraryService::new();
        service.page(&cd, &query()).unwrap();
        assert_eq!(service.cache_stats().0, 1);
        service.mark_frontend_ready();
        std::thread::sleep(Duration::from_millis(50));

        let mut writer = rusqlite::Connection::open(cd.db_path()).unwrap();
        let tx = writer.transaction().unwrap();
        wc_storage::sqlite::bump_library_revision(&tx).unwrap();
        tx.commit().unwrap();

        let deadline = Instant::now() + Duration::from_secs(1);
        while service.cache_stats().0 != 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(service.cache_stats().0, 0);
    }

    #[test]
    fn sqlite_progress_deadline_interrupts_and_is_cleared() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let started = Instant::now();
        let result = with_sql_deadline(&conn, Duration::from_millis(2), |conn| {
            conn.query_row(
                "WITH RECURSIVE count(x) AS (
                     VALUES(0) UNION ALL SELECT x + 1 FROM count WHERE x < 100000000
                 ) SELECT sum(x) FROM count",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|error| WcError::Sqlite(error.to_string()))
        });
        assert!(result.is_err());
        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(
            conn.query_row("SELECT 1", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
}
