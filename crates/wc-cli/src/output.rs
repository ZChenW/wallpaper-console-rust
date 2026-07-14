use std::io::Write;

use wc_core::types::WallpaperEntry;
use wc_storage::StorageApi;

pub(crate) fn print_help() {
    println!(concat!(
        "wallpaper-console-rust\n\n",
        "Commands:\n",
        "  apply FILE [--target OUTPUT|all] [--output OUTPUT]...\n",
        "                       browse              browse-all\n",
        "  browse-images        browse-gifs         browse-videos\n",
        "  random               random-all          random-image/gif/video\n",
        "  stop                 status               restore\n",
        "  displays             display-state        restore-displays [--output OUTPUT]...\n",
        "  restore-at-login     restore saved displays only when restore_on_login=on\n",
        "  add DIR              sources             remove (fzf)\n",
        "  remove-source DIR    steam-workshop      validate-sources\n",
        "  remove-missing       dedupe-sources\n",
        "  favorite-add FILE    favorite-add-current favorites (fzf)\n",
        "  favorite-random      favorite-remove [FILE]\n",
        "  search [QUERY]       search-source [Q]   search-type [Q]\n",
        "  sort-mtime           sort-size            sort-name\n",
        "  config-get KEY       config-set KEY VAL\n",
        "  rescan               library              library-count\n",
        "  browse-library (fzf) random-library       library-json [--tsv|--sqlite]\n",
        "  library-page-json    favorites-json\n",
        "  migrate-to-sqlite    sqlite-verify        sqlite-resync (diagnostic repair)\n",
        "  sqlite-export-flat   sqlite-backup         sqlite-restore BACKUP\n",
        "  sqlite-config-get KEY sqlite-sources-list   sqlite-favorites-list\n",
        "  sqlite-current-read  sqlite-last-backend-read\n",
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
    let db = s.cd.db_path();
    if !db.exists() {
        wc_storage::sqlite::try_ensure_sqlite_db(&s.cd)?;
        println!("[]");
        return Ok(());
    }
    let conn = wc_storage::sqlite::open_runtime_connection(&s.cd)?;
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

pub(crate) fn json_from_display_names(names: &[String]) -> serde_json::Value {
    serde_json::json!({
        "outputs": names
            .iter()
            .map(|name| serde_json::json!({ "name": name }))
            .collect::<Vec<_>>()
    })
}

pub(crate) fn json_from_display_state_rows(
    rows: &[wc_storage::sqlite::DisplayStateRow],
) -> serde_json::Value {
    serde_json::Value::Array(
        rows.iter()
            .map(|row| {
                let (target_key, kind, output) = match &row.target {
                    wc_storage::sqlite::DisplayStateTarget::AllDisplays => (
                        wc_storage::sqlite::ALL_DISPLAYS_TARGET_KEY.to_string(),
                        "allDisplays",
                        None,
                    ),
                    wc_storage::sqlite::DisplayStateTarget::Output(name) => {
                        (name.clone(), "output", Some(name.clone()))
                    }
                };
                serde_json::json!({
                    "targetKey": target_key,
                    "kind": kind,
                    "output": output,
                    "wallpaperPath": row.wallpaper_path,
                    "backend": row.backend,
                    "updatedAt": row.updated_at,
                })
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use wc_storage::sqlite::{DisplayStateRow, DisplayStateTarget};

    #[test]
    fn display_list_json_uses_typed_output_objects() {
        assert_eq!(
            json_from_display_names(&["eDP-1".into(), "HDMI-A-1".into()]),
            serde_json::json!({
                "outputs": [{ "name": "eDP-1" }, { "name": "HDMI-A-1" }]
            })
        );
    }

    #[test]
    fn display_state_json_preserves_all_and_named_targets() {
        let rows = vec![
            DisplayStateRow {
                target: DisplayStateTarget::AllDisplays,
                wallpaper_path: "/walls/all.jpg".into(),
                backend: "awww".into(),
                updated_at: "2026-07-13T00:00:00Z".into(),
            },
            DisplayStateRow {
                target: DisplayStateTarget::Output("eDP-1".into()),
                wallpaper_path: "/walls/laptop.mp4".into(),
                backend: "mpvpaper".into(),
                updated_at: "2026-07-13T00:01:00Z".into(),
            },
        ];

        assert_eq!(
            json_from_display_state_rows(&rows),
            serde_json::json!([
                {
                    "targetKey": "__all_displays__",
                    "kind": "allDisplays",
                    "output": null,
                    "wallpaperPath": "/walls/all.jpg",
                    "backend": "awww",
                    "updatedAt": "2026-07-13T00:00:00Z"
                },
                {
                    "targetKey": "eDP-1",
                    "kind": "output",
                    "output": "eDP-1",
                    "wallpaperPath": "/walls/laptop.mp4",
                    "backend": "mpvpaper",
                    "updatedAt": "2026-07-13T00:01:00Z"
                }
            ])
        );
    }
}
