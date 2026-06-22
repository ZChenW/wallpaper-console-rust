use std::io::Write;

use wc_core::types::WallpaperEntry;
use wc_storage::StorageApi;

pub(crate) fn print_help() {
    println!(concat!(
        "wallpaper-console-rust\n\n",
        "Commands:\n",
        "  apply FILE           browse              browse-all\n",
        "  browse-images        browse-gifs         browse-videos\n",
        "  random               random-all          random-image/gif/video\n",
        "  stop                 status               restore\n",
        "  add DIR              sources             remove (fzf)\n",
        "  remove-source DIR    steam-workshop      validate-sources\n",
        "  remove-missing       dedupe-sources\n",
        "  favorite-add FILE    favorite-add-current favorites (fzf)\n",
        "  favorite-random      favorite-remove [FILE]\n",
        "  history (fzf)        history-random      history-clear\n",
        "  search [QUERY]       search-source [Q]   search-type [Q]\n",
        "  sort-mtime           sort-size            sort-name\n",
        "  config-get KEY       config-set KEY VAL\n",
        "  rescan               library              library-count\n",
        "  browse-library (fzf) random-library       library-json [--tsv|--sqlite]\n",
        "  library-page-json    favorites-json       history-json\n",
        "  migrate-to-sqlite    sqlite-verify        sqlite-resync (diagnostic repair)\n",
        "  sqlite-export-flat   sqlite-backup         sqlite-restore BACKUP\n",
        "  sqlite-config-get KEY sqlite-sources-list   sqlite-favorites-list\n",
        "  sqlite-history-list  sqlite-current-read   sqlite-last-backend-read\n",
        "  tui\n",
    ));
}

pub(crate) fn write_library_tsv_entry<W: Write>(
    writer: &mut W,
    entry: &WallpaperEntry,
) -> std::io::Result<()> {
    writeln!(
        writer,
        "{}\t{}\t{}\t{}\t{}\t{}\t{}",
        entry.file_type.as_str(),
        entry.ext,
        entry.backend.as_str(),
        entry.size,
        entry.mtime,
        entry.resolution,
        entry.path
    )
}

pub(crate) fn json_from_entry(entry: &WallpaperEntry) -> serde_json::Value {
    serde_json::json!({
        "path": entry.path.to_string(),
        "type": entry.file_type.as_str(),
        "ext": entry.ext,
        "backend": entry.backend.as_str(),
        "size": entry.size,
        "mtime": entry.mtime,
        "resolution": entry.resolution,
        "projectType": entry.project.as_ref().map(|p| p.project_type.clone()),
        "previewPath": entry.project.as_ref().and_then(|p| p.preview_path.clone()),
        "workshopId": entry.project.as_ref().and_then(|p| p.workshop_id.clone()),
        "title": entry.project.as_ref().and_then(|p| p.title.clone()),
        "weFile": entry.project.as_ref().and_then(|p| p.we_file.clone()),
        "unsupportedReason": entry.project.as_ref().and_then(|p| p.unsupported_reason.clone()),
    })
}

pub(crate) fn json_from_sql_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<serde_json::Value> {
    let project_type: String = row.get(7)?;
    let preview_path: String = row.get(8)?;
    let workshop_id: String = row.get(9)?;
    let title: String = row.get(10)?;
    let we_file: String = row.get(11)?;
    let unsupported_reason: String = row.get(12)?;
    Ok(serde_json::json!({
        "path": row.get::<_, String>(0)?,
        "type": row.get::<_, String>(1)?,
        "ext": row.get::<_, String>(2)?,
        "backend": row.get::<_, String>(3)?,
        "size": row.get::<_, i64>(4)?,
        "mtime": row.get::<_, i64>(5)?,
        "resolution": row.get::<_, String>(6)?,
        "projectType": optional_json_string(project_type),
        "previewPath": optional_json_string(preview_path),
        "workshopId": optional_json_string(workshop_id),
        "title": optional_json_string(title),
        "weFile": optional_json_string(we_file),
        "unsupportedReason": optional_json_string(unsupported_reason),
    }))
}

fn optional_json_string(value: String) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

pub(crate) fn json_library_from_tsv(s: &StorageApi) -> anyhow::Result<()> {
    let entries = crate::library::library_entries(s)?;
    let json: Vec<serde_json::Value> = entries.iter().map(json_from_entry).collect();
    println!("{}", serde_json::to_string_pretty(&json)?);
    Ok(())
}

pub(crate) fn json_library_from_sqlite(s: &StorageApi) -> anyhow::Result<()> {
    use rusqlite::Connection;
    let db = s.cd.db_path();
    if !db.exists() {
        let conn = Connection::open(&db)?;
        wc_storage::sqlite::create_schema(&conn)?;
        println!("[]");
        return Ok(());
    }
    let conn = Connection::open(&db)?;
    wc_storage::sqlite::ensure_wallpaper_metadata_columns(&conn)?;
    let mut stmt = conn.prepare(
        "SELECT path, type, ext, backend, size, mtime, resolution,
                project_type, preview_path, workshop_id, title, we_file, unsupported_reason
         FROM wallpapers ORDER BY path",
    )?;
    // Propagate row errors instead of silently ignoring them.
    let rows: Result<Vec<serde_json::Value>, rusqlite::Error> =
        stmt.query_map([], json_from_sql_row)?.collect();
    let rows = rows?;
    println!("{}", serde_json::to_string_pretty(&rows)?);
    Ok(())
}

pub(crate) fn json_library_page(
    s: &StorageApi,
    source: &str,
    filter: &str,
    sort: &str,
    search: &str,
    offset: usize,
    limit: usize,
) -> anyhow::Result<()> {
    match source {
        "sqlite" => json_library_page_from_sqlite(s, filter, sort, search, offset, limit),
        "tsv" => json_library_page_from_tsv(s, filter, sort, search, offset, limit),
        other => anyhow::bail!("unknown library source: {}", other),
    }
}

fn json_library_page_from_sqlite(
    s: &StorageApi,
    filter: &str,
    sort: &str,
    search: &str,
    offset: usize,
    limit: usize,
) -> anyhow::Result<()> {
    let query = wc_storage::sqlite::LibraryPageQuery {
        filter: wc_storage::sqlite::LibraryFilter::parse(filter)?,
        sort: wc_storage::sqlite::LibrarySort::parse(sort)?,
        search: search.to_string(),
        offset,
        limit,
    };
    let page = wc_storage::sqlite::library_page_sqlite(&s.cd, &query)?;
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

fn validate_library_filter(filter: &str) -> anyhow::Result<&str> {
    match filter {
        "all" | "image" | "gif" | "video" | "we_scene" | "we_web" | "unsupported" => Ok(filter),
        other => anyhow::bail!("unknown library filter: {}", other),
    }
}

fn validate_library_sort(sort: &str) -> anyhow::Result<&str> {
    match sort {
        "newest" | "largest" | "name" => Ok(sort),
        other => anyhow::bail!("unknown library sort: {}", other),
    }
}

fn json_library_page_from_tsv(
    s: &StorageApi,
    filter: &str,
    sort: &str,
    search: &str,
    offset: usize,
    limit: usize,
) -> anyhow::Result<()> {
    let filter = validate_library_filter(filter)?;
    let _sort = validate_library_sort(sort)?;
    let (total, rows) = wc_storage::tsv::tsv_bounded_page(
        &s.cd.library_tsv_path(),
        filter,
        sort,
        search,
        offset,
        limit,
    );
    let items: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|r| {
            serde_json::json!({
                "path": r.path,
                "type": r.ftype,
                "ext": r.ext,
                "backend": r.backend,
                "size": r.size,
                "mtime": r.mtime,
                "resolution": r.resolution,
            })
        })
        .collect();
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "total": total,
            "items": items,
        }))?
    );
    Ok(())
}
