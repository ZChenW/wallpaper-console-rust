#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyStage {
    ResolveTarget,
    EnsureAwwwDaemon,
    AwwwSocketReady,
    StartLwe,
    WaitRendererAlive,
    CleanupPrevious,
    RefreshStatus,
}

#[derive(Debug, Clone)]
pub struct ApplyStageEvent {
    pub stage: ApplyStage,
    pub request_id: Option<String>,
}

pub trait ApplyStageReporter: Send {
    fn emit(&mut self, event: ApplyStageEvent);
}

pub struct NoopReporter;

impl ApplyStageReporter for NoopReporter {
    fn emit(&mut self, _event: ApplyStageEvent) {}
}

pub fn report_stage(
    reporter: &mut dyn ApplyStageReporter,
    stage: ApplyStage,
    request_id: Option<&str>,
) {
    reporter.emit(ApplyStageEvent {
        stage,
        request_id: request_id.map(|s| s.to_string()),
    });
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use std::sync::Mutex;

    pub struct CapturingReporter {
        events: Mutex<Vec<ApplyStageEvent>>,
    }

    impl CapturingReporter {
        pub fn new() -> Self {
            Self {
                events: Mutex::new(Vec::new()),
            }
        }

        pub fn stages(&self) -> Vec<ApplyStage> {
            self.events
                .lock()
                .unwrap()
                .iter()
                .map(|e| e.stage.clone())
                .collect()
        }
    }

    impl ApplyStageReporter for CapturingReporter {
        fn emit(&mut self, event: ApplyStageEvent) {
            self.events.lock().unwrap().push(event);
        }
    }
}
