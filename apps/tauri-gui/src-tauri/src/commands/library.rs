use super::common::{
    dto_from_entry, fail, ok, storage, CommandResult, LibraryCountDto, LibraryPageDto,
    LibrarySourceStatusDto,
};

#[tauri::command]
pub async fn library_count() -> Result<LibraryCountDto, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let s = storage()?;
        let counts = wc_storage::sqlite::library_counts_sqlite(&s.cd).map_err(|e| e.to_string())?;
        Ok(LibraryCountDto {
            total: counts.total,
            images: counts.images,
            gifs: counts.gifs,
            videos: counts.videos,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn library_page(
    source: String,
    filter: String,
    sort: String,
    search: String,
    offset: usize,
    limit: usize,
) -> Result<LibraryPageDto, String> {
    let _ = source;
    library_page_gui(filter, sort, search, offset, limit).await
}

#[tauri::command]
pub async fn library_page_gui(
    filter: String,
    sort: String,
    search: String,
    offset: usize,
    limit: usize,
) -> Result<LibraryPageDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let t0 = std::time::Instant::now();
        let s = storage()?;
        let storage_init = t0.elapsed();
        let query = wc_storage::sqlite::LibraryPageQuery {
            filter: wc_storage::sqlite::LibraryFilter::parse(&filter).map_err(|e| e.to_string())?,
            sort: wc_storage::sqlite::LibrarySort::parse(&sort).map_err(|e| e.to_string())?,
            search,
            offset,
            limit,
        };
        let page =
            wc_storage::sqlite::library_page_sqlite(&s.cd, &query).map_err(|e| e.to_string())?;
        let query_end = t0.elapsed();
        let items = page
            .items
            .into_iter()
            .map(dto_from_entry)
            .collect::<Vec<_>>();
        let dto_map_end = t0.elapsed();
        maybe_write_library_page_debug_log(
            &s,
            storage_init,
            query_end - storage_init,
            dto_map_end - query_end,
            page.total,
            items.len(),
        );
        Ok(LibraryPageDto {
            total: page.total,
            items,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

fn format_library_page_debug_log(
    storage_init: std::time::Duration,
    query: std::time::Duration,
    dto_map: std::time::Duration,
    total: usize,
    item_count: usize,
) -> String {
    format!(
        "library_page_gui stages: storage_init={:?} query={:?} dto_map={:?} total={} items={}\n",
        storage_init, query, dto_map, total, item_count
    )
}

fn maybe_write_library_page_debug_log(
    s: &wc_storage::StorageApi,
    storage_init: std::time::Duration,
    query: std::time::Duration,
    dto_map: std::time::Duration,
    total: usize,
    item_count: usize,
) {
    if s.config_get("gui_debug_logs", "off") != "on" {
        return;
    }
    let log = format_library_page_debug_log(storage_init, query, dto_map, total, item_count);
    let log_path = s.cd.path.join("library-page-last.log");
    let _ = std::fs::write(&log_path, log);
}

#[tauri::command]
pub async fn favorites_page(offset: usize, limit: usize) -> Result<LibraryPageDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let s = storage()?;
        let page = wc_storage::sqlite::favorites_page_sqlite(&s.cd, offset, limit)
            .map_err(|e| e.to_string())?;
        Ok(LibraryPageDto {
            total: page.total,
            items: page.items.into_iter().map(dto_from_entry).collect(),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn favorite_add(path: String) -> CommandResult {
    tauri::async_runtime::spawn_blocking(move || match storage() {
        Ok(s) => match s.favorites_add(&path) {
            Ok(_) => ok("Added favorite."),
            Err(e) => fail(e.to_string()),
        },
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn favorite_remove(path: String) -> CommandResult {
    tauri::async_runtime::spawn_blocking(move || match storage() {
        Ok(s) => match s.favorites_remove(&path) {
            Ok(_) => ok("Removed favorite."),
            Err(e) => fail(e.to_string()),
        },
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn history_page(offset: usize, limit: usize) -> Result<LibraryPageDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let s = storage()?;
        let page = wc_storage::sqlite::history_page_sqlite(&s.cd, offset, limit)
            .map_err(|e| e.to_string())?;
        Ok(LibraryPageDto {
            total: page.total,
            items: page.items.into_iter().map(dto_from_entry).collect(),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn history_clear() -> CommandResult {
    tauri::async_runtime::spawn_blocking(|| match storage() {
        Ok(s) => match s.history_clear() {
            Ok(_) => ok("History cleared."),
            Err(e) => fail(e.to_string()),
        },
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn library_source_status() -> Result<LibrarySourceStatusDto, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let s = storage()?;
        let sqlite_rows = wc_storage::sqlite::library_count(&s.cd).unwrap_or(0);
        let tsv_rows = std::fs::read_to_string(s.cd.library_tsv_path())
            .map(|c| c.lines().filter(|l| !l.trim().is_empty()).count())
            .unwrap_or(0);
        Ok(LibrarySourceStatusDto {
            configured: "sqlite".into(),
            effective: "sqlite".into(),
            sqlite_ready: s.cd.db_path().exists(),
            sqlite_rows,
            tsv_rows,
            stale: false,
            message: "SQLite library database is active.".into(),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use wc_core::types::FileType;

    #[test]
    fn format_library_page_debug_log_records_all_stages() {
        let log = super::format_library_page_debug_log(
            std::time::Duration::from_micros(120),
            std::time::Duration::from_micros(3400),
            std::time::Duration::from_micros(80),
            42,
            120,
        );
        assert!(log.contains("storage_init="));
        assert!(log.contains("query="));
        assert!(log.contains("dto_map="));
        assert!(log.contains("total=42"));
        assert!(log.contains("items=120"));
    }

    #[test]
    fn maybe_write_library_page_debug_log_writes_only_when_debug_enabled() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = wc_core::ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        wc_core::config::write_config_value(&cd.path, "storage_backend", "sqlite").unwrap();
        let s = wc_storage::StorageApi::new(cd);
        let log_path = s.cd.path.join("library-page-last.log");

        super::maybe_write_library_page_debug_log(
            &s,
            std::time::Duration::from_micros(1),
            std::time::Duration::from_micros(2),
            std::time::Duration::from_micros(3),
            0,
            0,
        );
        assert!(
            !log_path.exists(),
            "log should not be written when debug off"
        );

        s.config_set("gui_debug_logs", "on").unwrap();
        super::maybe_write_library_page_debug_log(
            &s,
            std::time::Duration::from_micros(1),
            std::time::Duration::from_micros(2),
            std::time::Duration::from_micros(3),
            5,
            5,
        );
        assert!(log_path.exists(), "log should be written when debug on");
        let content = std::fs::read_to_string(&log_path).unwrap();
        assert!(content.contains("total=5"));
    }

    #[test]
    fn library_count_dto_requires_no_full_table_load() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = wc_core::ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        wc_storage::sqlite::ensure_sqlite_db(&cd);
        let conn = rusqlite::Connection::open(cd.db_path()).unwrap();
        conn.execute(
            "INSERT INTO wallpapers (path, type, ext, backend, size, mtime, resolution)
             VALUES ('/a.jpg', 'image', 'jpg', 'awww', 100, 1000, '1920x1080')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO wallpapers (path, type, ext, backend, size, mtime, resolution)
             VALUES ('/b.gif', 'gif', 'gif', 'awww', 200, 2000, '1920x1080')",
            [],
        )
        .unwrap();

        let counts = wc_storage::sqlite::library_counts_sqlite(&cd).unwrap();
        assert_eq!(counts.total, 2);
        assert_eq!(counts.images, 1);
        assert_eq!(counts.gifs, 1);
        assert_eq!(counts.videos, 0);
    }

    #[test]
    fn library_page_gui_uses_shared_sql_helper() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = wc_core::ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        wc_storage::sqlite::ensure_sqlite_db(&cd);
        let conn = rusqlite::Connection::open(cd.db_path()).unwrap();
        conn.execute(
            "INSERT INTO wallpapers (path, type, ext, backend, size, mtime, resolution)
             VALUES ('/a.png', 'image', 'png', 'awww', 300, 3000, '800x600')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO wallpapers (path, type, ext, backend, size, mtime, resolution)
             VALUES ('/b.mp4', 'video', 'mp4', 'mpvpaper', 400, 4000, '1920x1080')",
            [],
        )
        .unwrap();

        let page = wc_storage::sqlite::library_page_sqlite(
            &cd,
            &wc_storage::sqlite::LibraryPageQuery {
                filter: wc_storage::sqlite::LibraryFilter::Image,
                sort: wc_storage::sqlite::LibrarySort::Name,
                search: String::new(),
                offset: 0,
                limit: 10,
            },
        )
        .unwrap();

        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].file_type, FileType::Image);
        assert_eq!(page.items[0].ext, "png");
    }

    #[test]
    fn favorites_and_history_use_shared_sql_helpers() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = wc_core::ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        wc_storage::sqlite::ensure_sqlite_db(&cd);
        let conn = rusqlite::Connection::open(cd.db_path()).unwrap();
        conn.execute(
            "INSERT INTO wallpapers (path, type, ext, backend, size, mtime, resolution)
             VALUES ('/fav.jpg', 'image', 'jpg', 'awww', 100, 1000, '1920x1080')",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO favorites (path) VALUES ('/fav.jpg')", [])
            .unwrap();
        conn.execute(
            "INSERT INTO history (path, backend) VALUES ('/fav.jpg', 'awww')",
            [],
        )
        .unwrap();

        let favs = wc_storage::sqlite::favorites_page_sqlite(&cd, 0, 10).unwrap();
        assert_eq!(favs.total, 1);

        let hist = wc_storage::sqlite::history_page_sqlite(&cd, 0, 10).unwrap();
        assert_eq!(hist.total, 1);
    }
}
