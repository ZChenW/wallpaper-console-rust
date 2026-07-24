mod common;
mod database;
mod files;
mod library;
pub mod path_guard;
pub mod scan;
mod settings;
mod sources;
mod thumbnails;
mod wallpaper;

pub use database::*;
pub use files::*;
pub use library::*;
pub use scan::*;
pub use settings::*;
pub use sources::*;
pub use thumbnails::*;
pub use wallpaper::*;

/// Called by the frontend after the first Library wallpapers are painted.
/// Idempotent — subsequent calls are no-ops.
#[tauri::command]
pub fn library_ready(state: tauri::State<'_, crate::library_service::LibraryService>) {
    state.mark_frontend_ready();
}

/// Map the main window once the shell has a document theme.
/// Uses the same Rust `show` path as the single-instance focus callback so the
/// first cold start does not depend on JS window ACLs.
#[tauri::command]
pub fn reveal_main_window(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;

    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "main window is missing".to_string())?;
    window.show().map_err(|error| error.to_string())?;
    window.unminimize().map_err(|error| error.to_string())?;
    // Focus is best-effort: some compositors reject it during early map.
    let _ = window.set_focus();
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::library_service::LibraryService;

    #[test]
    fn library_ready_command_delegates_to_service() {
        let svc = LibraryService::new();
        assert!(!svc.is_ready());

        // Simulate what Tauri does: call the inner logic directly.
        svc.mark_frontend_ready();
        assert!(svc.is_ready());

        svc.mark_frontend_ready();
        assert!(svc.is_ready());
    }
}
