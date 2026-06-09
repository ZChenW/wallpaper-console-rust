//! wc-cli — wallpaper-console Rust CLI (clap command dispatch).

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use wc_core::config::ConfigDir;
use wc_core::formats;
use wc_storage::{flat, mirror, sqlite};

#[derive(Parser)]
#[command(name = "wallpaper-console-rust", version = "0.1.0")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Apply a wallpaper file
    Apply { file: String },

    /// Stop all wallpaper backends
    Stop,

    /// Show current wallpaper state
    Status,

    /// Restore the last applied wallpaper
    Restore,

    // ── Sources ──────────────────────────────────────────────────────
    /// Add a source directory
    Add { dir: String },

    /// List configured source directories
    Sources,

    /// Remove missing source directories
    RemoveMissing,

    /// Deduplicate source entries
    DedupeSources,

    // ── Favorites ────────────────────────────────────────────────────
    /// Add a file to favorites
    FavoriteAdd { file: String },

    /// List favorites
    Favorites,

    /// Remove a favorite
    FavoriteRemove { file: Option<String> },

    // ── History ──────────────────────────────────────────────────────
    /// List history
    History,

    /// Clear history
    HistoryClear,

    // ── Config ───────────────────────────────────────────────────────
    /// Get a config value
    ConfigGet {
        key: String,
        default: Option<String>,
    },

    /// Set a config value
    ConfigSet { key: String, value: Vec<String> },

    // ── Library ──────────────────────────────────────────────────────
    /// Rescan library
    Rescan,

    /// List library
    Library,

    /// Library counts
    LibraryCount,

    /// Library as JSON
    LibraryJson {
        #[arg(long)]
        tsv: bool,
        #[arg(long)]
        sqlite: bool,
    },

    /// Favorites as JSON
    FavoritesJson,

    /// History as JSON
    HistoryJson,

    // ── SQLite ───────────────────────────────────────────────────────
    /// Import flat files into wallpapers.db
    MigrateToSqlite,

    /// Compare flat files against wallpapers.db
    SqliteVerify,

    /// Re-import flat files into wallpapers.db
    SqliteResync,

    /// Export wallpapers.db back to flat files
    SqliteExportFlat,

    /// Backup wallpapers.db
    SqliteBackup,

    /// Restore wallpapers.db from backup
    SqliteRestore { backup: String },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        None => {
            print_help();
            return Ok(());
        }

        Some(cmd) => {
            let cd = ConfigDir::new()?;
            cd.init()?;
            run_command(cmd, &cd)?;
        }
    }
    Ok(())
}

fn run_command(cmd: Commands, cd: &ConfigDir) -> anyhow::Result<()> {
    match cmd {
        Commands::Apply { file } => {
            let ext = formats::get_extension(&file)
                .ok_or_else(|| anyhow::anyhow!("unsupported file: {}", file))?;
            let (_ftype, backend) = formats::classify_extension(&ext)
                .ok_or_else(|| anyhow::anyhow!("unsupported file: {}", file))?;
            wc_backend::apply_wallpaper(cd, &file, backend)?;
            println!("Applied: {}", file);
        }

        Commands::Stop => {
            wc_backend::stop_all_backends()?;
            println!("All wallpaper backends stopped.");
        }

        Commands::Status => {
            let cur = flat::current_read(cd)?.unwrap_or_else(|| "(none)".into());
            let be = flat::last_backend_read(cd)?.unwrap_or_else(|| "(none)".into());
            let src_count = flat::sources_list(cd)?.len();
            println!("config directory:    {}", cd.path.display());
            println!("current wallpaper:   {}", cur);
            println!("last backend:        {}", be);
            println!("configured sources:  {}", src_count);
        }

        Commands::Restore => {
            wc_backend::restore(cd)?;
            println!("Wallpaper restored.");
        }

        // Sources
        Commands::Add { dir } => {
            let canonical = std::fs::canonicalize(&dir)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or(dir);
            flat::sources_add(cd, &canonical)?;
            mirror::mirror_source_add(cd, &canonical).ok();
            println!("Added source: {}", canonical);
        }

        Commands::Sources => {
            let srcs = flat::sources_list(cd)?;
            if srcs.is_empty() {
                println!("(no source directories configured)");
            } else {
                for s in &srcs {
                    println!("{}", s);
                }
            }
        }

        Commands::RemoveMissing => {
            let sources = flat::sources_list(cd)?;
            let mut remaining = Vec::new();
            let mut removed = 0;
            for s in &sources {
                if std::path::Path::new(s).is_dir() {
                    remaining.push(s.clone());
                } else {
                    println!("Removed missing source: {}", s);
                    mirror::mirror_source_remove(cd, s).ok();
                    removed += 1;
                }
            }
            flat::write_lines(&cd.sources_path(), &remaining)?;
            println!("Removed {} missing source(s).", removed);
        }

        Commands::DedupeSources => {
            flat::dedupe_file(&cd.sources_path())?;
            println!("Sources deduplicated.");
        }

        // Favorites
        Commands::FavoriteAdd { file } => {
            let added = flat::favorites_add(cd, &file)?;
            if added {
                mirror::mirror_favorite_add(cd, &file).ok();
                println!("Added to favorites: {}", file);
            } else {
                println!("Already in favorites");
            }
        }

        Commands::Favorites => {
            for f in flat::favorites_list(cd)? {
                println!("{}", f);
            }
        }

        Commands::FavoriteRemove { file } => {
            if let Some(path) = file {
                flat::favorites_remove(cd, &path)?;
                mirror::mirror_favorite_remove(cd, &path).ok();
                println!("Removed favorite: {}", path);
            } else {
                anyhow::bail!("interactive favorite-remove not implemented (pass FILE)");
            }
        }

        // History
        Commands::History => {
            for h in flat::history_list(cd)? {
                println!("{}", h);
            }
        }

        Commands::HistoryClear => {
            flat::history_clear(cd)?;
            mirror::mirror_history_clear(cd).ok();
            println!("History cleared.");
        }

        // Config
        Commands::ConfigGet { key, default } => {
            let val =
                wc_core::config::read_config_value(&cd.path, &key, &default.unwrap_or_default());
            println!("{}", val);
        }

        Commands::ConfigSet { key, value } => {
            let val = value.join(" ");
            wc_core::config::write_config_value(&cd.path, &key, &val)?;
            mirror::mirror_config_set(cd, &key, &val).ok();
            println!("{} = {}", key, val);
        }

        // Library
        Commands::Rescan => {
            let sources = flat::sources_list(cd)?;
            if sources.is_empty() {
                println!("(no sources configured)");
                return Ok(());
            }
            // Clear SQLite wallpapers if DB exists
            sqlite::library_clear(cd).ok();

            let files = wc_scan::scan_wallpapers(&sources);
            let mut entries: Vec<wc_core::types::WallpaperEntry> = Vec::new();
            for path in &files {
                if let Some(entry) = wc_scan::make_entry(path) {
                    sqlite::library_insert(
                        cd,
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

            // Write library.tsv
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
            std::fs::write(cd.library_tsv_path(), tsv)?;
            // Clear dirty flag
            let dirty = cd.path.join("library.dirty");
            if dirty.exists() {
                std::fs::remove_file(&dirty).ok();
            }
            println!(
                "library.tsv written ({} entries)  SQLite: {} wallpapers",
                entries.len(),
                sqlite::library_count(cd).unwrap_or(0)
            );
        }

        Commands::Library => {
            let content = std::fs::read_to_string(cd.library_tsv_path()).unwrap_or_default();
            print!("{}", content);
        }

        Commands::LibraryCount => {
            let content = std::fs::read_to_string(cd.library_tsv_path()).unwrap_or_default();
            let mut total = 0;
            let mut images = 0;
            let mut gifs = 0;
            let mut videos = 0;
            for line in content.lines() {
                if line.is_empty() {
                    continue;
                }
                total += 1;
                let ftype = line.split('\t').next().unwrap_or("");
                match ftype {
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

        Commands::LibraryJson {
            tsv: _tsv,
            sqlite: use_sqlite,
        } => {
            if use_sqlite {
                json_library_from_sqlite(cd)?;
            } else {
                json_library_from_tsv(cd)?;
            }
        }

        Commands::FavoritesJson => {
            let favs = flat::favorites_list(cd)?;
            let json = serde_json::to_string_pretty(&favs)?;
            println!("{}", json);
        }

        Commands::HistoryJson => {
            let hist: Vec<serde_json::Value> = flat::history_list(cd)?
                .into_iter()
                .map(|p| serde_json::json!({"path": p}))
                .collect();
            println!("{}", serde_json::to_string_pretty(&hist)?);
        }

        // SQLite
        Commands::MigrateToSqlite => {
            sqlite::migrate_to_sqlite(cd)?;
            println!("Migrated to: {}", cd.db_path().display());
            println!("Flat files unchanged. storage_backend remains 'file'.");
        }

        Commands::SqliteVerify => match sqlite::verify(cd) {
            Ok(()) => println!("VERIFY OK: flat files and SQLite are consistent."),
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
            sqlite::resync(cd)?;
            println!("Resync complete.");
            sqlite::verify(cd)?;
        }

        Commands::SqliteExportFlat => {
            sqlite::export_flat(cd)?;
            println!("Export complete.");
        }

        Commands::SqliteBackup => {
            let bak = sqlite::backup(cd)?;
            println!("{}", bak);
        }

        Commands::SqliteRestore { backup } => {
            sqlite::restore(cd, &PathBuf::from(&backup))?;
            println!("Restored: {}", cd.db_path().display());
            sqlite::verify(cd)?;
        }
    }
    Ok(())
}

fn print_help() {
    println!(concat!(
        "wallpaper-console-rust\n",
        "Usage: wallpaper-console-rust [COMMAND]\n\n",
        "Wallpaper:\n",
        "  apply FILE       Apply a wallpaper file\n",
        "  stop             Stop all wallpaper backends\n",
        "  status           Show current wallpaper state\n",
        "  restore          Restore the last applied wallpaper\n\n",
        "Sources:\n",
        "  add DIR          Add a source directory\n",
        "  sources          List configured sources\n",
        "  remove-missing   Remove missing source directories\n",
        "  dedupe-sources   Remove duplicate source entries\n\n",
        "Favorites:\n",
        "  favorite-add FILE          Add a file to favorites\n",
        "  favorites                  List favorites\n",
        "  favorite-remove [FILE]     Remove a favorite\n\n",
        "History:\n",
        "  history                    List history\n",
        "  history-clear              Clear history\n\n",
        "Config:\n",
        "  config-get KEY [DEFAULT]   Get a config value\n",
        "  config-set KEY VALUE...    Set a config value\n\n",
        "Library:\n",
        "  rescan                     Rebuild library index\n",
        "  library                    Print library index\n",
        "  library-count              Print library counts\n",
        "  library-json [--tsv|--sqlite]  Output library as JSON\n",
        "  favorites-json             Output favorites as JSON\n",
        "  history-json               Output history as JSON\n\n",
        "SQLite:\n",
        "  migrate-to-sqlite          Import flat files into wallpapers.db\n",
        "  sqlite-verify              Compare flat files against DB\n",
        "  sqlite-resync              Re-import flat files into DB\n",
        "  sqlite-export-flat         Export DB back to flat files\n",
        "  sqlite-backup              Backup wallpapers.db\n",
        "  sqlite-restore BACKUP      Restore from a backup file\n",
    ));
}

fn json_library_from_tsv(cd: &ConfigDir) -> anyhow::Result<()> {
    let content = std::fs::read_to_string(cd.library_tsv_path()).unwrap_or_default();
    let mut entries: Vec<serde_json::Value> = Vec::new();
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 7 {
            continue;
        }
        entries.push(serde_json::json!({
            "path": parts[6],
            "type": parts[0],
            "ext": parts[1],
            "backend": parts[2],
            "size": parts[3].parse::<u64>().unwrap_or(0),
            "mtime": parts[4].parse::<u64>().unwrap_or(0),
            "resolution": parts[5],
        }));
    }
    println!("{}", serde_json::to_string_pretty(&entries)?);
    Ok(())
}

fn json_library_from_sqlite(cd: &ConfigDir) -> anyhow::Result<()> {
    use rusqlite::Connection;
    let db = cd.db_path();
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
