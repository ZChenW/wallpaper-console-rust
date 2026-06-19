use wc_backend::apply_stage::ApplyStage;
use wc_core::types::Backend;

#[derive(Debug, Clone)]
pub struct ApplyStageContext {
    pub preview: bool,
    pub backend: Backend,
}

impl Default for ApplyStageContext {
    fn default() -> Self {
        Self {
            preview: false,
            backend: Backend::Awww,
        }
    }
}

pub fn apply_stage_label(stage: &ApplyStage) -> &'static str {
    match stage {
        ApplyStage::ResolveTarget => "Resolving target",
        ApplyStage::EnsureAwwwDaemon => "Starting awww daemon",
        ApplyStage::AwwwSocketReady => "Waiting for awww socket",
        ApplyStage::StartLwe => "Starting linux-wallpaperengine",
        ApplyStage::WaitRendererAlive => "Waiting for renderer",
        ApplyStage::CleanupPrevious => "Cleaning up previous backend",
        ApplyStage::RefreshStatus => "Refreshing status",
    }
}

pub fn apply_stage_detail(stage: &ApplyStage, ctx: &ApplyStageContext) -> String {
    match stage {
        ApplyStage::ResolveTarget => "Resolving apply target.".into(),
        ApplyStage::EnsureAwwwDaemon => "Starting awww daemon.".into(),
        ApplyStage::AwwwSocketReady => "Waiting for awww socket to be ready.".into(),
        ApplyStage::StartLwe => {
            if ctx.preview {
                "Starting Awww for preview GIF.".into()
            } else {
                "Starting linux-wallpaperengine.".into()
            }
        }
        ApplyStage::WaitRendererAlive => {
            if ctx.preview {
                "Waiting for Awww to display the preview.".into()
            } else {
                "Waiting for linux-wallpaperengine to start.".into()
            }
        }
        ApplyStage::CleanupPrevious => "Cleaning up previous wallpaper backend.".into(),
        ApplyStage::RefreshStatus => "Refreshing wallpaper status.".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_detail_differs_from_scene_for_lwe_stages() {
        let preview = ApplyStageContext {
            preview: true,
            backend: Backend::Awww,
        };
        let scene = ApplyStageContext {
            preview: false,
            backend: Backend::LinuxWallpaperEngine,
        };
        assert!(apply_stage_detail(&ApplyStage::StartLwe, &preview).contains("preview"));
        assert!(apply_stage_detail(&ApplyStage::StartLwe, &scene).contains("linux-wallpaperengine"));
        assert!(apply_stage_detail(&ApplyStage::WaitRendererAlive, &preview).contains("Awww"));
        assert!(apply_stage_detail(&ApplyStage::WaitRendererAlive, &scene)
            .contains("linux-wallpaperengine"));
    }
}
