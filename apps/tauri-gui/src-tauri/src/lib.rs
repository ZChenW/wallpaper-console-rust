mod commands;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            commands::schedule_startup_source_refresh();
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::status,
            commands::linux_wallpaperengine_status,
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
            commands::config_get,
            commands::config_get_many,
            commands::config_set,
            commands::sources_list,
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
            commands::library_page,
            commands::library_page_gui,
            commands::library_browser_page,
            commands::library_browser_random,
            commands::rescan,
            commands::migrate_to_sqlite,
            commands::import_legacy_flat_files,
            commands::sqlite_verify,
            commands::sqlite_repair,
            commands::sqlite_resync,
            commands::sqlite_backup,
            commands::sqlite_restore,
            commands::sqlite_export_flat,
            commands::thumbnail_for,
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
        .expect("error while running tauri application");
}
