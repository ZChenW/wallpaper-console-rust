//! Verified global Stop across persistent BackendDriver implementations.
use crate::{driver, runtime};
use wc_core::{error::WcError, types::Backend};
use wc_storage::StorageApi;

/// Stop all wallpaper backends.
pub fn stop_all_backends(s: Option<&StorageApi>) -> Result<(), WcError> {
    stop_all_backends_with_runtime(s, &mut runtime::SystemBackendRuntime)
}

/// Shared global Stop path for CLI/GUI. Attempt every backend even when one
/// stop fails, then verify that no persistent renderer remains. Saved restore
/// preferences are untouched; callers clear legacy runtime state only on success.
pub fn stop_all_backends_with_runtime(
    s: Option<&StorageApi>,
    runtime: &mut dyn runtime::BackendRuntime,
) -> Result<(), WcError> {
    let mut failures = Vec::new();
    for backend in [
        Backend::LinuxWallpaperEngine,
        Backend::Mpvpaper,
        Backend::Swaybg,
        Backend::Awww,
    ] {
        if let Err(error) = driver::driver_for(backend)
            .expect("persistent backend has a driver")
            .stop_checked(runtime, s)
        {
            failures.push(format!("{}: {error}", backend.as_str()));
        }
    }
    // In particular, a cleared LWE tracking PID or missing awww socket does
    // not prove that an untracked renderer/daemon process has exited.
    match runtime.renderer_command_lines() {
        Err(error) => failures.push(format!("cannot verify renderer process exit: {error}")),
        Ok(processes) => {
            for process in processes {
                let Some(binary) = process.argv.first().and_then(|name| {
                    std::path::Path::new(name)
                        .file_name()
                        .and_then(|name| name.to_str())
                }) else {
                    continue;
                };
                if matches!(
                    binary,
                    "linux-wallpaperengine" | "mpvpaper" | "swaybg" | "awww-daemon"
                ) {
                    failures.push(format!(
                        "{binary} process {} remains after Stop",
                        process.pid
                    ));
                }
            }
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(WcError::Other(format!(
            "Cannot verify all wallpaper backends stopped: {}",
            failures.join("; ")
        )))
    }
}
