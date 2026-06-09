//! wc-cli — wallpaper-console Rust CLI (clap command dispatch).

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use wc_core::config::ConfigDir;
use wc_core::formats;
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
    Search,
    #[command(name = "search-source")]
    SearchSource,
    #[command(name = "search-type")]
    SearchType,
    #[command(name = "sort-mtime")]
    SortMtime,
    #[command(name = "sort-size")]
    SortSize,
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

    // ── System ───────────────────────────────────────────────────────
    Tui,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        None => {
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
            // Safety: check file exists
            let p = std::path::Path::new(&file);
            if !p.is_file() {
                anyhow::bail!("not a regular file: {}", file);
            }
            let ext = formats::get_extension(&file).ok_or_else(|| {
                anyhow::anyhow!("unsupported file type: .{} ({}", "unknown", file)
            })?;
            let (ftype, _default_backend) = formats::classify_extension(&ext)
                .ok_or_else(|| anyhow::anyhow!("unsupported file: {}", file))?;
            // Config-driven backend routing
            let backend = match ftype {
                wc_core::types::FileType::Image => {
                    let be = s.config_get("image_backend", "awww");
                    match be.as_str() {
                        "mpvpaper" => Backend::Mpvpaper,
                        _ => Backend::Awww,
                    }
                }
                wc_core::types::FileType::Gif => {
                    let be = s.config_get("gif_backend", "awww");
                    match be.as_str() {
                        "mpvpaper" => Backend::Mpvpaper,
                        _ => Backend::Awww,
                    }
                }
                wc_core::types::FileType::Video => {
                    let be = s.config_get("video_backend", "mpvpaper");
                    match be.as_str() {
                        "awww" => Backend::Awww,
                        _ => Backend::Mpvpaper,
                    }
                }
            };
            wc_backend::apply_wallpaper(s, &file, backend)?;
            println!("Applied: {}", file);
        }

        Commands::Stop => {
            wc_backend::stop_all_backends()?;
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

        // Browse / Random (non-interactive: pipe to fzf)
        Commands::Browse
        | Commands::BrowseAll
        | Commands::BrowseImages
        | Commands::BrowseGifs
        | Commands::BrowseVideos => {
            let filter = match &cmd {
                Commands::BrowseImages => Some(wc_core::types::FileType::Image),
                Commands::BrowseGifs => Some(wc_core::types::FileType::Gif),
                Commands::BrowseVideos => Some(wc_core::types::FileType::Video),
                _ => None,
            };
            let paths = library_paths(s, filter)?;
            for p in &paths {
                println!("{}", p);
            }
        }

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
            // Pick random
            let idx = (std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos() as usize)
                % paths.len();
            let chosen = &paths[idx];
            let ext = formats::get_extension(chosen).unwrap_or_default();
            let (ftype, _) = formats::classify_extension(&ext)
                .ok_or_else(|| anyhow::anyhow!("unsupported: {}", chosen))?;
            let backend = config_backend_for_type(s, ftype);
            wc_backend::apply_wallpaper(s, chosen, backend)?;
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
            // Non-interactive: pick first (for tests); TUI uses fzf
            s.sources_remove(&paths[0])?;
            println!("Removed source: {}", paths[0]);
        }

        Commands::RemoveSource { dir } => {
            let canonical = std::fs::canonicalize(&dir)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or(dir.clone());
            let removed = s.sources_remove(&canonical)?;
            if removed {
                println!("Removed source: {}", canonical);
            } else {
                anyhow::bail!("source not found: {}", dir);
            }
        }

        Commands::SteamWorkshop => {
            let home = std::env::var("HOME").unwrap_or_default();
            for base in &[
                format!("{}/.steam/steam", home),
                format!("{}/.local/share/Steam", home),
            ] {
                let ws = std::path::Path::new(base).join("steamapps/workshop/content/431960");
                if ws.is_dir() {
                    for entry in std::fs::read_dir(&ws)? {
                        let entry = entry?;
                        if entry.file_type()?.is_dir() {
                            s.sources_add(&entry.path().to_string_lossy())?;
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
            for f in s.favorites_list()? {
                println!("{}", f);
            }
        }

        Commands::FavoriteRandom => {
            let favs = s.favorites_list()?;
            if favs.is_empty() {
                anyhow::bail!("no favorites configured");
            }
            let idx = rand::random::<usize>() % favs.len();
            let chosen = &favs[idx];
            let ext = formats::get_extension(chosen).unwrap_or_default();
            let (ftype, _) = formats::classify_extension(&ext)
                .ok_or_else(|| anyhow::anyhow!("unsupported: {}", chosen))?;
            let backend = config_backend_for_type(s, ftype);
            wc_backend::apply_wallpaper(s, chosen, backend)?;
        }

        Commands::FavoriteRemove { file } => {
            if let Some(path) = file {
                s.favorites_remove(&path)?;
                println!("Removed favorite: {}", path);
            } else {
                // Interactive fzf
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
            for h in s.history_list()? {
                println!("{}", h);
            }
        }

        Commands::HistoryRandom => {
            let hist = s.history_list()?;
            if hist.is_empty() {
                anyhow::bail!("no history entries");
            }
            let idx = rand::random::<usize>() % hist.len();
            let chosen = &hist[idx];
            let ext = formats::get_extension(chosen).unwrap_or_default();
            let (ftype, _) = formats::classify_extension(&ext)
                .ok_or_else(|| anyhow::anyhow!("unsupported: {}", chosen))?;
            let backend = config_backend_for_type(s, ftype);
            wc_backend::apply_wallpaper(s, chosen, backend)?;
        }

        Commands::HistoryClear => {
            s.history_clear()?;
            println!("History cleared.");
        }

        // ── Search / Sort ────────────────────────────────────────────
        Commands::Search
        | Commands::SearchSource
        | Commands::SearchType
        | Commands::SortMtime
        | Commands::SortSize
        | Commands::SortName => {
            let mut entries = library_entries(s)?;
            match &cmd {
                Commands::SortMtime => entries.sort_by_key(|e| std::cmp::Reverse(e.mtime)),
                Commands::SortSize => entries.sort_by_key(|e| std::cmp::Reverse(e.size)),
                Commands::SortName => entries.sort_by(|a, b| a.filename().cmp(b.filename())),
                _ => {}
            }
            for e in &entries {
                println!("{}", e.path);
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
            let sources = s.sources_list()?;
            if sources.is_empty() {
                println!("(no sources configured)");
                return Ok(());
            }
            wc_storage::sqlite::library_clear(&s.cd).ok();
            let files = wc_scan::scan_wallpapers(&sources);
            let mut entries: Vec<wc_core::types::WallpaperEntry> = Vec::new();
            for path in &files {
                if let Some(entry) = wc_scan::make_entry(path) {
                    wc_storage::sqlite::library_insert(
                        &s.cd,
                        path,
                        entry.file_type.as_str(),
                        &entry.ext,
                        entry.backend.as_str(),
                        entry.size,
                        entry.mtime,
                        &entry.resolution,
                    )
                    .ok();
                    entries.push(entry);
                }
            }
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
            std::fs::write(s.cd.library_tsv_path(), tsv)?;
            let dirty = s.cd.path.join("library.dirty");
            if dirty.exists() {
                std::fs::remove_file(&dirty).ok();
            }
            println!(
                "library.tsv written ({} entries)  SQLite: {} wallpapers",
                entries.len(),
                wc_storage::sqlite::library_count(&s.cd).unwrap_or(0)
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
            for e in library_entries(s)? {
                println!("{}", e.path);
            }
        }

        Commands::RandomLibrary => {
            let entries = library_entries(s)?;
            if entries.is_empty() {
                anyhow::bail!("library is empty");
            }
            let idx = rand::random::<usize>() % entries.len();
            let e = &entries[idx];
            let (_, _backend) = formats::classify_extension(&e.ext)
                .ok_or_else(|| anyhow::anyhow!("unsupported: {}", e.path))?;
            let backend = config_backend_for_type(s, e.file_type);
            wc_backend::apply_wallpaper(s, e.path.as_ref(), backend)?;
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
            Ok(()) => println!("VERIFY OK"),
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

        Commands::Tui => {
            println!("TUI not yet implemented in Rust — use the Bash wallpaper-console for TUI.");
        }
    }
    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────

fn config_backend_for_type(s: &StorageApi, ftype: wc_core::types::FileType) -> Backend {
    match ftype {
        wc_core::types::FileType::Image => match s.config_get("image_backend", "awww").as_str() {
            "mpvpaper" => Backend::Mpvpaper,
            _ => Backend::Awww,
        },
        wc_core::types::FileType::Gif => match s.config_get("gif_backend", "awww").as_str() {
            "mpvpaper" => Backend::Mpvpaper,
            _ => Backend::Awww,
        },
        wc_core::types::FileType::Video => {
            match s.config_get("video_backend", "mpvpaper").as_str() {
                "awww" => Backend::Awww,
                _ => Backend::Mpvpaper,
            }
        }
    }
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
                _ => wc_core::types::FileType::Image,
            },
            ext: parts[1].to_string(),
            backend: match parts[2] {
                "mpvpaper" => Backend::Mpvpaper,
                _ => Backend::Awww,
            },
            size: parts[3].parse().unwrap_or(0),
            mtime: parts[4].parse().unwrap_or(0),
            resolution: parts[5].to_string(),
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

fn fzf_select(items: &[String], prompt: &str) -> anyhow::Result<Option<String>> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new("fzf")
        .arg("--prompt")
        .arg(prompt)
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
        "  add DIR              sources             remove-source DIR\n",
        "  steam-workshop       validate-sources     remove-missing\n",
        "  dedupe-sources\n",
        "  favorite-add FILE    favorite-add-current favorites\n",
        "  favorite-random      favorite-remove [FILE]\n",
        "  history              history-random      history-clear\n",
        "  search               search-source       search-type\n",
        "  sort-mtime           sort-size            sort-name\n",
        "  config-get KEY       config-set KEY VAL\n",
        "  rescan               library              library-count\n",
        "  browse-library       random-library       library-json [--tsv|--sqlite]\n",
        "  favorites-json       history-json\n",
        "  migrate-to-sqlite    sqlite-verify        sqlite-resync\n",
        "  sqlite-export-flat   sqlite-backup         sqlite-restore BACKUP\n",
        "  tui\n",
    ));
}

fn json_library_from_tsv(s: &StorageApi) -> anyhow::Result<()> {
    let entries = library_entries(s)?;
    let json: Vec<serde_json::Value> = entries
        .iter()
        .map(|e| {
            serde_json::json!({
                "path": e.path.to_string(),
                "type": e.file_type.as_str(),
                "ext": e.ext,
                "backend": e.backend.as_str(),
                "size": e.size,
                "mtime": e.mtime,
                "resolution": e.resolution,
            })
        })
        .collect();
    println!("{}", serde_json::to_string_pretty(&json)?);
    Ok(())
}

fn json_library_from_sqlite(s: &StorageApi) -> anyhow::Result<()> {
    use rusqlite::Connection;
    let db = s.cd.db_path();
    if !db.exists() {
        anyhow::bail!("wallpapers.db not found. Run migrate-to-sqlite and rescan first.");
    }
    let conn = Connection::open(&db)?;
    let mut stmt = conn.prepare(
        "SELECT path, type, ext, backend, size, mtime, resolution FROM wallpapers ORDER BY path",
    )?;
    let rows: Vec<serde_json::Value> = stmt
        .query_map([], |row: &rusqlite::Row<'_>| {
            Ok(serde_json::json!({
                "path": row.get::<_, String>(0)?,
                "type": row.get::<_, String>(1)?,
                "ext": row.get::<_, String>(2)?,
                "backend": row.get::<_, String>(3)?,
                "size": row.get::<_, i64>(4)?,
                "mtime": row.get::<_, i64>(5)?,
                "resolution": row.get::<_, String>(6)?,
            }))
        })?
        .filter_map(|r: Result<serde_json::Value, rusqlite::Error>| r.ok())
        .collect();
    println!("{}", serde_json::to_string_pretty(&rows)?);
    Ok(())
}
