use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use tauri::async_runtime::Mutex;
use wc_preview::{ThumbnailSize, LARGE_THUMBNAIL_CONCURRENCY, PREVIEW_CLIP_CONCURRENCY};

use super::super::common::ThumbnailDto;

type CommandOutput = Result<ThumbnailDto, String>;
type ClipOutput = Result<Vec<u8>, String>;
type Flight<R> = Arc<Mutex<Option<Result<R, String>>>>;

struct PreviewLane<const WIDTH: usize> {
    slots: [Arc<Mutex<()>>; WIDTH],
    next_waiter: AtomicUsize,
}

impl<const WIDTH: usize> Default for PreviewLane<WIDTH> {
    fn default() -> Self {
        Self {
            slots: std::array::from_fn(|_| Arc::new(Mutex::new(()))),
            next_waiter: AtomicUsize::new(0),
        }
    }
}

type ThumbnailJobs<R> = PreviewJobs<R, LARGE_THUMBNAIL_CONCURRENCY>;
type ClipJobs<R> = PreviewJobs<R, PREVIEW_CLIP_CONCURRENCY>;
#[cfg(test)]
type LargeLane = PreviewLane<LARGE_THUMBNAIL_CONCURRENCY>;

struct PreviewJobs<R, const WIDTH: usize> {
    lane: Arc<PreviewLane<WIDTH>>,
    flights: StdMutex<HashMap<PathBuf, Flight<R>>>,
}

impl<R, const WIDTH: usize> Default for PreviewJobs<R, WIDTH> {
    fn default() -> Self {
        Self {
            lane: Arc::new(PreviewLane::default()),
            flights: StdMutex::new(HashMap::new()),
        }
    }
}

impl<R: Clone + Send + 'static, const WIDTH: usize> PreviewJobs<R, WIDTH> {
    async fn run<F>(self: &Arc<Self>, key: PathBuf, work: F) -> Result<R, String>
    where
        F: FnOnce() -> R + Send + 'static,
    {
        let flight = self
            .flights
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(key.clone())
            .or_default()
            .clone();
        // Same-key waiters await the published result rather than taking a lane
        // and repeating a failed generation once the first caller has finished.
        let result = flight.lock_owned().await;
        if let Some(result) = result.as_ref() {
            return result.clone();
        }
        let mut completion = FlightCompletion {
            jobs: Arc::clone(self),
            key,
            result,
        };
        run_in_lane(&self.lane, move || {
            let output = work();
            *completion.result = Some(Ok(output.clone()));
            output
        })
        .await
    }
}

struct FlightCompletion<
    R,
    G: std::ops::DerefMut<Target = Option<Result<R, String>>>,
    const WIDTH: usize,
> {
    jobs: Arc<PreviewJobs<R, WIDTH>>,
    key: PathBuf,
    result: G,
}

impl<R, G: std::ops::DerefMut<Target = Option<Result<R, String>>>, const WIDTH: usize> Drop
    for FlightCompletion<R, G, WIDTH>
{
    fn drop(&mut self) {
        self.result
            .get_or_insert(Err("preview generation interrupted".into()));
        self.jobs
            .flights
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.key);
    }
}

/// Wait asynchronously so queued previews do not occupy blocking-pool slots.
/// Workers retain permits through IPC cancellation. Standard bypasses both lanes.
pub(super) async fn run<F>(
    key: PathBuf,
    tier: ThumbnailSize,
    work: F,
) -> Result<CommandOutput, String>
where
    F: FnOnce() -> CommandOutput + Send + 'static,
{
    static JOBS: OnceLock<Arc<ThumbnailJobs<CommandOutput>>> = OnceLock::new();
    let jobs = JOBS.get_or_init(|| Arc::new(ThumbnailJobs::default()));
    match tier {
        ThumbnailSize::Large => jobs.run(key, work).await,
        ThumbnailSize::Standard => run_unrestricted(work).await,
    }
}

pub(super) async fn run_clip<F>(key: PathBuf, work: F) -> Result<ClipOutput, String>
where
    F: FnOnce() -> ClipOutput + Send + 'static,
{
    static JOBS: OnceLock<Arc<ClipJobs<ClipOutput>>> = OnceLock::new();
    JOBS.get_or_init(|| Arc::new(ClipJobs::default()))
        .run(key, work)
        .await
}

async fn run_in_lane<F, R, const WIDTH: usize>(
    lane: &PreviewLane<WIDTH>,
    work: F,
) -> Result<R, String>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    // Each worker holds one of the fixed slots until it exits. Prefer a
    // free slot; otherwise spread async waiters round-robin across them.
    let permit = match lane
        .slots
        .iter()
        .find_map(|slot| Arc::clone(slot).try_lock_owned().ok())
    {
        Some(permit) => permit,
        None => {
            let index = lane.next_waiter.fetch_add(1, Ordering::Relaxed) % WIDTH;
            Arc::clone(&lane.slots[index]).lock_owned().await
        }
    };
    run_unrestricted(move || {
        let _permit = permit;
        work()
    })
    .await
}

async fn run_unrestricted<F, R>(work: F) -> Result<R, String>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| error.to_string())
}

#[cfg(test)]
#[path = "thumbnail_lane_tests.rs"]
mod tests;
