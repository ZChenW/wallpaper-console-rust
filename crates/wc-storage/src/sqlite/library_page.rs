use crate::sqlite_err;
use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
#[cfg(test)]
use wc_config::ConfigDirExt;
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LibraryBrowserType {
    Usable,
    Image,
    Gif,
    Video,
    WeScene,
    Unsupported,
}

/// Stable sort orders supported by the unified library browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LibraryBrowserSort {
    RecentlyAdded,
    NameAsc,
    NameDesc,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LibraryBrowserQuery {
    pub source_id: Option<i64>,
    pub type_filter: LibraryBrowserType,
    pub favorites_only: bool,
    pub search: String,
    pub sort: LibraryBrowserSort,
    pub cursor: Option<String>,
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
            cursor: None,
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
    pub revision: u64,
    pub next_cursor: Option<String>,
    pub total: Option<usize>,
    pub items: Vec<LibraryBrowserItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryBrowserTotal {
    pub revision: u64,
    pub total: usize,
}

const BROWSER_CURSOR_VERSION: u8 = 1;
const MAX_CURSOR_BYTES: usize = 4096;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BrowserCursorV1 {
    version: u8,
    revision: u64,
    fingerprint: String,
    boundary: BrowserCursorBoundary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "sort", rename_all = "snake_case")]
enum BrowserCursorBoundary {
    RecentlyAdded { added_at: String, id: i64 },
    NameAsc { name: String, path: String, id: i64 },
    NameDesc { name: String, path: String, id: i64 },
}

pub fn library_count(cd: &ConfigDir) -> Result<usize, WcError> {
    let db_path = cd.db_path();
    if !db_path.exists() {
        return Ok(0);
    }
    let conn = open_runtime_connection(cd)?;
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM wallpapers", [], |row| row.get(0))
        .map_err(sqlite_err)?;
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
        .map_err(sqlite_err)?;
    Ok(count.max(0) as usize)
}

/// Read the entire source-backed legacy snapshot and its revision from one
/// SQLite read transaction. Callers must recheck the revision before publish.
pub fn source_backed_library_snapshot(
    cd: &ConfigDir,
) -> Result<(u64, Vec<WallpaperEntry>), WcError> {
    if !cd.db_path().exists() {
        return Ok((0, Vec::new()));
    }
    let mut conn = open_runtime_connection(cd)?;
    let transaction = conn.transaction().map_err(sqlite_err)?;
    let revision = super::read_library_revision(&transaction)?;
    let mut statement = transaction
        .prepare(
            "SELECT path, type, ext, backend, size, mtime, resolution,
                    project_type, preview_path, workshop_id, title, we_file, unsupported_reason
             FROM wallpapers wallpaper
             WHERE EXISTS (
                 SELECT 1 FROM wallpaper_sources membership
                 WHERE membership.wallpaper_id = wallpaper.id
             )
             ORDER BY COALESCE(title, path) COLLATE NOCASE ASC, path ASC, id ASC",
        )
        .map_err(sqlite_err)?;
    let entries = statement
        .query_map([], wallpaper_entry_from_row)
        .map_err(sqlite_err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sqlite_err)?;
    drop(statement);
    transaction.commit().map_err(sqlite_err)?;
    Ok((revision, entries))
}

pub fn library_wallpaper_exists(cd: &ConfigDir, wallpaper_id: i64) -> Result<bool, WcError> {
    if !cd.db_path().exists() {
        return Ok(false);
    }
    let connection = open_runtime_connection(cd)?;
    connection
        .query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM wallpaper_sources WHERE wallpaper_id = ?1
             )",
            [wallpaper_id],
            |row| row.get::<_, bool>(0),
        )
        .map_err(sqlite_err)
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
            .map_err(sqlite_err)?;

        let sql = format!(
            "SELECT path, type, ext, backend, size, mtime, resolution,
                    project_type, preview_path, workshop_id, title, we_file, unsupported_reason
             FROM wallpapers
             {where_sql}
             ORDER BY {order_by}
             LIMIT ?1 OFFSET ?2"
        );
        let mut stmt = conn.prepare(&sql).map_err(sqlite_err)?;
        let items = stmt
            .query_map(
                params![
                    i64::try_from(query.limit).unwrap_or(i64::MAX),
                    i64::try_from(query.offset).unwrap_or(i64::MAX)
                ],
                wallpaper_entry_from_row,
            )
            .map_err(sqlite_err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sqlite_err)?;

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
            .map_err(sqlite_err)?;

        let sql = format!(
            "SELECT w.path, w.type, w.ext, w.backend, w.size, w.mtime, w.resolution,
                    w.project_type, w.preview_path, w.workshop_id, w.title, w.we_file, w.unsupported_reason
             FROM wallpapers w
             JOIN wallpapers_fts ON wallpapers_fts.rowid = w.id
             {where_sql}
             ORDER BY {order_by}
             LIMIT ?2 OFFSET ?3"
        );
        let mut stmt = conn.prepare(&sql).map_err(sqlite_err)?;
        let items = stmt
            .query_map(
                params![
                    &fts,
                    i64::try_from(query.limit).unwrap_or(i64::MAX),
                    i64::try_from(query.offset).unwrap_or(i64::MAX)
                ],
                wallpaper_entry_from_row,
            )
            .map_err(sqlite_err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sqlite_err)?;

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

fn normalized_browser_search(search: &str) -> String {
    search.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn browser_type_key(filter: LibraryBrowserType) -> &'static str {
    match filter {
        LibraryBrowserType::Usable => "usable",
        LibraryBrowserType::Image => "image",
        LibraryBrowserType::Gif => "gif",
        LibraryBrowserType::Video => "video",
        LibraryBrowserType::WeScene => "we_scene",
        LibraryBrowserType::Unsupported => "unsupported",
    }
}

fn browser_sort_key(sort: LibraryBrowserSort) -> &'static str {
    match sort {
        LibraryBrowserSort::RecentlyAdded => "recently_added",
        LibraryBrowserSort::NameAsc => "name_asc",
        LibraryBrowserSort::NameDesc => "name_desc",
    }
}

/// Stable, non-cryptographic fingerprint. It binds a cursor to normalized
/// criteria without embedding raw search text in the token.
fn browser_query_fingerprint(query: &LibraryBrowserQuery) -> String {
    let normalized = format!(
        "source={:?}\ntype={}\nfavorite={}\nsearch={}\nsort={}",
        query.source_id,
        browser_type_key(query.type_filter),
        query.favorites_only,
        normalized_browser_search(&query.search),
        browser_sort_key(query.sort),
    );
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in normalized.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[(byte >> 4) as usize]));
        encoded.push(char::from(HEX[(byte & 0x0f) as usize]));
    }
    encoded
}

fn hex_decode(token: &str) -> Result<Vec<u8>, WcError> {
    if token.len() > MAX_CURSOR_BYTES * 2 {
        return Err(WcError::InvalidCursor {
            reason: "token too long",
        });
    }
    if !token.len().is_multiple_of(2) {
        return Err(WcError::InvalidCursor {
            reason: "malformed token",
        });
    }
    fn digit(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            _ => None,
        }
    }
    token
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = digit(pair[0]).ok_or(WcError::InvalidCursor {
                reason: "malformed token",
            })?;
            let low = digit(pair[1]).ok_or(WcError::InvalidCursor {
                reason: "malformed token",
            })?;
            Ok((high << 4) | low)
        })
        .collect()
}

fn encode_browser_cursor(cursor: &BrowserCursorV1) -> Result<String, WcError> {
    serde_json::to_vec(cursor)
        .map(|bytes| hex_encode(&bytes))
        .map_err(|_| WcError::InvalidCursor {
            reason: "encode failed",
        })
}

fn decode_browser_cursor(token: &str) -> Result<BrowserCursorV1, WcError> {
    let bytes = hex_decode(token)?;
    let cursor: BrowserCursorV1 =
        serde_json::from_slice(&bytes).map_err(|_| WcError::InvalidCursor {
            reason: "malformed token",
        })?;
    if cursor.version != BROWSER_CURSOR_VERSION {
        return Err(WcError::InvalidCursor {
            reason: "unsupported version",
        });
    }
    Ok(cursor)
}

/// Build the sole predicate used by browser count, page, and random queries.
///
/// Keeping source membership as `EXISTS` prevents overlapping source rows from
/// multiplying wallpapers. Search deliberately references only user-facing
/// basename/title/author/source-name fields.
fn browser_predicate(query: &LibraryBrowserQuery, use_fts: bool) -> BrowserPredicate {
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
        if use_fts {
            if let Some(fts_term) = super::library_fts::library_fts_match_term(term) {
                let fts_placeholder = push_browser_param(&mut params, Value::Text(fts_term));
                conditions.push(format!(
                    "w.id IN (
                         SELECT rowid FROM library_browser_fts
                         WHERE library_browser_fts MATCH {fts_placeholder}
                     )"
                ));
            }
        }
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

fn append_cursor_boundary(
    predicate: &mut BrowserPredicate,
    sort: LibraryBrowserSort,
    boundary: &BrowserCursorBoundary,
) -> Result<(), WcError> {
    let condition = match (sort, boundary) {
        (
            LibraryBrowserSort::RecentlyAdded,
            BrowserCursorBoundary::RecentlyAdded { added_at, id },
        ) => {
            let added_at = push_browser_param(&mut predicate.params, Value::Text(added_at.clone()));
            let id = push_browser_param(&mut predicate.params, Value::Integer(*id));
            format!("(w.added_at < {added_at} OR (w.added_at = {added_at} AND w.id < {id}))")
        }
        (LibraryBrowserSort::NameAsc, BrowserCursorBoundary::NameAsc { name, path, id }) => {
            name_cursor_condition(&mut predicate.params, name, path, *id, ">")
        }
        (LibraryBrowserSort::NameDesc, BrowserCursorBoundary::NameDesc { name, path, id }) => {
            name_cursor_condition(&mut predicate.params, name, path, *id, "<")
        }
        _ => {
            return Err(WcError::InvalidCursor {
                reason: "sort boundary mismatch",
            })
        }
    };
    predicate.where_sql.push_str(" AND ");
    predicate.where_sql.push_str(&condition);
    Ok(())
}

fn name_cursor_condition(
    params: &mut Vec<Value>,
    name: &str,
    path: &str,
    id: i64,
    name_operator: &str,
) -> String {
    let name = push_browser_param(params, Value::Text(name.to_string()));
    let path = push_browser_param(params, Value::Text(path.to_string()));
    let id = push_browser_param(params, Value::Integer(id));
    let expression = "COALESCE(NULLIF(w.title, ''), w.filename)";
    format!(
        "({expression} COLLATE NOCASE {name_operator} {name} COLLATE NOCASE
          OR ({expression} COLLATE NOCASE = {name} COLLATE NOCASE
              AND (w.path > {path} OR (w.path = {path} AND w.id > {id}))))"
    )
}

fn cursor_boundary_for_item(
    sort: LibraryBrowserSort,
    item: &LibraryBrowserItem,
) -> BrowserCursorBoundary {
    match sort {
        LibraryBrowserSort::RecentlyAdded => BrowserCursorBoundary::RecentlyAdded {
            added_at: item.added_at.clone(),
            id: item.wallpaper_id,
        },
        LibraryBrowserSort::NameAsc | LibraryBrowserSort::NameDesc => {
            let name = item
                .entry
                .project
                .as_ref()
                .and_then(|project| project.title.as_deref())
                .filter(|title| !title.is_empty())
                .unwrap_or_else(|| item.entry.filename())
                .to_string();
            let path = item.entry.path.to_string();
            if sort == LibraryBrowserSort::NameAsc {
                BrowserCursorBoundary::NameAsc {
                    name,
                    path,
                    id: item.wallpaper_id,
                }
            } else {
                BrowserCursorBoundary::NameDesc {
                    name,
                    path,
                    id: item.wallpaper_id,
                }
            }
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
    let mut statement = conn.prepare(&sql).map_err(sqlite_err)?;
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
        .map_err(sqlite_err)?;
    let mut sources_by_wallpaper = HashMap::<i64, Vec<LibraryBrowserSource>>::new();
    for row in rows {
        let (wallpaper_id, source) = row.map_err(sqlite_err)?;
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
    conn: &Connection,
    query: &LibraryBrowserQuery,
    after_count: F,
) -> Result<LibraryBrowserPage, WcError>
where
    F: FnOnce(),
{
    // The revision read is the first read and pins the snapshot used by row
    // selection and source hydration.
    let transaction = conn.unchecked_transaction().map_err(sqlite_err)?;
    let revision = super::library_revision::read_library_revision(&transaction)?;
    let fingerprint = browser_query_fingerprint(query);
    let cursor = query
        .cursor
        .as_deref()
        .map(decode_browser_cursor)
        .transpose()?;
    if let Some(cursor) = cursor.as_ref() {
        if cursor.revision != revision || cursor.fingerprint != fingerprint {
            return Err(WcError::RevisionChanged {
                expected: cursor.revision,
                observed: revision,
            });
        }
    }

    after_count();

    const MAX_BROWSER_PAGE_SIZE: usize = 500;
    let page_limit = query.limit.min(MAX_BROWSER_PAGE_SIZE);
    if page_limit == 0 {
        transaction.commit().map_err(sqlite_err)?;
        return Ok(LibraryBrowserPage {
            revision,
            next_cursor: None,
            total: None,
            items: Vec::new(),
        });
    }
    let use_fts = super::library_fts::library_fts_ready(&transaction, revision);
    let mut predicate = browser_predicate(query, use_fts);
    if let Some(cursor) = cursor.as_ref() {
        append_cursor_boundary(&mut predicate, query.sort, &cursor.boundary)?;
    }
    let mut page_params = predicate.params.clone();
    let limit = push_browser_param(
        &mut page_params,
        Value::Integer(i64::try_from(page_limit + 1).unwrap_or(501)),
    );
    let sql = format!(
        "{BROWSER_ITEM_SELECT}
         {}
         ORDER BY {}
         LIMIT {limit}",
        predicate.where_sql,
        browser_order_by(query.sort)
    );
    let mut items = {
        let mut statement = transaction.prepare(&sql).map_err(sqlite_err)?;
        let items = statement
            .query_map(params_from_iter(page_params.iter()), browser_item_from_row)
            .map_err(sqlite_err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sqlite_err)?;
        items
    };
    let has_more = items.len() > page_limit;
    if has_more {
        items.truncate(page_limit);
    }
    hydrate_browser_sources(&transaction, &mut items)?;
    let next_cursor = if has_more {
        items
            .last()
            .map(|item| {
                encode_browser_cursor(&BrowserCursorV1 {
                    version: BROWSER_CURSOR_VERSION,
                    revision,
                    fingerprint: fingerprint.clone(),
                    boundary: cursor_boundary_for_item(query.sort, item),
                })
            })
            .transpose()?
    } else {
        None
    };
    transaction.commit().map_err(sqlite_err)?;

    Ok(LibraryBrowserPage {
        revision,
        next_cursor,
        total: None,
        items,
    })
}

pub fn browser_library_exact_total(
    cd: &ConfigDir,
    query: &LibraryBrowserQuery,
    expected_revision: u64,
) -> Result<LibraryBrowserTotal, WcError> {
    if !cd.db_path().exists() {
        if expected_revision != 0 {
            return Err(WcError::RevisionChanged {
                expected: expected_revision,
                observed: 0,
            });
        }
        return Ok(LibraryBrowserTotal {
            revision: 0,
            total: 0,
        });
    }
    let conn = open_runtime_connection(cd)?;
    browser_library_exact_total_on_connection(&conn, query, expected_revision)
}

pub fn browser_library_exact_total_on_connection(
    conn: &Connection,
    query: &LibraryBrowserQuery,
    expected_revision: u64,
) -> Result<LibraryBrowserTotal, WcError> {
    let transaction = conn.unchecked_transaction().map_err(sqlite_err)?;
    let revision = super::library_revision::read_library_revision(&transaction)?;
    if revision != expected_revision {
        return Err(WcError::RevisionChanged {
            expected: expected_revision,
            observed: revision,
        });
    }
    let use_fts = super::library_fts::library_fts_ready(&transaction, revision);
    let predicate = browser_predicate(query, use_fts);
    let total = transaction
        .query_row(
            &format!("SELECT COUNT(*) FROM wallpapers w {}", predicate.where_sql),
            params_from_iter(predicate.params.iter()),
            |row| row.get::<_, i64>(0),
        )
        .map_err(sqlite_err)?;
    transaction.commit().map_err(sqlite_err)?;
    Ok(LibraryBrowserTotal {
        revision,
        total: total.max(0) as usize,
    })
}

/// Query the unified, source-backed library without materializing the full table.
pub fn browser_library_page(
    cd: &ConfigDir,
    query: &LibraryBrowserQuery,
) -> Result<LibraryBrowserPage, WcError> {
    if !cd.db_path().exists() {
        return Ok(LibraryBrowserPage {
            revision: 0,
            next_cursor: None,
            total: None,
            items: Vec::new(),
        });
    }
    let conn = open_runtime_connection(cd)?;
    browser_library_page_inner(&conn, query, || {})
}

/// Execute a browser page on a caller-owned connection. This lets the GUI
/// service install and reliably clear a SQLite progress deadline.
pub fn browser_library_page_on_connection(
    conn: &Connection,
    query: &LibraryBrowserQuery,
) -> Result<LibraryBrowserPage, WcError> {
    browser_library_page_inner(conn, query, || {})
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
    let conn = open_runtime_connection(cd)?;
    browser_library_page_inner(&conn, query, after_count)
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
    let transaction = conn.unchecked_transaction().map_err(sqlite_err)?;
    let revision = super::library_revision::read_library_revision(&transaction)?;
    let use_fts = super::library_fts::library_fts_ready(&transaction, revision);
    let predicate = browser_predicate(query, use_fts);
    let count_sql = format!("SELECT COUNT(*) FROM wallpapers w {}", predicate.where_sql);
    let count: i64 = transaction
        .query_row(
            &count_sql,
            params_from_iter(predicate.params.iter()),
            |row| row.get(0),
        )
        .map_err(sqlite_err)?;
    if count <= 0 {
        transaction.commit().map_err(sqlite_err)?;
        return Ok(None);
    }
    // Uniform offset over the filtered set. Filtered rowids are not contiguous,
    // so COUNT+OFFSET is safer than probing random rowids.
    let offset: i64 = transaction
        .query_row("SELECT abs(random()) % ?1", [count], |row| row.get(0))
        .map_err(sqlite_err)?;
    let sql = format!(
        "{BROWSER_ITEM_SELECT}
         {}
         ORDER BY w.id
         LIMIT 1 OFFSET ?{}",
        predicate.where_sql,
        predicate.params.len() + 1
    );
    let mut params = predicate.params;
    params.push(Value::Integer(offset));
    let mut item = transaction
        .query_row(&sql, params_from_iter(params.iter()), browser_item_from_row)
        .optional()
        .map_err(sqlite_err)?;
    if let Some(item) = item.as_mut() {
        hydrate_browser_sources(&transaction, std::slice::from_mut(item))?;
    }
    transaction.commit().map_err(sqlite_err)?;
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
        .map_err(sqlite_err)?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(sqlite_err)?;
    for row in rows {
        let (kind, count) = row.map_err(sqlite_err)?;
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
        .map_err(sqlite_err)?;
    let mut stmt = conn
        .prepare(
            "SELECT w.path, w.type, w.ext, w.backend, w.size, w.mtime, w.resolution,
                    w.project_type, w.preview_path, w.workshop_id, w.title, w.we_file, w.unsupported_reason
             FROM favorites f
             INNER JOIN wallpapers w ON w.path = f.path
             ORDER BY w.mtime DESC, w.path ASC
             LIMIT ?1 OFFSET ?2",
        )
        .map_err(sqlite_err)?;
    let items = stmt
        .query_map(
            params![
                i64::try_from(limit).unwrap_or(i64::MAX),
                i64::try_from(offset).unwrap_or(i64::MAX)
            ],
            wallpaper_entry_from_row,
        )
        .map_err(sqlite_err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sqlite_err)?;
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
        _offset: usize,
        limit: usize,
    ) -> LibraryBrowserQuery {
        LibraryBrowserQuery {
            source_id,
            type_filter,
            favorites_only,
            search: search.into(),
            sort,
            cursor: None,
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

        assert_eq!(page.total, None);
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
        assert_eq!(usable.total, None);
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
        assert_eq!(unsupported.total, None);
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
            assert_eq!(page.total, None);
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
            assert!(
                page.items.is_empty(),
                "private search term leaked: {private_term}"
            );
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
        assert_eq!(page.total, None);
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
        assert_eq!(apostrophe.total, None);
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

        let first_name_page = browser_library_page(
            &cd,
            &query(
                None,
                LibraryBrowserType::Image,
                false,
                "",
                LibraryBrowserSort::NameAsc,
                0,
                1,
            ),
        )
        .unwrap();
        let mut next_name_query = query(
            None,
            LibraryBrowserType::Image,
            false,
            "",
            LibraryBrowserSort::NameAsc,
            0,
            2,
        );
        next_name_query.cursor = first_name_page.next_cursor;
        let name_page = browser_library_page(&cd, &next_name_query).unwrap();
        assert_eq!(name_page.total, None);
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
        assert_eq!(count_only.total, None);
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
    fn browser_random_count_offset_covers_all_filtered_rows() {
        let (_tmp, cd) = fixture();
        let conn = Connection::open(cd.db_path()).unwrap();
        for id in 1..=5_i64 {
            insert_browser_wallpaper(
                &conn,
                id,
                &format!("/walls/img-{id}.jpg"),
                "image",
                "Title",
                "",
                "2025-01-01",
                "",
                "",
            );
            attach(&conn, id, 1);
        }
        insert_browser_wallpaper(
            &conn,
            99,
            "/walls/video.mp4",
            "video",
            "Skip",
            "",
            "2025-01-01",
            "",
            "",
        );
        attach(&conn, 99, 1);

        let matching = query(
            None,
            LibraryBrowserType::Image,
            false,
            "",
            LibraryBrowserSort::RecentlyAdded,
            0,
            20,
        );
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..80 {
            let item = browser_library_random(&cd, &matching)
                .unwrap()
                .expect("filtered library must yield a row");
            assert_ne!(item.wallpaper_id, 99, "type filter must exclude video");
            seen.insert(item.wallpaper_id);
        }
        assert_eq!(
            seen,
            [1, 2, 3, 4, 5].into_iter().collect(),
            "count+offset random must be able to hit every filtered row"
        );
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

        assert_eq!(page.total, None);
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

        assert_eq!(page.total, None);
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
            .items
            .len(),
            0
        );
    }

    #[test]
    fn browser_keyset_pages_do_not_duplicate_or_skip_for_every_sort() {
        let (_tmp, cd) = fixture();
        let conn = Connection::open(cd.db_path()).unwrap();
        for (id, filename, title, added_at) in [
            (1, "b.jpg", "Alpha", "2025-01-03"),
            (2, "a.jpg", "alpha", "2025-01-03"),
            (3, "d.jpg", "Delta", "2025-01-02"),
            (4, "c.jpg", "Charlie", "2025-01-01"),
            (5, "e.jpg", "Echo", "2025-01-01"),
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

        for sort in [
            LibraryBrowserSort::RecentlyAdded,
            LibraryBrowserSort::NameAsc,
            LibraryBrowserSort::NameDesc,
        ] {
            let mut request = query(None, LibraryBrowserType::Image, false, "", sort, 0, 2);
            let mut ids = Vec::new();
            loop {
                let page = browser_library_page(&cd, &request).unwrap();
                assert_eq!(page.revision, 0);
                ids.extend(page.items.iter().map(|item| item.wallpaper_id));
                let Some(cursor) = page.next_cursor else {
                    break;
                };
                request.cursor = Some(cursor);
            }
            assert_eq!(ids.len(), 5, "sort {sort:?}");
            let unique = ids
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>();
            assert_eq!(unique.len(), 5, "sort {sort:?}: {ids:?}");
        }
    }

    #[test]
    fn browser_cursor_is_opaque_and_rejects_query_or_revision_changes() {
        let (_tmp, cd) = fixture();
        let mut conn = Connection::open(cd.db_path()).unwrap();
        for id in 1..=3 {
            insert_browser_wallpaper(
                &conn,
                id,
                &format!("/private/secret-{id}.jpg"),
                "image",
                "secret title",
                "",
                "2025-01-01",
                "",
                "",
            );
            attach(&conn, id, 1);
        }
        let first_query = query(
            None,
            LibraryBrowserType::Image,
            false,
            "secret title",
            LibraryBrowserSort::RecentlyAdded,
            0,
            1,
        );
        let first = browser_library_page(&cd, &first_query).unwrap();
        let cursor = first.next_cursor.unwrap();
        assert!(!cursor.contains("secret"));
        assert!(!cursor.contains("/private"));

        let mut changed_query = first_query.clone();
        changed_query.search = "different".into();
        changed_query.cursor = Some(cursor.clone());
        assert!(matches!(
            browser_library_page(&cd, &changed_query),
            Err(WcError::RevisionChanged { .. })
        ));

        let tx = conn.transaction().unwrap();
        super::super::library_revision::bump_library_revision(&tx).unwrap();
        tx.commit().unwrap();
        let mut old_cursor_query = first_query;
        old_cursor_query.cursor = Some(cursor);
        assert!(matches!(
            browser_library_page(&cd, &old_cursor_query),
            Err(WcError::RevisionChanged {
                expected: 0,
                observed: 1
            })
        ));
    }

    #[test]
    fn browser_rejects_malformed_cursor_and_exact_total_is_revision_bound() {
        let (_tmp, cd) = fixture();
        let conn = Connection::open(cd.db_path()).unwrap();
        insert_browser_wallpaper(
            &conn,
            1,
            "/walls/one.jpg",
            "image",
            "One",
            "",
            "2025-01-01",
            "",
            "",
        );
        attach(&conn, 1, 1);
        let mut request = query(
            None,
            LibraryBrowserType::Image,
            false,
            "",
            LibraryBrowserSort::RecentlyAdded,
            0,
            20,
        );
        request.cursor = Some("not-hex".into());
        assert!(matches!(
            browser_library_page(&cd, &request),
            Err(WcError::InvalidCursor { .. })
        ));
        request.cursor = None;
        assert_eq!(
            browser_library_exact_total(&cd, &request, 0).unwrap(),
            LibraryBrowserTotal {
                revision: 0,
                total: 1
            }
        );
        assert!(matches!(
            browser_library_exact_total(&cd, &request, 9),
            Err(WcError::RevisionChanged {
                expected: 9,
                observed: 0
            })
        ));
    }

    #[test]
    fn stable_wallpaper_existence_tracks_database_identity() {
        let (_tmp, cd) = fixture();
        let conn = Connection::open(cd.db_path()).unwrap();
        insert_browser_wallpaper(
            &conn,
            41,
            "/walls/selected.jpg",
            "image",
            "Selected",
            "",
            "2025-01-01",
            "",
            "",
        );
        attach(&conn, 41, 1);

        assert!(library_wallpaper_exists(&cd, 41).unwrap());
        assert!(!library_wallpaper_exists(&cd, 99).unwrap());
        conn.execute("DELETE FROM wallpaper_sources WHERE wallpaper_id = 41", [])
            .unwrap();
        assert!(!library_wallpaper_exists(&cd, 41).unwrap());
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM wallpapers WHERE id = 41", [], |row| {
                row.get::<_, i64>(0)
            },)
                .unwrap(),
            1
        );
    }
}
