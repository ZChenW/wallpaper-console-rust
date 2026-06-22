use wc_core::error::WcError;
use wc_core::types::Backend;
use wc_storage::StorageApi;

use crate::apply_stage;
use crate::lifecycle;
use crate::runtime;

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
    match entry.file_type {
        wc_core::types::FileType::Image => {
            match wc_core::config::normalize_image_backend(&s.config_get("image_backend", "awww")) {
                "mpvpaper" => Backend::Mpvpaper,
                _ => Backend::Awww,
            }
        }
        wc_core::types::FileType::Gif => match s.config_get("gif_backend", "awww").as_str() {
            "mpvpaper" => Backend::Mpvpaper,
            _ => Backend::Awww,
        },
        wc_core::types::FileType::Video => match s.config_get("video_backend", "mpvpaper").as_str()
        {
            "awww" => Backend::Awww,
            _ => Backend::Mpvpaper,
        },
        wc_core::types::FileType::WeScene => Backend::LinuxWallpaperEngine,
        wc_core::types::FileType::WeWeb | wc_core::types::FileType::WeApplication => {
            Backend::Unsupported
        }
    }
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

pub fn restore(s: &StorageApi) -> Result<(), WcError> {
    restore_clean(s)
}
