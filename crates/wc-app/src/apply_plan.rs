use serde::{Deserialize, Serialize};
use wc_core::types::{Backend, FileType, WallpaperEntry};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplyAvailability {
    Available,
    Unsupported,
    RetryableFailure,
}

impl ApplyAvailability {
    pub fn as_str(&self) -> &'static str {
        match self {
            ApplyAvailability::Available => "available",
            ApplyAvailability::Unsupported => "unsupported",
            ApplyAvailability::RetryableFailure => "retryable_failure",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplyActionKind {
    Apply,
    RetryBackendApply,
    ApplyPreview,
    OpenFolder,
    CopyWorkshopId,
}

impl ApplyActionKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ApplyActionKind::Apply => "apply",
            ApplyActionKind::RetryBackendApply => "retry_backend_apply",
            ApplyActionKind::ApplyPreview => "apply_preview",
            ApplyActionKind::OpenFolder => "open_folder",
            ApplyActionKind::CopyWorkshopId => "copy_workshop_id",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyAction {
    pub kind: ApplyActionKind,
    pub label: String,
    pub enabled: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityKind {
    NativeScene { disclaimer: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyPlan {
    pub availability: ApplyAvailability,
    pub backend: Option<Backend>,
    pub apply_path: Option<String>,
    pub current_state_path: Option<String>,
    pub reason: Option<String>,
    pub actions: Vec<ApplyAction>,
    pub compatibility: Option<CompatibilityKind>,
}

pub fn plan_for_entry(entry: &WallpaperEntry, backend_failed: bool) -> ApplyPlan {
    plan_for_entry_with_kind(entry, backend_failed, None)
}

pub fn plan_for_entry_with_kind(
    entry: &WallpaperEntry,
    backend_failed: bool,
    error_kind: Option<&str>,
) -> ApplyPlan {
    match entry.file_type {
        FileType::Image | FileType::Gif | FileType::Video => plan_image(entry),
        FileType::WeScene => plan_we_scene(entry, backend_failed, error_kind),
        FileType::WeWeb => plan_we_web(entry),
        FileType::WeApplication => plan_we_application(entry),
    }
}

fn plan_image(entry: &WallpaperEntry) -> ApplyPlan {
    ApplyPlan {
        availability: ApplyAvailability::Available,
        backend: Some(entry.backend),
        apply_path: Some(entry.path.to_string()),
        current_state_path: Some(entry.path.to_string()),
        reason: None,
        actions: vec![
            ApplyAction {
                kind: ApplyActionKind::Apply,
                label: "Apply".into(),
                enabled: true,
                reason: None,
            },
            ApplyAction {
                kind: ApplyActionKind::OpenFolder,
                label: "Open folder".into(),
                enabled: true,
                reason: None,
            },
        ],
        compatibility: None,
    }
}

fn we_scene_compatibility() -> CompatibilityKind {
    CompatibilityKind::NativeScene {
        disclaimer: "Rendered by linux-wallpaperengine — may differ from Wallpaper Engine".into(),
    }
}

fn plan_we_scene(
    entry: &WallpaperEntry,
    backend_failed: bool,
    error_kind: Option<&str>,
) -> ApplyPlan {
    let has_preview = entry
        .project
        .as_ref()
        .and_then(|p| p.preview_path.as_ref())
        .is_some();
    let has_workshop_id = entry
        .project
        .as_ref()
        .and_then(|p| p.workshop_id.as_ref())
        .is_some();

    let is_renderer_limitation = error_kind == Some("renderer_limitation");

    if backend_failed {
        let mut actions = vec![ApplyAction {
            kind: ApplyActionKind::RetryBackendApply,
            label: "Retry backend apply".into(),
            enabled: true,
            reason: None,
        }];
        if has_preview {
            actions.push(ApplyAction {
                kind: ApplyActionKind::ApplyPreview,
                label: "Apply preview GIF".into(),
                enabled: true,
                reason: None,
            });
        }
        actions.push(ApplyAction {
            kind: ApplyActionKind::OpenFolder,
            label: "Open folder".into(),
            enabled: true,
            reason: None,
        });
        if has_workshop_id {
            actions.push(ApplyAction {
                kind: ApplyActionKind::CopyWorkshopId,
                label: "Copy Workshop ID".into(),
                enabled: true,
                reason: None,
            });
        }
        ApplyPlan {
            availability: ApplyAvailability::RetryableFailure,
            backend: Some(Backend::LinuxWallpaperEngine),
            apply_path: Some(entry.path.to_string()),
            current_state_path: Some(entry.path.to_string()),
            reason: if is_renderer_limitation {
                Some("Renderer limitation".into())
            } else {
                None
            },
            actions,
            compatibility: Some(we_scene_compatibility()),
        }
    } else {
        let mut actions = vec![ApplyAction {
            kind: ApplyActionKind::Apply,
            label: "Apply".into(),
            enabled: true,
            reason: None,
        }];
        if has_preview {
            actions.push(ApplyAction {
                kind: ApplyActionKind::ApplyPreview,
                label: "Apply preview GIF".into(),
                enabled: true,
                reason: None,
            });
        }
        actions.push(ApplyAction {
            kind: ApplyActionKind::OpenFolder,
            label: "Open folder".into(),
            enabled: true,
            reason: None,
        });
        if has_workshop_id {
            actions.push(ApplyAction {
                kind: ApplyActionKind::CopyWorkshopId,
                label: "Copy Workshop ID".into(),
                enabled: true,
                reason: None,
            });
        }
        ApplyPlan {
            availability: ApplyAvailability::Available,
            backend: Some(Backend::LinuxWallpaperEngine),
            apply_path: Some(entry.path.to_string()),
            current_state_path: Some(entry.path.to_string()),
            reason: None,
            actions,
            compatibility: Some(we_scene_compatibility()),
        }
    }
}

fn plan_we_web(entry: &WallpaperEntry) -> ApplyPlan {
    let has_workshop_id = entry
        .project
        .as_ref()
        .and_then(|p| p.workshop_id.as_ref())
        .is_some();
    let has_preview = entry
        .project
        .as_ref()
        .and_then(|p| p.preview_path.as_ref())
        .is_some();

    let mut actions = vec![ApplyAction {
        kind: ApplyActionKind::OpenFolder,
        label: "Open folder".into(),
        enabled: true,
        reason: None,
    }];
    if has_preview {
        actions.push(ApplyAction {
            kind: ApplyActionKind::ApplyPreview,
            label: "Apply preview only".into(),
            enabled: true,
            reason: Some(
                "Only the preview GIF can be applied as a static wallpaper; the Web scene itself is not supported.".into(),
            ),
        });
    }
    if has_workshop_id {
        actions.push(ApplyAction {
            kind: ApplyActionKind::CopyWorkshopId,
            label: "Copy Workshop ID".into(),
            enabled: true,
            reason: None,
        });
    }
    ApplyPlan {
        availability: ApplyAvailability::Unsupported,
        backend: None,
        apply_path: None,
        current_state_path: None,
        reason: Some("Wallpaper Engine Web projects are indexed for browsing only.".into()),
        actions,
        compatibility: None,
    }
}

fn plan_we_application(entry: &WallpaperEntry) -> ApplyPlan {
    let has_workshop_id = entry
        .project
        .as_ref()
        .and_then(|p| p.workshop_id.as_ref())
        .is_some();

    let mut actions = vec![ApplyAction {
        kind: ApplyActionKind::OpenFolder,
        label: "Open folder".into(),
        enabled: true,
        reason: None,
    }];
    if has_workshop_id {
        actions.push(ApplyAction {
            kind: ApplyActionKind::CopyWorkshopId,
            label: "Copy Workshop ID".into(),
            enabled: true,
            reason: None,
        });
    }
    ApplyPlan {
        availability: ApplyAvailability::Unsupported,
        backend: None,
        apply_path: None,
        current_state_path: None,
        reason: None,
        actions,
        compatibility: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;
    use wc_core::types::WallpaperProject;

    fn image_entry() -> WallpaperEntry {
        WallpaperEntry {
            path: Utf8PathBuf::from("/pics/photo.jpg"),
            file_type: FileType::Image,
            ext: "jpg".into(),
            backend: Backend::Awww,
            size: 1024,
            mtime: 1700000000,
            resolution: "1920x1080".into(),
            project: None,
        }
    }

    fn video_entry() -> WallpaperEntry {
        WallpaperEntry {
            path: Utf8PathBuf::from("/vids/clip.mp4"),
            file_type: FileType::Video,
            ext: "mp4".into(),
            backend: Backend::Mpvpaper,
            size: 1024,
            mtime: 1700000000,
            resolution: "1920x1080".into(),
            project: None,
        }
    }

    fn gif_entry() -> WallpaperEntry {
        WallpaperEntry {
            path: Utf8PathBuf::from("/pics/anim.gif"),
            file_type: FileType::Gif,
            ext: "gif".into(),
            backend: Backend::Awww,
            size: 1024,
            mtime: 1700000000,
            resolution: "1920x1080".into(),
            project: None,
        }
    }

    fn we_scene_entry() -> WallpaperEntry {
        WallpaperEntry {
            path: Utf8PathBuf::from("/steam/workshop/431960/12345"),
            file_type: FileType::WeScene,
            ext: "scene".into(),
            backend: Backend::LinuxWallpaperEngine,
            size: 4096,
            mtime: 1700000000,
            resolution: "WE".into(),
            project: Some(WallpaperProject {
                project_type: "we_scene".into(),
                preview_path: Some("/steam/workshop/431960/12345/preview.gif".into()),
                workshop_id: Some("12345".into()),
                title: Some("Test Scene".into()),
                we_file: Some("scene.json".into()),
                ..Default::default()
            }),
        }
    }

    fn we_scene_no_extra() -> WallpaperEntry {
        WallpaperEntry {
            path: Utf8PathBuf::from("/steam/workshop/431960/67890"),
            file_type: FileType::WeScene,
            ext: "scene".into(),
            backend: Backend::LinuxWallpaperEngine,
            size: 4096,
            mtime: 1700000000,
            resolution: "WE".into(),
            project: Some(WallpaperProject {
                project_type: "we_scene".into(),
                ..Default::default()
            }),
        }
    }

    fn we_web_entry() -> WallpaperEntry {
        WallpaperEntry {
            path: Utf8PathBuf::from("/steam/workshop/431960/3650880224"),
            file_type: FileType::WeWeb,
            ext: "web".into(),
            backend: Backend::Unsupported,
            size: 4096,
            mtime: 1700000000,
            resolution: "WE".into(),
            project: Some(WallpaperProject {
                project_type: "we_web".into(),
                preview_path: Some("/steam/workshop/431960/3650880224/preview.gif".into()),
                workshop_id: Some("3650880224".into()),
                title: Some("Web Test".into()),
                we_file: Some("index.html".into()),
                ..Default::default()
            }),
        }
    }

    fn we_application_entry() -> WallpaperEntry {
        WallpaperEntry {
            path: Utf8PathBuf::from("/steam/workshop/431960/44444"),
            file_type: FileType::WeApplication,
            ext: "application".into(),
            backend: Backend::Unsupported,
            size: 4096,
            mtime: 1700000000,
            resolution: "WE".into(),
            project: Some(WallpaperProject {
                project_type: "unsupported".into(),
                workshop_id: Some("44444".into()),
                title: Some("App Project".into()),
                we_file: Some("app.exe".into()),
                ..Default::default()
            }),
        }
    }

    fn action_kind(plan: &ApplyPlan, kind: ApplyActionKind) -> bool {
        plan.actions.iter().any(|a| a.kind == kind && a.enabled)
    }

    #[test]
    fn apply_plan_image_is_available() {
        let plan = plan_for_entry(&image_entry(), false);
        assert_eq!(plan.availability, ApplyAvailability::Available);
        assert_eq!(plan.backend, Some(Backend::Awww));
        assert!(action_kind(&plan, ApplyActionKind::Apply));
        assert!(action_kind(&plan, ApplyActionKind::OpenFolder));
        assert_eq!(plan.actions.len(), 2);
    }

    #[test]
    fn apply_plan_video_is_available() {
        let plan = plan_for_entry(&video_entry(), false);
        assert_eq!(plan.availability, ApplyAvailability::Available);
        assert_eq!(plan.backend, Some(Backend::Mpvpaper));
        assert!(action_kind(&plan, ApplyActionKind::Apply));
        assert!(action_kind(&plan, ApplyActionKind::OpenFolder));
        assert_eq!(plan.actions.len(), 2);
    }

    #[test]
    fn apply_plan_gif_is_available() {
        let plan = plan_for_entry(&gif_entry(), false);
        assert_eq!(plan.availability, ApplyAvailability::Available);
        assert!(action_kind(&plan, ApplyActionKind::Apply));
        assert!(action_kind(&plan, ApplyActionKind::OpenFolder));
        assert_eq!(plan.actions.len(), 2);
    }

    #[test]
    fn apply_plan_we_scene_available() {
        let plan = plan_for_entry(&we_scene_entry(), false);
        assert_eq!(plan.availability, ApplyAvailability::Available);
        assert_eq!(plan.backend, Some(Backend::LinuxWallpaperEngine));
        assert!(action_kind(&plan, ApplyActionKind::Apply));
        assert!(action_kind(&plan, ApplyActionKind::ApplyPreview));
        assert!(action_kind(&plan, ApplyActionKind::OpenFolder));
        assert!(action_kind(&plan, ApplyActionKind::CopyWorkshopId));
        assert_eq!(plan.actions.len(), 4);
        // Verify label does not contain linux-wallpaperengine
        let apply_action = plan
            .actions
            .iter()
            .find(|a| a.kind == ApplyActionKind::Apply)
            .unwrap();
        assert_eq!(apply_action.label, "Apply");
    }

    #[test]
    fn apply_plan_we_scene_available_no_extra() {
        let plan = plan_for_entry(&we_scene_no_extra(), false);
        assert_eq!(plan.availability, ApplyAvailability::Available);
        assert!(action_kind(&plan, ApplyActionKind::Apply));
        assert!(!action_kind(&plan, ApplyActionKind::ApplyPreview));
        assert!(!action_kind(&plan, ApplyActionKind::CopyWorkshopId));
        assert_eq!(plan.actions.len(), 2);
    }

    #[test]
    fn apply_plan_failed_we_scene_retryable() {
        let plan = plan_for_entry(&we_scene_entry(), true);
        assert_eq!(plan.availability, ApplyAvailability::RetryableFailure);
        assert!(action_kind(&plan, ApplyActionKind::RetryBackendApply));
        assert!(!action_kind(&plan, ApplyActionKind::Apply));
        assert!(action_kind(&plan, ApplyActionKind::ApplyPreview));
        assert!(action_kind(&plan, ApplyActionKind::OpenFolder));
        assert!(action_kind(&plan, ApplyActionKind::CopyWorkshopId));
        assert_eq!(plan.actions.len(), 4);
    }

    #[test]
    fn apply_plan_failed_we_scene_no_preview() {
        let plan = plan_for_entry(&we_scene_no_extra(), true);
        assert_eq!(plan.availability, ApplyAvailability::RetryableFailure);
        assert!(action_kind(&plan, ApplyActionKind::RetryBackendApply));
        assert!(!action_kind(&plan, ApplyActionKind::ApplyPreview));
        assert!(!action_kind(&plan, ApplyActionKind::CopyWorkshopId));
        assert_eq!(plan.actions.len(), 2);
    }

    #[test]
    fn apply_plan_we_web_unsupported() {
        let plan = plan_for_entry(&we_web_entry(), false);
        assert_eq!(plan.availability, ApplyAvailability::Unsupported);
        assert_eq!(plan.backend, None);
        assert!(!action_kind(&plan, ApplyActionKind::Apply));
        assert!(action_kind(&plan, ApplyActionKind::ApplyPreview));
        assert!(action_kind(&plan, ApplyActionKind::OpenFolder));
        assert!(action_kind(&plan, ApplyActionKind::CopyWorkshopId));
        assert!(plan.reason.as_deref().unwrap().contains("browsing only"));
        assert_eq!(plan.actions.len(), 3);
    }

    #[test]
    fn apply_plan_we_application_unsupported() {
        let plan = plan_for_entry(&we_application_entry(), false);
        assert_eq!(plan.availability, ApplyAvailability::Unsupported);
        assert_eq!(plan.backend, None);
        assert!(!action_kind(&plan, ApplyActionKind::Apply));
        assert!(action_kind(&plan, ApplyActionKind::OpenFolder));
        assert!(action_kind(&plan, ApplyActionKind::CopyWorkshopId));
        assert_eq!(plan.actions.len(), 2);
    }

    #[test]
    fn apply_plan_we_web_no_workshop_id() {
        let entry = WallpaperEntry {
            project: Some(WallpaperProject {
                project_type: "we_web".into(),
                ..Default::default()
            }),
            ..we_web_entry()
        };
        let plan = plan_for_entry(&entry, false);
        assert_eq!(plan.availability, ApplyAvailability::Unsupported);
        assert!(!action_kind(&plan, ApplyActionKind::CopyWorkshopId));
        assert_eq!(plan.actions.len(), 1);
    }

    #[test]
    fn apply_plan_we_application_no_workshop_id() {
        let entry = WallpaperEntry {
            project: Some(WallpaperProject {
                project_type: "unsupported".into(),
                ..Default::default()
            }),
            ..we_application_entry()
        };
        let plan = plan_for_entry(&entry, false);
        assert_eq!(plan.availability, ApplyAvailability::Unsupported);
        assert!(!action_kind(&plan, ApplyActionKind::CopyWorkshopId));
        assert_eq!(plan.actions.len(), 1);
    }
}
