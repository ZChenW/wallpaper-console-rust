use std::collections::BTreeMap;
use std::time::Instant;

use serde::{Deserialize, Serialize};

/// Non-overlapping wall-clock spans, in microseconds, for one display operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchTimings {
    pub total_micros: u64,
    pub stages: BTreeMap<String, u64>,
}

pub(crate) struct OperationTimer {
    id: String,
    started: Instant,
    checkpoint: Instant,
    stage: &'static str,
    stages: BTreeMap<String, u64>,
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
        }
    }

    pub(crate) fn enter(&mut self, stage: &'static str) {
        let now = Instant::now();
        *self.stages.entry(self.stage.into()).or_default() +=
            micros(now.duration_since(self.checkpoint));
        self.checkpoint = now;
        self.stage = stage;
    }

    pub(crate) fn snapshot(&self) -> SwitchTimings {
        let now = Instant::now();
        let mut stages = self.stages.clone();
        *stages.entry(self.stage.into()).or_default() +=
            micros(now.duration_since(self.checkpoint));
        SwitchTimings {
            total_micros: micros(now.duration_since(self.started)),
            stages,
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

pub(crate) fn log_timings(id: &str, event: &str, timings: &SwitchTimings) {
    log::debug!(target: "wc::performance", "{}", serde_json::json!({
        "event": event, "operationId": id, "timings": timings,
    }));
}
