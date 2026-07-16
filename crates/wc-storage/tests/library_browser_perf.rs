use std::time::{Duration, Instant};

use rusqlite::params;
use wc_core::config::ConfigDir;
use wc_storage::sqlite::{
    browser_library_page, build_library_fts_chunk, create_schema, invalidate_cached_connections,
    LibraryBrowserQuery, LibraryBrowserSort, LibraryBrowserType,
};

fn fixture() -> (tempfile::TempDir, ConfigDir) {
    let temp = tempfile::tempdir().unwrap();
    let cd = ConfigDir {
        path: temp.path().join("perf-fixture"),
    };
    std::fs::create_dir_all(&cd.path).unwrap();
    let mut connection = rusqlite::Connection::open(cd.path.join("wallpapers.db")).unwrap();
    create_schema(&connection).unwrap();
    let transaction = connection.transaction().unwrap();
    transaction
        .execute(
            "INSERT INTO sources (id, path, display_name)
             VALUES (1, '/fixture', 'Deterministic Fixture')",
            [],
        )
        .unwrap();
    {
        let mut wallpaper = transaction
            .prepare(
                "INSERT INTO wallpapers
                 (id, path, type, ext, backend, size, mtime, resolution, title, author)
                 VALUES (?1, ?2, 'image', 'jpg', 'awww', 1024, ?1, '1920x1080', ?3, 'Fixture Author')",
            )
            .unwrap();
        let mut membership = transaction
            .prepare("INSERT INTO wallpaper_sources (wallpaper_id, source_id) VALUES (?1, 1)")
            .unwrap();
        for id in 1..=10_000i64 {
            wallpaper
                .execute(params![
                    id,
                    format!("/fixture/wall-{id:05}.jpg"),
                    format!("Fixture {id:05}")
                ])
                .unwrap();
            membership.execute([id]).unwrap();
        }
    }
    wc_storage::sqlite::bump_library_revision(&transaction).unwrap();
    transaction.commit().unwrap();
    while !build_library_fts_chunk(&mut connection).unwrap() {}
    drop(connection);
    (temp, cd)
}

fn nearest_rank_p95(samples: &mut [Duration]) -> Duration {
    samples.sort_unstable();
    samples[((samples.len() * 95).div_ceil(100)).saturating_sub(1)]
}

#[test]
fn ten_thousand_wallpaper_browser_cold_p95_is_at_most_500ms() {
    let (_temp, cd) = fixture();
    let mut samples = Vec::new();
    for sample in 0usize..25 {
        invalidate_cached_connections();
        let query = LibraryBrowserQuery {
            source_id: None,
            type_filter: LibraryBrowserType::Usable,
            favorites_only: false,
            search: if sample.is_multiple_of(2) {
                format!("{:05}", 9_999 - sample)
            } else {
                String::new()
            },
            sort: LibraryBrowserSort::RecentlyAdded,
            cursor: None,
            limit: 60,
        };
        let started = Instant::now();
        let page = browser_library_page(&cd, &query).unwrap();
        samples.push(started.elapsed());
        assert!(!page.items.is_empty());
        std::hint::black_box(page);
    }
    let p95 = nearest_rank_p95(&mut samples);
    eprintln!("library_browser_10k cold p95={p95:?} samples={samples:?}");
    assert!(p95 <= Duration::from_millis(500), "cold p95 was {p95:?}");
}
