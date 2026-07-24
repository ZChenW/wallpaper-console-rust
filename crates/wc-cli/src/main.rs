//! wc-cli — wallpaper-console Rust CLI (clap command dispatch).

use clap::{Parser, Subcommand};

mod commands;
mod library;
mod output;
mod sqlite;
mod wallpaper;

#[derive(Parser)]
#[command(name = "wallpaper-console-rust", version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
pub(crate) enum Commands {
    // ── Wallpaper ────────────────────────────────────────────────────
    Apply {
        file: String,
        /// Apply to one connected output, or `all` for an explicit all-display plan.
        #[arg(long)]
        target: Option<String>,
        /// Complete connected-output set. Repeat for multiple outputs; omit to query Wayland.
        #[arg(long = "output")]
        outputs: Vec<String>,
    },
    Inspect {
        path: String,
    },
    Stop,
    Status,
    Restore,
    /// Print connected display outputs as JSON.
    Displays,
    /// Print persisted per-display wallpaper state as JSON.
    #[command(name = "display-state")]
    DisplayState,
    /// Restore persisted assignments for connected or explicitly supplied outputs.
    #[command(name = "restore-displays")]
    RestoreDisplays {
        /// Connected output name. Repeat for multiple outputs; omit to query Wayland.
        #[arg(long = "output")]
        outputs: Vec<String>,
    },
    /// Restore saved display assignments only when login restoration is enabled.
    #[command(name = "restore-at-login")]
    RestoreAtLogin,
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
    let args = std::env::args().collect::<Vec<_>>();
    if let Some(exit_code) = wc_app::scan_worker::try_run_worker_mode(&args) {
        std::process::exit(exit_code);
    }
    let cli = Cli::parse_from(args);
    commands::run(cli.command)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_without_target_parses_legacy_all_displays_default() {
        let cli = Cli::try_parse_from(["wallpaper-console-rust", "apply", "/walls/a.jpg"])
            .expect("legacy apply should still parse");

        let Some(Commands::Apply { file, target, .. }) = cli.command else {
            panic!("expected apply command");
        };
        assert_eq!(file, "/walls/a.jpg");
        assert_eq!(target, None);
    }

    #[test]
    fn apply_accepts_named_and_all_display_targets() {
        for (raw, expected) in [("eDP-1", "eDP-1"), ("all", "all")] {
            let cli = Cli::try_parse_from([
                "wallpaper-console-rust",
                "apply",
                "/walls/a.jpg",
                "--target",
                raw,
            ])
            .expect("targeted apply should parse");

            let Some(Commands::Apply { target, .. }) = cli.command else {
                panic!("expected apply command");
            };
            assert_eq!(target.as_deref(), Some(expected));
        }
    }

    #[test]
    fn display_api_commands_parse_without_changing_legacy_restore() {
        let displays = Cli::try_parse_from(["wallpaper-console-rust", "displays"]).unwrap();
        assert!(matches!(displays.command, Some(Commands::Displays)));

        let state = Cli::try_parse_from(["wallpaper-console-rust", "display-state"]).unwrap();
        assert!(matches!(state.command, Some(Commands::DisplayState)));

        let restore = Cli::try_parse_from([
            "wallpaper-console-rust",
            "restore-displays",
            "--output",
            "eDP-1",
            "--output",
            "HDMI-A-1",
        ])
        .unwrap();
        let Some(Commands::RestoreDisplays { outputs }) = restore.command else {
            panic!("expected restore-displays command");
        };
        assert_eq!(outputs, ["eDP-1", "HDMI-A-1"]);

        let legacy = Cli::try_parse_from(["wallpaper-console-rust", "restore"]).unwrap();
        assert!(matches!(legacy.command, Some(Commands::Restore)));
    }

    #[test]
    fn restore_at_login_command_parses_separately_from_manual_restore() {
        let cli = Cli::try_parse_from(["wallpaper-console-rust", "restore-at-login"])
            .expect("login restore should parse");

        assert!(matches!(cli.command, Some(Commands::RestoreAtLogin)));
    }

    #[test]
    fn targeted_apply_accepts_explicit_known_outputs() {
        let cli = Cli::try_parse_from([
            "wallpaper-console-rust",
            "apply",
            "/walls/video.mp4",
            "--target",
            "eDP-1",
            "--output",
            "eDP-1",
            "--output",
            "HDMI-A-1",
        ])
        .unwrap();

        let Some(Commands::Apply {
            target, outputs, ..
        }) = cli.command
        else {
            panic!("expected apply command");
        };
        assert_eq!(target.as_deref(), Some("eDP-1"));
        assert_eq!(outputs, ["eDP-1", "HDMI-A-1"]);
    }

    #[test]
    fn removed_history_commands_are_not_user_facing_cli_commands() {
        for command in [
            "history",
            "history-random",
            "history-clear",
            "history-json",
            "sqlite-history-list",
        ] {
            assert!(
                Cli::try_parse_from(["wallpaper-console-rust", command]).is_err(),
                "{command} must no longer be accepted"
            );
        }
    }
}
