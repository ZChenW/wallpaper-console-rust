use std::collections::BTreeMap;
use std::time::Instant;

use serde::{Deserialize, Serialize};

/// Non-overlapping wall-clock spans, in microseconds, for one display operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchTimings {
    pub total_micros: u64,
    pub stages: BTreeMap<String, u64>,
    /// Non-overlapping subspans keyed by `stage.call`, in microseconds.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub details: BTreeMap<String, u64>,
}

pub(crate) struct OperationTimer {
    id: String,
    started: Instant,
    checkpoint: Instant,
    stage: &'static str,
    stages: BTreeMap<String, u64>,
    details: BTreeMap<String, u64>,
}

impl OperationTimer {
    pub(crate) fn new(id: &str) -> Self {
        let now = Instant::now();
        Self {
            id: id.into(),
            started: now,
            checkpoint: now,
            stage: "preflight",
            stages: BTreeMap::new(),
            details: BTreeMap::new(),
        }
    }

    pub(crate) fn enter(&mut self, stage: &'static str) {
        let now = Instant::now();
        *self.stages.entry(self.stage.into()).or_default() +=
            micros(now.duration_since(self.checkpoint));
        self.checkpoint = now;
        self.stage = stage;
    }

    pub(crate) fn measure<T>(&mut self, call: &str, action: impl FnOnce() -> T) -> T {
        let started = Instant::now();
        let result = action();
        self.record_detail(call, started.elapsed());
        result
    }

    pub(crate) fn record_detail(&mut self, call: &str, elapsed: std::time::Duration) {
        *self
            .details
            .entry(format!("{}.{call}", self.stage))
            .or_default() += micros(elapsed);
    }

    pub(crate) fn snapshot(&self) -> SwitchTimings {
        let now = Instant::now();
        let mut stages = self.stages.clone();
        *stages.entry(self.stage.into()).or_default() +=
            micros(now.duration_since(self.checkpoint));
        SwitchTimings {
            total_micros: micros(now.duration_since(self.started)),
            stages,
            details: self.details.clone(),
        }
    }
}

impl Drop for OperationTimer {
    fn drop(&mut self) {
        log_timings(&self.id, "display_operation", &self.snapshot());
    }
}

pub(crate) fn micros(duration: std::time::Duration) -> u64 {
    duration.as_micros().min(u64::MAX as u128) as u64
}

/// Callers measure sibling calls only; nested spans would count elapsed time twice.
pub(crate) fn measure_detail<T>(
    details: &mut BTreeMap<String, u64>,
    key: &str,
    action: impl FnOnce() -> T,
) -> T {
    let started = Instant::now();
    let result = action();
    *details.entry(key.into()).or_default() += micros(started.elapsed());
    result
}

pub(crate) fn log_timings(id: &str, event: &str, timings: &SwitchTimings) {
    log::debug!(target: "wc::performance", "{}", serde_json::json!({
        "event": event, "operationId": id, "timings": timings,
    }));
}
