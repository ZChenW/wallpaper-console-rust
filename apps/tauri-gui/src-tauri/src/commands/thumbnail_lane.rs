use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use tauri::async_runtime::Mutex;
use wc_preview::{ThumbnailSize, LARGE_THUMBNAIL_CONCURRENCY};

use super::super::common::ThumbnailDto;

type CommandOutput = Result<ThumbnailDto, String>;
type Flight<R> = Arc<Mutex<Option<Result<R, String>>>>;

struct LargeLane {
    slots: [Arc<Mutex<()>>; LARGE_THUMBNAIL_CONCURRENCY],
    next_waiter: AtomicUsize,
}

impl Default for LargeLane {
    fn default() -> Self {
        Self {
            slots: std::array::from_fn(|_| Arc::new(Mutex::new(()))),
            next_waiter: AtomicUsize::new(0),
        }
    }
}

struct ThumbnailJobs<R> {
    large_lane: Arc<LargeLane>,
    flights: StdMutex<HashMap<PathBuf, Flight<R>>>,
}

impl<R> Default for ThumbnailJobs<R> {
    fn default() -> Self {
        Self {
            large_lane: Arc::new(LargeLane::default()),
            flights: StdMutex::new(HashMap::new()),
        }
    }
}

impl<R: Clone + Send + 'static> ThumbnailJobs<R> {
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
        run_in_lane(&self.large_lane, ThumbnailSize::Large, move || {
            let output = work();
            *completion.result = Some(Ok(output.clone()));
            output
        })
        .await
    }
}

struct FlightCompletion<R, G: std::ops::DerefMut<Target = Option<Result<R, String>>>> {
    jobs: Arc<ThumbnailJobs<R>>,
    key: PathBuf,
    result: G,
}

impl<R, G: std::ops::DerefMut<Target = Option<Result<R, String>>>> Drop for FlightCompletion<R, G> {
    fn drop(&mut self) {
        self.result
            .get_or_insert(Err("thumbnail generation interrupted".into()));
        self.jobs
            .flights
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.key);
    }
}

/// Wait asynchronously, so queued Large work never occupies blocking-pool slots
/// needed by Standard thumbnails. Move the permit into the worker so cancellation
/// of the IPC future cannot admit another Large job before this one finishes.
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
        ThumbnailSize::Standard => run_in_lane(&jobs.large_lane, tier, work).await,
    }
}

async fn run_in_lane<F, R>(lane: &LargeLane, tier: ThumbnailSize, work: F) -> Result<R, String>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    let permit = match tier {
        ThumbnailSize::Large => {
            // Each worker holds one of the fixed slots until it exits. Prefer a
            // free slot; otherwise spread async waiters round-robin across them.
            let permit = match lane
                .slots
                .iter()
                .find_map(|slot| Arc::clone(slot).try_lock_owned().ok())
            {
                Some(permit) => permit,
                None => {
                    let index = lane.next_waiter.fetch_add(1, Ordering::Relaxed)
                        % LARGE_THUMBNAIL_CONCURRENCY;
                    Arc::clone(&lane.slots[index]).lock_owned().await
                }
            };
            Some(permit)
        }
        ThumbnailSize::Standard => None,
    };
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        work()
    })
    .await
    .map_err(|error| error.to_string())
}

#[cfg(test)]
#[path = "thumbnail_lane_tests.rs"]
mod tests;
