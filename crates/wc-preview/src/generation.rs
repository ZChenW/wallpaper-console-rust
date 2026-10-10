//! Separate Large/clip capacity and share one physical generation per cache key.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::{
    ThumbnailFailure, ThumbnailSize, LARGE_THUMBNAIL_CONCURRENCY, LARGE_THUMBNAIL_TIMEOUT,
    METADATA_COMMAND_TIMEOUT, PREVIEW_CLIP_CONCURRENCY, PREVIEW_CLIP_TIMEOUT,
    THUMBNAIL_COMMAND_TIMEOUT,
};

type GenerationResult = Result<(PathBuf, bool), ThumbnailFailure>;

#[derive(Clone, Copy)]
enum GenerationKind {
    Thumbnail(ThumbnailSize),
    Clip,
}

#[derive(Default)]
struct Flight {
    result: Mutex<Option<GenerationResult>>,
    finished: Condvar,
}

#[derive(Default)]
pub(super) struct GenerationCoordinator {
    flights: Mutex<HashMap<PathBuf, Arc<Flight>>>,
    active_large: Mutex<usize>,
    lane_available: Condvar,
    active_clip: Mutex<usize>,
    clip_available: Condvar,
}

impl GenerationCoordinator {
    pub(super) fn run(
        &self,
        key: PathBuf,
        tier: ThumbnailSize,
        generate: impl FnOnce(ThumbnailDeadline) -> GenerationResult,
    ) -> GenerationResult {
        self.run_with_budget(key, GenerationKind::Thumbnail(tier), generate)
    }

    pub(super) fn run_clip(
        &self,
        key: PathBuf,
        generate: impl FnOnce(ThumbnailDeadline) -> GenerationResult,
    ) -> GenerationResult {
        self.run_with_budget(key, GenerationKind::Clip, generate)
    }

    pub(super) fn run_thumbnail_before(
        &self,
        key: PathBuf,
        tier: ThumbnailSize,
        deadline: ThumbnailDeadline,
        generate: impl FnOnce(ThumbnailDeadline) -> GenerationResult,
    ) -> GenerationResult {
        self.run_before(
            key,
            GenerationKind::Thumbnail(tier),
            Some(deadline),
            generate,
        )
    }

    fn run_with_budget(
        &self,
        key: PathBuf,
        kind: GenerationKind,
        generate: impl FnOnce(ThumbnailDeadline) -> GenerationResult,
    ) -> GenerationResult {
        self.run_before(key, kind, None, generate)
    }

    fn run_before(
        &self,
        key: PathBuf,
        kind: GenerationKind,
        parent: Option<ThumbnailDeadline>,
        generate: impl FnOnce(ThumbnailDeadline) -> GenerationResult,
    ) -> GenerationResult {
        let (flight, leader) = {
            let mut flights = self.flights.lock().unwrap_or_else(|e| e.into_inner());
            match flights.get(&key) {
                Some(flight) => (Arc::clone(flight), false),
                None => {
                    let flight = Arc::new(Flight::default());
                    flights.insert(key.clone(), Arc::clone(&flight));
                    (flight, true)
                }
            }
        };
        if !leader {
            let mut result = flight.result.lock().unwrap_or_else(|e| e.into_inner());
            while result.is_none() {
                result = if let Some(deadline) = parent {
                    let remaining = deadline.command_timeout();
                    if remaining.is_zero() {
                        return Err(ThumbnailFailure::TimedOut);
                    }
                    flight
                        .finished
                        .wait_timeout(result, remaining)
                        .unwrap_or_else(|e| e.into_inner())
                        .0
                } else {
                    flight
                        .finished
                        .wait(result)
                        .unwrap_or_else(|e| e.into_inner())
                };
            }
            return result.as_ref().unwrap().clone();
        }

        // Publish a failure and release waiters even if a producer unwinds.
        let completion = FlightCompletion {
            coordinator: self,
            key,
            flight,
        };
        let result = (|| {
            let _lane = match kind {
                GenerationKind::Thumbnail(ThumbnailSize::Large) => {
                    Some(GenerationPermit::acquire_before(
                        &self.active_large,
                        &self.lane_available,
                        LARGE_THUMBNAIL_CONCURRENCY,
                        parent,
                    )?)
                }
                GenerationKind::Clip => Some(GenerationPermit::acquire_before(
                    &self.active_clip,
                    &self.clip_available,
                    PREVIEW_CLIP_CONCURRENCY,
                    parent,
                )?),
                GenerationKind::Thumbnail(ThumbnailSize::Standard) => None,
            };
            let deadline = match kind {
                GenerationKind::Thumbnail(tier) => ThumbnailDeadline::new(tier),
                GenerationKind::Clip => ThumbnailDeadline::clip(),
            };
            let deadline = parent.map_or(deadline, |parent| deadline.bounded_by(parent));
            generate(deadline)
        })();
        *completion
            .flight
            .result
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(result.clone());
        result
    }
}

struct GenerationPermit<'a> {
    active: &'a Mutex<usize>,
    available: &'a Condvar,
}

impl<'a> GenerationPermit<'a> {
    fn acquire_before(
        active: &'a Mutex<usize>,
        available: &'a Condvar,
        width: usize,
        deadline: Option<ThumbnailDeadline>,
    ) -> Result<Self, ThumbnailFailure> {
        let mut count = active.lock().unwrap_or_else(|e| e.into_inner());
        while *count >= width {
            count = if let Some(deadline) = deadline {
                let remaining = deadline.command_timeout();
                if remaining.is_zero() {
                    return Err(ThumbnailFailure::TimedOut);
                }
                available
                    .wait_timeout(count, remaining)
                    .unwrap_or_else(|e| e.into_inner())
                    .0
            } else {
                available.wait(count).unwrap_or_else(|e| e.into_inner())
            };
        }
        if deadline.is_some_and(|deadline| deadline.command_timeout().is_zero()) {
            return Err(ThumbnailFailure::TimedOut);
        }
        *count += 1;
        Ok(Self { active, available })
    }
}

impl Drop for GenerationPermit<'_> {
    fn drop(&mut self) {
        let mut active = self.active.lock().unwrap_or_else(|e| e.into_inner());
        *active -= 1;
        self.available.notify_one();
    }
}

struct FlightCompletion<'a> {
    coordinator: &'a GenerationCoordinator,
    key: PathBuf,
    flight: Arc<Flight>,
}

impl Drop for FlightCompletion<'_> {
    fn drop(&mut self) {
        let mut flights = self
            .coordinator
            .flights
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut result = self.flight.result.lock().unwrap_or_else(|e| e.into_inner());
        result.get_or_insert(Err(ThumbnailFailure::ProbeFailed));
        flights.remove(&self.key);
        self.flight.finished.notify_all();
    }
}

/// Large/clip helpers share a generation budget, created only after lane admission.
/// Standard retains its original per-command timeouts. Native image decoding
/// is allocation-bounded but cannot be interrupted by an external-process deadline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ThumbnailDeadline {
    expires_at: Option<Instant>,
}

impl ThumbnailDeadline {
    pub(super) fn bounded_by(self, other: Self) -> Self {
        Self {
            expires_at: match (self.expires_at, other.expires_at) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            },
        }
    }

    pub(super) fn clip() -> Self {
        Self {
            expires_at: Some(Instant::now() + PREVIEW_CLIP_TIMEOUT),
        }
    }

    pub(super) fn new(tier: ThumbnailSize) -> Self {
        Self {
            expires_at: (tier == ThumbnailSize::Large)
                .then(|| Instant::now() + LARGE_THUMBNAIL_TIMEOUT),
        }
    }

    pub(super) fn is_large(self) -> bool {
        self.expires_at.is_some()
    }

    fn command_timeout(self) -> Duration {
        self.expires_at
            .map_or(THUMBNAIL_COMMAND_TIMEOUT, |deadline| {
                deadline.saturating_duration_since(Instant::now())
            })
    }

    pub(super) fn metadata_timeout(self) -> Duration {
        METADATA_COMMAND_TIMEOUT.min(self.command_timeout())
    }

    pub(super) fn command_succeeded(self, command: &mut Command, label: &str) -> bool {
        let timeout = self.command_timeout();
        !timeout.is_zero() && crate::command_succeeded(command, timeout, label)
    }

    pub(super) fn command_output(
        self,
        command: &mut Command,
    ) -> Result<crate::DeadlineCommandOutput, crate::DeadlineCommandError> {
        let timeout = self.command_timeout();
        if timeout.is_zero() {
            return Err(crate::DeadlineCommandError::TimedOut);
        }
        crate::run_command_with_deadline(command, timeout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{mpsc, Barrier};
    use std::thread;

    fn wait_until(condition: impl Fn() -> bool) {
        let stop = Instant::now() + Duration::from_secs(3);
        while !condition() {
            assert!(
                Instant::now() < stop,
                "concurrent callers did not reach the test gate"
            );
            thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn large_lane_caps_concurrent_generations_and_standard_bypasses_it() {
        assert_eq!(LARGE_THUMBNAIL_CONCURRENCY, 3);
        let coordinator = Arc::new(GenerationCoordinator::default());
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let start = Arc::new(Barrier::new(13));
        let workers: Vec<_> = (0..12)
            .map(|i| {
                let coordinator = Arc::clone(&coordinator);
                let active = Arc::clone(&active);
                let peak = Arc::clone(&peak);
                let gate = Arc::clone(&gate);
                let start = Arc::clone(&start);
                thread::spawn(move || {
                    start.wait();
                    coordinator
                        .run(format!("large-{i}").into(), ThumbnailSize::Large, |_| {
                            let count = active.fetch_add(1, Ordering::SeqCst) + 1;
                            assert!(count <= LARGE_THUMBNAIL_CONCURRENCY);
                            peak.fetch_max(count, Ordering::SeqCst);
                            let (lock, ready) = &*gate;
                            let mut open = lock.lock().unwrap();
                            while !*open {
                                open = ready.wait(open).unwrap();
                            }
                            active.fetch_sub(1, Ordering::SeqCst);
                            Ok((format!("large-{i}").into(), false))
                        })
                        .unwrap();
                })
            })
            .collect();
        start.wait();
        wait_until(|| {
            coordinator.flights.lock().unwrap().len() == 12
                && active.load(Ordering::SeqCst) == LARGE_THUMBNAIL_CONCURRENCY
        });
        let (tx, rx) = mpsc::channel();
        let standard = {
            let coordinator = Arc::clone(&coordinator);
            thread::spawn(move || {
                coordinator
                    .run("standard".into(), ThumbnailSize::Standard, |_| {
                        tx.send(()).unwrap();
                        Ok(("standard".into(), false))
                    })
                    .unwrap();
            })
        };
        let standard_result = rx.recv_timeout(Duration::from_secs(1));
        *gate.0.lock().unwrap() = true;
        gate.1.notify_all();
        standard.join().unwrap();
        for worker in workers {
            worker.join().unwrap();
        }
        assert!(
            standard_result.is_ok(),
            "Standard waited behind a full Large lane"
        );
        assert_eq!(peak.load(Ordering::SeqCst), LARGE_THUMBNAIL_CONCURRENCY);
        assert!(coordinator.flights.lock().unwrap().is_empty());
    }

    #[test]
    fn same_key_callers_share_one_generation_including_failures() {
        for kind in [
            GenerationKind::Thumbnail(ThumbnailSize::Standard),
            GenerationKind::Thumbnail(ThumbnailSize::Large),
            GenerationKind::Clip,
        ] {
            for expected in [
                Ok((PathBuf::from("same.webp"), false)),
                Err(ThumbnailFailure::ProbeFailed),
            ] {
                let coordinator = Arc::new(GenerationCoordinator::default());
                let gate = Arc::new((Mutex::new(false), Condvar::new()));
                let calls = Arc::new(AtomicUsize::new(0));
                let workers: Vec<_> = (0..8)
                    .map(|_| {
                        let coordinator = Arc::clone(&coordinator);
                        let gate = Arc::clone(&gate);
                        let calls = Arc::clone(&calls);
                        let expected = expected.clone();
                        thread::spawn(move || {
                            coordinator.run_with_budget("same-preview".into(), kind, |_| {
                                calls.fetch_add(1, Ordering::SeqCst);
                                let mut open = gate.0.lock().unwrap();
                                while !*open {
                                    open = gate.1.wait(open).unwrap();
                                }
                                expected
                            })
                        })
                    })
                    .collect();
                wait_until(|| {
                    coordinator
                        .flights
                        .lock()
                        .unwrap()
                        .get(&PathBuf::from("same-preview"))
                        .is_some_and(|flight| Arc::strong_count(flight) == 9)
                });
                *gate.0.lock().unwrap() = true;
                gate.1.notify_all();
                for worker in workers {
                    assert_eq!(worker.join().unwrap(), expected);
                }
                assert_eq!(calls.load(Ordering::SeqCst), 1);
                assert!(coordinator.flights.lock().unwrap().is_empty());
            }
        }
    }

    #[test]
    fn deadline_is_tier_specific_and_large_helpers_share_the_budget() {
        assert_eq!(
            ThumbnailDeadline::new(ThumbnailSize::Standard).command_timeout(),
            Duration::from_secs(8)
        );
        let large = ThumbnailDeadline::new(ThumbnailSize::Large);
        assert_eq!(LARGE_THUMBNAIL_TIMEOUT, Duration::from_secs(30));
        assert!(large.command_timeout() > Duration::from_secs(29));
        assert!(large.command_timeout() <= Duration::from_secs(30));
        assert_eq!(large.metadata_timeout(), METADATA_COMMAND_TIMEOUT);
        let almost_expired = ThumbnailDeadline {
            expires_at: Some(Instant::now() + Duration::from_millis(80)),
        };
        assert!(almost_expired.metadata_timeout() <= Duration::from_millis(80));
        thread::sleep(Duration::from_millis(90));
        assert!(almost_expired.command_timeout().is_zero());
        assert!(
            !almost_expired.command_succeeded(&mut Command::new("should-never-spawn"), "expired")
        );
    }

    #[test]
    fn clip_deadline_is_45_seconds_and_starts_after_its_own_lane_wait() {
        let coordinator = Arc::new(GenerationCoordinator::default());
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let first = {
            let coordinator = Arc::clone(&coordinator);
            thread::spawn(move || {
                coordinator.run_clip("first-clip".into(), |_| {
                    entered_tx.send(()).unwrap();
                    release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                    Ok(("first-clip".into(), false))
                })
            })
        };
        entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let released_at = Arc::new(Mutex::new(None));
        let second = {
            let coordinator = Arc::clone(&coordinator);
            let released_at = Arc::clone(&released_at);
            thread::spawn(move || {
                coordinator.run_clip("second-clip".into(), |deadline| {
                    assert_eq!(PREVIEW_CLIP_TIMEOUT, Duration::from_secs(45));
                    let release = released_at.lock().unwrap().unwrap();
                    assert!(deadline.expires_at.unwrap() >= release + PREVIEW_CLIP_TIMEOUT);
                    assert!(deadline.command_timeout() > Duration::from_secs(44));
                    Ok(("second-clip".into(), false))
                })
            })
        };
        wait_until(|| coordinator.flights.lock().unwrap().len() == 2);
        // Large/Standard must still be admitted while the clip lane is occupied.
        for tier in [ThumbnailSize::Large, ThumbnailSize::Standard] {
            coordinator
                .run("other-preview".into(), tier, |_| {
                    Ok(("other".into(), false))
                })
                .unwrap();
        }
        thread::sleep(Duration::from_millis(100));
        *released_at.lock().unwrap() = Some(Instant::now());
        release_tx.send(()).unwrap();
        first.join().unwrap().unwrap();
        second.join().unwrap().unwrap();
    }

    #[test]
    fn generation_deadline_starts_after_lane_wait() {
        let coordinator = Arc::new(GenerationCoordinator::default());
        let (entered_tx, entered_rx) = mpsc::channel();
        let mut releases = Vec::new();
        let workers: Vec<_> = (0..LARGE_THUMBNAIL_CONCURRENCY)
            .map(|i| {
                let coordinator = Arc::clone(&coordinator);
                let entered_tx = entered_tx.clone();
                let (release_tx, release_rx) = mpsc::channel();
                releases.push(release_tx);
                thread::spawn(move || {
                    coordinator.run(format!("first-{i}").into(), ThumbnailSize::Large, |_| {
                        entered_tx.send(()).unwrap();
                        release_rx.recv().unwrap();
                        Ok((format!("first-{i}").into(), false))
                    })
                })
            })
            .collect();
        for _ in 0..LARGE_THUMBNAIL_CONCURRENCY {
            entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        }
        let released_at = Arc::new(Mutex::new(None));
        let second = {
            let coordinator = Arc::clone(&coordinator);
            let released_at = Arc::clone(&released_at);
            thread::spawn(move || {
                coordinator.run("second".into(), ThumbnailSize::Large, |deadline| {
                    let release = released_at.lock().unwrap().unwrap();
                    assert!(deadline.expires_at.unwrap() >= release + LARGE_THUMBNAIL_TIMEOUT);
                    Ok(("second".into(), false))
                })
            })
        };
        wait_until(|| coordinator.flights.lock().unwrap().len() == LARGE_THUMBNAIL_CONCURRENCY + 1);
        thread::sleep(Duration::from_millis(100));
        *released_at.lock().unwrap() = Some(Instant::now());
        for release in releases {
            release.send(()).unwrap();
        }
        for worker in workers {
            worker.join().unwrap().unwrap();
        }
        second.join().unwrap().unwrap();
    }

    #[test]
    fn producer_panic_releases_lane_and_flight() {
        let coordinator = GenerationCoordinator::default();
        let panic = std::panic::catch_unwind(|| {
            coordinator.run("panic".into(), ThumbnailSize::Large, |_| panic!("producer"))
        });
        assert!(panic.is_err());
        assert!(coordinator.flights.lock().unwrap().is_empty());
        assert_eq!(*coordinator.active_large.lock().unwrap(), 0);
        assert!(coordinator
            .run("panic".into(), ThumbnailSize::Large, |_| Ok((
                "recovered".into(),
                false
            )))
            .is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn large_deadline_terminates_a_running_helper_and_does_not_restart_the_budget() {
        let deadline = ThumbnailDeadline {
            expires_at: Some(Instant::now() + Duration::from_millis(120)),
        };
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 10"]);
        let started = Instant::now();
        assert!(!deadline.command_succeeded(&mut command, "large deadline test"));
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(deadline.command_timeout().is_zero());
        assert!(!deadline.command_succeeded(&mut Command::new("true"), "second helper"));
    }

    #[cfg(unix)]
    #[test]
    fn clip_helpers_use_one_deadline_and_stop_after_timeout() {
        let deadline = ThumbnailDeadline {
            expires_at: Some(Instant::now() + Duration::from_millis(120)),
        };
        let mut helper = Command::new("sh");
        helper.args(["-c", "sleep 10"]);
        let started = Instant::now();
        assert_eq!(
            deadline.command_output(&mut helper),
            Err(crate::DeadlineCommandError::TimedOut)
        );
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(
            deadline.command_output(&mut Command::new("should-never-spawn")),
            Err(crate::DeadlineCommandError::TimedOut)
        );
    }

    #[test]
    fn clip_still_deadline_bounds_large_lane_wait_and_releases_its_flight() {
        let coordinator = Arc::new(GenerationCoordinator::default());
        let (entered_tx, entered_rx) = mpsc::channel();
        let mut releases = Vec::new();
        let workers: Vec<_> = (0..LARGE_THUMBNAIL_CONCURRENCY)
            .map(|i| {
                let coordinator = Arc::clone(&coordinator);
                let entered_tx = entered_tx.clone();
                let (tx, rx) = mpsc::channel();
                releases.push(tx);
                thread::spawn(move || {
                    coordinator.run(format!("busy-{i}").into(), ThumbnailSize::Large, |_| {
                        entered_tx.send(()).unwrap();
                        rx.recv_timeout(Duration::from_secs(5)).unwrap();
                        Ok(("still.webp".into(), false))
                    })
                })
            })
            .collect();
        for _ in 0..LARGE_THUMBNAIL_CONCURRENCY {
            entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        }
        let started = Instant::now();
        let parent = ThumbnailDeadline {
            expires_at: Some(started + Duration::from_millis(80)),
        };
        let result = coordinator.run_thumbnail_before(
            "clip-still".into(),
            ThumbnailSize::Large,
            parent,
            |_| panic!("expired still wait started generation"),
        );
        let flight_removed = !coordinator
            .flights
            .lock()
            .unwrap()
            .contains_key(&PathBuf::from("clip-still"));
        for release in releases {
            release.send(()).unwrap();
        }
        for worker in workers {
            worker.join().unwrap().unwrap();
        }
        assert_eq!(result, Err(ThumbnailFailure::TimedOut));
        assert!(flight_removed);
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(*coordinator.active_large.lock().unwrap(), 0);
    }

    #[test]
    fn clip_still_deadline_bounds_shared_flight_wait_without_cancelling_the_producer() {
        let coordinator = Arc::new(GenerationCoordinator::default());
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let worker = {
            let coordinator = Arc::clone(&coordinator);
            thread::spawn(move || {
                coordinator.run("still".into(), ThumbnailSize::Large, |_| {
                    entered_tx.send(()).unwrap();
                    release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                    Ok(("still.webp".into(), false))
                })
            })
        };
        entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let parent = ThumbnailDeadline {
            expires_at: Some(Instant::now() + Duration::from_millis(80)),
        };
        let result =
            coordinator.run_thumbnail_before("still".into(), ThumbnailSize::Large, parent, |_| {
                panic!("shared flight generated a second still")
            });
        let producer_remains = coordinator
            .flights
            .lock()
            .unwrap()
            .contains_key(&PathBuf::from("still"));
        release_tx.send(()).unwrap();
        let produced = worker.join().unwrap();
        assert_eq!(result, Err(ThumbnailFailure::TimedOut));
        assert!(producer_remains);
        assert_eq!(produced, Ok(("still.webp".into(), false)));
    }

    #[test]
    fn clip_still_generation_inherits_the_original_deadline_and_large_budget() {
        let coordinator = GenerationCoordinator::default();
        let clip = ThumbnailDeadline::clip();
        let large = ThumbnailDeadline::new(ThumbnailSize::Large).bounded_by(clip);
        assert!(large.expires_at < clip.expires_at);
        let short = ThumbnailDeadline {
            expires_at: Some(Instant::now() + Duration::from_millis(120)),
        };
        thread::sleep(Duration::from_millis(20));
        coordinator
            .run_thumbnail_before("still".into(), ThumbnailSize::Large, short, |deadline| {
                assert_eq!(deadline, short);
                assert!(deadline.command_timeout() < Duration::from_millis(110));
                Ok(("still.webp".into(), false))
            })
            .unwrap();
    }

    #[test]
    fn large_ignores_old_failure_markers_while_standard_still_honors_them() {
        let cache = tempfile::tempdir().unwrap();
        let media = tempfile::tempdir().unwrap();
        let source = media.path().join("image.png");
        image::RgbImage::from_pixel(20, 20, image::Rgb([20, 120, 200]))
            .save(&source)
            .unwrap();
        let path = source.to_str().unwrap();
        let meta = std::fs::metadata(path).unwrap();
        let mtime = meta
            .modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        for tier in [ThumbnailSize::Standard, ThumbnailSize::Large] {
            let key = crate::gui_thumb_cache_key(path, mtime, meta.len(), tier);
            let marker = crate::failure_marker_path(cache.path(), &key);
            crate::write_failure_marker(
                &marker,
                ThumbnailFailure::ProbeFailed,
                crate::current_epoch_secs() + 900,
            )
            .unwrap();
            let result = crate::thumbnail_for_sized(cache.path(), path, 900, tier);
            if tier == ThumbnailSize::Large {
                assert!(result.thumbnail.is_some());
                assert!(!marker.exists());
            } else {
                assert_eq!(result.failure_reason, Some(ThumbnailFailure::ProbeFailed));
                assert!(marker.exists());
            }
        }
    }

    #[test]
    fn large_failures_do_not_persist_markers() {
        let cache = tempfile::tempdir().unwrap();
        let media = tempfile::tempdir().unwrap();
        let source = media.path().join("invalid.unsupported");
        std::fs::write(&source, b"invalid").unwrap();
        let result = crate::thumbnail_for_sized(
            cache.path(),
            source.to_str().unwrap(),
            900,
            ThumbnailSize::Large,
        );
        assert_eq!(result.failure_reason, Some(ThumbnailFailure::Unsupported));
        assert_eq!(crate::thumbnail_cache_info(cache.path()).failure_entries, 0);
    }
}
