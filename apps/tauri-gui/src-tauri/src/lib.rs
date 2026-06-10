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
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::status,
            commands::apply,
            commands::stop,
            commands::restore,
            commands::config_get,
            commands::config_set,
            commands::sources_list,
            commands::source_add,
            commands::source_remove,
            commands::validate_sources,
            commands::remove_missing_sources,
            commands::scan_steam_workshop,
            commands::favorites_list,
            commands::favorite_add,
            commands::favorite_remove,
            commands::history_list,
            commands::history_clear,
            commands::library_count,
            commands::library_list,
            commands::library_page,
            commands::rescan,
            commands::migrate_to_sqlite,
            commands::sqlite_verify,
            commands::sqlite_resync,
            commands::sqlite_backup,
            commands::sqlite_restore,
            commands::sqlite_export_flat,
            commands::thumbnail_for,
            commands::thumbnail_cache_status,
            commands::thumbnail_cache_clear,
            commands::open_path,
            commands::reveal_in_file_manager,
            commands::browse_directory,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
