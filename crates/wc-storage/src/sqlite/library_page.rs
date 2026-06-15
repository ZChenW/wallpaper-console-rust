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

    let where_clause = library_where_clause(query.filter);
    let order_by = library_order_by(query.sort);
    let search = query.search.trim();

    let total: i64 = conn
        .query_row(
            &format!("SELECT COUNT(*) FROM wallpapers WHERE {where_clause}"),
            params![search],
            |row| row.get(0),
        )
        .map_err(|e| WcError::Sqlite(e.to_string()))?;

    let sql = format!(
        "SELECT path, type, ext, backend, size, mtime, resolution,
                project_type, preview_path, workshop_id, title, we_file, unsupported_reason
         FROM wallpapers
         WHERE {where_clause}
         ORDER BY {order_by}
         LIMIT ?2 OFFSET ?3"
    );
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    let items = stmt
        .query_map(
            params![
                search,
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

fn library_where_clause(filter: LibraryFilter) -> &'static str {
    match filter {
        LibraryFilter::All => {
            "(?1 = ''
              OR lower(path) LIKE '%' || lower(?1) || '%'
              OR lower(title) LIKE '%' || lower(?1) || '%'
              OR lower(workshop_id) LIKE '%' || lower(?1) || '%'
              OR lower(project_type) LIKE '%' || lower(?1) || '%')"
        }
        LibraryFilter::Image => {
            "type = 'image' AND (?1 = ''
              OR lower(path) LIKE '%' || lower(?1) || '%'
              OR lower(title) LIKE '%' || lower(?1) || '%'
              OR lower(workshop_id) LIKE '%' || lower(?1) || '%'
              OR lower(project_type) LIKE '%' || lower(?1) || '%')"
        }
        LibraryFilter::Gif => {
            "type = 'gif' AND (?1 = ''
              OR lower(path) LIKE '%' || lower(?1) || '%'
              OR lower(title) LIKE '%' || lower(?1) || '%'
              OR lower(workshop_id) LIKE '%' || lower(?1) || '%'
              OR lower(project_type) LIKE '%' || lower(?1) || '%')"
        }
        LibraryFilter::Video => {
            "type = 'video' AND (?1 = ''
              OR lower(path) LIKE '%' || lower(?1) || '%'
              OR lower(title) LIKE '%' || lower(?1) || '%'
              OR lower(workshop_id) LIKE '%' || lower(?1) || '%'
              OR lower(project_type) LIKE '%' || lower(?1) || '%')"
        }
        LibraryFilter::We => {
            "type IN ('we_scene', 'we_web') AND (?1 = ''
              OR lower(path) LIKE '%' || lower(?1) || '%'
              OR lower(title) LIKE '%' || lower(?1) || '%'
              OR lower(workshop_id) LIKE '%' || lower(?1) || '%'
              OR lower(project_type) LIKE '%' || lower(?1) || '%')"
        }
        LibraryFilter::WeScene => {
            "type = 'we_scene' AND (?1 = ''
              OR lower(path) LIKE '%' || lower(?1) || '%'
              OR lower(title) LIKE '%' || lower(?1) || '%'
              OR lower(workshop_id) LIKE '%' || lower(?1) || '%'
              OR lower(project_type) LIKE '%' || lower(?1) || '%')"
        }
        LibraryFilter::WeWeb => {
            "type = 'we_web' AND (?1 = ''
              OR lower(path) LIKE '%' || lower(?1) || '%'
              OR lower(title) LIKE '%' || lower(?1) || '%'
              OR lower(workshop_id) LIKE '%' || lower(?1) || '%'
              OR lower(project_type) LIKE '%' || lower(?1) || '%')"
        }
        LibraryFilter::Unsupported => {
            "type = 'unsupported' AND (?1 = ''
              OR lower(path) LIKE '%' || lower(?1) || '%'
              OR lower(title) LIKE '%' || lower(?1) || '%'
              OR lower(workshop_id) LIKE '%' || lower(?1) || '%'
              OR lower(project_type) LIKE '%' || lower(?1) || '%')"
        }
    }
}

fn library_order_by(sort: LibrarySort) -> &'static str {
    match sort {
        LibrarySort::Newest => {
            "CASE WHEN type = 'we_web' THEN 1 WHEN type = 'unsupported' THEN 2 ELSE 0 END ASC,
             mtime DESC, path ASC"
        }
        LibrarySort::Largest => {
            "CASE WHEN type = 'we_web' THEN 1 WHEN type = 'unsupported' THEN 2 ELSE 0 END ASC,
             size DESC, path ASC"
        }
        LibrarySort::Name => {
            "CASE WHEN type = 'we_web' THEN 1 WHEN type = 'unsupported' THEN 2 ELSE 0 END ASC,
             path ASC"
        }
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
}
