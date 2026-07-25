use std::io::{BufWriter, Write};

#[cfg(test)]
use wc_config::ConfigDirExt;
use wc_core::types::{Backend, WallpaperEntry};
use wc_storage::StorageApi;

use crate::output::{
    json_from_entry, json_library_from_sqlite, json_library_from_tsv, json_library_page,
    write_library_tsv_entry,
};
use crate::Commands;

pub(crate) fn run(cmd: Commands, s: &StorageApi) -> anyhow::Result<()> {
    match cmd {
        Commands::Rescan => rescan(s),
        Commands::Library => {
            let entries = library_entries(s)?;
            let stdout = std::io::stdout();
            let mut writer = BufWriter::new(stdout.lock());
            for entry in &entries {
                write_library_tsv_entry(&mut writer, entry)?;
            }
            writer.flush().map_err(Into::into)
        }
        Commands::LibraryCount => {
            let entries = library_entries(s)?;
            let mut total = 0;
            let mut images = 0;
            let mut gifs = 0;
            let mut videos = 0;
            for entry in entries {
                total += 1;
                match entry.file_type.as_str() {
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
                json_source_backed_library(s)
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
        } => json_cli_library_page(s, &source, &filter, &sort, &search, offset, limit),
        Commands::FavoritesJson => {
            let favs = s.favorites_list()?;
            println!("{}", serde_json::to_string_pretty(&favs)?);
            Ok(())
        }
        _ => unreachable!("library::run called with non-library command"),
    }
}

fn rescan(s: &StorageApi) -> anyhow::Result<()> {
    let t0 = std::time::Instant::now();
    let report =
        wc_app::library_rescan::run_library_rescan(s, |_, _| wc_scan::ScanControl::Continue)
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    if let wc_app::library_refresh_round::LegacyProjectionStatus::Degraded { message } =
        &report.projection
    {
        eprintln!(
            "Warning: SQLite refresh completed, but legacy library.tsv projection is stale: {message}"
        );
    }

    if report.source_count == 0 {
        println!(
            "(no sources configured; SQLite snapshot: {})",
            report.snapshot_count
        );
        return Ok(());
    }

    let refresh = report
        .refresh
        .as_ref()
        .expect("rescan with sources always includes a refresh report");
    let total_time = t0.elapsed();
    println!(
        "{}",
        format_rescan_summary(
            report.source_count,
            refresh,
            report.snapshot_count,
            report.refresh_time,
            report.snapshot_time,
            total_time,
        )
    );
    Ok(())
}

fn format_rescan_summary(
    source_count: usize,
    report: &wc_app::library_refresh::LibraryRefreshReport,
    sqlite_count: usize,
    refresh_time: std::time::Duration,
    snapshot_time: std::time::Duration,
    total_time: std::time::Duration,
) -> String {
    format!(
        "sources: {} (complete: {}, offline preserved: {}, incomplete preserved: {})  \
         candidates: {} files  entries: {}  sqlite: {}\n\
         visited: {}  source_entries_indexed: {}  reused_metadata: {}  \
         probed_metadata: {}  removed: {}\n\
         refresh: {:.2}s  snapshot: {}ms  total: {:.2}s",
        source_count,
        report.complete_sources,
        report.offline_sources,
        report.incomplete_sources,
        report.metadata.candidates_found,
        sqlite_count,
        sqlite_count,
        report.metadata.entries_visited,
        report.indexed,
        report.metadata.metadata_reused,
        report
            .metadata
            .entries_indexed
            .saturating_sub(report.metadata.metadata_reused),
        report.wallpapers_removed,
        refresh_time.as_secs_f64(),
        snapshot_time.as_millis(),
        total_time.as_secs_f64(),
    )
}

fn sqlite_library_snapshot(s: &StorageApi) -> anyhow::Result<Vec<WallpaperEntry>> {
    wc_app::library_rescan::ensure_dirty_sqlite_is_readable(s)
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    let total = wc_storage::sqlite::source_backed_library_count(&s.cd)?;
    if total == 0 {
        return Ok(Vec::new());
    }
    let page = wc_storage::sqlite::source_backed_library_page_sqlite(
        &s.cd,
        &wc_storage::sqlite::LibraryPageQuery {
            filter: wc_storage::sqlite::LibraryFilter::All,
            sort: wc_storage::sqlite::LibrarySort::Name,
            search: String::new(),
            offset: 0,
            limit: total,
        },
    )?;
    if page.total != total || page.items.len() != total {
        anyhow::bail!(
            "SQLite library snapshot changed while reading (expected {total}, found {} of {})",
            page.items.len(),
            page.total
        );
    }
    Ok(page.items)
}

fn dirty_marker_path(s: &StorageApi) -> std::path::PathBuf {
    wc_app::library_rescan::library_dirty_marker_path(s)
}

pub(crate) fn library_entries(s: &StorageApi) -> anyhow::Result<Vec<WallpaperEntry>> {
    if dirty_marker_path(s).exists() || s.cd.db_path().exists() {
        return sqlite_library_snapshot(s);
    }
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

fn json_source_backed_library(s: &StorageApi) -> anyhow::Result<()> {
    wc_app::library_rescan::ensure_dirty_sqlite_is_readable(s)
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    if !s.cd.db_path().exists() {
        return json_library_from_sqlite(s);
    }
    let entries = sqlite_library_snapshot(s)?;
    let json: Vec<serde_json::Value> = entries.iter().map(json_from_entry).collect();
    println!("{}", serde_json::to_string_pretty(&json)?);
    Ok(())
}

fn json_source_backed_library_page(
    s: &StorageApi,
    filter: &str,
    sort: &str,
    search: &str,
    offset: usize,
    limit: usize,
) -> anyhow::Result<()> {
    wc_app::library_rescan::ensure_dirty_sqlite_is_readable(s)
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    let query = wc_storage::sqlite::LibraryPageQuery {
        filter: wc_storage::sqlite::LibraryFilter::parse(filter)?,
        sort: wc_storage::sqlite::LibrarySort::parse(sort)?,
        search: search.to_string(),
        offset,
        limit,
    };
    let page = wc_storage::sqlite::source_backed_library_page_sqlite(&s.cd, &query)?;
    let items: Vec<serde_json::Value> = page.items.iter().map(json_from_entry).collect();
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "total": page.total,
            "items": items,
        }))?
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn json_cli_library_page(
    s: &StorageApi,
    source: &str,
    filter: &str,
    sort: &str,
    search: &str,
    offset: usize,
    limit: usize,
) -> anyhow::Result<()> {
    match source {
        "sqlite" => json_source_backed_library_page(s, filter, sort, search, offset, limit),
        "tsv" if dirty_marker_path(s).exists() || s.cd.db_path().exists() => {
            json_source_backed_library_page(s, filter, sort, search, offset, limit)
        }
        _ => json_library_page(s, source, filter, sort, search, offset, limit),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn storage() -> (tempfile::TempDir, StorageApi) {
        let tmp = tempfile::tempdir().unwrap();
        let cd = wc_core::ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        (tmp, StorageApi::new(cd))
    }

    fn library_and_membership_counts(storage: &StorageApi) -> (i64, i64) {
        let connection = rusqlite::Connection::open(storage.cd.db_path()).unwrap();
        connection
            .query_row(
                "SELECT
                    (SELECT COUNT(*) FROM wallpapers),
                    (SELECT COUNT(*) FROM wallpaper_sources)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap()
    }

    #[test]
    fn rescan_preserves_offline_membership_and_derives_tsv_from_sqlite() {
        let (tmp, storage) = storage();
        let source_path = tmp.path().join("offline-source");
        std::fs::create_dir_all(&source_path).unwrap();
        let wallpaper = source_path.join("wall.jpg");
        std::fs::write(&wallpaper, b"wallpaper").unwrap();
        storage
            .source_create(&source_path.to_string_lossy())
            .unwrap();
        wc_app::library_refresh::refresh_library_sources(&storage, |_, _| {
            wc_scan::ScanControl::Continue
        })
        .expect("initial complete refresh should publish the source snapshot");
        assert_eq!(library_and_membership_counts(&storage), (1, 1));

        std::fs::remove_dir_all(&source_path).unwrap();
        rescan(&storage).unwrap();

        assert_eq!(
            library_and_membership_counts(&storage),
            (1, 1),
            "CLI rescan must not clear an offline source's SQLite snapshot"
        );
        let tsv = std::fs::read_to_string(storage.cd.library_tsv_path()).unwrap();
        assert!(
            tsv.contains(&wallpaper.to_string_lossy().to_string()),
            "legacy TSV consumers should receive the preserved SQLite snapshot"
        );
        assert_eq!(
            storage.source_records().unwrap()[0].availability,
            wc_storage::SourceAvailability::Offline
        );
    }

    #[test]
    fn rescan_with_no_sources_replaces_stale_tsv_from_empty_sqlite_snapshot() {
        let (_tmp, storage) = storage();
        std::fs::write(
            storage.cd.library_tsv_path(),
            "image\tjpg\tawww\t1\t1\t1x1\t/stale/wall.jpg\n",
        )
        .unwrap();
        let dirty = storage.cd.path.join("library.dirty");
        std::fs::write(&dirty, b"stale").unwrap();

        rescan(&storage).unwrap();

        assert_eq!(
            std::fs::read_to_string(storage.cd.library_tsv_path()).unwrap(),
            "",
            "rescan must not leave legacy consumers on a stale TSV snapshot"
        );
        assert!(
            !dirty.exists(),
            "publishing the SQLite-derived TSV should clear its stale marker"
        );
    }

    #[test]
    fn rescan_after_last_source_removed_excludes_orphan_from_tsv_and_cli_entries() {
        let (tmp, storage) = storage();
        let source_path = tmp.path().join("walls");
        std::fs::create_dir(&source_path).unwrap();
        let wallpaper = source_path.join("orphan.jpg");
        std::fs::write(&wallpaper, b"wallpaper").unwrap();
        let source = storage
            .source_create(&source_path.to_string_lossy())
            .unwrap();
        rescan(&storage).unwrap();
        storage.source_remove_by_id(source.id).unwrap();
        assert_eq!(library_and_membership_counts(&storage), (1, 0));

        rescan(&storage).unwrap();

        assert_eq!(
            std::fs::read_to_string(storage.cd.library_tsv_path()).unwrap(),
            "",
            "TSV consumers must not see physical metadata rows with no source membership"
        );
        assert!(
            library_entries(&storage).unwrap().is_empty(),
            "CLI library readers must use source-backed library semantics"
        );
    }

    #[test]
    fn failed_tsv_publish_keeps_dirty_marker_and_cli_reads_current_sqlite() {
        let (tmp, storage) = storage();
        let source_path = tmp.path().join("walls");
        std::fs::create_dir(&source_path).unwrap();
        std::fs::write(source_path.join("one.jpg"), b"one").unwrap();
        storage
            .source_create(&source_path.to_string_lossy())
            .unwrap();
        rescan(&storage).unwrap();
        assert_eq!(
            std::fs::read_to_string(storage.cd.library_tsv_path())
                .unwrap()
                .lines()
                .count(),
            1
        );

        std::fs::write(source_path.join("two.jpg"), b"two").unwrap();
        std::fs::create_dir(storage.cd.library_tsv_path().with_extension("tsv.tmp")).unwrap();

        rescan(&storage).expect("TSV projection failure is degraded refresh success");
        assert!(
            storage.cd.path.join("library.dirty").exists(),
            "a failed TSV publish must leave a durable stale-snapshot signal"
        );
        assert_eq!(
            std::fs::read_to_string(storage.cd.library_tsv_path())
                .unwrap()
                .lines()
                .count(),
            1,
            "the atomic publish must leave the prior TSV intact"
        );
        let entries = library_entries(&storage).unwrap();
        assert_eq!(
            entries.len(),
            2,
            "dirty-aware CLI readers must fall back to the current SQLite snapshot"
        );
    }

    #[test]
    fn dirty_marker_is_established_before_refresh_operation_starts() {
        let (_tmp, storage) = storage();
        let dirty = storage.cd.path.join("library.dirty");

        let value = wc_app::library_rescan::with_dirty_library_marker(&storage, || {
            assert!(
                dirty.exists(),
                "the marker must predate every SQLite mutation in refresh"
            );
            Ok::<_, std::io::Error>(42)
        })
        .unwrap();

        assert_eq!(value, 42);
        assert!(
            dirty.exists(),
            "only a successful TSV publish clears marker"
        );
    }

    #[test]
    fn overlapping_sources_publish_one_legacy_tsv_row() {
        let (tmp, storage) = storage();
        let root = tmp.path().join("walls");
        let nested = root.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("shared.jpg"), b"shared").unwrap();
        storage.source_create(&root.to_string_lossy()).unwrap();
        storage.source_create(&nested.to_string_lossy()).unwrap();

        rescan(&storage).unwrap();

        assert_eq!(library_and_membership_counts(&storage), (1, 2));
        assert_eq!(
            std::fs::read_to_string(storage.cd.library_tsv_path())
                .unwrap()
                .lines()
                .count(),
            1
        );
        assert_eq!(library_entries(&storage).unwrap().len(), 1);
    }

    #[test]
    fn dirty_marker_without_sqlite_reports_stale_instead_of_returning_empty() {
        let (_tmp, storage) = storage();
        std::fs::write(
            storage.cd.library_tsv_path(),
            "image\tjpg\tawww\t1\t1\t1x1\t/stale/wall.jpg\n",
        )
        .unwrap();
        std::fs::write(dirty_marker_path(&storage), b"refresh interrupted").unwrap();
        std::fs::remove_file(storage.cd.db_path()).unwrap();
        assert!(!storage.cd.db_path().exists());

        let entries_error = library_entries(&storage).unwrap_err();
        assert!(entries_error.to_string().contains("stale"));

        let page_error =
            json_cli_library_page(&storage, "tsv", "all", "name", "", 0, 10).unwrap_err();
        assert!(page_error.to_string().contains("stale"));
    }

    #[test]
    fn rescan_lock_serializes_snapshot_refreshes() {
        let (_tmp, storage) = storage();
        let first_guard = wc_app::library_rescan::acquire_rescan_lock(&storage).unwrap();
        let config_path = storage.cd.path.clone();
        let (attempting_tx, attempting_rx) = std::sync::mpsc::channel();
        let (acquired_tx, acquired_rx) = std::sync::mpsc::channel();

        let second = std::thread::spawn(move || {
            let storage = StorageApi::new(wc_core::ConfigDir { path: config_path });
            attempting_tx.send(()).unwrap();
            let _guard = wc_app::library_rescan::acquire_rescan_lock(&storage).unwrap();
            acquired_tx.send(()).unwrap();
        });

        attempting_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        assert!(
            acquired_rx
                .recv_timeout(std::time::Duration::from_millis(100))
                .is_err(),
            "a second rescan must wait until the first snapshot publish is finished"
        );

        drop(first_guard);
        acquired_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        second.join().unwrap();
    }

    #[test]
    fn cli_rescan_reports_scan_busy_after_bounded_wait() {
        let (_tmp, storage) = storage();
        let _guard = wc_app::library_rescan::acquire_rescan_lock(&storage).unwrap();
        let started = std::time::Instant::now();

        let error = rescan(&storage).unwrap_err();

        assert!(
            error.to_string().contains("scan_busy"),
            "CLI must preserve the stable contention category: {error}"
        );
        assert!(
            started.elapsed() >= std::time::Duration::from_secs(2),
            "CLI manual rescan must wait for the bounded manual lock timeout"
        );
        assert!(
            started.elapsed() < std::time::Duration::from_secs(3),
            "CLI manual rescan must not wait indefinitely"
        );
    }

    #[test]
    fn rescan_summary_labels_candidate_and_visited_counts_precisely() {
        let report = wc_app::library_refresh::LibraryRefreshReport {
            complete_sources: 1,
            indexed: 3,
            metadata: wc_app::library_refresh::RefreshMetadataStats {
                entries_visited: 8,
                candidates_found: 3,
                entries_indexed: 3,
                metadata_reused: 2,
            },
            ..wc_app::library_refresh::LibraryRefreshReport::default()
        };

        let summary = format_rescan_summary(
            1,
            &report,
            3,
            std::time::Duration::from_secs(1),
            std::time::Duration::from_millis(2),
            std::time::Duration::from_secs(1),
        );

        assert!(summary.contains("candidates: 3 files"));
        assert!(summary.contains("visited: 8"));
        assert!(!summary.contains("walked: 3"));
    }
}
