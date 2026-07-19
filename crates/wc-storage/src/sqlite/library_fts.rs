use std::time::{Duration, Instant};

use rusqlite::{params, Connection, OptionalExtension};
use wc_core::error::WcError;

use crate::sqlite_err;

const BUILD_ROW_LIMIT: usize = 500;
const BUILD_TIME_LIMIT: Duration = Duration::from_secs(5);

pub fn create_library_fts_schema(connection: &Connection) -> Result<(), WcError> {
    connection
        .execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS library_browser_fts
                 USING fts5(searchable, content='', tokenize='trigram');
             CREATE TABLE IF NOT EXISTS library_fts_state (
                 singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                 status TEXT NOT NULL CHECK (status IN ('pending', 'ready')),
                 revision INTEGER NOT NULL,
                 next_wallpaper_id INTEGER NOT NULL
             ) STRICT;
             INSERT OR IGNORE INTO library_fts_state
                 (singleton, status, revision, next_wallpaper_id)
                 VALUES (1, 'pending', -1, 0);",
        )
        .map_err(sqlite_err)
}

/// Derived-index failures must never block a Library-visible transaction.
pub fn mark_library_fts_stale_best_effort(connection: &Connection) {
    let _ = connection.execute(
        "UPDATE library_fts_state
         SET status = 'pending', revision = -1, next_wallpaper_id = 0
         WHERE singleton = 1",
        [],
    );
}

pub fn library_fts_ready(connection: &Connection, revision: u64) -> bool {
    let state_ready = connection
        .query_row(
            "SELECT status = 'ready' AND revision = ?1
             FROM library_fts_state WHERE singleton = 1",
            [i64::try_from(revision).unwrap_or(i64::MAX)],
            |row| row.get::<_, bool>(0),
        )
        .unwrap_or(false);
    if !state_ready {
        return false;
    }
    let usable = connection
        .query_row(
            "SELECT rowid FROM library_browser_fts
             WHERE library_browser_fts MATCH '\"library-probe-token\"' LIMIT 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .is_ok();
    if !usable {
        mark_library_fts_stale_best_effort(connection);
    }
    usable
}

pub fn library_fts_match_term(term: &str) -> Option<String> {
    if term.chars().count() < 3 {
        return None;
    }
    Some(format!("\"{}\"", term.replace('"', "\"\"")))
}

/// Build at most 500 rows or five seconds. Returns true once the index is ready.
pub fn build_library_fts_chunk(connection: &mut Connection) -> Result<bool, WcError> {
    create_library_fts_schema(connection)?;
    let started = Instant::now();
    let transaction = connection.transaction().map_err(sqlite_err)?;
    let revision = super::read_library_revision(&transaction)?;
    let (state_revision, mut next_id): (i64, i64) = transaction
        .query_row(
            "SELECT revision, next_wallpaper_id FROM library_fts_state WHERE singleton = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(sqlite_err)?;
    if state_revision != i64::try_from(revision).unwrap_or(i64::MAX) {
        if state_revision == -1 {
            transaction
                .execute_batch(
                    "DROP TABLE IF EXISTS library_browser_fts;
                     CREATE VIRTUAL TABLE library_browser_fts
                         USING fts5(searchable, content='', tokenize='trigram');",
                )
                .map_err(sqlite_err)?;
        } else {
            transaction
                .execute(
                    "INSERT INTO library_browser_fts(library_browser_fts) VALUES('delete-all')",
                    [],
                )
                .map_err(sqlite_err)?;
        }
        transaction
            .execute(
                "UPDATE library_fts_state
                 SET status = 'pending', revision = ?1, next_wallpaper_id = 0
                 WHERE singleton = 1",
                [i64::try_from(revision).unwrap_or(i64::MAX)],
            )
            .map_err(sqlite_err)?;
        next_id = 0;
    }

    let mut rows = transaction
        .prepare(
            "SELECT w.id,
                    trim(w.filename || ' ' || w.title || ' ' || w.author || ' ' ||
                         COALESCE((
                             SELECT group_concat(s.display_name, ' ')
                             FROM wallpaper_sources ws
                             JOIN sources s ON s.id = ws.source_id
                             WHERE ws.wallpaper_id = w.id
                         ), ''))
             FROM wallpapers w
             WHERE w.id > ?1
             ORDER BY w.id
             LIMIT ?2",
        )
        .map_err(sqlite_err)?
        .query_map(
            params![next_id, i64::try_from(BUILD_ROW_LIMIT).unwrap_or(i64::MAX)],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        )
        .map_err(sqlite_err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sqlite_err)?;

    for (processed, (id, searchable)) in rows.drain(..).enumerate() {
        if processed > 0 && started.elapsed() >= BUILD_TIME_LIMIT {
            break;
        }
        transaction
            .execute(
                "INSERT INTO library_browser_fts(rowid, searchable) VALUES (?1, ?2)",
                params![id, searchable],
            )
            .map_err(sqlite_err)?;
        next_id = id;
    }
    let has_more = transaction
        .query_row(
            "SELECT 1 FROM wallpapers WHERE id > ?1 LIMIT 1",
            [next_id],
            |_| Ok(()),
        )
        .optional()
        .map_err(sqlite_err)?
        .is_some();
    transaction
        .execute(
            "UPDATE library_fts_state
             SET status = ?1, next_wallpaper_id = ?2
             WHERE singleton = 1",
            params![if has_more { "pending" } else { "ready" }, next_id],
        )
        .map_err(sqlite_err)?;
    transaction.commit().map_err(sqlite_err)?;
    Ok(!has_more)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn browser_query(search: &str) -> crate::sqlite::LibraryBrowserQuery {
        crate::sqlite::LibraryBrowserQuery {
            source_id: None,
            type_filter: crate::sqlite::LibraryBrowserType::Usable,
            favorites_only: false,
            search: search.into(),
            sort: crate::sqlite::LibraryBrowserSort::NameAsc,
            cursor: None,
            limit: 20,
        }
    }

    #[test]
    fn short_terms_fall_back_and_metacharacters_are_quoted() {
        assert_eq!(library_fts_match_term("中"), None);
        assert_eq!(library_fts_match_term("ab"), None);
        assert_eq!(library_fts_match_term("a%b"), Some("\"a%b\"".into()));
        assert_eq!(library_fts_match_term("a\"b"), Some("\"a\"\"b\"".into()));
    }

    #[test]
    fn browser_fts_indexes_public_metadata_without_paths_and_preserves_exact_search() {
        let mut connection = Connection::open_in_memory().unwrap();
        crate::sqlite::create_schema(&connection).unwrap();
        connection
            .execute(
                "INSERT INTO sources (id, path, display_name)
                 VALUES (1, '/private-secret/root', '图库来源')",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO wallpapers
                 (id, path, type, ext, backend, resolution, title, author)
                 VALUES (7, '/private-secret/root/Alpha%中图.jpg', 'image', 'jpg',
                         'awww', '1x1', 'Forest 100%', 'Ada Lovelace')",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO wallpaper_sources (wallpaper_id, source_id) VALUES (7, 1)",
                [],
            )
            .unwrap();
        let transaction = connection.transaction().unwrap();
        crate::sqlite::bump_library_revision(&transaction).unwrap();
        transaction.commit().unwrap();

        assert!(build_library_fts_chunk(&mut connection).unwrap());
        assert!(library_fts_ready(&connection, 1));

        for term in ["Alpha%", "Forest", "100%", "Lovelace", "图库来源", "中图"] {
            let page = crate::sqlite::browser_library_page_on_connection(
                &connection,
                &browser_query(term),
            )
            .unwrap();
            assert_eq!(page.items.len(), 1, "public search term {term}");
        }
        let private = crate::sqlite::browser_library_page_on_connection(
            &connection,
            &browser_query("private-secret"),
        )
        .unwrap();
        assert!(private.items.is_empty());
        let leaked_path_rows: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM library_browser_fts
                 WHERE library_browser_fts MATCH '\"private-secret\"'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(leaked_path_rows, 0);
    }

    #[test]
    fn stale_or_invalid_derived_table_is_dropped_and_rebuilt() {
        let mut connection = Connection::open_in_memory().unwrap();
        crate::sqlite::create_schema(&connection).unwrap();
        connection
            .execute_batch(
                "DROP TABLE library_browser_fts;
                 CREATE TABLE library_browser_fts (broken TEXT);",
            )
            .unwrap();

        assert!(!library_fts_ready(&connection, 0));
        assert!(build_library_fts_chunk(&mut connection).unwrap());
        assert!(library_fts_ready(&connection, 0));
        let definition: String = connection
            .query_row(
                "SELECT sql FROM sqlite_master
                 WHERE name = 'library_browser_fts'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(definition.contains("VIRTUAL TABLE"));
        assert!(definition.contains("tokenize='trigram'"));
    }
}
