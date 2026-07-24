use wc_core::error::WcError;
use wc_core::types::Backend;
use wc_storage::StorageApi;

use crate::apply_stage;
use crate::lifecycle;
use crate::runtime;

/// Legacy single-wallpaper restore via [`apply_wallpaper`].
///
/// Prefer [`wc_app::AppService::restore_displays`] so Restore goes through
/// display_plan + ApplyTransition. GUI/CLI restore already use that path.
pub fn restore_clean(s: &StorageApi) -> Result<(), WcError> {
    let mut runtime = runtime::SystemBackendRuntime;
    restore_clean_with_runtime(s, &mut runtime)
}

pub(crate) fn restore_clean_with_runtime(
    s: &StorageApi,
    runtime: &mut dyn runtime::BackendRuntime,
) -> Result<(), WcError> {
    let current = s
        .current_read()?
        .ok_or_else(|| WcError::Other("no previous wallpaper to restore".into()))?;
    let p = std::path::Path::new(&current);
    if !p.is_file() && !p.is_dir() {
        return Err(WcError::WallpaperMissing(p.to_path_buf()));
    }

    let entry = wc_scan::make_entry(&current)
        .ok_or_else(|| WcError::UnsupportedFileType(current.clone()))?;
    let backend = backend_for_restore_entry(s, &entry);
    let fallback_path = fallback_for_restore_entry(&entry, p);

    crate::execute_stop_plan_with_runtime(s, lifecycle::StopPlan::All, runtime)?;
    let mut reporter = apply_stage::NoopReporter;
    crate::apply_wallpaper_with_runtime(
        s,
        &current,
        backend,
        fallback_path.as_deref(),
        runtime,
        &mut reporter,
        None,
    )
}

fn backend_for_restore_entry(s: &StorageApi, entry: &wc_core::types::WallpaperEntry) -> Backend {
    s.backend_routing().backend_for(entry.file_type)
}

fn fallback_for_restore_entry(
    entry: &wc_core::types::WallpaperEntry,
    _path: &std::path::Path,
) -> Option<String> {
    match entry.file_type {
        wc_core::types::FileType::Image | wc_core::types::FileType::Gif => {
            Some(entry.path.to_string())
        }
        wc_core::types::FileType::Video
        | wc_core::types::FileType::WeScene
        | wc_core::types::FileType::WeWeb
        | wc_core::types::FileType::WeApplication => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wc_core::types::{FileType, WallpaperEntry};

    fn entry(file_type: FileType) -> WallpaperEntry {
        WallpaperEntry {
            path: "/tmp/wallpaper".into(),
            file_type,
            ext: String::new(),
            backend: Backend::Unsupported,
            size: 0,
            mtime: 0,
            resolution: String::new(),
            project: None,
        }
    }

    #[test]
    fn restore_routing_matches_shared_safe_routing_for_every_type() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = StorageApi::new(wc_core::ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        });
        storage.config_set("image_backend", "mpvpaper").unwrap();
        storage.config_set("gif_backend", "mpvpaper").unwrap();
        storage.config_set("video_backend", "awww").unwrap();
        let routing = storage.backend_routing();

        for file_type in [
            FileType::Image,
            FileType::Gif,
            FileType::Video,
            FileType::WeScene,
            FileType::WeWeb,
            FileType::WeApplication,
        ] {
            assert_eq!(
                backend_for_restore_entry(&storage, &entry(file_type)),
                routing.backend_for(file_type),
                "file_type={file_type:?}"
            );
        }
    }
}
