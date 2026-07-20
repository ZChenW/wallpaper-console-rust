mod commands;
mod instance_coordinator;
mod library_scheduler;
mod library_service;

use std::sync::Arc;

use tauri::{Emitter, Manager};

pub use commands::path_guard::{ensure_path_in_config_dir, ensure_path_in_sources};

fn fatal_startup_error(message: impl AsRef<str>) -> ! {
    instance_coordinator::show_error("Wallpaper Console", message.as_ref());
    std::process::exit(1);
}

fn configure_logging(app: &tauri::AppHandle, config_dir: &std::path::Path) -> tauri::Result<()> {
    use tauri_plugin_log::{Builder, RotationStrategy, Target, TargetKind};

    let plugin = if cfg!(debug_assertions) {
        Builder::default().level(log::LevelFilter::Info).build()
    } else {
        Builder::default()
            .level(log::LevelFilter::Info)
            .rotation_strategy(RotationStrategy::KeepAll)
            .targets([Target::new(TargetKind::Folder {
                path: config_dir.to_path_buf(),
                file_name: Some("wallpaper-console".into()),
            })])
            .build()
    };
    app.plugin(plugin)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Cross-process test entry point: when the test harness spawns this
    // binary with WC_INSTANCE_TEST set, run the coordinator test logic
    // and exit immediately — never start Tauri.
    if std::env::var("WC_INSTANCE_TEST").is_ok() {
        instance_coordinator::run_instance_test_entry_point();
    }

    // Phase 1: single-instance coordinator using a ConfigDir lock file and
    // Unix-domain socket. CLI paths are completely unaffected — this only
    // runs in the Tauri GUI entry point.
    let cd = match wc_config::resolve_config_dir() {
        Ok(path) => wc_core::config::ConfigDir::from_path(path),
        Err(error) => fatal_startup_error(format!("cannot resolve config directory: {error}")),
    };

    // Claim the instance lock BEFORE expensive init_config_dir / default
    // writes. Only resolves the path and creates the private dir (0700).
    let claim = match instance_coordinator::claim_instance(&cd) {
        Ok(claim) => claim,
        Err(error) => fatal_startup_error(format!("cannot claim instance lock: {error}")),
    };

    let (primary_socket, _lease) = match claim {
        instance_coordinator::ClaimResult::Primary(lease) => {
            // This is the first GUI instance. Bind the socket so
            // secondaries can connect while we initialise Tauri.
            let socket = match instance_coordinator::PrimarySocket::bind(&cd) {
                Ok(socket) => socket,
                Err(error) => fatal_startup_error(format!("cannot bind instance socket: {error}")),
            };
            (Some(socket), Some(lease))
        }
        instance_coordinator::ClaimResult::Secondary => {
            // Another GUI is already running. Request focus and exit.
            let outcome = instance_coordinator::try_focus_primary(&cd);
            match outcome {
                instance_coordinator::SecondaryOutcome::Ack => {
                    eprintln!("Focus request acknowledged by primary instance.");
                    std::process::exit(0);
                }
                instance_coordinator::SecondaryOutcome::Timeout => {
                    let msg = "Focus request to primary instance timed out after 2 seconds. \
                               The primary GUI may be unresponsive.";
                    instance_coordinator::show_error("Wallpaper Console", msg);
                    std::process::exit(1);
                }
                instance_coordinator::SecondaryOutcome::Failed(reason) => {
                    let msg = format!(
                        "Could not request focus from primary instance: {reason}. \
                         Is another wallpaper-console already running?"
                    );
                    instance_coordinator::show_error("Wallpaper Console", &msg);
                    std::process::exit(1);
                }
            }
        }
    };

    // Now safe to do expensive initialisation.
    let _ = wc_config::init_config_dir(&cd.path);

    tauri::Builder::default()
        .setup(move |app| {
            let log_config_dir = cd.path.clone();
            configure_logging(app.handle(), &log_config_dir)?;

            // Phase 1: manage the library service for readiness gating and
            // future Phase 2 caching / observation.
            let library_service = library_service::LibraryService::new();
            let app_handle = app.handle().clone();
            library_service.set_change_notifier(move |revision| {
                let _ = app_handle.emit("library-revision-changed", revision);
            });
            app.manage(library_service);

            // Start the instance socket accept loop now that the main window
            // exists and can be focused.
            if let Some(primary) = primary_socket {
                let window = app
                    .get_webview_window("main")
                    .expect("main window must exist after setup");
                let on_focus: instance_coordinator::FocusCallback = Arc::new(move || {
                    window.show().map_err(|e| e.to_string())?;
                    window.unminimize().map_err(|e| e.to_string())?;
                    window.set_focus().map_err(|e| e.to_string())?;
                    Ok(())
                });
                let _handle = instance_coordinator::start_accept_loop(primary, on_focus);
                // Leak both for the process lifetime (run() does not return).
                // PrimarySocket::bind already removed any stale socket inode;
                // mem::forget skips CoordinatorHandle::shutdown, so the bound
                // socket and instance lock file are reclaimed by the OS on exit.
                std::mem::forget(_handle);
                if let Some(lease) = _lease {
                    std::mem::forget(lease);
                }
            }

            // Phase 1: startup no longer schedules an automatic source scan.
            // Background work starts only after the frontend signals its
            // first Library paint via the `library_ready` command.
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::status,
            commands::linux_wallpaperengine_status,
            commands::renderer_statuses,
            commands::apply,
            commands::apply_action,
            commands::apply_to_display,
            commands::stop,
            commands::we_clear_backend_error,
            commands::we_debug_info,
            commands::restore,
            commands::restore_displays,
            commands::displays_list,
            commands::display_state_list,
            commands::runtime_wallpaper_observations,
            commands::config_get,
            commands::config_get_many,
            commands::config_set,
            commands::sources_list,
            commands::first_run_source_suggestions,
            commands::source_add,
            commands::source_remove,
            commands::source_rename,
            commands::source_set_recursive,
            commands::source_refresh,
            commands::source_remove_by_id,
            commands::validate_sources,
            commands::remove_missing_sources,
            commands::scan_steam_workshop,
            commands::favorite_add,
            commands::favorite_remove,
            commands::favorites_page,
            commands::library_count,
            commands::library_page_gui,
            commands::library_browser_page,
            commands::library_browser_total,
            commands::library_browser_random,
            commands::library_wallpaper_exists,
            commands::rescan,
            commands::library_ready,
            commands::migrate_to_sqlite,
            commands::import_legacy_flat_files,
            commands::sqlite_verify,
            commands::sqlite_repair,
            commands::sqlite_resync,
            commands::sqlite_backup,
            commands::sqlite_restore,
            commands::sqlite_export_flat,
            commands::thumbnail_for,
            commands::preview_asset_authorize,
            commands::thumbnail_cache_status,
            commands::thumbnail_cache_clear,
            commands::thumbnail_cache_cleanup_old,
            commands::scan_progress,
            commands::scan_cancel,
            commands::library_source_status,
            commands::open_project_location,
            commands::open_path,
            commands::reveal_in_file_manager,
            commands::browse_directory,
            commands::export_diagnostics,
        ])
        .run(tauri::generate_context!())
        .unwrap_or_else(|error| {
            fatal_startup_error(format!("error while running application: {error}"))
        });
}
