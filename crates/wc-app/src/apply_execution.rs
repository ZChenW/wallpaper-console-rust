use serde::{Deserialize, Serialize};
use wc_core::types::{Backend, FileType};

use crate::apply_stage_labels::ApplyStageContext;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplyRequestKind {
    Apply,
    RetryBackendApply,
    ApplyPreview,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyRequest {
    pub kind: ApplyRequestKind,
    pub path: String,
    pub request_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyExecutionTarget {
    pub input_path: String,
    pub resolved_path: String,
    pub state_path: String,
    pub file_type: FileType,
    pub backend: Backend,
    pub preview: bool,
    pub fallback_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyExecutionResult {
    pub request_id: Option<String>,
    pub applied_path: String,
    pub state_path: String,
    pub backend: Backend,
    pub file_type: FileType,
    pub preview: bool,
}

#[derive(Default)]
pub struct ApplyExecutionOptions {
    pub stage_reporter: Option<Box<dyn wc_backend::apply_stage::ApplyStageReporter + Send>>,
    pub on_target_resolved: Option<Box<dyn FnMut(ApplyStageContext) + Send>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use wc_core::config::ConfigDir;

    fn temp_service() -> (tempfile::TempDir, crate::AppService) {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("config"),
        };
        cd.init().unwrap();
        wc_core::config::write_config_value(&cd.path, "storage_backend", "sqlite").unwrap();
        (tmp, crate::AppService::from_config_dir(cd))
    }

    fn insert_history(service: &crate::AppService, path: &str, backend: &str) {
        let cd = &service.storage_for_tests().cd;
        let conn = wc_storage::sqlite::open_runtime_connection(cd).unwrap();
        conn.execute(
            "INSERT INTO history (path, backend) VALUES (?1, ?2)",
            [path, backend],
        )
        .unwrap();
    }

    fn history_rows(service: &crate::AppService) -> Vec<(String, String)> {
        let cd = &service.storage_for_tests().cd;
        let conn = wc_storage::sqlite::open_runtime_connection(cd).unwrap();
        let mut stmt = conn
            .prepare("SELECT path, backend FROM history ORDER BY id")
            .unwrap();
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    }

    fn web_project(root: &Path) -> std::path::PathBuf {
        let project = root.join("steamapps/workshop/content/431960/3650880224");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("index.html"), "<html></html>").unwrap();
        std::fs::write(
            project.join("project.json"),
            r#"{"type":"web","file":"index.html"}"#,
        )
        .unwrap();
        project
    }

    #[test]
    fn execute_apply_request_rejects_we_web_without_stopping() {
        let (tmp, service) = temp_service();
        let project = web_project(tmp.path());
        let request = ApplyRequest {
            kind: ApplyRequestKind::Apply,
            path: project.to_string_lossy().to_string(),
            request_id: Some("test-1".into()),
        };

        let err = service.execute_apply_request(request).unwrap_err();
        assert_eq!(err.code, "we_web_unsupported");
        assert!(err.recoverable);
    }

    fn scene_project_with_preview(root: &Path) -> std::path::PathBuf {
        let project = root.join("steamapps/workshop/content/431960/3558034522");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("preview.gif"), b"gif").unwrap();
        std::fs::write(project.join("scene.json"), "{}").unwrap();
        std::fs::write(
            project.join("project.json"),
            r#"{"type":"scene","file":"scene.json","preview":"preview.gif","title":"Scene"}"#,
        )
        .unwrap();
        project
    }

    #[test]
    fn apply_preview_uses_preview_file_not_project_dir() {
        let (tmp, service) = temp_service();
        let project = scene_project_with_preview(tmp.path());
        let request = ApplyRequest {
            kind: ApplyRequestKind::ApplyPreview,
            path: project.to_string_lossy().to_string(),
            request_id: Some("preview-1".into()),
        };

        let target = service.resolve_apply_request_target(&request).unwrap();
        assert!(target.resolved_path.ends_with("preview.gif"));
        assert_eq!(target.file_type, wc_core::types::FileType::Gif);
        assert_eq!(target.backend, wc_core::types::Backend::Awww);
        assert!(target.preview);
    }

    #[test]
    fn apply_preview_without_preview_is_structured_error() {
        let (tmp, service) = temp_service();
        let project = tmp.path().join("steamapps/workshop/content/431960/1");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("scene.json"), "{}").unwrap();
        std::fs::write(
            project.join("project.json"),
            r#"{"type":"scene","file":"scene.json"}"#,
        )
        .unwrap();

        let request = ApplyRequest {
            kind: ApplyRequestKind::ApplyPreview,
            path: project.to_string_lossy().to_string(),
            request_id: None,
        };

        let err = service.resolve_apply_request_target(&request).unwrap_err();
        assert_eq!(err.code, "preview_missing");
    }

    #[test]
    fn unsupported_request_does_not_add_history() {
        let (tmp, service) = temp_service();
        let project = web_project(tmp.path());
        insert_history(&service, "/walls/legacy.jpg", "awww");
        let before = history_rows(&service);
        let request = ApplyRequest {
            kind: ApplyRequestKind::Apply,
            path: project.to_string_lossy().to_string(),
            request_id: None,
        };
        assert!(service.execute_apply_request(request).is_err());
        let after = history_rows(&service);
        assert_eq!(before, after);
    }

    #[test]
    fn apply_preview_target_state_path_is_preview_file() {
        let (tmp, service) = temp_service();
        let project = scene_project_with_preview(tmp.path());
        let request = ApplyRequest {
            kind: ApplyRequestKind::ApplyPreview,
            path: project.to_string_lossy().to_string(),
            request_id: Some("preview-state".into()),
        };

        let target = service.resolve_apply_request_target(&request).unwrap();
        assert!(target.preview);
        assert!(target.resolved_path.ends_with("preview.gif"));
        assert_eq!(target.state_path, target.resolved_path);
        assert_eq!(target.backend, Backend::Awww);
    }

    #[test]
    fn apply_scene_target_state_path_is_project_dir() {
        let (tmp, service) = temp_service();
        let project = scene_project_with_preview(tmp.path());
        let request = ApplyRequest {
            kind: ApplyRequestKind::Apply,
            path: project.to_string_lossy().to_string(),
            request_id: Some("scene-state".into()),
        };

        let target = service.resolve_apply_request_target(&request).unwrap();
        assert!(!target.preview);
        assert_eq!(target.resolved_path, project.to_string_lossy());
        assert_eq!(target.state_path, project.to_string_lossy());
        assert_eq!(target.backend, Backend::LinuxWallpaperEngine);
    }

    #[test]
    fn scene_apply_target_has_no_preview_fallback_for_clean_handoff() {
        let (tmp, service) = temp_service();
        let project = scene_project_with_preview(tmp.path());
        let request = ApplyRequest {
            kind: ApplyRequestKind::Apply,
            path: project.to_string_lossy().to_string(),
            request_id: Some("fb-scene".into()),
        };
        let target = service.resolve_apply_request_target(&request).unwrap();
        assert!(target.fallback_path.is_none());
        assert_eq!(target.backend, Backend::LinuxWallpaperEngine);
    }

    #[test]
    fn image_apply_target_exposes_self_fallback_path() {
        let (tmp, service) = temp_service();
        let img = tmp.path().join("test.png");
        std::fs::write(&img, b"png").unwrap();
        let request = ApplyRequest {
            kind: ApplyRequestKind::Apply,
            path: img.to_string_lossy().to_string(),
            request_id: Some("fb-img".into()),
        };
        let target = service.resolve_apply_request_target(&request).unwrap();
        assert!(target.fallback_path.is_some());
        let fb = target.fallback_path.unwrap();
        assert!(
            fb.ends_with("test.png"),
            "fallback should be the image itself, got: {}",
            fb
        );
    }

    #[test]
    fn video_apply_target_has_no_fallback_path_initially() {
        let (tmp, service) = temp_service();
        let video = tmp.path().join("test.mp4");
        std::fs::write(&video, b"mp4").unwrap();
        let request = ApplyRequest {
            kind: ApplyRequestKind::Apply,
            path: video.to_string_lossy().to_string(),
            request_id: Some("fb-video".into()),
        };
        let target = service.resolve_apply_request_target(&request).unwrap();
        assert!(
            target.fallback_path.is_none(),
            "video should have no fallback yet"
        );
    }

    #[test]
    fn scene_without_preview_has_no_fallback_path() {
        let (tmp, service) = temp_service();
        let project = tmp.path().join("steamapps/workshop/content/431960/1");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("scene.json"), "{}").unwrap();
        std::fs::write(
            project.join("project.json"),
            r#"{"type":"scene","file":"scene.json"}"#,
        )
        .unwrap();
        let request = ApplyRequest {
            kind: ApplyRequestKind::Apply,
            path: project.to_string_lossy().to_string(),
            request_id: Some("fb-nopreview".into()),
        };
        let target = service.resolve_apply_request_target(&request).unwrap();
        assert!(
            target.fallback_path.is_none(),
            "scene without preview should have no fallback"
        );
    }

    #[test]
    fn execute_apply_request_emits_resolve_target_before_resolution_failure() {
        use std::sync::{Arc, Mutex};
        use wc_backend::apply_stage::{ApplyStage, ApplyStageEvent, ApplyStageReporter};

        let (tmp, service) = temp_service();
        let project = web_project(tmp.path());
        let events = Arc::new(Mutex::new(Vec::<ApplyStageEvent>::new()));
        let events_for_reporter = events.clone();

        struct SharedReporter(Arc<Mutex<Vec<ApplyStageEvent>>>);
        impl ApplyStageReporter for SharedReporter {
            fn emit(&mut self, event: ApplyStageEvent) {
                self.0.lock().unwrap().push(event);
            }
        }

        let request = ApplyRequest {
            kind: ApplyRequestKind::Apply,
            path: project.to_string_lossy().to_string(),
            request_id: Some("resolve-only".into()),
        };
        let options = ApplyExecutionOptions {
            stage_reporter: Some(Box::new(SharedReporter(events_for_reporter))),
            on_target_resolved: None,
        };

        let err = service
            .execute_apply_request_with_options(request, options)
            .unwrap_err();
        assert_eq!(err.code, "we_web_unsupported");
        let captured = events.lock().unwrap();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].stage, ApplyStage::ResolveTarget);
        assert_eq!(captured[0].request_id.as_deref(), Some("resolve-only"));
    }

    #[cfg(unix)]
    #[test]
    fn execute_apply_request_lwe_crash_includes_resolve_and_wait_stages() {
        use std::os::unix::fs::PermissionsExt;
        use std::sync::{Arc, Mutex};
        use wc_backend::apply_stage::{ApplyStage, ApplyStageEvent, ApplyStageReporter};

        let (tmp, service) = temp_service();
        let bin = tmp.path().join("test-lwe-app-stages");
        std::fs::write(&bin, "#!/bin/sh\nexit 1\n").unwrap();
        let mut perms = std::fs::metadata(&bin).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&bin, perms).unwrap();
        service
            .storage_for_tests()
            .config_set("linux_wallpaperengine_path", &bin.to_string_lossy())
            .unwrap();

        let scene = tmp.path().join("steamapps/workshop/content/431960/888888");
        std::fs::create_dir_all(&scene).unwrap();
        std::fs::write(scene.join("scene.pkg"), b"scene").unwrap();
        std::fs::write(
            scene.join("project.json"),
            r#"{"type":"scene","file":"scene.pkg","workshopid":"888888"}"#,
        )
        .unwrap();

        let events = Arc::new(Mutex::new(Vec::<ApplyStageEvent>::new()));
        let events_for_reporter = events.clone();
        struct SharedReporter(Arc<Mutex<Vec<ApplyStageEvent>>>);
        impl ApplyStageReporter for SharedReporter {
            fn emit(&mut self, event: ApplyStageEvent) {
                self.0.lock().unwrap().push(event);
            }
        }

        let request = ApplyRequest {
            kind: ApplyRequestKind::Apply,
            path: scene.to_string_lossy().to_string(),
            request_id: Some("lwe-crash".into()),
        };
        let options = ApplyExecutionOptions {
            stage_reporter: Some(Box::new(SharedReporter(events_for_reporter))),
            on_target_resolved: None,
        };

        assert!(service
            .execute_apply_request_with_options(request, options)
            .is_err());
        let stages: Vec<_> = events
            .lock()
            .unwrap()
            .iter()
            .map(|e| e.stage.clone())
            .collect();
        assert_eq!(
            stages,
            vec![
                ApplyStage::ResolveTarget,
                ApplyStage::StartLwe,
                ApplyStage::WaitRendererAlive,
            ]
        );
    }
}
