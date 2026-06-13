//! wc-cli — wallpaper-console Rust CLI (clap command dispatch).

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use wc_core::config::ConfigDir;
use wc_core::types::Backend;
use wc_storage::StorageApi;

#[derive(Parser)]
#[command(name = "wallpaper-console-rust", version = "0.1.0")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
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
    match cli.command {
        None => {
            println!(
                "Rust TUI is not implemented yet. Use wallpaper-console-gui-rust for the GUI.\n"
            );
            print_help();
            Ok(())
        }
        Some(cmd) => {
            let cd = ConfigDir::new()?;
            cd.init()?;
            let storage = StorageApi::new(cd);
            run_command(cmd, &storage)
        }
    }
}

fn run_command(cmd: Commands, s: &StorageApi) -> anyhow::Result<()> {
    match cmd {
        // ── Wallpaper ────────────────────────────────────────────────
        Commands::Apply { file } => {
            let service = wc_app::AppService::from_config_dir(wc_core::ConfigDir {
                path: s.cd.path.clone(),
            });
            let target = service
                .apply(&file)
                .map_err(|e| anyhow::anyhow!(e.message))?;
            println!("Applied: {}", target.resolved_path);
        }

        Commands::Inspect { path } => {
            let service = wc_app::AppService::from_config_dir(wc_core::ConfigDir {
                path: s.cd.path.clone(),
            });
            let inspected = service.inspect_path(&path).map_err(|e| {
                anyhow::anyhow!(serde_json::to_string_pretty(&e).unwrap_or(e.message))
            })?;
            println!("{}", serde_json::to_string_pretty(&inspected)?);
        }

        Commands::Stop => {
            wc_backend::stop_all_backends(Some(s))?;
            println!("All wallpaper backends stopped.");
        }

        Commands::Status => {
            let cur = s.current_read()?.unwrap_or_else(|| "(none)".into());
            let be = s.last_backend_read()?.unwrap_or_else(|| "(none)".into());
            let src_count = s.sources_list()?.len();
            println!("config directory:    {}", s.cd.path.display());
            println!("current wallpaper:   {}", cur);
            println!("last backend:        {}", be);
            println!("configured sources:  {}", src_count);
        }

        Commands::Restore => {
            wc_backend::restore(s)?;
            println!("Wallpaper restored.");
        }

        // ── Browse (fzf interactive, apply on selection) ─────────────
        Commands::Browse
        | Commands::BrowseAll
        | Commands::BrowseImages
        | Commands::BrowseGifs
        | Commands::BrowseVideos => {
            let (filter, label) = match &cmd {
                Commands::BrowseImages => (Some(wc_core::types::FileType::Image), "browse-images"),
                Commands::BrowseGifs => (Some(wc_core::types::FileType::Gif), "browse-gifs"),
                Commands::BrowseVideos => (Some(wc_core::types::FileType::Video), "browse-videos"),
                _ => (None, "browse"),
            };
            let candidates = scan_paths(s, filter)?;
            if candidates.is_empty() {
                anyhow::bail!("no wallpapers found");
            }
            let selection = fzf_select(&candidates, &format!("{}> ", label))?;
            if let Some(path) = selection {
                apply_selected(s, &path)?;
            }
        }

        // ── Random ───────────────────────────────────────────────────
        Commands::Random
        | Commands::RandomAll
        | Commands::RandomImage
        | Commands::RandomGif
        | Commands::RandomVideo => {
            let filter = match &cmd {
                Commands::RandomImage => Some(wc_core::types::FileType::Image),
                Commands::RandomGif => Some(wc_core::types::FileType::Gif),
                Commands::RandomVideo => Some(wc_core::types::FileType::Video),
                _ => None,
            };
            let paths = library_paths(s, filter)?;
            if paths.is_empty() {
                anyhow::bail!("no matching wallpapers found");
            }
            let idx = rand::random::<usize>() % paths.len();
            let chosen = &paths[idx];
            apply_selected(s, chosen)?;
        }

        // ── Sources ──────────────────────────────────────────────────
        Commands::Add { dir } => {
            let canonical = std::fs::canonicalize(&dir)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or(dir);
            s.sources_add(&canonical)?;
            println!("Added source: {}", canonical);
        }

        Commands::Sources => {
            let srcs = s.sources_list()?;
            if srcs.is_empty() {
                println!("(no source directories configured)");
            } else {
                for src in &srcs {
                    println!("{}", src);
                }
            }
        }

        Commands::Remove => {
            let paths = s.sources_list()?;
            if paths.is_empty() {
                anyhow::bail!("no sources configured");
            }
            let selection = fzf_select(&paths, "remove source> ")?;
            if let Some(path) = selection {
                s.sources_remove(&path)?;
                println!("Removed source: {}", path);
            }
        }

        Commands::RemoveSource { dir } => {
            // Try exact match first (works even when dir no longer exists).
            let removed = s.sources_remove(&dir)?;
            if removed {
                println!("Removed source: {}", dir);
                return Ok(());
            }
            // Canonicalise and scan stored sources for a match.
            let canonical = std::fs::canonicalize(&dir)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| dir.clone());
            let sources = s.sources_list()?;
            for stored in &sources {
                let stored_canon = std::fs::canonicalize(stored)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| stored.clone());
                if stored_canon == canonical {
                    s.sources_remove(stored)?;
                    println!("Removed source: {}", stored);
                    return Ok(());
                }
            }
            anyhow::bail!("source not found: {}", dir);
        }

        Commands::SteamWorkshop => {
            let home = std::env::var("HOME").unwrap_or_default();
            let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
            for base in &[
                format!("{}/.local/share/Steam", home),
                format!("{}/.steam/steam", home),
                format!(
                    "{}/.var/app/com.valvesoftware.Steam/.local/share/Steam",
                    home
                ),
                format!("{}/.var/app/com.valvesoftware.Steam/.steam/steam", home),
            ] {
                let ws_rel = std::path::Path::new(base).join("steamapps/workshop/content/431960");
                let ws = std::fs::canonicalize(&ws_rel).unwrap_or(ws_rel);
                if ws.is_dir() {
                    // Skip if this canonical workshop root was already processed.
                    let ws_canon = ws.to_string_lossy().to_string();
                    if !seen.insert(ws_canon) {
                        continue;
                    }
                    for entry in std::fs::read_dir(&ws)? {
                        let entry = entry?;
                        if entry.file_type()?.is_dir() {
                            let canonical = std::fs::canonicalize(entry.path())
                                .map(|p| p.to_string_lossy().to_string())
                                .unwrap_or_else(|_| entry.path().to_string_lossy().to_string());
                            if seen.insert(canonical.clone()) {
                                s.sources_add(&canonical)?;
                                println!("Added: {}", canonical);
                            }
                        }
                    }
                }
            }
            println!("Steam Workshop scan complete.");
        }

        Commands::ValidateSources => {
            for src in s.sources_list()? {
                let exists = std::path::Path::new(&src).is_dir();
                println!("{}  {}", if exists { "✓" } else { "✕" }, src);
            }
        }

        Commands::RemoveMissing => {
            let sources = s.sources_list()?;
            let mut removed = 0;
            for src in &sources {
                if !std::path::Path::new(src).is_dir() {
                    s.sources_remove(src)?;
                    println!("Removed missing source: {}", src);
                    removed += 1;
                }
            }
            println!("Removed {} missing source(s).", removed);
        }

        Commands::DedupeSources => {
            wc_storage::flat::dedupe_file(&s.cd.sources_path())?;
            println!("Sources deduplicated.");
        }

        // ── Favorites ────────────────────────────────────────────────
        Commands::FavoriteAdd { file } => {
            let added = s.favorites_add(&file)?;
            if added {
                println!("Added to favorites: {}", file);
            } else {
                println!("Already in favorites");
            }
        }

        Commands::FavoriteAddCurrent => {
            if let Some(cur) = s.current_read()? {
                s.favorites_add(&cur)?;
                println!("Added to favorites: {}", cur);
            } else {
                anyhow::bail!("no current wallpaper (apply one first)");
            }
        }

        Commands::Favorites => {
            let favs = s.favorites_list()?;
            if favs.is_empty() {
                println!("(no favorites)");
                return Ok(());
            }
            let selection = fzf_select(&favs, "favorites> ")?;
            if let Some(path) = selection {
                apply_selected(s, &path)?;
            }
        }

        Commands::FavoriteRandom => {
            let favs = s.favorites_list()?;
            if favs.is_empty() {
                anyhow::bail!("no favorites configured");
            }
            let idx = rand::random::<usize>() % favs.len();
            let chosen = &favs[idx];
            apply_selected(s, chosen)?;
        }

        Commands::FavoriteRemove { file } => {
            if let Some(path) = file {
                s.favorites_remove(&path)?;
                println!("Removed favorite: {}", path);
            } else {
                let favs = s.favorites_list()?;
                if favs.is_empty() {
                    anyhow::bail!("no favorites configured");
                }
                let selection = fzf_select(&favs, "remove favorite> ")?;
                if let Some(path) = selection {
                    s.favorites_remove(&path)?;
                    println!("Removed favorite: {}", path);
                }
            }
        }

        // ── History ──────────────────────────────────────────────────
        Commands::History => {
            let hist = s.history_list()?;
            if hist.is_empty() {
                println!("(no history)");
                return Ok(());
            }
            let selection = fzf_select(&hist, "history> ")?;
            if let Some(path) = selection {
                apply_selected(s, &path)?;
            }
        }

        Commands::HistoryRandom => {
            let hist = s.history_list()?;
            if hist.is_empty() {
                anyhow::bail!("no history entries");
            }
            let idx = rand::random::<usize>() % hist.len();
            let chosen = &hist[idx];
            apply_selected(s, chosen)?;
        }

        Commands::HistoryClear => {
            s.history_clear()?;
            println!("History cleared.");
        }

        // ── Search / Sort ────────────────────────────────────────────
        Commands::Search { query } => {
            let q = resolve_query(&query, "Search query")?;
            let candidates = scan_paths_matching_filename(s, &q)?;
            if candidates.is_empty() {
                anyhow::bail!("no wallpapers matching: {}", q);
            }
            let selection = fzf_select(&candidates, &format!("search:{}> ", q))?;
            if let Some(path) = selection {
                apply_selected(s, &path)?;
            }
        }

        Commands::SearchSource { query } => {
            let q = resolve_query(&query, "Source query")?;
            let candidates = scan_paths_matching_source(s, &q)?;
            if candidates.is_empty() {
                anyhow::bail!("no sources matching: {}", q);
            }
            let selection = fzf_select(&candidates, &format!("search-source:{}> ", q))?;
            if let Some(path) = selection {
                apply_selected(s, &path)?;
            }
        }

        Commands::SearchType { query } => {
            let q = resolve_query(&query, "Type (image/gif/video/we_scene/we_web)")?;
            let filter = match q.to_lowercase().as_str() {
                "image" => Some(wc_core::types::FileType::Image),
                "gif" => Some(wc_core::types::FileType::Gif),
                "video" => Some(wc_core::types::FileType::Video),
                "we_scene" | "scene" => Some(wc_core::types::FileType::WeScene),
                "we_web" | "web" => Some(wc_core::types::FileType::WeWeb),
                other => {
                    anyhow::bail!(
                        "unknown type '{}' — use image, gif, video, we_scene, or we_web",
                        other
                    )
                }
            };
            // Live scan (matches Bash: scan_wallpapers_by_type), not library.tsv
            let candidates = scan_paths(s, filter)?;
            if candidates.is_empty() {
                anyhow::bail!("no wallpapers of type: {}", q);
            }
            let selection = fzf_select(&candidates, &format!("search-type:{}> ", q))?;
            if let Some(path) = selection {
                apply_selected(s, &path)?;
            }
        }

        Commands::SortMtime | Commands::SortSize | Commands::SortName => {
            let candidates = scan_paths(s, None)?;
            if candidates.is_empty() {
                anyhow::bail!("no wallpapers found");
            }
            let sorted = sort_paths(&candidates, &cmd);
            let label = match &cmd {
                Commands::SortMtime => "sort:mtime",
                Commands::SortSize => "sort:size",
                Commands::SortName => "sort:name",
                _ => "sort",
            };
            let selection = fzf_select(&sorted, &format!("{}> ", label))?;
            if let Some(path) = selection {
                apply_selected(s, &path)?;
            }
        }

        // ── Config ───────────────────────────────────────────────────
        Commands::ConfigGet { key, default } => {
            let val = s.config_get(&key, &default.unwrap_or_default());
            println!("{}", val);
        }

        Commands::ConfigSet { key, value } => {
            let val = value.join(" ");
            s.config_set(&key, &val)?;
            println!("{} = {}", key, val);
        }

        // ── Library ──────────────────────────────────────────────────
        Commands::Rescan => {
            let t0 = std::time::Instant::now();
            let raw_sources = s.sources_list()?;
            if raw_sources.is_empty() {
                println!("(no sources configured)");
                return Ok(());
            }
            // Dedupe by canonical path (catches symlinks, .steam vs .local/share).
            let sources = wc_scan::dedupe_sources(&raw_sources);
            let dup_count = raw_sources.len() - sources.len();

            let files = wc_scan::scan_wallpapers(&sources);
            let walk_time = t0.elapsed();

            // Load prior metadata to skip unchanged files.
            let prior_cache = wc_scan::prior_metadata_cache(&s.cd.library_tsv_path());

            let t1 = std::time::Instant::now();
            let mut entries: Vec<wc_core::types::WallpaperEntry> = Vec::new();
            let mut reused = 0usize;
            let mut probed = 0usize;
            for path in &files {
                let (entry, was_reused) = wc_scan::make_entry_cached(path, &prior_cache);
                if let Some(entry) = entry {
                    if was_reused {
                        reused += 1;
                    } else {
                        probed += 1;
                    }
                    entries.push(entry);
                }
            }
            let probe_time = t1.elapsed();

            let mut tsv = String::new();
            for e in &entries {
                tsv.push_str(&format!(
                    "{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                    e.file_type.as_str(),
                    e.ext,
                    e.backend.as_str(),
                    e.size,
                    e.mtime,
                    e.resolution,
                    e.path
                ));
            }
            let tsv_path = s.cd.library_tsv_path();
            let tsv_tmp = tsv_path.with_extension("tsv.tmp");
            std::fs::write(&tsv_tmp, tsv)?;

            // Atomically replace the SQLite library when a DB exists.
            let t2 = std::time::Instant::now();
            let sqlite_count =
                match wc_storage::sqlite::library_replace_entries_batch_atomic(&s.cd, &entries) {
                    Ok(count) => count,
                    Err(err) => {
                        let _ = std::fs::remove_file(&tsv_tmp);
                        return Err(err.into());
                    }
                };
            let sqlite_time = t2.elapsed();

            std::fs::rename(&tsv_tmp, &tsv_path)?;
            let dirty = s.cd.path.join("library.dirty");
            if dirty.exists() {
                std::fs::remove_file(&dirty).ok();
            }
            let total_time = t0.elapsed();
            println!(
                "sources: {} canonical{}  walked: {} files  entries: {}  sqlite: {}\n\
                 reused_metadata: {}  probed_metadata: {}\n\
                 walk: {:.2}s  probe: {:.2}s  sqlite: {}ms  total: {:.2}s",
                sources.len(),
                if dup_count > 0 {
                    format!(" ({} duplicates skipped)", dup_count)
                } else {
                    String::new()
                },
                files.len(),
                entries.len(),
                sqlite_count,
                reused,
                probed,
                walk_time.as_secs_f64(),
                probe_time.as_secs_f64(),
                sqlite_time.as_millis(),
                total_time.as_secs_f64(),
            );
        }

        Commands::Library => {
            let content = std::fs::read_to_string(s.cd.library_tsv_path()).unwrap_or_default();
            print!("{}", content);
        }

        Commands::LibraryCount => {
            let content = std::fs::read_to_string(s.cd.library_tsv_path()).unwrap_or_default();
            let mut total = 0;
            let mut images = 0;
            let mut gifs = 0;
            let mut videos = 0;
            for line in content.lines() {
                if line.is_empty() {
                    continue;
                }
                total += 1;
                match line.split('\t').next().unwrap_or("") {
                    "image" => images += 1,
                    "gif" => gifs += 1,
                    "video" => videos += 1,
                    _ => {}
                }
            }
            println!(
                "total={}\nimages={}\ngifs={}\nvideos={}",
                total, images, gifs, videos
            );
        }

        Commands::BrowseLibrary => {
            let entries = library_entries(s)?;
            if entries.is_empty() {
                anyhow::bail!("library is empty — run rescan first");
            }
            let paths: Vec<String> = entries.iter().map(|e| e.path.to_string()).collect();
            let selection = fzf_select(&paths, "browse-library> ")?;
            if let Some(path) = selection {
                apply_selected(s, &path)?;
            }
        }

        Commands::RandomLibrary => {
            let entries = library_entries(s)?;
            if entries.is_empty() {
                anyhow::bail!("library is empty");
            }
            let idx = rand::random::<usize>() % entries.len();
            let e = &entries[idx];
            apply_selected(s, e.path.as_ref())?;
        }

        Commands::LibraryJson {
            tsv: _tsv,
            sqlite: use_sqlite,
        } => {
            if use_sqlite {
                json_library_from_sqlite(s)?;
            } else {
                json_library_from_tsv(s)?;
            }
        }
        Commands::LibraryPageJson {
            source,
            filter,
            sort,
            search,
            offset,
            limit,
        } => {
            json_library_page(s, &source, &filter, &sort, &search, offset, limit)?;
        }

        Commands::FavoritesJson => {
            let favs = s.favorites_list()?;
            println!("{}", serde_json::to_string_pretty(&favs)?);
        }

        Commands::HistoryJson => {
            let hist: Vec<serde_json::Value> = s
                .history_list()?
                .into_iter()
                .map(|p| serde_json::json!({"path": p}))
                .collect();
            println!("{}", serde_json::to_string_pretty(&hist)?);
        }

        // ── SQLite ───────────────────────────────────────────────────
        Commands::MigrateToSqlite => {
            wc_storage::sqlite::migrate_to_sqlite(&s.cd)?;
            println!("Migrated to: {}", s.cd.db_path().display());
        }

        Commands::SqliteVerify => match wc_storage::sqlite::verify(&s.cd) {
            Ok(wc_storage::sqlite::VerifyResult::Ok) => println!("VERIFY OK"),
            Ok(wc_storage::sqlite::VerifyResult::OkWithWarnings(warnings)) => {
                println!("VERIFY OK WITH WARNINGS");
                for w in &warnings {
                    println!("  warning: flat compatibility copy differs: {}", w);
                }
            }
            Ok(wc_storage::sqlite::VerifyResult::Failed(errors)) => {
                eprintln!(
                    "VERIFY FAILED: {} mismatch(es) found: {}",
                    errors.len(),
                    errors.join(", ")
                );
                std::process::exit(1);
            }
            Err(e) => {
                eprintln!("{}", e);
                let msg = e.to_string();
                if msg.contains("not found") {
                    std::process::exit(2);
                } else {
                    std::process::exit(1);
                }
            }
        },

        Commands::SqliteResync => {
            wc_storage::sqlite::resync(&s.cd)?;
            println!("Resync complete.");
        }

        Commands::SqliteExportFlat => {
            wc_storage::sqlite::export_flat(&s.cd)?;
            println!("Export complete.");
        }

        Commands::SqliteBackup => {
            let bak = wc_storage::sqlite::backup(&s.cd)?;
            println!("{}", bak);
        }

        Commands::SqliteRestore { backup } => {
            wc_storage::sqlite::restore(&s.cd, &PathBuf::from(&backup))?;
            println!("Restored.");
        }
        Commands::SqliteConfigGet { key } => {
            if let Some(value) = sqlite_config_get(&s.cd, &key)? {
                println!("{}", value);
            }
        }
        Commands::SqliteSourcesList => {
            for path in sqlite_list_table_paths(&s.cd, "sources", "ORDER BY path")? {
                println!("{}", path);
            }
        }
        Commands::SqliteFavoritesList => {
            for path in sqlite_list_table_paths(&s.cd, "favorites", "ORDER BY path")? {
                println!("{}", path);
            }
        }
        Commands::SqliteHistoryList => {
            for path in sqlite_list_table_paths(&s.cd, "history", "ORDER BY id DESC")? {
                println!("{}", path);
            }
        }
        Commands::SqliteCurrentRead => {
            if let Some(value) = sqlite_state_get(&s.cd, "current")? {
                println!("{}", value);
            }
        }
        Commands::SqliteLastBackendRead => {
            if let Some(value) = sqlite_state_get(&s.cd, "last_backend")? {
                println!("{}", value);
            }
        }

        Commands::Tui => {
            println!("TUI not yet implemented in Rust — use the Bash wallpaper-console for TUI.");
        }

        Commands::Preview { file } => {
            wc_preview::render_preview(&s.cd, &file);
        }

        Commands::Thumbnail { file } => {
            let cache_dir = s.cd.gui_thumbnail_cache_dir();
            let result = wc_preview::thumbnail_for(&cache_dir, &file);
            if let Some(thumb) = result.thumbnail {
                println!("{}", thumb);
            } else if let Some(err) = result.error {
                eprintln!("{}", err);
                std::process::exit(1);
            } else {
                eprintln!("thumbnail generation failed");
                std::process::exit(1);
            }
        }
        Commands::ThumbnailBatch { files } => {
            let cache_dir = s.cd.gui_thumbnail_cache_dir();
            let results: Vec<serde_json::Value> = files
                .into_iter()
                .map(|path| {
                    let result = wc_preview::thumbnail_for(&cache_dir, &path);
                    let mut obj = serde_json::json!({
                        "path": path,
                        "cacheHit": result.cache_hit,
                    });
                    if let Some(thumb) = result.thumbnail {
                        obj["thumbnail"] = serde_json::json!(thumb);
                    }
                    obj
                })
                .collect();
            println!("{}", serde_json::to_string(&results)?);
        }
    }
    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────

fn sqlite_connection(cd: &ConfigDir) -> anyhow::Result<rusqlite::Connection> {
    let db_path = cd.db_path();
    wc_storage::sqlite::ensure_sqlite_db(cd);
    rusqlite::Connection::open(&db_path)
        .map_err(|e| anyhow::anyhow!("failed to open wallpapers.db: {}", e))
}

fn sqlite_config_get(cd: &ConfigDir, key: &str) -> anyhow::Result<Option<String>> {
    let conn = sqlite_connection(cd)?;
    match conn.query_row(
        "SELECT value FROM config WHERE key=?1",
        rusqlite::params![key],
        |row| row.get(0),
    ) {
        Ok(value) => Ok(Some(value)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(anyhow::anyhow!("SQLite config read failed: {}", e)),
    }
}

fn sqlite_state_get(cd: &ConfigDir, key: &str) -> anyhow::Result<Option<String>> {
    let conn = sqlite_connection(cd)?;
    match conn.query_row(
        "SELECT value FROM state WHERE key=?1",
        rusqlite::params![key],
        |row| row.get(0),
    ) {
        Ok(value) => Ok(Some(value)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(anyhow::anyhow!("SQLite state read failed: {}", e)),
    }
}

fn sqlite_list_table_paths(
    cd: &ConfigDir,
    table: &str,
    order_clause: &str,
) -> anyhow::Result<Vec<String>> {
    let conn = sqlite_connection(cd)?;
    let sql = match table {
        "sources" | "favorites" | "history" => {
            format!("SELECT path FROM {} {}", table, order_clause)
        }
        _ => anyhow::bail!("unsupported SQLite path table: {}", table),
    };
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| anyhow::anyhow!("SQLite {} read failed: {}", table, e))?;
    let rows = stmt
        .query_map([], |row| row.get(0))
        .map_err(|e| anyhow::anyhow!("SQLite {} read failed: {}", table, e))?
        .collect::<Result<Vec<String>, _>>()
        .map_err(|e| anyhow::anyhow!("SQLite {} read failed: {}", table, e))?;
    Ok(rows)
}

fn apply_selected(s: &StorageApi, path: &str) -> anyhow::Result<()> {
    let service = wc_app::AppService::from_config_dir(wc_core::ConfigDir {
        path: s.cd.path.clone(),
    });
    let target = service
        .apply(path)
        .map_err(|e| anyhow::anyhow!(e.message))?;
    println!("Applied: {}", target.resolved_path);
    Ok(())
}

fn library_entries(s: &StorageApi) -> anyhow::Result<Vec<wc_core::types::WallpaperEntry>> {
    let content = std::fs::read_to_string(s.cd.library_tsv_path()).unwrap_or_default();
    let mut entries = Vec::new();
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 7 {
            continue;
        }
        entries.push(wc_core::types::WallpaperEntry {
            path: parts[6].into(),
            file_type: match parts[0] {
                "gif" => wc_core::types::FileType::Gif,
                "video" => wc_core::types::FileType::Video,
                "we_scene" => wc_core::types::FileType::WeScene,
                "we_web" => wc_core::types::FileType::WeWeb,
                "unsupported" => wc_core::types::FileType::WeApplication,
                _ => wc_core::types::FileType::Image,
            },
            ext: parts[1].to_string(),
            backend: match parts[2] {
                "mpvpaper" => Backend::Mpvpaper,
                "swww" => Backend::Awww,
                "linux-wallpaperengine" => Backend::LinuxWallpaperEngine,
                "chromium-web" | "webkit-layer-shell" | "unsupported" => Backend::Unsupported,
                _ => Backend::Awww,
            },
            size: parts[3].parse().unwrap_or(0),
            mtime: parts[4].parse().unwrap_or(0),
            resolution: parts[5].to_string(),
            project: None,
        });
    }
    Ok(entries)
}

fn library_paths(
    s: &StorageApi,
    filter: Option<wc_core::types::FileType>,
) -> anyhow::Result<Vec<String>> {
    let entries = library_entries(s)?;
    Ok(entries
        .into_iter()
        .filter(|e| filter.is_none_or(|f| e.file_type == f))
        .map(|e| e.path.to_string())
        .collect())
}

/// Live-scan all sources for wallpaper paths (bypasses library.tsv cache).
fn scan_paths(
    s: &StorageApi,
    filter: Option<wc_core::types::FileType>,
) -> anyhow::Result<Vec<String>> {
    let sources = s.sources_list()?;
    let all = wc_scan::scan_wallpapers(&sources);
    if let Some(ft) = filter {
        Ok(all
            .into_iter()
            .filter(|p| {
                wc_scan::make_entry(p)
                    .map(|entry| entry.file_type == ft)
                    .unwrap_or(false)
            })
            .collect())
    } else {
        Ok(all)
    }
}

/// Live-scan and filter by filename (case-insensitive substring match).
fn scan_paths_matching_filename(s: &StorageApi, query: &str) -> anyhow::Result<Vec<String>> {
    let sources = s.sources_list()?;
    let all = wc_scan::scan_wallpapers(&sources);
    let q = query.to_lowercase();
    Ok(all
        .into_iter()
        .filter(|p| {
            std::path::Path::new(p)
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.to_lowercase().contains(&q))
                .unwrap_or(false)
        })
        .collect())
}

/// Live-scan sources whose path contains the query, return all files.
fn scan_paths_matching_source(s: &StorageApi, query: &str) -> anyhow::Result<Vec<String>> {
    let sources = s.sources_list()?;
    let q = query.to_lowercase();
    let matching: Vec<String> = sources
        .into_iter()
        .filter(|src| src.to_lowercase().contains(&q))
        .collect();
    if matching.is_empty() {
        return Ok(Vec::new());
    }
    Ok(wc_scan::scan_wallpapers(&matching))
}

fn sort_paths(candidates: &[String], cmd: &Commands) -> Vec<String> {
    // Build (key, path) pairs
    let mut pairs: Vec<(String, String)> = candidates
        .iter()
        .map(|p| {
            let key = match cmd {
                Commands::SortMtime => {
                    let m = std::fs::metadata(p)
                        .ok()
                        .and_then(|m| m.modified().ok())
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    format!("{:020}", m)
                }
                Commands::SortSize => {
                    let s = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
                    format!("{:020}", s)
                }
                Commands::SortName => std::path::Path::new(p)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_lowercase(),
                _ => String::new(),
            };
            (key, p.clone())
        })
        .collect();

    match cmd {
        Commands::SortMtime | Commands::SortSize => {
            pairs.sort_by(|a, b| b.0.cmp(&a.0)); // descending
        }
        Commands::SortName => {
            pairs.sort_by(|a, b| a.0.cmp(&b.0)); // ascending
        }
        _ => {}
    }
    pairs.into_iter().map(|(_, p)| p).collect()
}

/// Get the query string, prompting interactively if empty.
fn resolve_query(args: &[String], prompt: &str) -> anyhow::Result<String> {
    if !args.is_empty() {
        return Ok(args.join(" "));
    }
    use std::io::{BufRead, Write};
    let mut stderr = std::io::stderr();
    write!(stderr, "{}: ", prompt)?;
    stderr.flush()?;
    let stdin = std::io::stdin();
    let mut line = String::new();
    stdin.lock().read_line(&mut line)?;
    let trimmed = line.trim().to_string();
    if trimmed.is_empty() {
        anyhow::bail!("cancelled");
    }
    Ok(trimmed)
}

fn fzf_select(items: &[String], prompt: &str) -> anyhow::Result<Option<String>> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let self_path = std::env::current_exe()
        .unwrap_or_else(|_| std::path::PathBuf::from("wallpaper-console-rust"));
    let preview_cmd = format!("{} __preview__ {{}}", self_path.to_string_lossy());

    let mut child = Command::new("fzf")
        .arg("--prompt")
        .arg(prompt)
        .arg("--preview")
        .arg(&preview_cmd)
        .arg("--preview-window=right:60%:wrap")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    if let Some(stdin) = child.stdin.as_mut() {
        for item in items {
            writeln!(stdin, "{}", item)?;
        }
    }
    let output = child.wait_with_output()?;
    if output.status.success() {
        let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if s.is_empty() {
            Ok(None)
        } else {
            Ok(Some(s))
        }
    } else {
        // fzf exits 130 on Ctrl-C / Esc
        Ok(None)
    }
}

fn print_help() {
    println!(concat!(
        "wallpaper-console-rust\n\n",
        "Commands:\n",
        "  apply FILE           browse              browse-all\n",
        "  browse-images        browse-gifs         browse-videos\n",
        "  random               random-all          random-image/gif/video\n",
        "  stop                 status               restore\n",
        "  add DIR              sources             remove (fzf)\n",
        "  remove-source DIR    steam-workshop      validate-sources\n",
        "  remove-missing       dedupe-sources\n",
        "  favorite-add FILE    favorite-add-current favorites (fzf)\n",
        "  favorite-random      favorite-remove [FILE]\n",
        "  history (fzf)        history-random      history-clear\n",
        "  search [QUERY]       search-source [Q]   search-type [Q]\n",
        "  sort-mtime           sort-size            sort-name\n",
        "  config-get KEY       config-set KEY VAL\n",
        "  rescan               library              library-count\n",
        "  browse-library (fzf) random-library       library-json [--tsv|--sqlite]\n",
        "  library-page-json    favorites-json       history-json\n",
        "  migrate-to-sqlite    sqlite-verify        sqlite-resync\n",
        "  sqlite-export-flat   sqlite-backup         sqlite-restore BACKUP\n",
        "  sqlite-config-get KEY sqlite-sources-list   sqlite-favorites-list\n",
        "  sqlite-history-list  sqlite-current-read   sqlite-last-backend-read\n",
        "  tui\n",
    ));
}

fn json_library_from_tsv(s: &StorageApi) -> anyhow::Result<()> {
    let entries = library_entries(s)?;
    let json: Vec<serde_json::Value> = entries.iter().map(json_from_entry).collect();
    println!("{}", serde_json::to_string_pretty(&json)?);
    Ok(())
}

fn json_from_entry(entry: &wc_core::types::WallpaperEntry) -> serde_json::Value {
    serde_json::json!({
        "path": entry.path.to_string(),
        "type": entry.file_type.as_str(),
        "ext": entry.ext,
        "backend": entry.backend.as_str(),
        "size": entry.size,
        "mtime": entry.mtime,
        "resolution": entry.resolution,
        "projectType": entry.project.as_ref().map(|p| p.project_type.clone()),
        "previewPath": entry.project.as_ref().and_then(|p| p.preview_path.clone()),
        "workshopId": entry.project.as_ref().and_then(|p| p.workshop_id.clone()),
        "title": entry.project.as_ref().and_then(|p| p.title.clone()),
        "weFile": entry.project.as_ref().and_then(|p| p.we_file.clone()),
        "unsupportedReason": entry.project.as_ref().and_then(|p| p.unsupported_reason.clone()),
    })
}

fn json_from_sql_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<serde_json::Value> {
    let project_type: String = row.get(7)?;
    let preview_path: String = row.get(8)?;
    let workshop_id: String = row.get(9)?;
    let title: String = row.get(10)?;
    let we_file: String = row.get(11)?;
    let unsupported_reason: String = row.get(12)?;
    Ok(serde_json::json!({
        "path": row.get::<_, String>(0)?,
        "type": row.get::<_, String>(1)?,
        "ext": row.get::<_, String>(2)?,
        "backend": row.get::<_, String>(3)?,
        "size": row.get::<_, i64>(4)?,
        "mtime": row.get::<_, i64>(5)?,
        "resolution": row.get::<_, String>(6)?,
        "projectType": optional_json_string(project_type),
        "previewPath": optional_json_string(preview_path),
        "workshopId": optional_json_string(workshop_id),
        "title": optional_json_string(title),
        "weFile": optional_json_string(we_file),
        "unsupportedReason": optional_json_string(unsupported_reason),
    }))
}

fn optional_json_string(value: String) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn json_library_from_sqlite(s: &StorageApi) -> anyhow::Result<()> {
    use rusqlite::Connection;
    let db = s.cd.db_path();
    if !db.exists() {
        let conn = Connection::open(&db)?;
        wc_storage::sqlite::create_schema(&conn)?;
        println!("[]");
        return Ok(());
    }
    let conn = Connection::open(&db)?;
    wc_storage::sqlite::ensure_wallpaper_metadata_columns(&conn)?;
    let mut stmt = conn.prepare(
        "SELECT path, type, ext, backend, size, mtime, resolution,
                project_type, preview_path, workshop_id, title, we_file, unsupported_reason
         FROM wallpapers ORDER BY path",
    )?;
    // Propagate row errors instead of silently ignoring them.
    let rows: Result<Vec<serde_json::Value>, rusqlite::Error> =
        stmt.query_map([], json_from_sql_row)?.collect();
    let rows = rows?;
    println!("{}", serde_json::to_string_pretty(&rows)?);
    Ok(())
}

fn json_library_page(
    s: &StorageApi,
    source: &str,
    filter: &str,
    sort: &str,
    search: &str,
    offset: usize,
    limit: usize,
) -> anyhow::Result<()> {
    match source {
        "sqlite" => json_library_page_from_sqlite(s, filter, sort, search, offset, limit),
        "tsv" => json_library_page_from_tsv(s, filter, sort, search, offset, limit),
        other => anyhow::bail!("unknown library source: {}", other),
    }
}

fn validate_library_filter(filter: &str) -> anyhow::Result<&str> {
    match filter {
        "all" | "image" | "gif" | "video" | "we_scene" | "we_web" | "unsupported" => Ok(filter),
        other => anyhow::bail!("unknown library filter: {}", other),
    }
}

fn validate_library_sort(sort: &str) -> anyhow::Result<&str> {
    match sort {
        "newest" | "largest" | "name" => Ok(sort),
        other => anyhow::bail!("unknown library sort: {}", other),
    }
}

fn json_library_page_from_sqlite(
    s: &StorageApi,
    filter: &str,
    sort: &str,
    search: &str,
    offset: usize,
    limit: usize,
) -> anyhow::Result<()> {
    use rusqlite::{params, Connection};

    let filter = validate_library_filter(filter)?;
    let sort = validate_library_sort(sort)?;
    let db = s.cd.db_path();
    if !db.exists() {
        let conn = Connection::open(&db)?;
        wc_storage::sqlite::create_schema(&conn)?;
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "total": 0,
                "items": []
            }))?
        );
        return Ok(());
    }
    let conn = Connection::open(&db)?;
    wc_storage::sqlite::ensure_wallpaper_query_indexes(&conn)?;
    let order_by = match sort {
        "newest" => "mtime DESC, path ASC",
        "largest" => "size DESC, path ASC",
        "name" => "path ASC",
        _ => unreachable!(),
    };
    let where_clause = "WHERE (?1 = 'all' OR type = ?1)
        AND (?2 = ''
          OR lower(path) LIKE '%' || lower(?2) || '%'
          OR lower(title) LIKE '%' || lower(?2) || '%'
          OR lower(workshop_id) LIKE '%' || lower(?2) || '%'
          OR lower(project_type) LIKE '%' || lower(?2) || '%')";

    let total: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM wallpapers {}", where_clause),
        params![filter, search],
        |row| row.get(0),
    )?;

    let sql = format!(
        "SELECT path, type, ext, backend, size, mtime, resolution,
                project_type, preview_path, workshop_id, title, we_file, unsupported_reason
         FROM wallpapers {} ORDER BY {} LIMIT ?3 OFFSET ?4",
        where_clause, order_by
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows: Result<Vec<serde_json::Value>, rusqlite::Error> = stmt
        .query_map(
            params![filter, search, limit as i64, offset as i64],
            json_from_sql_row,
        )?
        .collect();
    let items = rows?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "total": total,
            "items": items,
        }))?
    );
    Ok(())
}

fn json_library_page_from_tsv(
    s: &StorageApi,
    filter: &str,
    sort: &str,
    search: &str,
    offset: usize,
    limit: usize,
) -> anyhow::Result<()> {
    let filter = validate_library_filter(filter)?;
    let _sort = validate_library_sort(sort)?;
    let (total, rows) = wc_storage::tsv::tsv_bounded_page(
        &s.cd.library_tsv_path(),
        filter,
        sort,
        search,
        offset,
        limit,
    );
    let items: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|r| {
            serde_json::json!({
                "path": r.path,
                "type": r.ftype,
                "ext": r.ext,
                "backend": r.backend,
                "size": r.size,
                "mtime": r.mtime,
                "resolution": r.resolution,
            })
        })
        .collect();
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "total": total,
            "items": items,
        }))?
    );
    Ok(())
}
