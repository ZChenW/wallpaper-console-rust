use std::io::{BufWriter, Write};
use std::time::Duration;

use wc_core::types::{Backend, WallpaperEntry};
use wc_storage::StorageApi;

use crate::output::{json_library_from_sqlite, json_library_from_tsv, json_library_page, write_library_tsv_entry};
use crate::Commands;

const RESCAN_BATCH_SIZE: usize = 250;

pub(crate) fn run(cmd: Commands, s: &StorageApi) -> anyhow::Result<()> {
    match cmd {
        Commands::Rescan => rescan(s),
        Commands::Library => {
            let content = std::fs::read_to_string(s.cd.library_tsv_path()).unwrap_or_default();
            print!("{}", content);
            Ok(())
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
            Ok(())
        }
        Commands::BrowseLibrary => {
            let entries = library_entries(s)?;
            if entries.is_empty() {
                anyhow::bail!("library is empty — run rescan first");
            }
            let paths: Vec<String> = entries.iter().map(|e| e.path.to_string()).collect();
            let selection = crate::wallpaper::fzf_select(&paths, "browse-library> ")?;
            if let Some(path) = selection {
                crate::wallpaper::apply_selected(s, &path)?;
            }
            Ok(())
        }
        Commands::RandomLibrary => {
            let entries = library_entries(s)?;
            if entries.is_empty() {
                anyhow::bail!("library is empty");
            }
            let idx = rand::random::<usize>() % entries.len();
            let e = &entries[idx];
            crate::wallpaper::apply_selected(s, e.path.as_ref())?;
            Ok(())
        }
        Commands::LibraryJson {
            tsv: _tsv,
            sqlite: use_sqlite,
        } => {
            if use_sqlite {
                json_library_from_sqlite(s)
            } else {
                json_library_from_tsv(s)
            }
        }
        Commands::LibraryPageJson {
            source,
            filter,
            sort,
            search,
            offset,
            limit,
        } => json_library_page(s, &source, &filter, &sort, &search, offset, limit),
        Commands::FavoritesJson => {
            let favs = s.favorites_list()?;
            println!("{}", serde_json::to_string_pretty(&favs)?);
            Ok(())
        }
        Commands::HistoryJson => {
            let hist: Vec<serde_json::Value> = s
                .history_list()?
                .into_iter()
                .map(|p| serde_json::json!({"path": p}))
                .collect();
            println!("{}", serde_json::to_string_pretty(&hist)?);
            Ok(())
        }
        _ => unreachable!("library::run called with non-library command"),
    }
}

fn rescan(s: &StorageApi) -> anyhow::Result<()> {
    let t0 = std::time::Instant::now();
    let raw_sources = s.sources_list()?;
    if raw_sources.is_empty() {
        println!("(no sources configured)");
        return Ok(());
    }
    // Dedupe by canonical path (catches symlinks, .steam vs .local/share).
    let sources = wc_scan::dedupe_sources(&raw_sources);
    let dup_count = raw_sources.len() - sources.len();

    // Load prior metadata to skip unchanged files.
    let prior_cache = wc_scan::prior_metadata_cache(&s.cd.library_tsv_path());

    let scan_start = std::time::Instant::now();
    let mut metadata_time = Duration::ZERO;
    let mut candidate_count = 0usize;
    let mut entry_count = 0usize;
    let mut reused = 0usize;
    let mut probed = 0usize;
    let mut batch: Vec<WallpaperEntry> = Vec::with_capacity(RESCAN_BATCH_SIZE);
    let tsv_path = s.cd.library_tsv_path();
    let tsv_tmp = tsv_path.with_extension("tsv.tmp");
    let tsv_file = std::fs::File::create(&tsv_tmp)?;
    let mut tsv_writer = BufWriter::new(tsv_file);
    let mut sqlite_session = wc_storage::sqlite::library_replace_session_start(&s.cd)?;
    let mut stream_error: Option<anyhow::Error> = None;

    wc_scan::visit_wallpapers_with_callback(
        &sources,
        |_| wc_scan::ScanControl::Continue,
        |path| {
            candidate_count += 1;
            let probe_start = std::time::Instant::now();
            let (entry, was_reused) = wc_scan::make_entry_cached(&path, &prior_cache);
            metadata_time += probe_start.elapsed();

            let Some(entry) = entry else {
                return wc_scan::ScanVisitControl::Continue;
            };

            if was_reused {
                reused += 1;
            } else {
                probed += 1;
            }
            entry_count += 1;

            if let Err(err) = write_library_tsv_entry(&mut tsv_writer, &entry) {
                stream_error = Some(err.into());
                return wc_scan::ScanVisitControl::Cancel;
            }
            batch.push(entry);
            if batch.len() >= RESCAN_BATCH_SIZE {
                if let Err(err) =
                    wc_storage::sqlite::library_replace_session_push(&mut sqlite_session, &batch)
                {
                    stream_error = Some(err.into());
                    return wc_scan::ScanVisitControl::Cancel;
                }
                batch.clear();
            }
            wc_scan::ScanVisitControl::Continue
        },
    );
    let scan_time = scan_start.elapsed();
    let walk_time = scan_time.checked_sub(metadata_time).unwrap_or_default();
    let probe_time = metadata_time;

    if let Some(err) = stream_error {
        let _ = wc_storage::sqlite::library_replace_session_abort(sqlite_session);
        let _ = std::fs::remove_file(&tsv_tmp);
        return Err(err);
    }

    if !batch.is_empty() {
        if let Err(err) =
            wc_storage::sqlite::library_replace_session_push(&mut sqlite_session, &batch)
        {
            let _ = wc_storage::sqlite::library_replace_session_abort(sqlite_session);
            let _ = std::fs::remove_file(&tsv_tmp);
            return Err(err.into());
        }
    }
    if let Err(err) = tsv_writer.flush() {
        let _ = wc_storage::sqlite::library_replace_session_abort(sqlite_session);
        let _ = std::fs::remove_file(&tsv_tmp);
        return Err(err.into());
    }
    drop(tsv_writer);

    // Atomically replace the SQLite library when staging succeeded.
    let t2 = std::time::Instant::now();
    let sqlite_count = match wc_storage::sqlite::library_replace_session_commit(sqlite_session) {
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
        candidate_count,
        entry_count,
        sqlite_count,
        reused,
        probed,
        walk_time.as_secs_f64(),
        probe_time.as_secs_f64(),
        sqlite_time.as_millis(),
        total_time.as_secs_f64(),
    );
    Ok(())
}

pub(crate) fn library_entries(s: &StorageApi) -> anyhow::Result<Vec<WallpaperEntry>> {
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
        entries.push(WallpaperEntry {
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

pub(crate) fn library_paths(
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
