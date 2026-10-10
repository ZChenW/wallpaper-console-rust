//! Separate Large capacity and share one physical generation per cache key.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::{
    ThumbnailFailure, ThumbnailSize, LARGE_THUMBNAIL_CONCURRENCY, LARGE_THUMBNAIL_TIMEOUT,
    METADATA_COMMAND_TIMEOUT, THUMBNAIL_COMMAND_TIMEOUT,
};

type GenerationResult = Result<(PathBuf, bool), ThumbnailFailure>;

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
}

impl GenerationCoordinator {
    pub(super) fn run(
        &self,
        key: PathBuf,
        tier: ThumbnailSize,
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
                result = flight
                    .finished
                    .wait(result)
                    .unwrap_or_else(|e| e.into_inner());
            }
            return result.as_ref().unwrap().clone();
        }

        // Publish a failure and release waiters even if a producer unwinds.
        let completion = FlightCompletion {
            coordinator: self,
            key,
            flight,
        };
        let _lane = if tier == ThumbnailSize::Large {
            let mut active = self.active_large.lock().unwrap_or_else(|e| e.into_inner());
            while *active >= LARGE_THUMBNAIL_CONCURRENCY {
                active = self
                    .lane_available
                    .wait(active)
                    .unwrap_or_else(|e| e.into_inner());
            }
            *active += 1;
            Some(LargePermit(self))
        } else {
            None
        };
        let result = generate(ThumbnailDeadline::new(tier));
        *completion
            .flight
            .result
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(result.clone());
        result
    }
}

struct LargePermit<'a>(&'a GenerationCoordinator);

impl Drop for LargePermit<'_> {
    fn drop(&mut self) {
        let mut active = self
            .0
            .active_large
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *active -= 1;
        self.0.lane_available.notify_one();
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

/// Large helpers share a generation budget, created only after lane admission.
/// Standard retains its original per-command timeouts. Native image decoding
/// is allocation-bounded but cannot be interrupted by an external-process deadline.
#[derive(Clone, Copy)]
pub(super) struct ThumbnailDeadline {
    expires_at: Option<Instant>,
}

impl ThumbnailDeadline {
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
}

#[cfg(test)]
#[path = "generation_tests.rs"]
mod tests;
