use super::common::{
    dto_from_entry_with_routing, fail, ok, storage, CommandResult, LibraryBrowserItemDto,
    LibraryBrowserPageDto, LibraryBrowserQueryDto, LibraryBrowserSourceDto, LibraryBrowserTotalDto,
    LibraryCountDto, LibraryPageDto, LibraryQueryErrorDto, LibrarySourceStatusDto,
};

#[tauri::command]
pub async fn library_count() -> Result<LibraryCountDto, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let s = storage()?;
        library_count_for_storage(s)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn library_count_for_storage(s: &wc_storage::StorageApi) -> Result<LibraryCountDto, String> {
    let counts = wc_storage::sqlite::source_backed_library_counts_sqlite(&s.cd)
        .map_err(|e| e.to_string())?;
    Ok(LibraryCountDto {
        total: counts.total,
        images: counts.images,
        gifs: counts.gifs,
        videos: counts.videos,
    })
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
        let page = library_page_for_storage(s, &query)?;
        let query_end = t0.elapsed();
        let routing = s.backend_routing();
        let items = page
            .items
            .into_iter()
            .map(|entry| dto_from_entry_with_routing(entry, &routing))
            .collect::<Vec<_>>();
        let dto_map_end = t0.elapsed();
        maybe_write_library_page_debug_log(
            s,
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

fn library_page_for_storage(
    s: &wc_storage::StorageApi,
    query: &wc_storage::sqlite::LibraryPageQuery,
) -> Result<wc_storage::sqlite::LibraryPage, String> {
    wc_storage::sqlite::source_backed_library_page_sqlite(&s.cd, query).map_err(|e| e.to_string())
}

fn browser_query_from_dto(
    query: LibraryBrowserQueryDto,
) -> Result<wc_storage::sqlite::LibraryBrowserQuery, wc_core::error::WcError> {
    let type_filter = match query.type_filter.as_str() {
        "usable" => wc_storage::sqlite::LibraryBrowserType::Usable,
        "image" => wc_storage::sqlite::LibraryBrowserType::Image,
        "gif" => wc_storage::sqlite::LibraryBrowserType::Gif,
        "video" => wc_storage::sqlite::LibraryBrowserType::Video,
        "weScene" => wc_storage::sqlite::LibraryBrowserType::WeScene,
        "unsupported" => wc_storage::sqlite::LibraryBrowserType::Unsupported,
        other => {
            return Err(wc_core::error::WcError::Other(format!(
                "unknown library browser type: {other}; expected usable, image, gif, video, weScene, or unsupported"
            )))
        }
    };
    let sort = match query.sort.as_str() {
        "recentlyAdded" => wc_storage::sqlite::LibraryBrowserSort::RecentlyAdded,
        "nameAsc" => wc_storage::sqlite::LibraryBrowserSort::NameAsc,
        "nameDesc" => wc_storage::sqlite::LibraryBrowserSort::NameDesc,
        other => {
            return Err(wc_core::error::WcError::Other(format!(
            "unknown library browser sort: {other}; expected recentlyAdded, nameAsc, or nameDesc"
        )))
        }
    };
    Ok(wc_storage::sqlite::LibraryBrowserQuery {
        source_id: query.source_id,
        type_filter,
        favorites_only: query.favorites_only,
        search: query.search,
        sort,
        cursor: query.cursor,
        limit: query.limit,
    })
}

fn browser_item_dto(
    item: wc_storage::sqlite::LibraryBrowserItem,
    routing: &wc_core::backend_routing::BackendRouting,
) -> LibraryBrowserItemDto {
    LibraryBrowserItemDto {
        wallpaper: dto_from_entry_with_routing(item.entry, routing),
        wallpaper_id: item.wallpaper_id,
        favorite: item.favorite,
        author: item.author,
        added_at: item.added_at,
        sources: item
            .sources
            .into_iter()
            .map(|source| LibraryBrowserSourceDto {
                id: source.id,
                display_name: source.display_name,
            })
            .collect(),
    }
}

#[cfg(test)]
fn library_browser_page_for_storage(
    s: &wc_storage::StorageApi,
    query: LibraryBrowserQueryDto,
) -> Result<LibraryBrowserPageDto, wc_core::error::WcError> {
    let query = browser_query_from_dto(query)?;
    let page = wc_storage::sqlite::browser_library_page(&s.cd, &query)?;
    let routing = s.backend_routing();
    Ok(LibraryBrowserPageDto {
        revision: page.revision,
        next_cursor: page.next_cursor,
        total: page.total,
        items: page
            .items
            .into_iter()
            .map(|item| browser_item_dto(item, &routing))
            .collect(),
    })
}

fn library_browser_page_for_service(
    service: &crate::library_service::LibraryService,
    s: &wc_storage::StorageApi,
    query: LibraryBrowserQueryDto,
) -> Result<LibraryBrowserPageDto, crate::library_service::LibraryServiceError> {
    let query =
        browser_query_from_dto(query).map_err(crate::library_service::LibraryServiceError::from)?;
    let page = service.page(&s.cd, &query)?;
    let routing = s.backend_routing();
    Ok(LibraryBrowserPageDto {
        revision: page.revision,
        next_cursor: page.next_cursor.clone(),
        total: page.total,
        items: page
            .items
            .iter()
            .cloned()
            .map(|item| browser_item_dto(item, &routing))
            .collect(),
    })
}

fn library_browser_random_for_storage(
    s: &wc_storage::StorageApi,
    query: LibraryBrowserQueryDto,
) -> Result<Option<LibraryBrowserItemDto>, String> {
    let query = browser_query_from_dto(query).map_err(|error| error.to_string())?;
    let routing = s.backend_routing();
    wc_storage::sqlite::browser_library_random(&s.cd, &query)
        .map(|item| item.map(|item| browser_item_dto(item, &routing)))
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn library_browser_page(
    state: tauri::State<'_, crate::library_service::LibraryService>,
    query: LibraryBrowserQueryDto,
) -> Result<LibraryBrowserPageDto, LibraryQueryErrorDto> {
    let service = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let s = storage().map_err(|message| LibraryQueryErrorDto {
            kind: "storage_error",
            message,
        })?;
        library_browser_page_for_service(&service, s, query).map_err(|error| LibraryQueryErrorDto {
            kind: error.kind,
            message: error.message,
        })
    })
    .await
    .map_err(|error| LibraryQueryErrorDto {
        kind: "join_error",
        message: error.to_string(),
    })?
}

#[tauri::command]
pub async fn library_browser_total(
    state: tauri::State<'_, crate::library_service::LibraryService>,
    query: LibraryBrowserQueryDto,
    expected_revision: u64,
) -> Result<LibraryBrowserTotalDto, LibraryQueryErrorDto> {
    let service = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let s = storage().map_err(|message| LibraryQueryErrorDto {
            kind: "storage_error",
            message,
        })?;
        let query = browser_query_from_dto(query).map_err(LibraryQueryErrorDto::from)?;
        let total = service
            .exact_total(&s.cd, &query, expected_revision)
            .map_err(|error| LibraryQueryErrorDto {
                kind: error.kind,
                message: error.message,
            })?;
        Ok(LibraryBrowserTotalDto {
            revision: total.revision,
            total: total.total,
        })
    })
    .await
    .map_err(|error| LibraryQueryErrorDto {
        kind: "join_error",
        message: error.to_string(),
    })?
}

#[tauri::command]
pub async fn library_browser_random(
    query: LibraryBrowserQueryDto,
) -> Result<Option<LibraryBrowserItemDto>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let s = storage()?;
        library_browser_random_for_storage(s, query)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn library_wallpaper_exists(wallpaper_id: i64) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let storage = storage()?;
        wc_storage::sqlite::library_wallpaper_exists(&storage.cd, wallpaper_id)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
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
        let routing = s.backend_routing();
        Ok(LibraryPageDto {
            total: page.total,
            items: page
                .items
                .into_iter()
                .map(|entry| dto_from_entry_with_routing(entry, &routing))
                .collect(),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn favorite_add(
    state: tauri::State<'_, crate::library_service::LibraryService>,
    path: String,
) -> Result<CommandResult, String> {
    let service = state.inner().clone();
    Ok(
        tauri::async_runtime::spawn_blocking(move || match storage() {
            Ok(s) => match s.favorites_add(&path) {
                Ok(changed) => {
                    if changed {
                        service.invalidate_local_write();
                    }
                    ok("Added favorite.")
                }
                Err(e) => fail(e.to_string()),
            },
            Err(e) => fail(e),
        })
        .await
        .unwrap_or_else(|e| fail(e.to_string())),
    )
}

#[tauri::command]
pub async fn favorite_remove(
    state: tauri::State<'_, crate::library_service::LibraryService>,
    path: String,
) -> Result<CommandResult, String> {
    let service = state.inner().clone();
    Ok(
        tauri::async_runtime::spawn_blocking(move || match storage() {
            Ok(s) => match s.favorites_remove(&path) {
                Ok(()) => {
                    service.invalidate_local_write();
                    ok("Removed favorite.")
                }
                Err(e) => fail(e.to_string()),
            },
            Err(e) => fail(e),
        })
        .await
        .unwrap_or_else(|e| fail(e.to_string())),
    )
}

fn build_library_source_status(
    s: &wc_storage::StorageApi,
) -> Result<LibrarySourceStatusDto, String> {
    let source_count = s.sources_list().map_err(|e| e.to_string())?.len();
    let sqlite_ready = s.cd.db_path().exists();
    let sqlite_rows = wc_storage::sqlite::source_backed_library_count(&s.cd).unwrap_or(0);
    let tsv_rows = std::fs::read_to_string(s.cd.library_tsv_path())
        .map(|c| c.lines().filter(|l| !l.trim().is_empty()).count())
        .unwrap_or(0);
    let stale = source_count > 0 && sqlite_rows == 0 && tsv_rows > 0;
    let message = if source_count == 0 {
        "No sources configured. Add a source or scan Wallpaper Engine.".to_string()
    } else if stale {
        "Sources exist, but the SQLite library index is empty while legacy library.tsv has rows. Rebuild the SQLite index.".to_string()
    } else if sqlite_rows == 0 {
        "Sources exist, but the SQLite library index has no wallpapers. Rescan or repair the library index.".to_string()
    } else {
        "SQLite library database is active.".to_string()
    };
    Ok(LibrarySourceStatusDto {
        configured: "sqlite".into(),
        effective: "sqlite".into(),
        sqlite_ready,
        sqlite_rows,
        tsv_rows,
        source_count,
        stale,
        message,
    })
}

#[tauri::command]
pub async fn library_source_status() -> Result<LibrarySourceStatusDto, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let s = storage()?;
        build_library_source_status(s)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    #[cfg(test)]
    use wc_config::ConfigDirExt;
    use wc_core::types::FileType;

    fn browser_fixture() -> (tempfile::TempDir, wc_storage::StorageApi, i64) {
        let tmp = tempfile::tempdir().unwrap();
        let storage = wc_storage::StorageApi::new(wc_core::ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        });
        let root = tmp.path().join("workshop");
        std::fs::create_dir(&root).unwrap();
        let source = storage.source_create(&root.to_string_lossy()).unwrap();
        storage.source_rename(source.id, "Curated Scenes").unwrap();

        let conn = rusqlite::Connection::open(storage.cd.db_path()).unwrap();
        conn.execute(
            "INSERT INTO wallpapers
             (id, path, type, ext, backend, size, mtime, resolution,
              project_type, preview_path, workshop_id, title, we_file,
              unsupported_reason, author, added_at)
             VALUES
             (41, '/wallpapers/scene-41', 'we_scene', 'scene',
              'linux-wallpaperengine', 4096, 1700000000, 'WE',
              'scene', '/wallpapers/scene-41/preview.gif', '41',
              'Aurora Scene', 'scene.json', '', 'Ada Lovelace',
              '2026-07-14T10:00:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO wallpaper_sources (wallpaper_id, source_id) VALUES (41, ?1)",
            [source.id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO favorites (path) VALUES ('/wallpapers/scene-41')",
            [],
        )
        .unwrap();
        drop(conn);

        (tmp, storage, source.id)
    }

    fn browser_query(source_id: Option<i64>) -> super::LibraryBrowserQueryDto {
        super::LibraryBrowserQueryDto {
            source_id,
            type_filter: "weScene".into(),
            favorites_only: true,
            search: "aurora ada curated".into(),
            sort: "recentlyAdded".into(),
            cursor: None,
            limit: 20,
        }
    }

    #[test]
    fn browser_query_maps_every_supported_type_and_sort_and_rejects_unknown_values() {
        let types = [
            ("usable", wc_storage::sqlite::LibraryBrowserType::Usable),
            ("image", wc_storage::sqlite::LibraryBrowserType::Image),
            ("gif", wc_storage::sqlite::LibraryBrowserType::Gif),
            ("video", wc_storage::sqlite::LibraryBrowserType::Video),
            ("weScene", wc_storage::sqlite::LibraryBrowserType::WeScene),
            (
                "unsupported",
                wc_storage::sqlite::LibraryBrowserType::Unsupported,
            ),
        ];
        for (raw, expected) in types {
            let mut dto = browser_query(None);
            dto.type_filter = raw.into();
            let mapped = super::browser_query_from_dto(dto).unwrap();
            assert_eq!(mapped.type_filter, expected);
        }

        let sorts = [
            (
                "recentlyAdded",
                wc_storage::sqlite::LibraryBrowserSort::RecentlyAdded,
            ),
            ("nameAsc", wc_storage::sqlite::LibraryBrowserSort::NameAsc),
            ("nameDesc", wc_storage::sqlite::LibraryBrowserSort::NameDesc),
        ];
        for (raw, expected) in sorts {
            let mut dto = browser_query(None);
            dto.sort = raw.into();
            let mapped = super::browser_query_from_dto(dto).unwrap();
            assert_eq!(mapped.sort, expected);
        }

        let mut unknown_type = browser_query(None);
        unknown_type.type_filter = "we_scene".into();
        assert!(super::browser_query_from_dto(unknown_type)
            .unwrap_err()
            .to_string()
            .contains("unknown library browser type"));

        let mut unknown_sort = browser_query(None);
        unknown_sort.sort = "recently_added".into();
        assert!(super::browser_query_from_dto(unknown_sort)
            .unwrap_err()
            .to_string()
            .contains("unknown library browser sort"));
    }

    #[test]
    fn browser_query_deserializes_camel_case_wire_fields() {
        let dto: super::LibraryBrowserQueryDto = serde_json::from_value(serde_json::json!({
            "sourceId": 9,
            "typeFilter": "weScene",
            "favoritesOnly": true,
            "search": "aurora",
            "sort": "nameAsc",
            "cursor": "opaque-cursor",
            "limit": 20
        }))
        .unwrap();

        assert_eq!(dto.source_id, Some(9));
        assert_eq!(dto.type_filter, "weScene");
        assert!(dto.favorites_only);
        assert_eq!(dto.search, "aurora");
        assert_eq!(dto.sort, "nameAsc");
        assert_eq!(dto.cursor.as_deref(), Some("opaque-cursor"));
        assert_eq!(dto.limit, 20);
    }

    #[test]
    fn browser_page_flattens_wallpaper_dto_and_keeps_browser_metadata() {
        let (_tmp, storage, source_id) = browser_fixture();

        let page =
            super::library_browser_page_for_storage(&storage, browser_query(Some(source_id)))
                .unwrap();
        assert_eq!(page.total, None);
        assert_eq!(
            page.revision,
            wc_storage::sqlite::read_library_revision(
                &rusqlite::Connection::open(storage.cd.db_path()).unwrap(),
            )
            .unwrap(),
        );
        assert_eq!(page.items.len(), 1);

        let json = serde_json::to_value(&page.items[0]).unwrap();
        assert_eq!(json["wallpaperId"], 41);
        assert_eq!(json["path"], "/wallpapers/scene-41");
        assert_eq!(json["type"], "we_scene");
        assert_eq!(json["favorite"], true);
        assert_eq!(json["author"], "Ada Lovelace");
        assert_eq!(json["addedAt"], "2026-07-14T10:00:00Z");
        assert_eq!(json["sources"][0]["id"], source_id);
        assert_eq!(json["sources"][0]["displayName"], "Curated Scenes");
        assert_eq!(json["applyAvailability"], "available");
        assert_eq!(json["applyBackend"], "linux-wallpaperengine");
        assert!(json["applyActions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| { action["kind"] == "apply" && action["enabled"] == true }));
        assert!(
            json.get("wallpaper").is_none(),
            "WallpaperDTO must be flattened"
        );
    }

    #[test]
    fn browser_page_exposes_the_renderer_that_apply_will_use() {
        let (_tmp, storage, source_id) = browser_fixture();
        storage.config_set("image_backend", "mpvpaper").unwrap();
        let conn = rusqlite::Connection::open(storage.cd.db_path()).unwrap();
        conn.execute(
            "INSERT INTO wallpapers
             (id, path, type, ext, backend, size, mtime, resolution, added_at)
             VALUES
             (42, '/wallpapers/still.jpg', 'image', 'jpg', 'awww', 1024,
              1700000001, '1920x1080', '2026-07-14T11:00:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO wallpaper_sources (wallpaper_id, source_id) VALUES (42, ?1)",
            [source_id],
        )
        .unwrap();
        drop(conn);
        let mut query = browser_query(Some(source_id));
        query.type_filter = "image".into();
        query.favorites_only = false;
        query.search.clear();

        let page = super::library_browser_page_for_storage(&storage, query).unwrap();

        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].wallpaper.backend, "mpvpaper");
        assert_eq!(
            page.items[0].wallpaper.apply_backend.as_deref(),
            Some("mpvpaper")
        );
    }

    #[test]
    fn browser_random_reuses_query_semantics_and_flattened_item_shape() {
        let (_tmp, storage, source_id) = browser_fixture();

        let item = super::library_browser_random_for_storage(&storage, {
            let mut query = browser_query(Some(source_id));
            query.cursor = Some("ignored-by-random".into());
            query.limit = 0;
            query
        })
        .unwrap()
        .expect("matching item");
        assert_eq!(item.wallpaper_id, 41);
        assert_eq!(item.wallpaper.path, "/wallpapers/scene-41");
        assert_eq!(item.sources[0].display_name, "Curated Scenes");

        let mut no_match = browser_query(Some(source_id));
        no_match.search = "missing".into();
        assert!(
            super::library_browser_random_for_storage(&storage, no_match)
                .unwrap()
                .is_none()
        );
    }

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
        wc_config::write_config_value(&cd.path, "storage_backend", "sqlite").unwrap();
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
    fn favorites_use_shared_sql_helper() {
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
        let favs = wc_storage::sqlite::favorites_page_sqlite(&cd, 0, 10).unwrap();
        assert_eq!(favs.total, 1);
    }

    #[test]
    fn build_library_source_status_reports_stale_when_tsv_has_rows_but_sqlite_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = wc_core::ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        wc_storage::sqlite::ensure_sqlite_db(&cd);
        std::fs::write(cd.library_tsv_path(), "/tmp/a.jpg\n/tmp/b.jpg\n").unwrap();

        let s = wc_storage::StorageApi::new(cd);
        s.sources_add("/tmp/wallpapers").unwrap();
        let status = super::build_library_source_status(&s).unwrap();

        assert_eq!(status.source_count, 1);
        assert_eq!(status.sqlite_rows, 0);
        assert_eq!(status.tsv_rows, 2);
        assert!(status.stale);
        assert!(status.sqlite_ready);
        assert!(status.message.contains("legacy library.tsv"));
    }

    #[test]
    fn gui_library_views_exclude_orphans_and_deduplicate_overlapping_sources() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = wc_storage::StorageApi::new(wc_core::ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        });
        let first_root = tmp.path().join("first");
        let second_root = tmp.path().join("second");
        std::fs::create_dir(&first_root).unwrap();
        std::fs::create_dir(&second_root).unwrap();
        let first = storage
            .source_create(&first_root.to_string_lossy())
            .unwrap();
        let second = storage
            .source_create(&second_root.to_string_lossy())
            .unwrap();
        let conn = rusqlite::Connection::open(storage.cd.db_path()).unwrap();
        conn.execute(
            "INSERT INTO wallpapers (path, type, ext, backend, size, mtime, resolution)
             VALUES ('/member.jpg', 'image', 'jpg', 'awww', 100, 1000, '1920x1080')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO wallpapers (path, type, ext, backend, size, mtime, resolution)
             VALUES ('/orphan.gif', 'gif', 'gif', 'awww', 200, 2000, '1920x1080')",
            [],
        )
        .unwrap();
        for source_id in [first.id, second.id] {
            conn.execute(
                "INSERT INTO wallpaper_sources (wallpaper_id, source_id)
                 SELECT id, ?1 FROM wallpapers WHERE path = '/member.jpg'",
                [source_id],
            )
            .unwrap();
        }
        drop(conn);

        assert_eq!(wc_storage::sqlite::library_count(&storage.cd).unwrap(), 2);
        let counts = super::library_count_for_storage(&storage).unwrap();
        assert_eq!(counts.total, 1);
        assert_eq!(counts.images, 1);
        assert_eq!(counts.gifs, 0);

        let page = super::library_page_for_storage(
            &storage,
            &wc_storage::sqlite::LibraryPageQuery {
                filter: wc_storage::sqlite::LibraryFilter::All,
                sort: wc_storage::sqlite::LibrarySort::Name,
                search: String::new(),
                offset: 0,
                limit: 10,
            },
        )
        .unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].path.as_str(), "/member.jpg");

        let status = super::build_library_source_status(&storage).unwrap();
        assert_eq!(status.source_count, 2);
        assert_eq!(status.sqlite_rows, 1);
        assert!(!status.stale);
    }

    #[test]
    fn build_library_source_status_reports_no_sources_message() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = wc_core::ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        wc_storage::sqlite::ensure_sqlite_db(&cd);

        let s = wc_storage::StorageApi::new(cd);
        let status = super::build_library_source_status(&s).unwrap();

        assert_eq!(status.source_count, 0);
        assert_eq!(status.sqlite_rows, 0);
        assert!(!status.stale);
        assert_eq!(
            status.message,
            "No sources configured. Add a source or scan Wallpaper Engine."
        );
    }
}
