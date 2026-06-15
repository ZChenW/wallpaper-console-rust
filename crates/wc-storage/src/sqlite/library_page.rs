use rusqlite::{params, Connection};
use wc_core::config::ConfigDir;
use wc_core::error::WcError;
use wc_core::types::WallpaperEntry;

use super::row_map::wallpaper_entry_from_row;
use super::schema::{ensure_sqlite_db, ensure_wallpaper_query_indexes};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryFilter {
    All,
    Image,
    Gif,
    Video,
    We,
    WeScene,
    WeWeb,
    Unsupported,
}

impl LibraryFilter {
    pub fn parse(raw: &str) -> Result<Self, WcError> {
        match raw {
            "all" | "" => Ok(Self::All),
            "image" | "images" => Ok(Self::Image),
            "gif" | "gifs" => Ok(Self::Gif),
            "video" | "videos" => Ok(Self::Video),
            "we" => Ok(Self::We),
            "we_scene" => Ok(Self::WeScene),
            "we_web" => Ok(Self::WeWeb),
            "unsupported" => Ok(Self::Unsupported),
            other => Err(WcError::Other(format!("unknown library filter: {other}"))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibrarySort {
    Newest,
    Largest,
    Name,
}

impl LibrarySort {
    pub fn parse(raw: &str) -> Result<Self, WcError> {
        match raw {
            "newest" | "" => Ok(Self::Newest),
            "largest" | "size" => Ok(Self::Largest),
            "name" => Ok(Self::Name),
            other => Err(WcError::Other(format!("unknown library sort: {other}"))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryPageQuery {
    pub filter: LibraryFilter,
    pub sort: LibrarySort,
    pub search: String,
    pub offset: usize,
    pub limit: usize,
}

#[derive(Debug, Clone)]
pub struct LibraryPage {
    pub total: usize,
    pub items: Vec<WallpaperEntry>,
}

pub fn library_count(cd: &ConfigDir) -> Result<usize, WcError> {
    let db_path = cd.db_path();
    if !db_path.exists() {
        return Ok(0);
    }
    let conn = Connection::open(&db_path).map_err(|e| WcError::Sqlite(e.to_string()))?;
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM wallpapers", [], |row| row.get(0))
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    Ok(count as usize)
}

pub fn library_page_sqlite(
    cd: &ConfigDir,
    query: &LibraryPageQuery,
) -> Result<LibraryPage, WcError> {
    ensure_sqlite_db(cd);
    let conn = Connection::open(cd.db_path()).map_err(|e| WcError::Sqlite(e.to_string()))?;
    ensure_wallpaper_query_indexes(&conn)?;

    let filter_cond = library_filter_condition(query.filter);
    let search = query.search.trim();

    if search.is_empty() {
        let order_by = library_order_by(query.sort, None);
        let where_sql = if filter_cond.is_empty() {
            String::new()
        } else {
            format!("WHERE {filter_cond}")
        };

        let total: i64 = conn
            .query_row(
                &format!("SELECT COUNT(*) FROM wallpapers {where_sql}"),
                [],
                |row| row.get(0),
            )
            .map_err(|e| WcError::Sqlite(e.to_string()))?;

        let sql = format!(
            "SELECT path, type, ext, backend, size, mtime, resolution,
                    project_type, preview_path, workshop_id, title, we_file, unsupported_reason
             FROM wallpapers
             {where_sql}
             ORDER BY {order_by}
             LIMIT ?1 OFFSET ?2"
        );
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        let items = stmt
            .query_map(
                params![
                    i64::try_from(query.limit).unwrap_or(i64::MAX),
                    i64::try_from(query.offset).unwrap_or(i64::MAX)
                ],
                wallpaper_entry_from_row,
            )
            .map_err(|e| WcError::Sqlite(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| WcError::Sqlite(e.to_string()))?;

        Ok(LibraryPage {
            total: total.max(0) as usize,
            items,
        })
    } else {
        let fts = fts_query(search);
        if fts.is_empty() {
            // All terms were filtered out by sanitizer, fall back to empty search
            return library_page_sqlite(
                cd,
                &LibraryPageQuery {
                    search: String::new(),
                    ..query.clone()
                },
            );
        }
        let order_by = library_order_by(query.sort, Some("w"));

        let where_sql = if filter_cond.is_empty() {
            "WHERE wallpapers_fts MATCH ?1".to_string()
        } else {
            format!("WHERE wallpapers_fts MATCH ?1 AND {filter_cond}")
        };

        let total: i64 = conn
            .query_row(
                &format!(
                    "SELECT COUNT(*) FROM wallpapers w JOIN wallpapers_fts ON wallpapers_fts.rowid = w.id {where_sql}"
                ),
                params![&fts],
                |row| row.get(0),
            )
            .map_err(|e| WcError::Sqlite(e.to_string()))?;

        let sql = format!(
            "SELECT w.path, w.type, w.ext, w.backend, w.size, w.mtime, w.resolution,
                    w.project_type, w.preview_path, w.workshop_id, w.title, w.we_file, w.unsupported_reason
             FROM wallpapers w
             JOIN wallpapers_fts ON wallpapers_fts.rowid = w.id
             {where_sql}
             ORDER BY {order_by}
             LIMIT ?2 OFFSET ?3"
        );
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        let items = stmt
            .query_map(
                params![
                    &fts,
                    i64::try_from(query.limit).unwrap_or(i64::MAX),
                    i64::try_from(query.offset).unwrap_or(i64::MAX)
                ],
                wallpaper_entry_from_row,
            )
            .map_err(|e| WcError::Sqlite(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| WcError::Sqlite(e.to_string()))?;

        Ok(LibraryPage {
            total: total.max(0) as usize,
            items,
        })
    }
}

pub fn library_counts_sqlite(cd: &ConfigDir) -> Result<wc_core::types::LibraryCounts, WcError> {
    ensure_sqlite_db(cd);
    let conn = Connection::open(cd.db_path()).map_err(|e| WcError::Sqlite(e.to_string()))?;
    let mut counts = wc_core::types::LibraryCounts {
        total: 0,
        images: 0,
        gifs: 0,
        videos: 0,
    };
    let mut stmt = conn
        .prepare("SELECT type, COUNT(*) FROM wallpapers GROUP BY type")
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    for row in rows {
        let (kind, count) = row.map_err(|e| WcError::Sqlite(e.to_string()))?;
        let count = count.max(0) as usize;
        counts.total += count;
        match kind.as_str() {
            "image" => counts.images = count,
            "gif" => counts.gifs = count,
            "video" => counts.videos = count,
            _ => {}
        }
    }
    Ok(counts)
}

pub fn favorites_page_sqlite(
    cd: &ConfigDir,
    offset: usize,
    limit: usize,
) -> Result<LibraryPage, WcError> {
    ensure_sqlite_db(cd);
    let conn = Connection::open(cd.db_path()).map_err(|e| WcError::Sqlite(e.to_string()))?;
    ensure_wallpaper_query_indexes(&conn)?;
    let total: i64 = conn
        .query_row(
            "SELECT COUNT(*)
             FROM favorites f
             INNER JOIN wallpapers w ON w.path = f.path",
            [],
            |row| row.get(0),
        )
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    let mut stmt = conn
        .prepare(
            "SELECT w.path, w.type, w.ext, w.backend, w.size, w.mtime, w.resolution,
                    w.project_type, w.preview_path, w.workshop_id, w.title, w.we_file, w.unsupported_reason
             FROM favorites f
             INNER JOIN wallpapers w ON w.path = f.path
             ORDER BY w.mtime DESC, w.path ASC
             LIMIT ?1 OFFSET ?2",
        )
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    let items = stmt
        .query_map(
            params![
                i64::try_from(limit).unwrap_or(i64::MAX),
                i64::try_from(offset).unwrap_or(i64::MAX)
            ],
            wallpaper_entry_from_row,
        )
        .map_err(|e| WcError::Sqlite(e.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    Ok(LibraryPage {
        total: total.max(0) as usize,
        items,
    })
}

pub fn history_page_sqlite(
    cd: &ConfigDir,
    offset: usize,
    limit: usize,
) -> Result<LibraryPage, WcError> {
    ensure_sqlite_db(cd);
    let conn = Connection::open(cd.db_path()).map_err(|e| WcError::Sqlite(e.to_string()))?;
    ensure_wallpaper_query_indexes(&conn)?;
    let total: i64 = conn
        .query_row(
            "SELECT COUNT(*)
             FROM history h
             INNER JOIN wallpapers w ON w.path = h.path",
            [],
            |row| row.get(0),
        )
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    let mut stmt = conn
        .prepare(
            "SELECT w.path, w.type, w.ext, w.backend, w.size, w.mtime, w.resolution,
                    w.project_type, w.preview_path, w.workshop_id, w.title, w.we_file, w.unsupported_reason
             FROM history h
             INNER JOIN wallpapers w ON w.path = h.path
             ORDER BY h.id DESC
             LIMIT ?1 OFFSET ?2",
        )
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    let items = stmt
        .query_map(
            params![
                i64::try_from(limit).unwrap_or(i64::MAX),
                i64::try_from(offset).unwrap_or(i64::MAX)
            ],
            wallpaper_entry_from_row,
        )
        .map_err(|e| WcError::Sqlite(e.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    Ok(LibraryPage {
        total: total.max(0) as usize,
        items,
    })
}

fn fts_query(raw: &str) -> String {
    raw.split_whitespace()
        .filter(|part| !part.is_empty())
        .map(|part| {
            let cleaned = part
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-' || *c == '.')
                .collect::<String>();
            let trimmed = cleaned.trim_matches(|c: char| !c.is_alphanumeric());
            if trimmed.is_empty() {
                String::new()
            } else {
                format!("\"{trimmed}\"*")
            }
        })
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn library_filter_condition(filter: LibraryFilter) -> &'static str {
    match filter {
        LibraryFilter::All => "",
        LibraryFilter::Image => "type = 'image'",
        LibraryFilter::Gif => "type = 'gif'",
        LibraryFilter::Video => "type = 'video'",
        LibraryFilter::We => "type IN ('we_scene', 'we_web')",
        LibraryFilter::WeScene => "type = 'we_scene'",
        LibraryFilter::WeWeb => "type = 'we_web'",
        LibraryFilter::Unsupported => "type = 'unsupported'",
    }
}

fn library_order_by(sort: LibrarySort, table_prefix: Option<&str>) -> String {
    let prefix = table_prefix.map(|s| format!("{s}.")).unwrap_or_default();
    match sort {
        LibrarySort::Newest => format!(
            "CASE WHEN {prefix}type = 'we_web' THEN 1 WHEN {prefix}type = 'unsupported' THEN 2 ELSE 0 END ASC,
             {prefix}mtime DESC, {prefix}path ASC"
        ),
        LibrarySort::Largest => format!(
            "CASE WHEN {prefix}type = 'we_web' THEN 1 WHEN {prefix}type = 'unsupported' THEN 2 ELSE 0 END ASC,
             {prefix}size DESC, {prefix}path ASC"
        ),
        LibrarySort::Name => format!(
            "CASE WHEN {prefix}type = 'we_web' THEN 1 WHEN {prefix}type = 'unsupported' THEN 2 ELSE 0 END ASC,
             {prefix}path ASC"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn insert_wallpaper_for_page_test(
        conn: &rusqlite::Connection,
        path: &str,
        kind: &str,
        size: i64,
        mtime: i64,
        title: &str,
        workshop_id: &str,
    ) {
        conn.execute(
            "INSERT INTO wallpapers
             (path, type, ext, backend, size, mtime, resolution, project_type, preview_path, workshop_id, title, we_file, unsupported_reason)
             VALUES (?1, ?2, 'jpg', 'awww', ?3, ?4, '1920x1080', '', '', ?5, ?6, '', '')",
            rusqlite::params![path, kind, size, mtime, workshop_id, title],
        )
        .unwrap();
    }

    #[test]
    fn library_page_sqlite_filters_sorts_and_limits_without_full_table_callers() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = wc_core::ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        ensure_sqlite_db(&cd);
        let conn = rusqlite::Connection::open(cd.db_path()).unwrap();
        insert_wallpaper_for_page_test(&conn, "/walls/a.png", "image", 100, 1000, "Alpha", "");
        insert_wallpaper_for_page_test(&conn, "/walls/b.png", "image", 300, 3000, "Beta", "");
        insert_wallpaper_for_page_test(&conn, "/walls/c.mp4", "video", 200, 2000, "Clip", "");

        let page = library_page_sqlite(
            &cd,
            &LibraryPageQuery {
                filter: LibraryFilter::Image,
                sort: LibrarySort::Name,
                search: ".png".to_string(),
                offset: 1,
                limit: 1,
            },
        )
        .unwrap();

        assert_eq!(page.total, 2);
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].path.as_str(), "/walls/b.png");
    }

    #[test]
    fn library_page_sqlite_keeps_we_web_and_unsupported_after_normal_items() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = wc_core::ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        ensure_sqlite_db(&cd);
        let conn = rusqlite::Connection::open(cd.db_path()).unwrap();
        insert_wallpaper_for_page_test(&conn, "/walls/web", "we_web", 999, 9000, "Web", "1");
        insert_wallpaper_for_page_test(&conn, "/walls/app", "unsupported", 999, 8000, "App", "2");
        insert_wallpaper_for_page_test(&conn, "/walls/image.jpg", "image", 100, 1000, "Image", "");

        let page = library_page_sqlite(
            &cd,
            &LibraryPageQuery {
                filter: LibraryFilter::All,
                sort: LibrarySort::Newest,
                search: String::new(),
                offset: 0,
                limit: 10,
            },
        )
        .unwrap();

        let kinds: Vec<&str> = page
            .items
            .iter()
            .map(|entry| entry.file_type.as_str())
            .collect();
        assert_eq!(kinds, vec!["image", "we_web", "unsupported"]);
    }

    #[test]
    fn favorites_and_history_page_sqlite_join_to_wallpaper_metadata() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = wc_core::ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        ensure_sqlite_db(&cd);
        let conn = rusqlite::Connection::open(cd.db_path()).unwrap();
        insert_wallpaper_for_page_test(&conn, "/walls/a.jpg", "image", 100, 1000, "A", "");
        insert_wallpaper_for_page_test(&conn, "/walls/b.jpg", "image", 200, 2000, "B", "");
        conn.execute("INSERT INTO favorites (path) VALUES ('/walls/a.jpg')", [])
            .unwrap();
        conn.execute(
            "INSERT INTO history (path, backend) VALUES ('/walls/a.jpg', 'awww')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO history (path, backend) VALUES ('/walls/b.jpg', 'awww')",
            [],
        )
        .unwrap();

        let favs = favorites_page_sqlite(&cd, 0, 10).unwrap();
        assert_eq!(favs.total, 1);
        assert_eq!(favs.items[0].size, 100);

        let hist = history_page_sqlite(&cd, 0, 1).unwrap();
        assert_eq!(hist.total, 2);
        assert_eq!(hist.items.len(), 1);
        assert_eq!(hist.items[0].path.as_str(), "/walls/b.jpg");
    }

    #[test]
    fn fts_query_strips_sql_syntax_and_adds_prefix_suffix() {
        assert_eq!(fts_query("forest scene"), "\"forest\"* \"scene\"*");
        assert_eq!(fts_query("abc' OR 1=1 --"), "\"abc\"* \"OR\"* \"11\"*");
    }

    #[test]
    fn library_page_search_uses_title_and_workshop_id() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = wc_core::ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        crate::sqlite::ensure_sqlite_db(&cd);
        let conn = rusqlite::Connection::open(cd.db_path()).unwrap();
        conn.execute(
            "INSERT INTO wallpapers (path, type, ext, backend, size, mtime, resolution, title, workshop_id)
             VALUES ('/walls/a.jpg', 'image', 'jpg', 'awww', 1, 1, '1x1', 'Blue Forest', '123456')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO wallpapers (path, type, ext, backend, size, mtime, resolution, title, workshop_id)
             VALUES ('/walls/b.jpg', 'image', 'jpg', 'awww', 1, 2, '1x1', 'Red City', '999999')",
            [],
        )
        .unwrap();

        let page = library_page_sqlite(
            &cd,
            &LibraryPageQuery {
                filter: LibraryFilter::All,
                sort: LibrarySort::Newest,
                search: "forest".into(),
                offset: 0,
                limit: 20,
            },
        )
        .unwrap();

        assert_eq!(page.total, 1);
        assert_eq!(
            page.items[0]
                .project
                .as_ref()
                .and_then(|p| p.title.as_deref()),
            Some("Blue Forest")
        );
    }
}
