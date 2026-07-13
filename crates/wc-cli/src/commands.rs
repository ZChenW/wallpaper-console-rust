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
        Some(Commands::MigrateToSqlite) => crate::sqlite::migrate_to_sqlite(),
        Some(cmd) => {
            let cd = ConfigDir::new()?;
            let storage = StorageApi::try_new(cd)?;
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
        Commands::Stop => crate::wallpaper::stop(storage),
        Commands::Status => crate::wallpaper::status(storage),
        Commands::Restore => crate::wallpaper::restore(storage),
        Commands::Displays => crate::wallpaper::displays(),
        Commands::DisplayState => crate::wallpaper::display_state(storage),
        Commands::RestoreDisplays { outputs } => {
            crate::wallpaper::restore_displays(storage, outputs)
        }
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
        | Commands::FavoritesJson
        | Commands::HistoryJson => crate::library::run(command, storage),

        Commands::MigrateToSqlite
        | Commands::SqliteVerify
        | Commands::SqliteResync
        | Commands::SqliteExportFlat
        | Commands::SqliteBackup
        | Commands::SqliteRestore { .. }
        | Commands::SqliteConfigGet { .. }
        | Commands::SqliteSourcesList
        | Commands::SqliteFavoritesList
        | Commands::SqliteHistoryList
        | Commands::SqliteCurrentRead
        | Commands::SqliteLastBackendRead => crate::sqlite::run(command, storage),

        other => crate::wallpaper::run(other, storage),
    }
}
