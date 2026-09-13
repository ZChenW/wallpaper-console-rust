//! Reload only confirmed mpvpaper wallpapers, retaining assignments and themes.

use serde::Serialize;
use wc_backend::apply_stage::NoopReporter;
use wc_backend::runtime::BackendRuntime;
use wc_backend::runtime_observation::{
    observe_runtime_wallpapers_with, ProcessCommandLine, RuntimeObservationIo,
    RuntimeObservationStatus,
};
use wc_core::types::Backend;
use wc_storage::sqlite::DisplayStateTarget;

use crate::apply_execution::ApplyExecutionTarget;
use crate::display_apply::DisplayApplyRuntimeOpts;
use crate::{AppError, AppService, ApplyRequest, ApplyRequestKind, DisplayTarget};

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MpvpaperReapplyResult {
    pub applied_outputs: Vec<String>,
    pub failures: Vec<MpvpaperReapplyFailure>,
}

#[derive(Debug, Serialize)]
pub struct MpvpaperReapplyFailure {
    pub output: String,
    pub message: String,
}

struct ObservationSnapshot {
    live_media: bool,
    awww: Result<String, String>,
    processes: Result<Vec<ProcessCommandLine>, String>,
}

impl RuntimeObservationIo for ObservationSnapshot {
    fn mpvpaper_media_loaded(&self, process: &ProcessCommandLine, path: &str) -> bool {
        !self.live_media
            || wc_backend::runtime_observation::SystemRuntimeObservationIo
                .mpvpaper_media_loaded(process, path)
    }
    fn awww_query_json(&self) -> Result<String, String> {
        self.awww.clone()
    }

    fn current_user_process_command_lines(&self) -> Result<Vec<ProcessCommandLine>, String> {
        self.processes.clone()
    }
}

impl AppService {
    /// Caller must serialize this operation with other renderer mutations.
    pub fn reapply_mpvpaper_with_runtime(
        &self,
        known_outputs: &[String],
        runtime: &mut dyn BackendRuntime,
    ) -> Result<MpvpaperReapplyResult, AppError> {
        let _guard = crate::output_recovery::RendererMutationGuard::acquire(&self.storage)
            .map_err(AppError::from_wc_error)?;
        let rows = self
            .storage
            .display_state_list()
            .map_err(AppError::from_wc_error)?;
        let snapshot = ObservationSnapshot {
            live_media: runtime.supports_output_recovery(),
            awww: runtime.awww_query_json().map_err(|e| e.to_string()),
            processes: runtime.renderer_command_lines().map_err(|e| e.to_string()),
        };
        let observations = observe_runtime_wallpapers_with(known_outputs, &rows, &snapshot);
        let mut result = MpvpaperReapplyResult::default();
        for observation in observations {
            let assigned = rows
                .iter()
                .find(|row| row.target == DisplayStateTarget::Output(observation.output.clone()))
                .or_else(|| {
                    rows.iter()
                        .find(|row| row.target == DisplayStateTarget::AllDisplays)
                });
            let Some(assigned) = assigned.filter(|row| row.backend == "mpvpaper") else {
                continue;
            };
            if observation.status != RuntimeObservationStatus::Confirmed {
                // A saved assignment alone must never restart a stopped wallpaper.
                continue;
            }
            let path = &assigned.wallpaper_path;
            let outcome = wc_core::formats::get_extension(path)
                .and_then(|ext| wc_core::formats::classify_extension(&ext))
                .ok_or_else(|| AppError::unsupported_path(path))
                .and_then(|(file_type, _)| {
                    self.execute_resolved_display_apply(
                        ApplyRequest {
                            kind: ApplyRequestKind::Apply,
                            path: path.clone(),
                            request_id: None,
                        },
                        DisplayTarget::Output(observation.output.clone()),
                        known_outputs,
                        runtime,
                        &mut NoopReporter,
                        DisplayApplyRuntimeOpts::default(),
                        None,
                        ApplyExecutionTarget {
                            input_path: path.clone(),
                            resolved_path: path.clone(),
                            state_path: path.clone(),
                            file_type,
                            backend: Backend::Mpvpaper,
                            preview: false,
                            fallback_path: None,
                        },
                        false,
                    )
                });
            match outcome {
                Ok(_) => result.applied_outputs.push(observation.output),
                Err(error) => result.failures.push(MpvpaperReapplyFailure {
                    output: observation.output,
                    message: error.message,
                }),
            }
        }
        Ok(result)
    }
}
