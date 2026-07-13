use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension};
use std::collections::HashMap;
use wc_core::config::ConfigDir;
use wc_core::error::WcError;
use wc_core::types::WallpaperEntry;

use super::row_map::wallpaper_entry_from_row;
use super::schema::open_runtime_connection;

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

/// User-facing wallpaper categories for the unified library browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryBrowserType {
    Usable,
    Image,
    Gif,
    Video,
    WeScene,
    Unsupported,
}

/// Stable sort orders supported by the unified library browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryBrowserSort {
    RecentlyAdded,
    NameAsc,
    NameDesc,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryBrowserQuery {
    pub source_id: Option<i64>,
    pub type_filter: LibraryBrowserType,
    pub favorites_only: bool,
    pub search: String,
    pub sort: LibraryBrowserSort,
    pub offset: usize,
    pub limit: usize,
}

impl Default for LibraryBrowserQuery {
    fn default() -> Self {
        Self {
            source_id: None,
            type_filter: LibraryBrowserType::Usable,
            favorites_only: false,
            search: String::new(),
            sort: LibraryBrowserSort::RecentlyAdded,
            offset: 0,
            limit: 100,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryBrowserSource {
    pub id: i64,
    pub display_name: String,
}

#[derive(Debug, Clone)]
pub struct LibraryBrowserItem {
    pub wallpaper_id: i64,
    pub entry: WallpaperEntry,
    pub favorite: bool,
    pub author: Option<String>,
    pub added_at: String,
    pub sources: Vec<LibraryBrowserSource>,
}

#[derive(Debug, Clone)]
pub struct LibraryBrowserPage {
    pub total: usize,
    pub items: Vec<LibraryBrowserItem>,
}

pub fn library_count(cd: &ConfigDir) -> Result<usize, WcError> {
    let db_path = cd.db_path();
    if !db_path.exists() {
        return Ok(0);
    }
    let conn = open_runtime_connection(cd)?;
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM wallpapers", [], |row| row.get(0))
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    Ok(count as usize)
}

/// Count unique wallpaper rows that still belong to at least one configured source.
///
/// Unlike [`library_count`], this is a user-visible library view rather than a
/// count of every physical metadata row retained for recovery or reconciliation.
pub fn source_backed_library_count(cd: &ConfigDir) -> Result<usize, WcError> {
    let db_path = cd.db_path();
    if !db_path.exists() {
        return Ok(0);
    }
    let conn = open_runtime_connection(cd)?;
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*)
             FROM wallpapers wallpaper
             WHERE EXISTS (
                 SELECT 1 FROM wallpaper_sources membership
                 WHERE membership.wallpaper_id = wallpaper.id
             )",
            [],
            |row| row.get(0),
        )
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    Ok(count.max(0) as usize)
}

fn empty_library_page() -> LibraryPage {
    LibraryPage {
        total: 0,
        items: Vec::new(),
    }
}

pub fn library_page_sqlite(
    cd: &ConfigDir,
    query: &LibraryPageQuery,
) -> Result<LibraryPage, WcError> {
    library_page_sqlite_with_scope(cd, query, LibraryRowScope::AllPhysicalRows)
}

/// Page unique wallpaper rows that still belong to at least one configured source.
///
/// Multiple source memberships are collapsed by the `EXISTS` predicate, while
/// metadata rows with no membership remain available to lower-level repair and
/// reconciliation APIs without leaking into user-visible library results.
pub fn source_backed_library_page_sqlite(
    cd: &ConfigDir,
    query: &LibraryPageQuery,
) -> Result<LibraryPage, WcError> {
    library_page_sqlite_with_scope(cd, query, LibraryRowScope::SourceBackedRows)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LibraryRowScope {
    AllPhysicalRows,
    SourceBackedRows,
}

impl LibraryRowScope {
    fn condition(self, wallpaper_alias: &str) -> Option<String> {
        match self {
            Self::AllPhysicalRows => None,
            Self::SourceBackedRows => Some(format!(
                "EXISTS (
                     SELECT 1 FROM wallpaper_sources membership
                     WHERE membership.wallpaper_id = {wallpaper_alias}.id
                 )"
            )),
        }
    }
}

fn sql_where_clause(conditions: Vec<String>) -> String {
    if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    }
}

fn library_page_sqlite_with_scope(
    cd: &ConfigDir,
    query: &LibraryPageQuery,
    scope: LibraryRowScope,
) -> Result<LibraryPage, WcError> {
    if !cd.db_path().exists() {
        return Ok(empty_library_page());
    }
    let conn = open_runtime_connection(cd)?;

    let filter_cond = library_filter_condition(query.filter);
    let search = query.search.trim();

    if search.is_empty() {
        let order_by = library_order_by(query.sort, None);
        let mut conditions = Vec::new();
        if !filter_cond.is_empty() {
            conditions.push(filter_cond.to_string());
        }
        conditions.extend(scope.condition("wallpapers"));
        let where_sql = sql_where_clause(conditions);

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
            return library_page_sqlite_with_scope(
                cd,
                &LibraryPageQuery {
                    search: String::new(),
                    ..query.clone()
                },
                scope,
            );
        }
        let order_by = library_order_by(query.sort, Some("w"));

        let mut conditions = vec!["wallpapers_fts MATCH ?1".to_string()];
        if !filter_cond.is_empty() {
            conditions.push(filter_cond.to_string());
        }
        conditions.extend(scope.condition("w"));
        let where_sql = sql_where_clause(conditions);

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

#[derive(Debug, Clone)]
struct BrowserPredicate {
    where_sql: String,
    params: Vec<Value>,
}

fn push_browser_param(params: &mut Vec<Value>, value: Value) -> String {
    params.push(value);
    format!("?{}", params.len())
}

fn escape_like_term(term: &str) -> String {
    let mut escaped = String::with_capacity(term.len());
    for character in term.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '%' => escaped.push_str("\\%"),
            '_' => escaped.push_str("\\_"),
            _ => escaped.push(character),
        }
    }
    format!("%{escaped}%")
}

fn browser_type_condition(filter: LibraryBrowserType) -> &'static str {
    match filter {
        LibraryBrowserType::Usable => "w.type IN ('image', 'gif', 'video', 'we_scene')",
        LibraryBrowserType::Image => "w.type = 'image'",
        LibraryBrowserType::Gif => "w.type = 'gif'",
        LibraryBrowserType::Video => "w.type = 'video'",
        LibraryBrowserType::WeScene => "w.type = 'we_scene'",
        LibraryBrowserType::Unsupported => "w.type IN ('we_web', 'unsupported')",
    }
}

/// Build the sole predicate used by browser count, page, and random queries.
///
/// Keeping source membership as `EXISTS` prevents overlapping source rows from
/// multiplying wallpapers. Search deliberately references only user-facing
/// basename/title/author/source-name fields.
fn browser_predicate(query: &LibraryBrowserQuery) -> BrowserPredicate {
    let mut params = Vec::new();
    let mut conditions = vec![
        "EXISTS (
             SELECT 1
             FROM wallpaper_sources visible_membership
             WHERE visible_membership.wallpaper_id = w.id
         )"
        .to_string(),
        browser_type_condition(query.type_filter).to_string(),
    ];

    if let Some(source_id) = query.source_id {
        let placeholder = push_browser_param(&mut params, Value::Integer(source_id));
        conditions.push(format!(
            "EXISTS (
                 SELECT 1
                 FROM wallpaper_sources selected_membership
                 WHERE selected_membership.wallpaper_id = w.id
                   AND selected_membership.source_id = {placeholder}
             )"
        ));
    }
    if query.favorites_only {
        conditions.push(
            "EXISTS (
                 SELECT 1
                 FROM favorites favorite_filter
                 WHERE favorite_filter.path = w.path
             )"
            .to_string(),
        );
    }

    for term in query.search.split_whitespace() {
        let placeholder = push_browser_param(&mut params, Value::Text(escape_like_term(term)));
        conditions.push(format!(
            "(
                 w.filename COLLATE NOCASE LIKE {placeholder} ESCAPE '\\'
                 OR (w.title <> '' AND w.title COLLATE NOCASE LIKE {placeholder} ESCAPE '\\')
                 OR (w.author <> '' AND w.author COLLATE NOCASE LIKE {placeholder} ESCAPE '\\')
                 OR EXISTS (
                     SELECT 1
                     FROM wallpaper_sources search_membership
                     JOIN sources search_source
                       ON search_source.id = search_membership.source_id
                     WHERE search_membership.wallpaper_id = w.id
                       AND search_source.display_name COLLATE NOCASE
                           LIKE {placeholder} ESCAPE '\\'
                 )
             )"
        ));
    }

    BrowserPredicate {
        where_sql: format!("WHERE {}", conditions.join(" AND ")),
        params,
    }
}

fn browser_order_by(sort: LibraryBrowserSort) -> &'static str {
    match sort {
        LibraryBrowserSort::RecentlyAdded => "w.added_at DESC, w.id DESC",
        LibraryBrowserSort::NameAsc => {
            "COALESCE(NULLIF(w.title, ''), w.filename) COLLATE NOCASE ASC, w.path ASC, w.id ASC"
        }
        LibraryBrowserSort::NameDesc => {
            "COALESCE(NULLIF(w.title, ''), w.filename) COLLATE NOCASE DESC, w.path ASC, w.id ASC"
        }
    }
}

fn browser_item_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<LibraryBrowserItem> {
    let entry = wallpaper_entry_from_row(row)?;
    let author = row.get::<_, String>(15)?;
    Ok(LibraryBrowserItem {
        wallpaper_id: row.get(13)?,
        entry,
        favorite: row.get(14)?,
        author: if author.is_empty() {
            None
        } else {
            Some(author)
        },
        added_at: row.get(16)?,
        sources: Vec::new(),
    })
}

const BROWSER_ITEM_SELECT: &str =
    "SELECT w.path, w.type, w.ext, w.backend, w.size, w.mtime, w.resolution,
            w.project_type, w.preview_path, w.workshop_id, w.title, w.we_file,
            w.unsupported_reason, w.id,
            EXISTS (SELECT 1 FROM favorites item_favorite WHERE item_favorite.path = w.path),
            w.author, w.added_at
     FROM wallpapers w";

fn hydrate_browser_sources(
    conn: &Connection,
    items: &mut [LibraryBrowserItem],
) -> Result<(), WcError> {
    if items.is_empty() {
        return Ok(());
    }
    let ids = items
        .iter()
        .map(|item| Value::Integer(item.wallpaper_id))
        .collect::<Vec<_>>();
    let placeholders = (1..=ids.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT membership.wallpaper_id, source.id, source.display_name
         FROM wallpaper_sources membership
         JOIN sources source ON source.id = membership.source_id
         WHERE membership.wallpaper_id IN ({placeholders})
         ORDER BY membership.wallpaper_id ASC, source.id ASC"
    );
    let mut statement = conn
        .prepare(&sql)
        .map_err(|error| WcError::Sqlite(error.to_string()))?;
    let rows = statement
        .query_map(params_from_iter(ids.iter()), |row| {
            Ok((
                row.get::<_, i64>(0)?,
                LibraryBrowserSource {
                    id: row.get(1)?,
                    display_name: row.get(2)?,
                },
            ))
        })
        .map_err(|error| WcError::Sqlite(error.to_string()))?;
    let mut sources_by_wallpaper = HashMap::<i64, Vec<LibraryBrowserSource>>::new();
    for row in rows {
        let (wallpaper_id, source) = row.map_err(|error| WcError::Sqlite(error.to_string()))?;
        sources_by_wallpaper
            .entry(wallpaper_id)
            .or_default()
            .push(source);
    }
    for item in items {
        item.sources = sources_by_wallpaper
            .remove(&item.wallpaper_id)
            .unwrap_or_default();
    }
    Ok(())
}

fn browser_library_page_inner<F>(
    cd: &ConfigDir,
    query: &LibraryBrowserQuery,
    after_count: F,
) -> Result<LibraryBrowserPage, WcError>
where
    F: FnOnce(),
{
    if !cd.db_path().exists() {
        return Ok(LibraryBrowserPage {
            total: 0,
            items: Vec::new(),
        });
    }
    let conn = open_runtime_connection(cd)?;
    // rusqlite's default unchecked transaction is DEFERRED. The count is the
    // first read and pins the snapshot used by row selection and source hydration.
    let transaction = conn
        .unchecked_transaction()
        .map_err(|error| WcError::Sqlite(error.to_string()))?;
    let predicate = browser_predicate(query);
    let total = transaction
        .query_row(
            &format!("SELECT COUNT(*) FROM wallpapers w {}", predicate.where_sql),
            params_from_iter(predicate.params.iter()),
            |row| row.get::<_, i64>(0),
        )
        .map_err(|error| WcError::Sqlite(error.to_string()))?;

    after_count();

    const MAX_BROWSER_PAGE_SIZE: usize = 500;
    let page_limit = query.limit.min(MAX_BROWSER_PAGE_SIZE);
    let mut page_params = predicate.params.clone();
    let limit = push_browser_param(
        &mut page_params,
        Value::Integer(i64::try_from(page_limit).unwrap_or(500)),
    );
    let offset = push_browser_param(
        &mut page_params,
        Value::Integer(i64::try_from(query.offset).unwrap_or(i64::MAX)),
    );
    let sql = format!(
        "{BROWSER_ITEM_SELECT}
         {}
         ORDER BY {}
         LIMIT {limit} OFFSET {offset}",
        predicate.where_sql,
        browser_order_by(query.sort)
    );
    let mut items = {
        let mut statement = transaction
            .prepare(&sql)
            .map_err(|error| WcError::Sqlite(error.to_string()))?;
        let items = statement
            .query_map(params_from_iter(page_params.iter()), browser_item_from_row)
            .map_err(|error| WcError::Sqlite(error.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| WcError::Sqlite(error.to_string()))?;
        items
    };
    hydrate_browser_sources(&transaction, &mut items)?;
    transaction
        .commit()
        .map_err(|error| WcError::Sqlite(error.to_string()))?;

    Ok(LibraryBrowserPage {
        total: total.max(0) as usize,
        items,
    })
}

/// Query the unified, source-backed library without materializing the full table.
pub fn browser_library_page(
    cd: &ConfigDir,
    query: &LibraryBrowserQuery,
) -> Result<LibraryBrowserPage, WcError> {
    browser_library_page_inner(cd, query, || {})
}

#[cfg(test)]
fn browser_library_page_with_after_count<F>(
    cd: &ConfigDir,
    query: &LibraryBrowserQuery,
    after_count: F,
) -> Result<LibraryBrowserPage, WcError>
where
    F: FnOnce(),
{
    browser_library_page_inner(cd, query, after_count)
}

/// Pick one random wallpaper using exactly the same membership and filter
/// predicate as [`browser_library_page`]. Paging fields are intentionally ignored.
pub fn browser_library_random(
    cd: &ConfigDir,
    query: &LibraryBrowserQuery,
) -> Result<Option<LibraryBrowserItem>, WcError> {
    if !cd.db_path().exists() {
        return Ok(None);
    }
    let conn = open_runtime_connection(cd)?;
    let transaction = conn
        .unchecked_transaction()
        .map_err(|error| WcError::Sqlite(error.to_string()))?;
    let predicate = browser_predicate(query);
    let sql = format!(
        "{BROWSER_ITEM_SELECT}
         {}
         ORDER BY RANDOM()
         LIMIT 1",
        predicate.where_sql
    );
    let mut item = transaction
        .query_row(
            &sql,
            params_from_iter(predicate.params.iter()),
            browser_item_from_row,
        )
        .optional()
        .map_err(|error| WcError::Sqlite(error.to_string()))?;
    if let Some(item) = item.as_mut() {
        hydrate_browser_sources(&transaction, std::slice::from_mut(item))?;
    }
    transaction
        .commit()
        .map_err(|error| WcError::Sqlite(error.to_string()))?;
    Ok(item)
}

pub fn library_counts_sqlite(cd: &ConfigDir) -> Result<wc_core::types::LibraryCounts, WcError> {
    library_counts_sqlite_with_scope(cd, LibraryRowScope::AllPhysicalRows)
}

/// Count user-visible wallpapers by type, excluding metadata rows with no source membership.
pub fn source_backed_library_counts_sqlite(
    cd: &ConfigDir,
) -> Result<wc_core::types::LibraryCounts, WcError> {
    library_counts_sqlite_with_scope(cd, LibraryRowScope::SourceBackedRows)
}

fn library_counts_sqlite_with_scope(
    cd: &ConfigDir,
    scope: LibraryRowScope,
) -> Result<wc_core::types::LibraryCounts, WcError> {
    if !cd.db_path().exists() {
        return Ok(wc_core::types::LibraryCounts::default());
    }
    let conn = open_runtime_connection(cd)?;
    let mut counts = wc_core::types::LibraryCounts {
        total: 0,
        images: 0,
        gifs: 0,
        videos: 0,
    };
    let where_sql = sql_where_clause(scope.condition("wallpapers").into_iter().collect());
    let mut stmt = conn
        .prepare(&format!(
            "SELECT type, COUNT(*) FROM wallpapers {where_sql} GROUP BY type"
        ))
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
    if !cd.db_path().exists() {
        return Ok(empty_library_page());
    }
    let conn = open_runtime_connection(cd)?;
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
    use crate::sqlite::ensure_sqlite_db;
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

    fn attach_wallpaper_to_source(
        conn: &rusqlite::Connection,
        wallpaper_path: &str,
        source_id: i64,
    ) {
        conn.execute(
            "INSERT INTO wallpaper_sources (wallpaper_id, source_id)
             SELECT id, ?2 FROM wallpapers WHERE path = ?1",
            rusqlite::params![wallpaper_path, source_id],
        )
        .unwrap();
    }

    #[test]
    fn source_backed_library_excludes_orphans_and_deduplicates_overlapping_memberships() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = wc_core::ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        ensure_sqlite_db(&cd);
        let first_root = tmp.path().join("first");
        let second_root = tmp.path().join("second");
        std::fs::create_dir(&first_root).unwrap();
        std::fs::create_dir(&second_root).unwrap();
        let (first, _) = crate::sqlite::source_create(&cd, &first_root.to_string_lossy()).unwrap();
        let (second, _) =
            crate::sqlite::source_create(&cd, &second_root.to_string_lossy()).unwrap();
        let conn = rusqlite::Connection::open(cd.db_path()).unwrap();
        insert_wallpaper_for_page_test(
            &conn,
            "/walls/member.jpg",
            "image",
            100,
            1000,
            "Member",
            "",
        );
        insert_wallpaper_for_page_test(
            &conn,
            "/walls/orphan.jpg",
            "image",
            200,
            2000,
            "Orphan",
            "",
        );
        attach_wallpaper_to_source(&conn, "/walls/member.jpg", first.id);
        attach_wallpaper_to_source(&conn, "/walls/member.jpg", second.id);

        assert_eq!(
            library_count(&cd).unwrap(),
            2,
            "physical API stays unchanged"
        );
        assert_eq!(source_backed_library_count(&cd).unwrap(), 1);
        let counts = source_backed_library_counts_sqlite(&cd).unwrap();
        assert_eq!(counts.total, 1);
        assert_eq!(counts.images, 1);
        assert_eq!(counts.gifs, 0);
        assert_eq!(counts.videos, 0);
        let page = source_backed_library_page_sqlite(
            &cd,
            &LibraryPageQuery {
                filter: LibraryFilter::All,
                sort: LibrarySort::Name,
                search: String::new(),
                offset: 0,
                limit: 10,
            },
        )
        .unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].path.as_str(), "/walls/member.jpg");
        assert_eq!(
            library_page_sqlite(
                &cd,
                &LibraryPageQuery {
                    filter: LibraryFilter::All,
                    sort: LibrarySort::Name,
                    search: String::new(),
                    offset: 0,
                    limit: 10,
                },
            )
            .unwrap()
            .total,
            2,
            "legacy physical page API stays unchanged"
        );
    }

    #[test]
    fn source_backed_library_search_does_not_match_orphans() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = wc_core::ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        ensure_sqlite_db(&cd);
        let root = tmp.path().join("walls");
        std::fs::create_dir(&root).unwrap();
        let (source, _) = crate::sqlite::source_create(&cd, &root.to_string_lossy()).unwrap();
        let conn = rusqlite::Connection::open(cd.db_path()).unwrap();
        insert_wallpaper_for_page_test(
            &conn,
            "/walls/member.jpg",
            "image",
            100,
            1000,
            "Forest Member",
            "",
        );
        insert_wallpaper_for_page_test(
            &conn,
            "/walls/orphan.jpg",
            "image",
            200,
            2000,
            "Forest Orphan",
            "",
        );
        attach_wallpaper_to_source(&conn, "/walls/member.jpg", source.id);

        let page = source_backed_library_page_sqlite(
            &cd,
            &LibraryPageQuery {
                filter: LibraryFilter::All,
                sort: LibrarySort::Name,
                search: "forest".into(),
                offset: 0,
                limit: 10,
            },
        )
        .unwrap();

        assert_eq!(page.total, 1);
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].path.as_str(), "/walls/member.jpg");
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
    fn favorites_page_sqlite_joins_to_wallpaper_metadata() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = wc_core::ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        ensure_sqlite_db(&cd);
        let conn = rusqlite::Connection::open(cd.db_path()).unwrap();
        insert_wallpaper_for_page_test(&conn, "/walls/a.jpg", "image", 100, 1000, "A", "");
        conn.execute("INSERT INTO favorites (path) VALUES ('/walls/a.jpg')", [])
            .unwrap();

        let favs = favorites_page_sqlite(&cd, 0, 10).unwrap();
        assert_eq!(favs.total, 1);
        assert_eq!(favs.items[0].size, 100);
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

#[cfg(test)]
mod browser_tests {
    use super::*;
    use crate::sqlite::ensure_sqlite_db;
    use rusqlite::{params, Connection};
    use std::collections::BTreeSet;

    fn fixture() -> (tempfile::TempDir, ConfigDir) {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        ensure_sqlite_db(&cd);
        let conn = Connection::open(cd.db_path()).unwrap();
        conn.execute_batch(
            "INSERT INTO sources
                 (id, path, display_name, kind, recursive, availability)
             VALUES
                 (1, '/private/secret-root', 'Alpha Catalog', 'directory', 1, 'available'),
                 (2, '/mnt/other-place', 'City Vault', 'directory', 1, 'available');",
        )
        .unwrap();
        (tmp, cd)
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_browser_wallpaper(
        conn: &Connection,
        id: i64,
        path: &str,
        kind: &str,
        title: &str,
        author: &str,
        added_at: &str,
        workshop_id: &str,
        project_type: &str,
    ) {
        conn.execute(
            "INSERT INTO wallpapers
             (id, path, type, ext, backend, size, mtime, resolution,
              project_type, preview_path, workshop_id, title, we_file,
              unsupported_reason, added_at, author)
             VALUES (?1, ?2, ?3, 'dat', 'awww', ?1 * 10, ?1 * 100,
                     '1920x1080', ?8, '', ?7, ?4, '', '', ?6, ?5)",
            params![
                id,
                path,
                kind,
                title,
                author,
                added_at,
                workshop_id,
                project_type
            ],
        )
        .unwrap();
    }

    fn attach(conn: &Connection, wallpaper_id: i64, source_id: i64) {
        conn.execute(
            "INSERT INTO wallpaper_sources (wallpaper_id, source_id)
             VALUES (?1, ?2)",
            params![wallpaper_id, source_id],
        )
        .unwrap();
    }

    fn query(
        source_id: Option<i64>,
        type_filter: LibraryBrowserType,
        favorites_only: bool,
        search: &str,
        sort: LibraryBrowserSort,
        offset: usize,
        limit: usize,
    ) -> LibraryBrowserQuery {
        LibraryBrowserQuery {
            source_id,
            type_filter,
            favorites_only,
            search: search.into(),
            sort,
            offset,
            limit,
        }
    }

    #[test]
    fn browser_query_composes_source_type_favorite_and_search_terms() {
        let (_tmp, cd) = fixture();
        let conn = Connection::open(cd.db_path()).unwrap();
        insert_browser_wallpaper(
            &conn,
            10,
            "/private/nebula_scene.jpg",
            "image",
            "Blue Forest",
            "Ada Lovelace",
            "2025-03-01",
            "123456",
            "scene",
        );
        insert_browser_wallpaper(
            &conn,
            11,
            "/private/not-favorite.jpg",
            "image",
            "Blue Forest",
            "Ada Lovelace",
            "2025-03-02",
            "",
            "",
        );
        insert_browser_wallpaper(
            &conn,
            12,
            "/private/nebula_scene.mp4",
            "video",
            "Blue Forest",
            "Ada Lovelace",
            "2025-03-03",
            "",
            "",
        );
        attach(&conn, 10, 1);
        attach(&conn, 10, 2);
        attach(&conn, 11, 1);
        attach(&conn, 12, 1);
        conn.execute(
            "INSERT INTO favorites (path) VALUES ('/private/nebula_scene.jpg')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO favorites (path) VALUES ('/private/not-favorite.jpg')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO favorites (path) VALUES ('/private/nebula_scene.mp4')",
            [],
        )
        .unwrap();

        let page = browser_library_page(
            &cd,
            &query(
                Some(1),
                LibraryBrowserType::Image,
                true,
                "Blue Ada City nebula_scene.jpg",
                LibraryBrowserSort::RecentlyAdded,
                0,
                20,
            ),
        )
        .unwrap();

        assert_eq!(page.total, 1);
        assert_eq!(page.items.len(), 1);
        let item = &page.items[0];
        assert_eq!(item.wallpaper_id, 10);
        assert_eq!(item.entry.path.as_str(), "/private/nebula_scene.jpg");
        assert!(item.favorite);
        assert_eq!(item.author.as_deref(), Some("Ada Lovelace"));
        assert_eq!(item.added_at, "2025-03-01");
        assert_eq!(
            item.sources,
            vec![
                LibraryBrowserSource {
                    id: 1,
                    display_name: "Alpha Catalog".into(),
                },
                LibraryBrowserSource {
                    id: 2,
                    display_name: "City Vault".into(),
                },
            ]
        );
    }

    #[test]
    fn browser_type_groups_require_membership_and_exclude_private_metadata_fields() {
        let (_tmp, cd) = fixture();
        let conn = Connection::open(cd.db_path()).unwrap();
        for (id, kind) in [
            (1, "image"),
            (2, "gif"),
            (3, "video"),
            (4, "we_scene"),
            (5, "we_web"),
            (6, "unsupported"),
            (7, "we_application"),
        ] {
            insert_browser_wallpaper(
                &conn,
                id,
                &format!("/private/secret-root/{kind}-{id}.dat"),
                kind,
                "Public title",
                "",
                &format!("2025-01-{id:02}"),
                "777777",
                "hidden-project-kind",
            );
            attach(&conn, id, 1);
        }
        insert_browser_wallpaper(
            &conn,
            8,
            "/private/orphan.jpg",
            "image",
            "Orphan",
            "",
            "2025-01-08",
            "",
            "",
        );

        let usable = browser_library_page(
            &cd,
            &query(
                None,
                LibraryBrowserType::Usable,
                false,
                "",
                LibraryBrowserSort::RecentlyAdded,
                0,
                20,
            ),
        )
        .unwrap();
        assert_eq!(usable.total, 4);
        assert_eq!(
            usable
                .items
                .iter()
                .map(|item| item.entry.file_type.as_str())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["gif", "image", "video", "we_scene"])
        );
        let unsupported = browser_library_page(
            &cd,
            &query(
                None,
                LibraryBrowserType::Unsupported,
                false,
                "",
                LibraryBrowserSort::RecentlyAdded,
                0,
                20,
            ),
        )
        .unwrap();
        assert_eq!(unsupported.total, 2);
        assert_eq!(
            unsupported
                .items
                .iter()
                .map(|item| item.wallpaper_id)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([5, 6])
        );
        for (filter, expected_id) in [
            (LibraryBrowserType::Image, 1),
            (LibraryBrowserType::Gif, 2),
            (LibraryBrowserType::Video, 3),
            (LibraryBrowserType::WeScene, 4),
        ] {
            let page = browser_library_page(
                &cd,
                &query(
                    None,
                    filter,
                    false,
                    "",
                    LibraryBrowserSort::RecentlyAdded,
                    0,
                    20,
                ),
            )
            .unwrap();
            assert_eq!(page.total, 1);
            assert_eq!(page.items[0].wallpaper_id, expected_id);
            assert_eq!(page.items[0].author, None);
            assert!(!page.items[0].favorite);
        }
        for private_term in ["secret-root", "/private", "777777", "hidden-project-kind"] {
            let page = browser_library_page(
                &cd,
                &query(
                    None,
                    LibraryBrowserType::Usable,
                    false,
                    private_term,
                    LibraryBrowserSort::RecentlyAdded,
                    0,
                    20,
                ),
            )
            .unwrap();
            assert_eq!(page.total, 0, "private search term leaked: {private_term}");
        }
    }

    #[test]
    fn browser_search_treats_like_metacharacters_and_backslash_literally() {
        let (_tmp, cd) = fixture();
        let conn = Connection::open(cd.db_path()).unwrap();
        for (id, filename) in [
            (1, "literal%_back\\slash.jpg"),
            (2, "literalXXbackslash.jpg"),
            (3, "plain_percent.jpg"),
            (4, "artist's-choice.jpg"),
        ] {
            insert_browser_wallpaper(
                &conn,
                id,
                &format!("/walls/{filename}"),
                "image",
                "",
                "",
                "2025-01-01",
                "",
                "",
            );
            attach(&conn, id, 1);
        }

        let page = browser_library_page(
            &cd,
            &query(
                None,
                LibraryBrowserType::Image,
                false,
                "%_back\\slash",
                LibraryBrowserSort::NameAsc,
                0,
                20,
            ),
        )
        .unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].wallpaper_id, 1);

        let apostrophe = browser_library_page(
            &cd,
            &query(
                None,
                LibraryBrowserType::Image,
                false,
                "artist's",
                LibraryBrowserSort::NameAsc,
                0,
                20,
            ),
        )
        .unwrap();
        assert_eq!(apostrophe.total, 1);
        assert_eq!(apostrophe.items[0].wallpaper_id, 4);
    }

    #[test]
    fn browser_sorts_by_added_at_or_title_fallback_and_pages_stably() {
        let (_tmp, cd) = fixture();
        let conn = Connection::open(cd.db_path()).unwrap();
        for (id, filename, title, added_at) in [
            (10, "zulu.jpg", "Bravo", "2025-04-01"),
            (11, "alpha", "", "2025-04-01"),
            (12, "other.jpg", "ALPHA", "2025-05-01"),
            (13, "charlie.jpg", "charlie", "2025-03-01"),
        ] {
            insert_browser_wallpaper(
                &conn,
                id,
                &format!("/walls/{filename}"),
                "image",
                title,
                "",
                added_at,
                "",
                "",
            );
            attach(&conn, id, 1);
        }

        let recent = browser_library_page(
            &cd,
            &query(
                None,
                LibraryBrowserType::Usable,
                false,
                "",
                LibraryBrowserSort::RecentlyAdded,
                0,
                20,
            ),
        )
        .unwrap();
        assert_eq!(
            recent
                .items
                .iter()
                .map(|item| item.wallpaper_id)
                .collect::<Vec<_>>(),
            vec![12, 11, 10, 13]
        );

        let name_page = browser_library_page(
            &cd,
            &query(
                None,
                LibraryBrowserType::Image,
                false,
                "",
                LibraryBrowserSort::NameAsc,
                1,
                2,
            ),
        )
        .unwrap();
        assert_eq!(name_page.total, 4);
        assert_eq!(
            name_page
                .items
                .iter()
                .map(|item| item.wallpaper_id)
                .collect::<Vec<_>>(),
            vec![12, 10]
        );

        let descending = browser_library_page(
            &cd,
            &query(
                None,
                LibraryBrowserType::Image,
                false,
                "",
                LibraryBrowserSort::NameDesc,
                0,
                20,
            ),
        )
        .unwrap();
        assert_eq!(
            descending
                .items
                .iter()
                .map(|item| item.wallpaper_id)
                .collect::<Vec<_>>(),
            vec![13, 10, 11, 12]
        );

        let count_only = browser_library_page(
            &cd,
            &query(
                None,
                LibraryBrowserType::Image,
                false,
                "",
                LibraryBrowserSort::NameAsc,
                0,
                0,
            ),
        )
        .unwrap();
        assert_eq!(count_only.total, 4);
        assert!(count_only.items.is_empty());
    }

    #[test]
    fn browser_random_reuses_the_page_predicate_and_returns_none_for_no_match() {
        let (_tmp, cd) = fixture();
        let conn = Connection::open(cd.db_path()).unwrap();
        insert_browser_wallpaper(
            &conn,
            1,
            "/walls/only-video.mp4",
            "video",
            "Ocean Motion",
            "Mira",
            "2025-01-01",
            "",
            "",
        );
        insert_browser_wallpaper(
            &conn,
            2,
            "/walls/non-favorite-video.mp4",
            "video",
            "Ocean Motion",
            "Mira",
            "2025-01-02",
            "",
            "",
        );
        attach(&conn, 1, 1);
        attach(&conn, 2, 1);
        conn.execute(
            "INSERT INTO favorites (path) VALUES ('/walls/only-video.mp4')",
            [],
        )
        .unwrap();

        let matching = query(
            Some(1),
            LibraryBrowserType::Video,
            true,
            "Ocean Mira Alpha",
            LibraryBrowserSort::NameAsc,
            999,
            0,
        );
        for _ in 0..8 {
            assert_eq!(
                browser_library_random(&cd, &matching)
                    .unwrap()
                    .unwrap()
                    .wallpaper_id,
                1
            );
        }
        let impossible = query(
            Some(999),
            LibraryBrowserType::Video,
            false,
            "",
            LibraryBrowserSort::RecentlyAdded,
            0,
            20,
        );
        assert!(browser_library_random(&cd, &impossible).unwrap().is_none());
    }

    #[test]
    fn browser_page_caps_source_hydration_to_a_bound_parameter_safe_size() {
        let (_tmp, cd) = fixture();
        let conn = Connection::open(cd.db_path()).unwrap();
        let transaction = conn.unchecked_transaction().unwrap();
        for id in 1..=510_i64 {
            insert_browser_wallpaper(
                &transaction,
                id,
                &format!("/walls/{id}.jpg"),
                "image",
                "",
                "",
                "2025-01-01",
                "",
                "",
            );
            attach(&transaction, id, 1);
        }
        transaction.commit().unwrap();

        let page = browser_library_page(
            &cd,
            &query(
                None,
                LibraryBrowserType::Image,
                false,
                "",
                LibraryBrowserSort::RecentlyAdded,
                0,
                usize::MAX,
            ),
        )
        .unwrap();

        assert_eq!(page.total, 510);
        assert_eq!(page.items.len(), 500);
        assert!(page.items.iter().all(|item| item.sources.len() == 1));
    }

    #[test]
    fn browser_page_count_rows_and_sources_share_one_read_snapshot() {
        let (_tmp, cd) = fixture();
        let conn = Connection::open(cd.db_path()).unwrap();
        insert_browser_wallpaper(
            &conn,
            1,
            "/walls/kept.jpg",
            "image",
            "Kept",
            "",
            "2025-01-01",
            "",
            "",
        );
        attach(&conn, 1, 1);
        drop(conn);
        let writer_path = cd.db_path();
        crate::sqlite::reset_runtime_connection_open_count();

        let page = browser_library_page_with_after_count(
            &cd,
            &query(
                None,
                LibraryBrowserType::Usable,
                false,
                "",
                LibraryBrowserSort::RecentlyAdded,
                0,
                20,
            ),
            move || {
                let writer = Connection::open(writer_path).unwrap();
                writer
                    .execute("DELETE FROM wallpaper_sources WHERE wallpaper_id = 1", [])
                    .unwrap();
            },
        )
        .unwrap();

        assert_eq!(page.total, 1);
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].sources.len(), 1);
        assert_eq!(crate::sqlite::runtime_connection_open_count(), 1);
        assert_eq!(
            browser_library_page(
                &cd,
                &query(
                    None,
                    LibraryBrowserType::Usable,
                    false,
                    "",
                    LibraryBrowserSort::RecentlyAdded,
                    0,
                    20,
                ),
            )
            .unwrap()
            .total,
            0
        );
    }
}
