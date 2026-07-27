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

fn backend_renderer_name(backend: Backend) -> &'static str {
    match backend {
        Backend::LinuxWallpaperEngine => "linux-wallpaperengine",
        Backend::Awww => "Awww",
        Backend::Mpvpaper => "mpvpaper",
        Backend::Swaybg => "swaybg",
        Backend::Feh => "feh",
        Backend::Unsupported => "renderer",
    }
}

pub fn apply_stage_detail(stage: &ApplyStage, ctx: &ApplyStageContext) -> String {
    match stage {
        ApplyStage::ResolveTarget => "Resolving apply target.".into(),
        ApplyStage::EnsureAwwwDaemon => "Starting awww daemon.".into(),
        ApplyStage::AwwwSocketReady => "Waiting for awww socket to be ready.".into(),
        ApplyStage::StartLwe => {
            let renderer = backend_renderer_name(ctx.backend);
            if ctx.preview {
                format!("Starting {renderer} for preview.")
            } else {
                format!("Starting {renderer}.")
            }
        }
        ApplyStage::WaitRendererAlive => {
            let renderer = backend_renderer_name(ctx.backend);
            if ctx.preview {
                format!("Waiting for {renderer} to display the preview.")
            } else {
                format!("Waiting for {renderer} to start.")
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
    fn lwe_stage_detail_uses_backend_renderer_name() {
        let awww_preview = ApplyStageContext {
            preview: true,
            backend: Backend::Awww,
        };
        let lwe_scene = ApplyStageContext {
            preview: false,
            backend: Backend::LinuxWallpaperEngine,
        };
        assert_eq!(
            apply_stage_detail(&ApplyStage::StartLwe, &awww_preview),
            "Starting Awww for preview."
        );
        assert_eq!(
            apply_stage_detail(&ApplyStage::StartLwe, &lwe_scene),
            "Starting linux-wallpaperengine."
        );
        assert_eq!(
            apply_stage_detail(&ApplyStage::WaitRendererAlive, &awww_preview),
            "Waiting for Awww to display the preview."
        );
        assert_eq!(
            apply_stage_detail(&ApplyStage::WaitRendererAlive, &lwe_scene),
            "Waiting for linux-wallpaperengine to start."
        );
    }
}
