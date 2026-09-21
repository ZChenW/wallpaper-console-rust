use wc_config::ConfigDirExt;
use wc_core::config::ConfigDir;
use wc_storage::StorageApi;

use crate::Commands;

pub(crate) fn run(command: Option<Commands>) -> anyhow::Result<()> {
    match command {
        None => {
            println!(
                "Rust TUI is not implemented yet. Use wallpaper-console-gui-rust for the GUI.\n"
            );
            crate::output::print_help();
            Ok(())
        }
        Some(Commands::WatchDisplays { config_dir }) => {
            let service =
                wc_app::AppService::try_from_config_dir(ConfigDir::from_path(config_dir.into()))
                    .map_err(|e| anyhow::anyhow!(e.message))?;
            service
                .watch_displays()
                .map_err(|e| anyhow::anyhow!(e.message))
        }
        Some(Commands::MigrateToSqlite) => crate::sqlite::migrate_to_sqlite(),
        Some(cmd) => {
            let cd = ConfigDir::new()?;
            let storage = StorageApi::try_new(cd)?;
            if let Err(error) = wc_app::login_restore::reconcile(&storage) {
                eprintln!("Login restore registration needs attention: {error}");
            }
            run_with_storage(cmd, &storage)
        }
    }
}

fn run_with_storage(cmd: Commands, storage: &StorageApi) -> anyhow::Result<()> {
    match cmd {
        Commands::Apply {
            file,
            target,
            outputs,
        } => crate::wallpaper::apply(storage, file, target, outputs),
        Commands::Inspect { path } => crate::wallpaper::inspect(storage, path),
        Commands::Stop { targets } => crate::wallpaper::stop_targeted(storage, targets),
        Commands::Status => crate::wallpaper::status(storage),
        Commands::PostApplyStatus => {
            let report = wc_app::post_apply::last_report(storage).map_err(anyhow::Error::msg)?;
            println!("{}", serde_json::to_string(&report)?);
            Ok(())
        }
        Commands::PostApplyRetry => {
            let report =
                wc_app::post_apply::retry_last_action(storage).map_err(anyhow::Error::msg)?;
            println!("{}", serde_json::to_string(&report)?);
            anyhow::ensure!(
                matches!(
                    report.status,
                    wc_app::post_apply::PostApplyStatus::Succeeded
                ),
                "{}",
                report.detail
            );
            Ok(())
        }
        Commands::Restore => crate::wallpaper::restore(storage),
        Commands::Displays => crate::wallpaper::displays(),
        Commands::DisplayState => crate::wallpaper::display_state(storage),
        Commands::RestoreDisplays { outputs, targets } => {
            crate::wallpaper::restore_displays_targeted(storage, outputs, targets)
        }
        Commands::RestoreAtLogin => crate::wallpaper::restore_at_login(storage),
        other => run_remaining(other, storage),
    }
}

fn run_remaining(command: Commands, storage: &StorageApi) -> anyhow::Result<()> {
    match command {
        Commands::Rescan
        | Commands::Library
        | Commands::LibraryCount
        | Commands::BrowseLibrary
        | Commands::RandomLibrary
        | Commands::LibraryJson { .. }
        | Commands::LibraryPageJson { .. }
        | Commands::FavoritesJson => crate::library::run(command, storage),

        Commands::MigrateToSqlite
        | Commands::SqliteVerify
        | Commands::SqliteResync
        | Commands::SqliteExportFlat
        | Commands::SqliteBackup
        | Commands::SqliteRestore { .. }
        | Commands::SqliteConfigGet { .. }
        | Commands::SqliteSourcesList
        | Commands::SqliteFavoritesList
        | Commands::SqliteCurrentRead
        | Commands::SqliteLastBackendRead => crate::sqlite::run(command, storage),

        other => crate::wallpaper::run(other, storage),
    }
}
