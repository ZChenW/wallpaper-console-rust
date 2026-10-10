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
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::time::Duration;

    fn wait_until(condition: impl Fn() -> bool) {
        let stop = std::time::Instant::now() + Duration::from_secs(3);
        while !condition() {
            assert!(std::time::Instant::now() < stop, "Large jobs did not queue");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn concurrent_large_commands_share_the_first_result_including_failure() {
        same_key_requests_share::<LARGE_THUMBNAIL_CONCURRENCY>();
    }

    #[test]
    fn concurrent_clip_commands_share_the_first_result_including_failure() {
        same_key_requests_share::<PREVIEW_CLIP_CONCURRENCY>();
    }

    fn same_key_requests_share<const WIDTH: usize>() {
        let runtime = tauri::async_runtime::TokioRuntime::new().unwrap();
        runtime.block_on(async {
            for expected in [
                Ok("large.webp".to_string()),
                Err("probe_failed".to_string()),
            ] {
                let jobs = Arc::new(PreviewJobs::<_, WIDTH>::default());
                let calls = Arc::new(AtomicUsize::new(0));
                let (entered_tx, entered_rx) = mpsc::channel();
                let (release_tx, release_rx) = mpsc::channel();
                let first = {
                    let jobs = Arc::clone(&jobs);
                    let calls = Arc::clone(&calls);
                    let expected = expected.clone();
                    runtime.spawn(async move {
                        jobs.run("same-key".into(), move || {
                            calls.fetch_add(1, Ordering::SeqCst);
                            entered_tx.send(()).unwrap();
                            release_rx.recv().unwrap();
                            expected
                        })
                        .await
                        .unwrap()
                    })
                };
                entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
                let waiters: Vec<_> = (0..7)
                    .map(|_| {
                        let jobs = Arc::clone(&jobs);
                        let calls = Arc::clone(&calls);
                        runtime.spawn(async move {
                            jobs.run("same-key".into(), move || {
                                calls.fetch_add(1, Ordering::SeqCst);
                                Err("second producer must not run".into())
                            })
                            .await
                            .unwrap()
                        })
                    })
                    .collect();
                let stop = std::time::Instant::now() + Duration::from_secs(2);
                loop {
                    let attached = jobs
                        .flights
                        .lock()
                        .unwrap()
                        .get(&PathBuf::from("same-key"))
                        .is_some_and(|flight| Arc::strong_count(flight) == 9);
                    if attached {
                        break;
                    }
                    assert!(
                        std::time::Instant::now() < stop,
                        "command waiters did not attach"
                    );
                    std::thread::sleep(Duration::from_millis(1));
                }
                release_tx.send(()).unwrap();
                assert_eq!(first.await.unwrap(), expected);
                for waiter in waiters {
                    assert_eq!(waiter.await.unwrap(), expected);
                }
                assert_eq!(calls.load(Ordering::SeqCst), 1);
                assert!(jobs.flights.lock().unwrap().is_empty());
            }
        });
    }

    #[test]
    fn large_jobs_wait_outside_the_pool_and_standard_bypasses_the_lane() {
        assert_eq!(LARGE_THUMBNAIL_CONCURRENCY, 3);
        let runtime = tauri::async_runtime::TokioRuntime::new().unwrap();
        runtime.block_on(async {
            let lane = Arc::new(LargeLane::default());
            let active = Arc::new(AtomicUsize::new(0));
            let peak = Arc::new(AtomicUsize::new(0));
            let (entered_tx, entered_rx) = mpsc::channel();
            let mut releases = Vec::new();
            let holders: Vec<_> = (0..LARGE_THUMBNAIL_CONCURRENCY)
                .map(|_| {
                    let lane = Arc::clone(&lane);
                    let active = Arc::clone(&active);
                    let peak = Arc::clone(&peak);
                    let entered_tx = entered_tx.clone();
                    let (release_tx, release_rx) = mpsc::channel();
                    releases.push(release_tx);
                    runtime.spawn(async move {
                        run_in_lane(&lane, move || {
                            let count = active.fetch_add(1, Ordering::SeqCst) + 1;
                            assert!(count <= LARGE_THUMBNAIL_CONCURRENCY);
                            peak.fetch_max(count, Ordering::SeqCst);
                            entered_tx.send(()).unwrap();
                            release_rx.recv().unwrap();
                            active.fetch_sub(1, Ordering::SeqCst);
                        })
                        .await
                        .unwrap()
                    })
                })
                .collect();
            for _ in 0..LARGE_THUMBNAIL_CONCURRENCY {
                entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            }
            let jobs: Vec<_> = (0..12)
                .map(|_| {
                    let lane = Arc::clone(&lane);
                    let active = Arc::clone(&active);
                    let peak = Arc::clone(&peak);
                    runtime.spawn(async move {
                        run_in_lane(&lane, move || {
                            let count = active.fetch_add(1, Ordering::SeqCst) + 1;
                            assert!(count <= LARGE_THUMBNAIL_CONCURRENCY);
                            peak.fetch_max(count, Ordering::SeqCst);
                            active.fetch_sub(1, Ordering::SeqCst);
                        })
                        .await
                        .unwrap()
                    })
                })
                .collect();
            wait_until(|| lane.next_waiter.load(Ordering::Relaxed) == jobs.len());
            assert_eq!(active.load(Ordering::SeqCst), LARGE_THUMBNAIL_CONCURRENCY);
            let (standard_tx, standard_rx) = mpsc::channel();
            let standard = {
                runtime.spawn(async move {
                    run_unrestricted(move || standard_tx.send(()).unwrap())
                        .await
                        .unwrap()
                })
            };
            let result = standard_rx.recv_timeout(Duration::from_secs(2));
            for release in releases {
                release.send(()).unwrap();
            }
            standard.await.unwrap();
            for holder in holders {
                holder.await.unwrap();
            }
            for job in jobs {
                job.await.unwrap();
            }
            assert!(result.is_ok(), "Standard was queued behind Large work");
            assert_eq!(peak.load(Ordering::SeqCst), LARGE_THUMBNAIL_CONCURRENCY);
            assert_eq!(active.load(Ordering::SeqCst), 0);
        });
    }

    #[test]
    fn cancelling_a_caller_does_not_release_a_running_large_worker() {
        let runtime = tauri::async_runtime::TokioRuntime::new().unwrap();
        runtime.block_on(async {
            let lane = Arc::new(LargeLane::default());
            let (entered_tx, entered_rx) = mpsc::channel();
            let (finished_tx, finished_rx) = mpsc::channel();
            let mut releases = Vec::new();
            let mut jobs: Vec<_> = (0..LARGE_THUMBNAIL_CONCURRENCY)
                .map(|_| {
                    let lane = Arc::clone(&lane);
                    let entered_tx = entered_tx.clone();
                    let finished_tx = finished_tx.clone();
                    let (release_tx, release_rx) = mpsc::channel();
                    releases.push(release_tx);
                    let job = runtime.spawn(async move {
                        run_in_lane(&lane, move || {
                            entered_tx.send(()).unwrap();
                            release_rx.recv().unwrap();
                            finished_tx.send(()).unwrap();
                        })
                        .await
                        .unwrap()
                    });
                    // Admit in order so the cancelled caller owns slot zero, which
                    // the first queued waiter will use after that worker exits.
                    entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
                    job
                })
                .collect();
            let job = jobs.remove(0);
            job.abort();
            assert!(job.await.unwrap_err().is_cancelled());
            let held = lane.slots.iter().all(|slot| slot.try_lock().is_err());
            assert!(held, "aborted IPC released a worker's capacity early");
            let queued = {
                let lane = Arc::clone(&lane);
                runtime.spawn(async move {
                    run_in_lane(&lane, move || {
                        entered_tx.send(()).unwrap();
                    })
                    .await
                    .unwrap()
                })
            };
            wait_until(|| lane.next_waiter.load(Ordering::Relaxed) == 1);
            let premature = entered_rx.recv_timeout(Duration::from_millis(50));
            releases.remove(0).send(()).unwrap();
            finished_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            if premature.is_err() {
                entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            }
            queued.await.unwrap();
            for release in releases {
                release.send(()).unwrap();
            }
            for job in jobs {
                job.await.unwrap();
            }
            assert!(
                matches!(premature, Err(mpsc::RecvTimeoutError::Timeout)),
                "queued Large job started before the cancelled caller's worker finished"
            );
            assert!(lane.slots.iter().all(|slot| slot.try_lock().is_ok()));
        });
    }

    #[test]
    fn clip_lane_serializes_work_and_is_independent_of_large_and_standard() {
        assert_eq!(PREVIEW_CLIP_CONCURRENCY, 1);
        let runtime = tauri::async_runtime::TokioRuntime::new().unwrap();
        runtime.block_on(async {
            let clips = Arc::new(ClipJobs::default());
            let large = Arc::new(ThumbnailJobs::default());
            let active = Arc::new(AtomicUsize::new(0));
            let peak = Arc::new(AtomicUsize::new(0));
            let (entered_tx, entered_rx) = mpsc::channel();
            let (release_tx, release_rx) = mpsc::channel();
            let first_clip = {
                let clips = Arc::clone(&clips);
                let active = Arc::clone(&active);
                let peak = Arc::clone(&peak);
                let entered_tx = entered_tx.clone();
                runtime.spawn(async move {
                    clips
                        .run("held-clip".into(), move || {
                            let count = active.fetch_add(1, Ordering::SeqCst) + 1;
                            peak.fetch_max(count, Ordering::SeqCst);
                            entered_tx.send(()).unwrap();
                            release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                            active.fetch_sub(1, Ordering::SeqCst);
                        })
                        .await
                        .unwrap();
                })
            };
            entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            let queued: Vec<_> = (0..12)
                .map(|i| {
                    let clips = Arc::clone(&clips);
                    let active = Arc::clone(&active);
                    let peak = Arc::clone(&peak);
                    runtime.spawn(async move {
                        clips
                            .run(format!("clip-{i}").into(), move || {
                                let count = active.fetch_add(1, Ordering::SeqCst) + 1;
                                assert_eq!(count, 1, "two clips ran at once");
                                peak.fetch_max(count, Ordering::SeqCst);
                                active.fetch_sub(1, Ordering::SeqCst);
                            })
                            .await
                            .unwrap();
                    })
                })
                .collect();
            wait_until(|| clips.lane.next_waiter.load(Ordering::Relaxed) == queued.len());

            // Fill Large while a clip runs and many more clips await admission.
            let mut large_releases = Vec::new();
            let large_holders: Vec<_> = (0..LARGE_THUMBNAIL_CONCURRENCY)
                .map(|i| {
                    let large = Arc::clone(&large);
                    let entered_tx = entered_tx.clone();
                    let (tx, rx) = mpsc::channel();
                    large_releases.push(tx);
                    runtime.spawn(async move {
                        large
                            .run(format!("large-{i}").into(), move || {
                                entered_tx.send(()).unwrap();
                                rx.recv_timeout(Duration::from_secs(10)).unwrap();
                            })
                            .await
                            .unwrap();
                    })
                })
                .collect();
            for _ in 0..LARGE_THUMBNAIL_CONCURRENCY {
                entered_rx
                    .recv_timeout(Duration::from_secs(2))
                    .expect("clip blocked Large");
            }
            let (standard_release, standard_rx) = mpsc::channel();
            let standard = runtime.spawn(async move {
                run(PathBuf::new(), ThumbnailSize::Standard, move || {
                    entered_tx.send(()).unwrap();
                    standard_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                    Err("standard test result".into())
                })
                .await
                .unwrap()
            });
            entered_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("clip/Large blocked Standard");

            // Queued clips must complete while Large and Standard remain held.
            release_tx.send(()).unwrap();
            first_clip.await.unwrap();
            for clip in queued {
                clip.await.unwrap();
            }
            assert_eq!(peak.load(Ordering::SeqCst), 1);
            assert_eq!(active.load(Ordering::SeqCst), 0);
            assert!(!standard.is_finished());
            assert!(large_holders.iter().all(|holder| !holder.is_finished()));
            for release in large_releases {
                release.send(()).unwrap();
            }
            standard_release.send(()).unwrap();
            for holder in large_holders {
                holder.await.unwrap();
            }
            assert!(standard.await.unwrap().is_err());
        });
    }

    #[test]
    fn cancelling_clip_ipc_keeps_its_worker_and_same_key_flight_alive() {
        let runtime = tauri::async_runtime::TokioRuntime::new().unwrap();
        runtime.block_on(async {
            let clips = Arc::new(ClipJobs::default());
            let (entered_tx, entered_rx) = mpsc::channel();
            let (release_tx, release_rx) = mpsc::channel();
            let first = {
                let clips = Arc::clone(&clips);
                runtime.spawn(async move {
                    clips
                        .run("same-clip".into(), move || {
                            entered_tx.send(()).unwrap();
                            release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                            vec![1, 2, 3]
                        })
                        .await
                })
            };
            entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            first.abort();
            assert!(first.await.unwrap_err().is_cancelled());
            assert!(clips.lane.slots[0].try_lock().is_err());
            let waiter = {
                let clips = Arc::clone(&clips);
                runtime.spawn(async move {
                    clips
                        .run("same-clip".into(), || panic!("repeated clip producer"))
                        .await
                })
            };
            wait_until(|| {
                clips
                    .flights
                    .lock()
                    .unwrap()
                    .get(&PathBuf::from("same-clip"))
                    .is_some_and(|flight| Arc::strong_count(flight) == 3)
            });
            release_tx.send(()).unwrap();
            assert_eq!(waiter.await.unwrap().unwrap(), vec![1, 2, 3]);
        });
    }
}
