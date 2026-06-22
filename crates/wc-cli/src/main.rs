//! wc-cli — wallpaper-console Rust CLI (clap command dispatch).

use clap::{Parser, Subcommand};

mod commands;
mod library;
mod output;
mod sqlite;
mod wallpaper;

#[derive(Parser)]
#[command(name = "wallpaper-console-rust", version = "0.1.0")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
pub(crate) enum Commands {
    // ── Wallpaper ────────────────────────────────────────────────────
    Apply {
        file: String,
    },
    Inspect {
        path: String,
    },
    Stop,
    Status,
    Restore,
    Browse,
    #[command(name = "browse-all")]
    BrowseAll,
    #[command(name = "browse-images")]
    BrowseImages,
    #[command(name = "browse-gifs")]
    BrowseGifs,
    #[command(name = "browse-videos")]
    BrowseVideos,
    Random,
    #[command(name = "random-all")]
    RandomAll,
    #[command(name = "random-image")]
    RandomImage,
    #[command(name = "random-gif")]
    RandomGif,
    #[command(name = "random-video")]
    RandomVideo,

    // ── Sources ──────────────────────────────────────────────────────
    Add {
        dir: String,
    },
    Remove,
    #[command(name = "remove-source")]
    RemoveSource {
        dir: String,
    },
    Sources,
    #[command(name = "steam-workshop")]
    SteamWorkshop,
    #[command(name = "validate-sources")]
    ValidateSources,
    #[command(name = "remove-missing")]
    RemoveMissing,
    #[command(name = "dedupe-sources")]
    DedupeSources,

    // ── Favorites ────────────────────────────────────────────────────
    #[command(name = "favorite-add")]
    FavoriteAdd {
        file: String,
    },
    #[command(name = "favorite-add-current")]
    FavoriteAddCurrent,
    Favorites,
    #[command(name = "favorite-random")]
    FavoriteRandom,
    #[command(name = "favorite-remove")]
    FavoriteRemove {
        file: Option<String>,
    },

    // ── History ──────────────────────────────────────────────────────
    History,
    #[command(name = "history-random")]
    HistoryRandom,
    #[command(name = "history-clear")]
    HistoryClear,

    // ── Search / Sort ────────────────────────────────────────────────
    /// Search wallpapers by filename (fzf interactive).
    Search {
        /// Search query (prompts interactively if omitted)
        query: Vec<String>,
    },
    /// Search wallpapers by source directory (fzf interactive).
    #[command(name = "search-source")]
    SearchSource {
        query: Vec<String>,
    },
    /// Search wallpapers by type: image, gif, or video (fzf interactive).
    #[command(name = "search-type")]
    SearchType {
        query: Vec<String>,
    },
    /// Sort wallpapers by modification time (fzf interactive).
    #[command(name = "sort-mtime")]
    SortMtime,
    /// Sort wallpapers by file size (fzf interactive).
    #[command(name = "sort-size")]
    SortSize,
    /// Sort wallpapers by filename (fzf interactive).
    #[command(name = "sort-name")]
    SortName,

    // ── Config ───────────────────────────────────────────────────────
    #[command(name = "config-get")]
    ConfigGet {
        key: String,
        default: Option<String>,
    },
    #[command(name = "config-set")]
    ConfigSet {
        key: String,
        value: Vec<String>,
    },

    // ── Library ──────────────────────────────────────────────────────
    Rescan,
    Library,
    #[command(name = "library-count")]
    LibraryCount,
    #[command(name = "browse-library")]
    BrowseLibrary,
    #[command(name = "random-library")]
    RandomLibrary,
    #[command(name = "library-json")]
    LibraryJson {
        #[arg(long)]
        tsv: bool,
        #[arg(long)]
        sqlite: bool,
    },
    #[command(name = "library-page-json")]
    LibraryPageJson {
        #[arg(long, default_value = "sqlite")]
        source: String,
        #[arg(long, default_value = "all")]
        filter: String,
        #[arg(long, default_value = "newest")]
        sort: String,
        #[arg(long, default_value = "")]
        search: String,
        #[arg(long, default_value_t = 0)]
        offset: usize,
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    #[command(name = "favorites-json")]
    FavoritesJson,
    #[command(name = "history-json")]
    HistoryJson,

    // ── SQLite ───────────────────────────────────────────────────────
    #[command(name = "migrate-to-sqlite")]
    MigrateToSqlite,
    #[command(name = "sqlite-verify")]
    SqliteVerify,
    #[command(name = "sqlite-resync")]
    SqliteResync,
    #[command(name = "sqlite-export-flat")]
    SqliteExportFlat,
    #[command(name = "sqlite-backup")]
    SqliteBackup,
    #[command(name = "sqlite-restore")]
    SqliteRestore {
        backup: String,
    },
    #[command(name = "sqlite-config-get")]
    SqliteConfigGet {
        key: String,
    },
    #[command(name = "sqlite-sources-list")]
    SqliteSourcesList,
    #[command(name = "sqlite-favorites-list")]
    SqliteFavoritesList,
    #[command(name = "sqlite-history-list")]
    SqliteHistoryList,
    #[command(name = "sqlite-current-read")]
    SqliteCurrentRead,
    #[command(name = "sqlite-last-backend-read")]
    SqliteLastBackendRead,

    // ── System ───────────────────────────────────────────────────────
    Tui,

    // ── Internal (hidden) ────────────────────────────────────────────
    /// fzf preview renderer (called by fzf --preview)
    #[command(name = "__preview__", hide = true)]
    Preview {
        file: String,
    },
    /// Generate a GUI thumbnail for a wallpaper file.
    #[command(name = "thumbnail", hide = true)]
    Thumbnail {
        file: String,
    },
    /// Generate GUI thumbnails for multiple wallpaper files (batch, JSON output).
    #[command(name = "thumbnail-batch-json", hide = true)]
    ThumbnailBatch {
        files: Vec<String>,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    commands::run(cli.command)
}
