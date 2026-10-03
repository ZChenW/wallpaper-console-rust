//! Explicit rendering changes merge into each selected assignment, never defaults.
use crate::{
    display_operation::{OperationBatch, SwitchReport},
    AppError, AppService, DisplayTarget,
};
use serde::Serialize;
use std::collections::HashMap;
use wc_backend::{
    apply_stage::ApplyStageReporter, apply_transition::TransitionStep, runtime::BackendRuntime,
};
use wc_core::{
    display_assignment::{RenderOptions, RenderRecipe},
    error::WcError,
};
use wc_storage::sqlite::{self, DisplayStateTarget};

#[derive(Debug, Serialize)]
pub struct DisplayRenderingSettings {
    pub output: String,
    pub backend: String,
    pub values: HashMap<String, String>,
}

impl AppService {
    fn selected_recipes(
        &self,
        outputs: &[String],
    ) -> Result<Vec<(DisplayStateTarget, RenderRecipe)>, AppError> {
        outputs
            .iter()
            .map(|output| {
                let target = DisplayStateTarget::Output(output.clone());
                let recipe = match self
                    .storage
                    .display_recipe(&target)
                    .map_err(AppError::from_wc_error)?
                {
                    Some(recipe) => Some(recipe),
                    None => self
                        .storage
                        .display_recipe(&DisplayStateTarget::AllDisplays)
                        .map_err(AppError::from_wc_error)?,
                }
                .ok_or_else(|| {
                    AppError::from_wc_error(WcError::Other(format!(
                        "no saved wallpaper for {output}"
                    )))
                })?;
                Ok((target, recipe))
            })
            .collect()
    }

    pub fn display_rendering_settings(
        &self,
        target: &DisplayTarget,
        known: &[String],
    ) -> Result<Vec<DisplayRenderingSettings>, AppError> {
        let outputs = target
            .outputs(known)
            .map_err(|e| AppError::from_wc_error(WcError::Other(e)))?;
        self.selected_recipes(&outputs).map(|recipes| {
            recipes
                .into_iter()
                .map(|(target, recipe)| DisplayRenderingSettings {
                    output: target.storage_key().into(),
                    backend: recipe.options.backend().as_str().into(),
                    values: recipe.options.config_entries(),
                })
                .collect()
        })
    }

    pub fn update_display_rendering_with_runtime(
        &self,
        target: &DisplayTarget,
        known: &[String],
        patch: &HashMap<String, String>,
        runtime: &mut dyn BackendRuntime,
        reporter: &mut dyn ApplyStageReporter,
    ) -> Result<SwitchReport, AppError> {
        let _guard = crate::output_recovery::RendererMutationGuard::acquire(&self.storage)
            .map_err(AppError::from_wc_error)?;
        let outputs = target
            .outputs(known)
            .map_err(|e| AppError::from_wc_error(WcError::Other(e)))?;
        if patch.is_empty() {
            return Err(AppError::from_wc_error(WcError::Other(
                "no rendering changes selected".into(),
            )));
        }
        let mut recipes = self.selected_recipes(&outputs)?;
        let mut running: Vec<_> =
            wc_backend::runtime_observation::observe_output_ownership(known, runtime)
                .outputs
                .into_iter()
                .filter_map(|(output, state)| {
                    if let wc_backend::runtime_observation::OutputOwnership::Occupied(backend) =
                        state
                    {
                        Some(crate::display_plan::RunningAssignment { output, backend })
                    } else {
                        None
                    }
                })
                .collect();
        let mut steps = Vec::new();
        for (stored_target, recipe) in &mut recipes {
            let mut values = recipe.options.config_entries();
            for (key, value) in patch {
                if !values.contains_key(key) {
                    return Err(AppError::from_wc_error(WcError::Other(format!(
                        "{key} is not a rendering option for {}",
                        recipe.options.backend().as_str()
                    ))));
                }
                wc_core::config::validate_config_entry(key, value)
                    .map_err(AppError::from_wc_error)?;
                values.insert(key.clone(), value.clone());
            }
            recipe.options = RenderOptions::capture(recipe.options.backend(), &values)
                .map_err(AppError::from_wc_error)?;
            recipe.validate().map_err(AppError::from_wc_error)?;
            self.resolve_recipe_target(recipe)?;
            let output = stored_target.storage_key().to_string();
            let target = DisplayTarget::Output(output.clone());
            let backend = recipe.options.backend();
            let plan = crate::display_plan::plan_display_apply(
                &crate::display_plan::DisplayApplyRequest {
                    target: target.clone(),
                    backend,
                    known_outputs: known.to_vec(),
                    running: running.clone(),
                },
            )
            .map_err(crate::display_apply::rejection_to_app_error)?;
            let actions = plan
                .actions
                .into_iter()
                .map(|action| {
                    crate::display_apply::to_exec_action(
                        action,
                        &recipe.media_path,
                        &target,
                        known,
                        true,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            steps.push(TransitionStep {
                scope: wc_backend::ExecutionScope::Named(vec![output.clone()]),
                target: backend,
                recipe: Some(recipe.clone()),
                core_actions: actions,
                fallback_path: None,
            });
            running.retain(|a| a.output != output);
            running.push(crate::display_plan::RunningAssignment { output, backend });
        }
        crate::display_operation::execute(
            &self.storage,
            OperationBatch {
                outputs: &outputs,
                known_outputs: known,
                steps: &steps,
                request_id: None,
            },
            runtime,
            reporter,
            || {
                let conn = sqlite::open_runtime_connection(&self.storage.cd)?;
                sqlite::display_state_commit_distinct_recipes(&conn, &recipes)
            },
        )
        .map_err(AppError::from)
    }
}
