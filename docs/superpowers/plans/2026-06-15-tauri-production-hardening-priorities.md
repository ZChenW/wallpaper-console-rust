# Tauri Production Hardening Priorities Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Execute five production-hardening priorities for the Tauri GUI without changing the public CLI contract, reintroducing Wails, or weakening SQLite/TSV compatibility.

**Architecture:** Keep Rust crates as the source of truth. Move reusable data-access and scan behavior into `wc-storage` / `wc-scan`, keep Tauri commands as thin DTO adapters, and keep frontend state coordination in focused hooks. Every priority is independently reviewable and must be closed by `code-review-expert` before the next priority starts.

**Tech Stack:** Rust workspace, `rusqlite`, Tauri 2 commands, React 19, TypeScript, Node test runner, Playwright smoke tests, existing `wc-*` crates.

---

## Non-Negotiable Execution Rules For DeepSeek

- Do not implement adjacent features. Only implement the currently selected priority.
- Do not reintroduce Wails, Go bindings, `apps/wails-gui`, or subprocess-based GUI calls.
- Do not remove the Rust CLI, TSV support, or existing public CLI command names/output shapes.
- Do not change Tauri command names consumed by `apps/tauri-gui/frontend/src/api/bridge.ts` unless this plan explicitly says so.
- Do not delete compatibility commands such as `library_list`, `favorites_list`, or `history_list`; if deprecated internally, keep them as wrappers.
- Do not trust README performance claims over source. Verify runtime paths in code.
- After finishing each priority, stop. Run the required tests, then invoke `code-review-expert` on the current git diff.
- Do not start the next priority until `code-review-expert` reports no P0/P1 issues. P2 issues must be fixed or explicitly recorded as accepted follow-up.
- Keep commits small: one priority, one commit after tests and review fixes.

## Review Gate Required After Every Priority

Use this exact review prompt after each priority:

```text
Use code-review-expert to review the current git diff for priority <N>.
Focus on correctness, SOLID/architecture, security, path handling, async races, performance regressions, and missing tests.
Do not implement fixes during review. Report P0/P1/P2/P3 findings with file and line references.
```

Minimum review preflight:

```bash
git status -sb
git diff --stat
git diff
```

Minimum review acceptance:

- P0: none
- P1: none
- P2: fixed now or documented with a specific follow-up issue in the final report
- P3: optional

---

## Priority 1: Real SQL Pagination For GUI Library, Favorites, And History

**Goal:** Stop reading the full `wallpapers` table for paged GUI views. Use SQLite `COUNT`, `ORDER BY`, `LIMIT`, and `OFFSET` in shared storage helpers, while preserving current DTO shape and visual ordering rules.

**Current Evidence:**

- `apps/tauri-gui/src-tauri/src/commands/library.rs` currently loads all rows in `read_sqlite_entries()` and pages in memory.
- `sort_filter_page()` applies filter/search/sort in Rust.
- `favorites_page()` and `history_page()` load all wallpapers and then filter or linear-search paths.
- `crates/wc-storage/src/sqlite.rs` already creates indexes for type, mtime, and size.
- `crates/wc-cli/src/main.rs::json_library_page_from_sqlite()` already has a SQL paging pattern, but it is CLI-local and does not include the GUI applyability rank ordering.

**Files:**

- Modify: `crates/wc-storage/src/sqlite.rs`
- Modify: `apps/tauri-gui/src-tauri/src/commands/library.rs`
- Modify: `crates/wc-cli/src/main.rs`
- Test: `crates/wc-storage/src/sqlite.rs`
- Test: `apps/tauri-gui/src-tauri/src/commands/library.rs`
- Optional docs after implementation: `README.md`, `docs/PERFORMANCE_BASELINE.md`

### Task 1.1: Add Shared Query Types In `wc-storage`

- [ ] **Step 1: Add storage query/result structs near the existing library helpers in `crates/wc-storage/src/sqlite.rs`**

Add these public types above `library_count()`:

```rust
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
```

- [ ] **Step 2: Run a targeted compile check**

Run:

```bash
cargo check -p wc-storage
```

Expected: compilation succeeds or only fails because new helpers are not used yet. If it fails for syntax/type reasons, fix before continuing.

### Task 1.2: Add Row Mapping And WHERE/ORDER Builders

- [ ] **Step 1: Add a reusable row mapper**

Add this helper in `crates/wc-storage/src/sqlite.rs` near `prior_metadata_cache_from_sqlite()`:

```rust
fn wallpaper_entry_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<WallpaperEntry> {
    let path: String = row.get(0)?;
    let ftype_s: String = row.get(1)?;
    let ext: String = row.get(2)?;
    let backend_s: String = row.get(3)?;
    let size: i64 = row.get(4)?;
    let mtime: i64 = row.get(5)?;
    let resolution: String = row.get(6)?;
    let project_type: String = row.get(7)?;
    let preview_path: String = row.get(8)?;
    let workshop_id: String = row.get(9)?;
    let title: String = row.get(10)?;
    let we_file: String = row.get(11)?;
    let unsupported_reason: String = row.get(12)?;

    let file_type = match ftype_s.as_str() {
        "image" => FileType::Image,
        "gif" => FileType::Gif,
        "video" => FileType::Video,
        "we_scene" => FileType::WeScene,
        "we_web" => FileType::WeWeb,
        _ => FileType::WeApplication,
    };
    let backend = match backend_s.as_str() {
        "awww" => Backend::Awww,
        "mpvpaper" => Backend::Mpvpaper,
        "linux-wallpaperengine" => Backend::LinuxWallpaperEngine,
        _ => Backend::Unsupported,
    };
    let project = if project_type.is_empty() {
        None
    } else {
        Some(WallpaperProject {
            project_type,
            preview_path: non_empty_string(preview_path),
            workshop_id: non_empty_string(workshop_id),
            title: non_empty_string(title),
            we_file: non_empty_string(we_file),
            backend: Some(backend.as_str().to_string()),
            unsupported_reason: non_empty_string(unsupported_reason),
        })
    };

    Ok(WallpaperEntry {
        path: Utf8PathBuf::from(path),
        file_type,
        ext,
        backend,
        size: size.max(0) as u64,
        mtime: mtime.max(0) as u64,
        resolution,
        project,
    })
}

fn non_empty_string(value: String) -> Option<String> {
    if value.is_empty() { None } else { Some(value) }
}
```

- [ ] **Step 2: Add SQL builders with fixed allowlists**

Add:

```rust
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
```

Do not interpolate user input into SQL. Only interpolate strings returned by these allowlist builders.

### Task 1.3: Implement SQL Page Helpers

- [ ] **Step 1: Add `library_page_sqlite`**

Add:

```rust
pub fn library_page_sqlite(cd: &ConfigDir, query: &LibraryPageQuery) -> Result<LibraryPage, WcError> {
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
    let mut stmt = conn.prepare(&sql).map_err(|e| WcError::Sqlite(e.to_string()))?;
    let items = stmt
        .query_map(params![search, query.limit as i64, query.offset as i64], wallpaper_entry_from_row)
        .map_err(|e| WcError::Sqlite(e.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| WcError::Sqlite(e.to_string()))?;

    Ok(LibraryPage {
        total: total.max(0) as usize,
        items,
    })
}
```

- [ ] **Step 2: Add `library_counts_sqlite`**

Add:

```rust
pub fn library_counts_sqlite(cd: &ConfigDir) -> Result<wc_core::types::LibraryCounts, WcError> {
    ensure_sqlite_db(cd);
    let conn = Connection::open(cd.db_path()).map_err(|e| WcError::Sqlite(e.to_string()))?;
    let mut counts = wc_core::types::LibraryCounts::default();
    let mut stmt = conn
        .prepare("SELECT type, COUNT(*) FROM wallpapers GROUP BY type")
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))
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
```

- [ ] **Step 3: Add `favorites_page_sqlite` and `history_page_sqlite`**

Add:

```rust
pub fn favorites_page_sqlite(cd: &ConfigDir, offset: usize, limit: usize) -> Result<LibraryPage, WcError> {
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
        .query_map(params![limit as i64, offset as i64], wallpaper_entry_from_row)
        .map_err(|e| WcError::Sqlite(e.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    Ok(LibraryPage { total: total.max(0) as usize, items })
}

pub fn history_page_sqlite(cd: &ConfigDir, offset: usize, limit: usize) -> Result<LibraryPage, WcError> {
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
        .query_map(params![limit as i64, offset as i64], wallpaper_entry_from_row)
        .map_err(|e| WcError::Sqlite(e.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| WcError::Sqlite(e.to_string()))?;
    Ok(LibraryPage { total: total.max(0) as usize, items })
}
```

Do not add canonical-path joins in this priority. That changes semantics and belongs in a separate dedupe task.

### Task 1.4: Add Storage Tests

- [ ] **Step 1: Add a fixture helper in `crates/wc-storage/src/sqlite.rs` tests**

Inside the existing `#[cfg(test)] mod tests`, add:

```rust
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
```

- [ ] **Step 2: Add SQL pagination test**

Add:

```rust
#[test]
fn library_page_sqlite_filters_sorts_and_limits_without_full_table_callers() {
    let tmp = tempfile::tempdir().unwrap();
    let cd = wc_core::ConfigDir { path: tmp.path().join("wallpaper-console") };
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
```

- [ ] **Step 3: Add applyability ordering test**

Add:

```rust
#[test]
fn library_page_sqlite_keeps_we_web_and_unsupported_after_normal_items() {
    let tmp = tempfile::tempdir().unwrap();
    let cd = wc_core::ConfigDir { path: tmp.path().join("wallpaper-console") };
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

    let kinds: Vec<&str> = page.items.iter().map(|entry| entry.file_type.as_str()).collect();
    assert_eq!(kinds, vec!["image", "we_web", "unsupported"]);
}
```

- [ ] **Step 4: Add favorites/history SQL page tests**

Add:

```rust
#[test]
fn favorites_and_history_page_sqlite_join_to_wallpaper_metadata() {
    let tmp = tempfile::tempdir().unwrap();
    let cd = wc_core::ConfigDir { path: tmp.path().join("wallpaper-console") };
    cd.init().unwrap();
    ensure_sqlite_db(&cd);
    let conn = rusqlite::Connection::open(cd.db_path()).unwrap();
    insert_wallpaper_for_page_test(&conn, "/walls/a.jpg", "image", 100, 1000, "A", "");
    insert_wallpaper_for_page_test(&conn, "/walls/b.jpg", "image", 200, 2000, "B", "");
    conn.execute("INSERT INTO favorites (path) VALUES ('/walls/a.jpg')", []).unwrap();
    conn.execute("INSERT INTO history (path, backend) VALUES ('/walls/a.jpg', 'awww')", []).unwrap();
    conn.execute("INSERT INTO history (path, backend) VALUES ('/walls/b.jpg', 'awww')", []).unwrap();

    let favs = favorites_page_sqlite(&cd, 0, 10).unwrap();
    assert_eq!(favs.total, 1);
    assert_eq!(favs.items[0].size, 100);

    let hist = history_page_sqlite(&cd, 0, 1).unwrap();
    assert_eq!(hist.total, 2);
    assert_eq!(hist.items.len(), 1);
    assert_eq!(hist.items[0].path.as_str(), "/walls/b.jpg");
}
```

- [ ] **Step 5: Run storage tests**

Run:

```bash
cargo test -p wc-storage library_page_sqlite
cargo test -p wc-storage favorites_and_history_page_sqlite
```

Expected: tests pass.

### Task 1.5: Wire Tauri Commands To Shared SQL Helpers

- [ ] **Step 1: Replace `library_count()` implementation**

In `apps/tauri-gui/src-tauri/src/commands/library.rs`, replace the current `library_count()` body so it calls `wc_storage::sqlite::library_counts_sqlite(&s.cd)` and maps to `LibraryCountDto`.

Use:

```rust
let counts = wc_storage::sqlite::library_counts_sqlite(&s.cd).map_err(|e| e.to_string())?;
Ok(LibraryCountDto {
    total: counts.total,
    images: counts.images,
    gifs: counts.gifs,
    videos: counts.videos,
})
```

- [ ] **Step 2: Replace `library_page_gui()` implementation**

Construct a `wc_storage::sqlite::LibraryPageQuery` from string args:

```rust
let query = wc_storage::sqlite::LibraryPageQuery {
    filter: wc_storage::sqlite::LibraryFilter::parse(&filter).map_err(|e| e.to_string())?,
    sort: wc_storage::sqlite::LibrarySort::parse(&sort).map_err(|e| e.to_string())?,
    search,
    offset,
    limit,
};
let page = wc_storage::sqlite::library_page_sqlite(&s.cd, &query).map_err(|e| e.to_string())?;
Ok(LibraryPageDto {
    total: page.total,
    items: page.items.into_iter().map(dto_from_entry).collect(),
})
```

- [ ] **Step 3: Replace `favorites_page()` and `history_page()`**

Use `wc_storage::sqlite::favorites_page_sqlite(&s.cd, offset, limit)` and `history_page_sqlite(&s.cd, offset, limit)`.

- [ ] **Step 4: Keep compatibility wrappers**

Do not delete:

```rust
pub async fn library_list(_source: String) -> Result<Vec<WallpaperDto>, String>
pub async fn favorites_list() -> Result<Vec<WallpaperDto>, String>
pub async fn history_list() -> Result<Vec<WallpaperDto>, String>
```

They may remain wrappers around page calls with `usize::MAX`, or can use a large explicit limit if tests reveal SQLite rejects `usize::MAX` on this platform. If changing the limit, use `i64::MAX as usize`.

- [ ] **Step 5: Remove duplicate parsing only after tests pass**

After Tauri commands compile, delete `parse_file_type`, `parse_backend`, and `sort_filter_page` from `library.rs` only if they have no remaining callers. Do not remove `read_sqlite_entries()` until `library_list()` no longer uses it.

### Task 1.6: Reuse Shared SQL Helper In CLI

- [ ] **Step 1: Replace `json_library_page_from_sqlite()` internals**

In `crates/wc-cli/src/main.rs`, keep the function name and JSON output shape. Replace local SQL with a call to:

```rust
let query = wc_storage::sqlite::LibraryPageQuery {
    filter: wc_storage::sqlite::LibraryFilter::parse(filter)?,
    sort: wc_storage::sqlite::LibrarySort::parse(sort)?,
    search: search.to_string(),
    offset,
    limit,
};
let page = wc_storage::sqlite::library_page_sqlite(&s.cd, &query)?;
```

Then serialize `page.items` into the same JSON field names currently produced by `json_from_sql_row`.

- [ ] **Step 2: Preserve CLI tests**

Run:

```bash
cargo test -p wc-cli library_page_json_filters_sorts_and_paginates_sqlite
```

Expected: pass. If it fails because ordering changed for unsupported WE entries, add a separate test for the new GUI ordering and preserve existing normal image/video behavior.

### Task 1.7: Verification And Review Gate

- [ ] **Step 1: Run targeted tests**

```bash
cargo test -p wc-storage library_page_sqlite
cargo test -p wallpaper-console-tauri --lib
cargo test -p wc-cli library_page_json_filters_sorts_and_paginates_sqlite
```

- [ ] **Step 2: Run frontend checks because DTO shape must remain stable**

```bash
cd apps/tauri-gui/frontend
npm run typecheck
npm run test:unit
```

- [ ] **Step 3: Run code-review-expert**

Use the exact review prompt from the top of this document.

- [ ] **Step 4: Commit only after review fixes**

```bash
git add crates/wc-storage/src/sqlite.rs apps/tauri-gui/src-tauri/src/commands/library.rs crates/wc-cli/src/main.rs apps/tauri-gui/frontend
git commit -m "perf: use sqlite paging for tauri library views"
```

---

## Priority 2: Extract Frontend Apply Queue And Feedback Coordination From `App.tsx`

**Goal:** Reduce `App.tsx` coordination complexity without changing user-visible apply behavior, Tauri command names, feedback wording, or persistent tab behavior.

**Current Evidence:**

- `apps/tauri-gui/frontend/src/App.tsx` owns view routing, settings modal, feedback event bridge, status refresh, apply queue, stale request handling, and toolbar actions.
- `handleApplyAction()` currently implements latest-intent queueing inside `App.tsx`.
- Backend stale guard still lives in Rust. This priority only moves frontend coordination into focused hooks.

**Files:**

- Create: `apps/tauri-gui/frontend/src/hooks/useApplyQueue.ts`
- Create: `apps/tauri-gui/frontend/src/hooks/useApplyQueue.test.ts`
- Create: `apps/tauri-gui/frontend/src/hooks/useFeedbackBridge.ts`
- Modify: `apps/tauri-gui/frontend/src/App.tsx`
- Test: `apps/tauri-gui/frontend/src/hooks/useApplyQueue.test.ts`

### Task 2.1: Add A Pure Apply Queue Core

- [ ] **Step 1: Create `useApplyQueue.ts` with a testable class**

Add:

```ts
import { useCallback, useRef, useState } from 'react';
import { api, ApplyRequestDTO, ApplyResultDTO } from '../api/bridge';
import { commandErrorFeedback } from '../api/feedback';

export type ApplyFeedback =
  | { state: 'running'; label: string; detail?: string }
  | { state: 'success'; label: string; detail?: string }
  | { state: 'error' | 'warning'; label: string; detail?: string };

export interface ApplyQueueDeps {
  applyAction: (request: ApplyRequestDTO) => Promise<{ success: boolean; stdout: string; stderr: string; error?: { message: string } }>;
  refreshStatus: () => Promise<void>;
  invalidateHistory: () => void;
  setFeedback: (feedback: ApplyFeedback) => void;
  makeErrorFeedback: (label: string, error: unknown) => ApplyFeedback;
}

export class ApplyQueueController {
  private applying = false;
  private pending: ApplyRequestDTO | null = null;
  private readonly onApplyingChange: (value: boolean) => void;
  private readonly deps: ApplyQueueDeps;

  constructor(deps: ApplyQueueDeps, onApplyingChange: (value: boolean) => void) {
    this.deps = deps;
    this.onApplyingChange = onApplyingChange;
  }

  isApplying(): boolean {
    return this.applying;
  }

  enqueue(request: ApplyRequestDTO): void {
    if (this.applying) {
      this.pending = request;
      return;
    }
    void this.run(request);
  }

  private async run(first: ApplyRequestDTO): Promise<void> {
    this.applying = true;
    this.onApplyingChange(true);
    let current: ApplyRequestDTO | null = first;

    while (current !== null) {
      const req = current;
      current = null;
      this.deps.setFeedback({ state: 'running', label: 'Applying wallpaper' });
      try {
        const result = await this.deps.applyAction(req);
        if (result.success) {
          this.deps.invalidateHistory();
          let detail: ApplyResultDTO | undefined;
          try {
            detail = result.stdout ? JSON.parse(result.stdout) as ApplyResultDTO : undefined;
          } catch {
            detail = undefined;
          }
          this.deps.setFeedback({
            state: 'success',
            label: 'Applied',
            detail: detail?.preview ? 'Preview wallpaper applied.' : detail?.appliedPath?.split('/').pop(),
          });
        } else {
          this.deps.setFeedback(this.deps.makeErrorFeedback('Apply', result));
        }
        await this.deps.refreshStatus();
      } catch (error) {
        this.deps.setFeedback(this.deps.makeErrorFeedback('Apply', error));
      }

      const next = this.pending;
      this.pending = null;
      if (next && next.requestId !== req.requestId) {
        current = next;
      }
    }

    this.applying = false;
    this.onApplyingChange(false);
  }
}

export function useApplyQueue(args: {
  refreshStatus: () => Promise<void>;
  setFeedbackWithAutoDismiss: (feedback: ApplyFeedback) => void;
  invalidateHistory: () => void;
}) {
  const [applying, setApplying] = useState(false);
  const controllerRef = useRef<ApplyQueueController | null>(null);

  if (!controllerRef.current) {
    controllerRef.current = new ApplyQueueController(
      {
        applyAction: api.applyAction,
        refreshStatus: args.refreshStatus,
        invalidateHistory: args.invalidateHistory,
        setFeedback: args.setFeedbackWithAutoDismiss,
        makeErrorFeedback: (label, error) => commandErrorFeedback(label, error) as ApplyFeedback,
      },
      setApplying,
    );
  }

  const handleApplyAction = useCallback((request: ApplyRequestDTO) => {
    controllerRef.current?.enqueue(request);
  }, []);

  const handleApply = useCallback((path: string) => {
    handleApplyAction({
      kind: 'apply',
      path,
      requestId: `${Date.now()}-${Math.random().toString(36).slice(2)}`,
    });
  }, [handleApplyAction]);

  return { applying, handleApply, handleApplyAction };
}
```

If TypeScript rejects the narrower `ApplyFeedback` union against the existing `CommandFeedback` type, import and use `CommandFeedback` from `../api/feedback` instead. Do not use `any`.

### Task 2.2: Add Unit Coverage For Latest-Intent Behavior

- [ ] **Step 1: Create `useApplyQueue.test.ts`**

Add:

```ts
import assert from 'node:assert/strict';
import test from 'node:test';
import { ApplyQueueController, ApplyQueueDeps } from './useApplyQueue.ts';
import type { ApplyRequestDTO } from '../api/bridge.ts';

const req = (id: string, path = `/wall/${id}.jpg`): ApplyRequestDTO => ({
  kind: 'apply',
  path,
  requestId: id,
});

test('apply queue runs current request then latest pending request only', async () => {
  const calls: string[] = [];
  let releaseFirst: (() => void) | undefined;
  const firstBlock = new Promise<void>((resolve) => { releaseFirst = resolve; });
  const feedback: string[] = [];
  const applyingStates: boolean[] = [];

  const deps: ApplyQueueDeps = {
    applyAction: async (request) => {
      calls.push(request.requestId ?? '');
      if (request.requestId === 'a') await firstBlock;
      return {
        success: true,
        stdout: JSON.stringify({
          requestId: request.requestId,
          appliedPath: request.path,
          statePath: request.path,
          backend: 'awww',
          fileType: 'image',
          preview: false,
        }),
        stderr: '',
      };
    },
    refreshStatus: async () => {},
    invalidateHistory: () => {},
    setFeedback: (value) => feedback.push(`${value.state}:${value.label}`),
    makeErrorFeedback: (label) => ({ state: 'error', label }),
  };

  const controller = new ApplyQueueController(deps, (value) => applyingStates.push(value));
  controller.enqueue(req('a'));
  controller.enqueue(req('b'));
  controller.enqueue(req('c'));
  releaseFirst?.();
  await new Promise((resolve) => setTimeout(resolve, 20));

  assert.deepEqual(calls, ['a', 'c']);
  assert.deepEqual(applyingStates, [true, false]);
  assert(feedback.includes('running:Applying wallpaper'));
  assert(feedback.includes('success:Applied'));
});
```

- [ ] **Step 2: Run the test and fix type issues only**

Run:

```bash
cd apps/tauri-gui/frontend
npm run test:unit -- src/hooks/useApplyQueue.test.ts
```

Expected: Node test runner may ignore the extra file arg because the package script already provides a glob. If so, run:

```bash
node --experimental-strip-types --test "src/hooks/useApplyQueue.test.ts"
```

Expected: pass.

### Task 2.3: Extract Feedback Event Bridge

- [ ] **Step 1: Create `useFeedbackBridge.ts`**

Add:

```ts
import { useEffect } from 'react';
import type { CommandFeedback } from '../api/feedback';

export function useFeedbackBridge(setFeedback: (feedback: CommandFeedback) => void): void {
  useEffect(() => {
    const handler = (event: Event) => {
      const detail = (event as CustomEvent<CommandFeedback>).detail;
      if (detail) setFeedback(detail);
    };
    window.addEventListener('wc-feedback', handler);
    return () => window.removeEventListener('wc-feedback', handler);
  }, [setFeedback]);
}
```

Do not change the `wc-feedback` event name.

### Task 2.4: Simplify `App.tsx`

- [ ] **Step 1: Replace local apply state with hook**

In `App.tsx`, remove:

```ts
const [applying, setApplying] = useState(false);
const applyingRef = useRef(false);
const pendingApplyActionRef = useRef<ApplyRequestDTO | null>(null);
const handleApplyAction = ...
const handleApply = ...
```

Import:

```ts
import { useApplyQueue } from './hooks/useApplyQueue';
import { useFeedbackBridge } from './hooks/useFeedbackBridge';
```

Use:

```ts
useFeedbackBridge(setFeedbackWithAutoDismiss);

const { applying, handleApply, handleApplyAction } = useApplyQueue({
  refreshStatus,
  setFeedbackWithAutoDismiss,
  invalidateHistory: invalidateHistoryCache,
});
```

- [ ] **Step 2: Keep toolbar and view props unchanged**

Do not change these prop names:

```tsx
<Toolbar view={view} onAction={handleToolbarAction} applying={applying} />
<LibraryView onApply={handleApply} onApplyAction={handleApplyAction} applying={applying} active={view === v} />
<FavoritesView onApply={handleApply} onApplyAction={handleApplyAction} applying={applying} active={view === v} />
<HistoryView onApply={handleApply} onApplyAction={handleApplyAction} applying={applying} active={view === v} />
```

### Task 2.5: Verification And Review Gate

- [ ] **Step 1: Run frontend checks**

```bash
cd apps/tauri-gui/frontend
npm run test:unit
npm run typecheck
npm run smoke
```

- [ ] **Step 2: Run code-review-expert**

Use the exact review prompt from the top of this document.

- [ ] **Step 3: Commit**

```bash
git add apps/tauri-gui/frontend/src/App.tsx apps/tauri-gui/frontend/src/hooks/useApplyQueue.ts apps/tauri-gui/frontend/src/hooks/useApplyQueue.test.ts apps/tauri-gui/frontend/src/hooks/useFeedbackBridge.ts
git commit -m "refactor: isolate frontend apply queue"
```

---

## Priority 3: Harden Thumbnail Queue Against Stale Async Completions

**Goal:** Prevent old thumbnail requests from writing stale state after `reset()`, `forget()`, or provider disposal. Expose invalidation through the thumbnail store so cache-clearing and future rescan flows can invalidate specific paths safely.

**Current Evidence:**

- `ThumbnailRequestQueue.reset()` clears `thumbs`, `queue`, and `inFlight`, but async `load()` promises already started can still complete and write to `thumbs`.
- `forget()` removes pending/cached paths but cannot stop an in-flight completion from writing the same path back.
- `ThumbnailStoreContext` currently exposes `thumbs`, `enqueue`, and `reset`, but not `forget()`.

**Files:**

- Modify: `apps/tauri-gui/frontend/src/hooks/thumbnailQueueCore.ts`
- Modify: `apps/tauri-gui/frontend/src/hooks/thumbnailQueueCore.test.ts`
- Modify: `apps/tauri-gui/frontend/src/hooks/useThumbnailQueue.ts`
- Modify: `apps/tauri-gui/frontend/src/state/ThumbnailStoreContext.tsx`
- Optional: `apps/tauri-gui/frontend/src/views/SettingsView.tsx` if wiring thumbnail cache clear to queue reset is straightforward

### Task 3.1: Add Generation Tokens To Queue Items

- [ ] **Step 1: Update `QueueItem` and class fields**

Change:

```ts
type QueueItem = { path: string };
```

to:

```ts
type QueueItem = { path: string; generation: number; pathVersion: number };
```

Add fields:

```ts
private generation = 0;
private pathVersions = new Map<string, number>();
```

- [ ] **Step 2: Add a version helper**

Add method:

```ts
private versionFor(path: string): number {
  return this.pathVersions.get(path) ?? 0;
}
```

### Task 3.2: Gate Completion Writes

- [ ] **Step 1: Update `enqueue()` to capture tokens**

Replace item creation:

```ts
const items = unique.map((path) => ({ path }));
```

with:

```ts
const items = unique.map((path) => ({
  path,
  generation: this.generation,
  pathVersion: this.versionFor(path),
}));
```

- [ ] **Step 2: Update `pump()` write path**

Inside `.then((thumb) => { ... })`, only write when tokens still match:

```ts
if (
  !this.disposed &&
  item.generation === this.generation &&
  item.pathVersion === this.versionFor(item.path) &&
  thumb.thumbnail
) {
  this.thumbs = { ...this.thumbs, [item.path]: thumb.thumbnail };
}
this.emit();
```

Inside `.catch()`, keep `this.emit()` but do not mutate `thumbs`.

Inside `.finally()`, delete from `inFlight` and call `pump()` only when not disposed:

```ts
this.inFlight.delete(item.path);
if (!this.disposed) this.pump();
```

### Task 3.3: Make Reset/Forget Invalidate In-Flight Work

- [ ] **Step 1: Update `forget()`**

Replace `forget()` with:

```ts
forget(paths: string[]): void {
  const set = new Set(paths);
  for (const path of set) {
    delete this.thumbs[path];
    this.pathVersions.set(path, this.versionFor(path) + 1);
  }
  this.queue = this.queue.filter((item) => !set.has(item.path));
  this.emit();
}
```

- [ ] **Step 2: Update `reset()`**

Replace `reset()` with:

```ts
reset(): void {
  this.generation += 1;
  this.thumbs = {};
  this.queue = [];
  this.inFlight.clear();
  this.emit();
}
```

- [ ] **Step 3: Update `dispose()`**

Replace `dispose()` with:

```ts
dispose(): void {
  this.disposed = true;
  this.generation += 1;
  this.queue = [];
  this.inFlight.clear();
}
```

### Task 3.4: Add Race Tests

- [ ] **Step 1: Add reset stale completion test**

In `thumbnailQueueCore.test.ts`, add:

```ts
test('thumbnail queue ignores in-flight completion after reset', async () => {
  let release: (() => void) | undefined;
  const blocked = new Promise<void>((resolve) => { release = resolve; });
  let latestState: Record<string, string> = {};
  const queue = new ThumbnailRequestQueue({
    concurrency: 1,
    load: async (path) => {
      await blocked;
      return thumb(path);
    },
    onUpdate: (state) => { latestState = state; },
  });

  queue.enqueue(['stale']);
  queue.reset();
  release?.();
  await new Promise((resolve) => setTimeout(resolve, 20));

  assert.deepEqual(latestState, {});
});
```

- [ ] **Step 2: Add forget stale completion test**

Add:

```ts
test('thumbnail queue ignores in-flight completion after forget', async () => {
  let release: (() => void) | undefined;
  const blocked = new Promise<void>((resolve) => { release = resolve; });
  let latestState: Record<string, string> = {};
  const queue = new ThumbnailRequestQueue({
    concurrency: 1,
    load: async (path) => {
      await blocked;
      return thumb(path);
    },
    onUpdate: (state) => { latestState = state; },
  });

  queue.enqueue(['x']);
  queue.forget(['x']);
  release?.();
  await new Promise((resolve) => setTimeout(resolve, 20));

  assert.deepEqual(latestState, {});
});
```

- [ ] **Step 3: Run the focused test**

```bash
cd apps/tauri-gui/frontend
node --experimental-strip-types --test "src/hooks/thumbnailQueueCore.test.ts"
```

Expected: all thumbnail queue tests pass.

### Task 3.5: Expose `forget()` Through Hooks And Context

- [ ] **Step 1: Update `useThumbnailQueue.ts` return value**

Add:

```ts
const forget = useCallback((paths: string[]) => {
  queueRef.current?.forget(paths);
}, []);

return { thumbs, enqueue, reset, forget };
```

- [ ] **Step 2: Update `ThumbnailStoreContext.tsx` interface**

Add:

```ts
forget: (paths: string[]) => void;
```

Do not rename `reset()`.

### Task 3.6: Optional Wiring For Cache Clear

Only do this if it is a small local change. If it requires broad prop drilling, skip it and record as follow-up.

- [ ] **Step 1: If `SettingsView` can access thumbnail store without awkward coupling, call `reset()` after successful `thumbnailCacheClear()` or cleanup**

Do not import thumbnail store into low-level settings page components. Only wire at `SettingsView` level.

### Task 3.7: Verification And Review Gate

- [ ] **Step 1: Run frontend verification**

```bash
cd apps/tauri-gui/frontend
npm run test:unit
npm run typecheck
npm run smoke
```

- [ ] **Step 2: Run code-review-expert**

Use the exact review prompt from the top of this document. Ask the reviewer to focus on stale async completions and React state races.

- [ ] **Step 3: Commit**

```bash
git add apps/tauri-gui/frontend/src/hooks/thumbnailQueueCore.ts apps/tauri-gui/frontend/src/hooks/thumbnailQueueCore.test.ts apps/tauri-gui/frontend/src/hooks/useThumbnailQueue.ts apps/tauri-gui/frontend/src/state/ThumbnailStoreContext.tsx apps/tauri-gui/frontend/src/views/SettingsView.tsx
git commit -m "fix: ignore stale thumbnail completions"
```

---

## Priority 4: Streaming Scan Progress And Cancellation

**Goal:** Make Tauri rescan progress/cancel responsive during directory walking, not only after `scan_wallpapers()` has returned a complete `Vec`.

**Current Evidence:**

- `apps/tauri-gui/src-tauri/src/commands/scan.rs::index_current_sources()` calls `wc_scan::scan_wallpapers(&sources)` before it can set `total_hint`.
- `wc_scan::scan_wallpapers()` owns the walking loop and exposes only a completed `Vec<String>`.
- CLI still depends on `scan_wallpapers()` returning `Vec<String>`, so keep that function as a compatibility wrapper.

**Files:**

- Modify: `crates/wc-scan/src/lib.rs`
- Modify: `apps/tauri-gui/src-tauri/src/commands/scan.rs`
- Test: `crates/wc-scan/src/lib.rs`
- Test: `apps/tauri-gui/src-tauri/src/commands/scan.rs`

### Task 4.1: Add A Scan Event API Without Removing `scan_wallpapers`

- [ ] **Step 1: Add event/control types to `wc-scan`**

Near `scan_wallpapers()`, add:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanEvent {
    SourceStarted { source: String },
    CandidateFound { path: String, count: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanControl {
    Continue,
    Cancel,
}
```

- [ ] **Step 2: Add callback-based scanner**

Add:

```rust
pub fn scan_wallpapers_with_callback<F>(sources: &[String], mut on_event: F) -> Vec<String>
where
    F: FnMut(ScanEvent) -> ScanControl,
{
    let deduped = dedupe_sources(sources);
    let mut seen: HashSet<String> = HashSet::new();
    let mut files: Vec<String> = Vec::new();

    for source in &deduped {
        if matches!(on_event(ScanEvent::SourceStarted { source: source.clone() }), ScanControl::Cancel) {
            break;
        }
        let src_path = Path::new(source);
        if !src_path.is_dir() {
            continue;
        }
        let cancelled = match we_source_kind(source) {
            WeKind::WorkshopRoot => scan_we_workshop_root_with_callback(src_path, &mut seen, &mut files, &mut on_event),
            WeKind::ProjectDir => scan_we_project_dir_with_callback(src_path, &mut seen, &mut files, &mut on_event),
            WeKind::Normal => scan_dir_recursive_with_callback(src_path, &mut seen, &mut files, &mut on_event),
        };
        if cancelled {
            break;
        }
    }
    files
}
```

Implement the three `_with_callback` helpers by copying the existing logic from `scan_we_workshop_root`, `ProjectDir` handling, and `scan_dir_recursive`, with this rule: immediately after pushing a new canonical candidate, call:

```rust
if matches!(
    on_event(ScanEvent::CandidateFound { path: c.clone(), count: files.len() }),
    ScanControl::Cancel
) {
    return true;
}
```

Return `true` when cancelled and `false` otherwise.

- [ ] **Step 3: Keep existing wrapper**

Replace old `scan_wallpapers()` internals with:

```rust
pub fn scan_wallpapers(sources: &[String]) -> Vec<String> {
    scan_wallpapers_with_callback(sources, |_| ScanControl::Continue)
}
```

Do not change CLI call sites in this task.

### Task 4.2: Test Callback Cancellation

- [ ] **Step 1: Add wc-scan test**

In `crates/wc-scan/src/lib.rs` tests, add:

```rust
#[test]
fn scan_wallpapers_with_callback_can_cancel_after_first_candidate() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("walls");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.jpg"), b"a").unwrap();
    std::fs::write(dir.join("b.jpg"), b"b").unwrap();
    let source = dir.to_string_lossy().to_string();
    let mut seen_candidates = 0usize;

    let files = scan_wallpapers_with_callback(&[source], |event| {
        if matches!(event, ScanEvent::CandidateFound { .. }) {
            seen_candidates += 1;
            return ScanControl::Cancel;
        }
        ScanControl::Continue
    });

    assert_eq!(seen_candidates, 1);
    assert_eq!(files.len(), 1);
}
```

- [ ] **Step 2: Run test**

```bash
cargo test -p wc-scan scan_wallpapers_with_callback_can_cancel_after_first_candidate
```

Expected: pass.

### Task 4.3: Wire Tauri Scan Progress To Callback

- [ ] **Step 1: Replace walking stage in `index_current_sources()`**

In `apps/tauri-gui/src-tauri/src/commands/scan.rs`, replace:

```rust
let paths = wc_scan::scan_wallpapers(&sources);
if let Ok(mut state) = scan_state().lock() {
    state.total_hint = Some(paths.len());
}
```

with:

```rust
let paths = wc_scan::scan_wallpapers_with_callback(&sources, |event| {
    match scan_state().lock() {
        Ok(mut state) => {
            if state.cancel_requested {
                return wc_scan::ScanControl::Cancel;
            }
            match event {
                wc_scan::ScanEvent::SourceStarted { source } => {
                    state.stage = "walking files".into();
                    state.current_path = Some(source);
                }
                wc_scan::ScanEvent::CandidateFound { path, count } => {
                    state.stage = "walking files".into();
                    state.total_hint = Some(count);
                    state.current_path = Some(path);
                }
            }
            wc_scan::ScanControl::Continue
        }
        Err(_) => wc_scan::ScanControl::Cancel,
    }
});
if scan_cancelled()? {
    return Err("scan cancelled".to_string());
}
if let Ok(mut state) = scan_state().lock() {
    state.total_hint = Some(paths.len());
}
```

Do not hold the scan-state mutex while calling metadata probes or SQLite writes.

### Task 4.4: Preserve Metadata Caching During Scan

- [ ] **Step 1: Use SQLite prior cache in Tauri scan metadata phase**

In `index_current_sources()`, before metadata loop, add:

```rust
let prior_cache = wc_storage::sqlite::prior_metadata_cache_from_sqlite(&s.cd);
```

Replace:

```rust
if let Some(entry) = wc_scan::make_entry(path) {
    entries.push(entry);
}
```

with:

```rust
let (entry, was_reused) = wc_scan::make_entry_cached(path, &prior_cache);
if let Ok(mut state) = scan_state().lock() {
    if was_reused {
        state.reused_metadata += 1;
    } else {
        state.probed_metadata += 1;
    }
}
if let Some(entry) = entry {
    entries.push(entry);
}
```

If current code already has this caching by the time DeepSeek starts, do not duplicate it. Verify and leave it unchanged.

### Task 4.5: Add Tauri Scan State Tests

- [ ] **Step 1: Add test for cancel flag handling**

In `apps/tauri-gui/src-tauri/src/commands/scan.rs` tests, add a focused test for state transitions:

```rust
#[test]
fn scan_cancel_sets_cancel_requested_when_running() {
    let _guard = TEST_SCAN_LOCK.lock().unwrap();
    reset_scan_state_for_test();
    mark_scan_started("walking files").unwrap();
    {
        let mut state = scan_state().lock().unwrap();
        state.cancel_requested = true;
    }
    assert!(scan_cancelled().unwrap());
    finish_scan_error("scan cancelled");
}
```

If `scan_cancelled()` is private and the test module cannot access it, keep the test in the same module as current tests so private access works.

### Task 4.6: Verification And Review Gate

- [ ] **Step 1: Run targeted Rust tests**

```bash
cargo test -p wc-scan scan_wallpapers_with_callback_can_cancel_after_first_candidate
cargo test -p wallpaper-console-tauri --lib scan
```

- [ ] **Step 2: Run broader Rust checks**

```bash
cargo test -p wc-scan
cargo test -p wallpaper-console-tauri --lib
cargo check --workspace
```

- [ ] **Step 3: Run code-review-expert**

Use the exact review prompt from the top of this document. Ask the reviewer to focus on lock scope, cancellation behavior, and compatibility of `scan_wallpapers()`.

- [ ] **Step 4: Commit**

```bash
git add crates/wc-scan/src/lib.rs apps/tauri-gui/src-tauri/src/commands/scan.rs
git commit -m "feat: report scan progress during directory walk"
```

---

## Priority 5: Align Documentation, Benchmarks, And Verification With Runtime Truth

**Goal:** Update docs and tests so they describe what the code actually does after priorities 1-4. Close the known manual GUI acceptance gap without claiming unverified desktop success.

**Current Evidence:**

- `README.md` currently says "SQLite-backed paging" and "SQLite query indexes" for GUI loading.
- Before Priority 1, the GUI path did not actually use SQL-level pagination.
- `docs/CURRENT_STATUS.md` says real desktop GUI visual acceptance is environment-limited and must not be marked accepted without an interactive desktop run.
- `docs/PERFORMANCE_BASELINE.md` measures CLI `library-page-json`, not the Tauri command directly.

**Files:**

- Modify: `README.md`
- Modify: `docs/CURRENT_STATUS.md`
- Modify: `docs/PERFORMANCE_BASELINE.md`
- Modify: `docs/DEVELOPMENT.md`
- Optional create: `scripts/benchmark_tauri_library_command.sh`
- Optional docs: `docs/TAURI_MANUAL_SMOKE_CHECKLIST.md`

### Task 5.1: Update README To Match The Implemented State

- [ ] **Step 1: If Priority 1 was completed, keep and sharpen SQLite paging claim**

In `README.md`, update the GUI feature line to:

```markdown
- Library: SQLite-backed SQL paging, filter by type, sort, filename/title/Workshop ID search
```

Update performance bullet to:

```markdown
- **SQLite query indexes**: indexed type/sort paths used by GUI and CLI paged loading
```

- [ ] **Step 2: If Priority 1 was not completed, weaken the claim instead**

Use:

```markdown
- Library: SQLite-backed loading with frontend paging controls, filter by type, sort, filename/title/Workshop ID search
```

and:

```markdown
- **SQLite query indexes**: available for paged loading; GUI SQL-level pagination is tracked as hardening work
```

Do not claim SQL-level GUI paging unless code actually calls shared SQLite page helpers from Tauri.

### Task 5.2: Update Current Status

- [ ] **Step 1: Add a new closeout row**

In `docs/CURRENT_STATUS.md`, add a row under "Current Closeout Status" for the completed priorities. Use exact wording based on what was actually implemented:

```markdown
| SQL-level GUI pagination | Completed | `library_page_gui`, `favorites_page`, and `history_page` use shared SQLite `COUNT` + `LIMIT/OFFSET` helpers; storage and Tauri tests pass |
```

Only add rows for priorities that were completed and reviewed.

- [ ] **Step 2: Preserve manual GUI acceptance gap**

Do not remove or soften:

```markdown
Real desktop GUI visual acceptance | Environment-limited
```

If manual GUI was not run, add:

```markdown
Manual desktop acceptance remains open; automated smoke tests do not prove compositor/runtime behavior on niri.
```

### Task 5.3: Update Performance Baseline Scope

- [ ] **Step 1: Clarify what benchmark measures**

In `docs/PERFORMANCE_BASELINE.md`, update the environment section:

```markdown
- Shell-run benchmark mode: Rust CLI `library-page-json` calls; after SQL helper extraction this exercises the same `wc-storage` SQLite page helper used by Tauri.
```

Only use "same helper" after Priority 1 has actually refactored CLI and Tauri to shared storage functions.

- [ ] **Step 2: Add Tauri command benchmark note**

Add:

```markdown
Tauri command latency is not identical to CLI process latency. For GUI-specific timing, use the frontend performance overlay (`library.page.ms`) or add a Tauri command harness before comparing WebView behavior.
```

### Task 5.4: Optional Tauri Command Benchmark Script

Only create this script if there is a simple existing way to invoke Tauri commands headlessly. If not, do not invent a fragile harness.

- [ ] **Step 1: If no reliable harness exists, document the non-goal**

Add to `docs/PERFORMANCE_BASELINE.md`:

```markdown
No headless Tauri command benchmark is currently included. The supported GUI timing signal is the frontend performance overlay plus Playwright smoke behavior.
```

### Task 5.5: Add Final Verification Checklist To DEVELOPMENT

- [ ] **Step 1: Add per-priority review rule**

In `docs/DEVELOPMENT.md`, add a section:

```markdown
## Priority Hardening Review Rule

For production-hardening work that touches Tauri commands, scan performance, thumbnail queues, or backend lifecycle:

1. Implement one priority at a time.
2. Run targeted tests for the touched subsystem.
3. Run `code-review-expert` on the current git diff.
4. Fix all P0/P1 findings before continuing.
5. Run the broader verification matrix before final closeout.
```

### Task 5.6: Final Full Verification And Review Gate

- [ ] **Step 1: Run formatting and Rust verification**

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo build --workspace
```

- [ ] **Step 2: Run frontend verification**

```bash
cd apps/tauri-gui/frontend
npm run test:unit
npm run typecheck
npm run build
npm run smoke
```

- [ ] **Step 3: Run install/package checks if touched docs mention release readiness**

```bash
cd /home/chakew/Projects/wallpaper-console-rust
./install.sh --build-only
./scripts/test_install_build_only.sh
cd apps/tauri-gui/src-tauri
cargo tauri build --bundles deb,rpm
```

- [ ] **Step 4: Run whitespace check**

```bash
git diff --check
```

- [ ] **Step 5: Run code-review-expert**

Use the exact review prompt from the top of this document. For Priority 5, ask the reviewer to focus on doc truthfulness: docs must not claim manual GUI acceptance unless it was actually completed.

- [ ] **Step 6: Commit**

```bash
git add README.md docs/CURRENT_STATUS.md docs/PERFORMANCE_BASELINE.md docs/DEVELOPMENT.md docs/TAURI_MANUAL_SMOKE_CHECKLIST.md scripts/benchmark_tauri_library_command.sh
git commit -m "docs: align hardening status with runtime behavior"
```

If optional files were not created or changed, omit them from `git add`.

---

## Final Closeout Report Required From DeepSeek

After all selected priorities are complete, report:

```markdown
## Completed Priorities
- Priority 1: <done/skipped> - <one sentence>
- Priority 2: <done/skipped> - <one sentence>
- Priority 3: <done/skipped> - <one sentence>
- Priority 4: <done/skipped> - <one sentence>
- Priority 5: <done/skipped> - <one sentence>

## Review Results
- Priority 1 code-review-expert: <P0/P1/P2/P3 counts>
- Priority 2 code-review-expert: <P0/P1/P2/P3 counts>
- Priority 3 code-review-expert: <P0/P1/P2/P3 counts>
- Priority 4 code-review-expert: <P0/P1/P2/P3 counts>
- Priority 5 code-review-expert: <P0/P1/P2/P3 counts>

## Verification
- cargo fmt --all -- --check: <pass/fail/not run>
- cargo test --workspace: <pass/fail/not run>
- cargo clippy --workspace -- -D warnings: <pass/fail/not run>
- cargo build --workspace: <pass/fail/not run>
- npm run test:unit: <pass/fail/not run>
- npm run typecheck: <pass/fail/not run>
- npm run build: <pass/fail/not run>
- npm run smoke: <pass/fail/not run>
- git diff --check: <pass/fail/not run>

## Residual Risks
- Manual GUI acceptance on real niri/Wayland desktop: <done/open>
- Any skipped optional task: <reason>
```

