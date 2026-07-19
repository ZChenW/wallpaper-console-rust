#[cfg(test)]
use wc_config::ConfigDirExt;
use wc_core::types::Backend;
use wc_storage::StorageApi;

use crate::lifecycle;
use crate::visual_handoff;

pub(crate) fn write_apply_stage_timings(
    s: &StorageApi,
    pre_stop: std::time::Duration,
    fallback: std::time::Duration,
    target: std::time::Duration,
    settle: std::time::Duration,
    backend: Backend,
) {
    if s.config_get("gui_debug_logs", "off") != "on" {
        return;
    }
    let log = format!(
        "apply stages: backend={:?} pre_stop={:?} fallback={:?} target={:?} settle={:?}\n",
        backend, pre_stop, fallback, target, settle
    );
    let _ = std::fs::write(s.cd.path.join("backend-apply-timings-last.log"), log);
}

pub(crate) fn write_debug_handoff_log(
    s: &StorageApi,
    lifecycle: &lifecycle::ApplyLifecyclePlan,
    backend: Backend,
    fallback_path: Option<&str>,
    visual: &visual_handoff::VisualHandoffPlan,
    fallback_error: &str,
    path: &str,
) {
    if s.config_get("gui_debug_logs", "off") != "on" {
        return;
    }
    let log_path = s.cd.path.join("backend-handoff-last.log");
    let fb_name = fallback_path
        .and_then(|p| std::path::Path::new(p).file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let path_name = std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    let log = format!(
        "previous={:?}\ntarget={:?}\npath={}\nfallback={}\npre_stop={:?}\nfallback_stage={:?}\ntarget_startup_settle_ms={}\npost_success_stop={:?}\nfallback_error={}\n",
        lifecycle.previous,
        backend,
        path_name,
        fb_name,
        lifecycle.pre_stop,
        visual.fallback_stage,
        visual.target_startup_settle_ms,
        lifecycle.post_success_stop,
        fallback_error,
    );
    let _ = std::fs::write(&log_path, log);
}

#[cfg(test)]
mod tests {
    use super::*;
    use wc_core::config::ConfigDir;
    use wc_storage::StorageApi;

    fn temp_storage() -> (tempfile::TempDir, StorageApi) {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        wc_config::write_config_value(&cd.path, "storage_backend", "sqlite").unwrap();
        let s = StorageApi::new(cd);
        (tmp, s)
    }

    #[test]
    fn write_apply_stage_timings_only_writes_when_debug_enabled() {
        let (_tmp, s) = temp_storage();
        let log_path = s.cd.path.join("backend-apply-timings-last.log");

        write_apply_stage_timings(
            &s,
            std::time::Duration::from_micros(1),
            std::time::Duration::from_micros(2),
            std::time::Duration::from_micros(3),
            std::time::Duration::from_micros(4),
            Backend::Awww,
        );
        assert!(
            !log_path.exists(),
            "timings log must not be written when debug off"
        );

        s.config_set("gui_debug_logs", "on").unwrap();
        write_apply_stage_timings(
            &s,
            std::time::Duration::from_micros(1),
            std::time::Duration::from_micros(2),
            std::time::Duration::from_micros(3),
            std::time::Duration::from_micros(4),
            Backend::Awww,
        );
        assert!(
            log_path.exists(),
            "timings log should be written when debug on"
        );
        let content = std::fs::read_to_string(&log_path).unwrap();
        assert!(content.contains("backend=Awww"));
        assert!(content.contains("pre_stop="));
        assert!(content.contains("target="));
        assert!(content.contains("settle="));
    }
}
