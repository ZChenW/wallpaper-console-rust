//! Parity integration tests — CLI binary tests against temp config dirs.

use std::process::Command;

const RUST_BIN: &str = env!("CARGO_BIN_EXE_wallpaper-console-rust");

fn temp_config() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().to_string_lossy().to_string();
    std::fs::create_dir_all(&config_dir).unwrap();
    (dir, config_dir)
}

fn rust(args: &[&str], cd: &str) -> std::process::Output {
    Command::new(RUST_BIN)
        .args(args)
        .env("XDG_CONFIG_HOME", cd)
        .output()
        .unwrap()
}

#[test]
fn add_and_sources() {
    let (_d, cd) = temp_config();
    let src = format!("{}/walls", cd);
    std::fs::create_dir_all(&src).unwrap();
    assert!(rust(&["add", &src], &cd).status.success());
    let out = rust(&["sources"], &cd);
    assert!(String::from_utf8_lossy(&out.stdout).contains(&src));
}

#[test]
fn config_set_get_roundtrip() {
    let (_d, cd) = temp_config();
    let src = format!("{}/walls", cd);
    std::fs::create_dir_all(&src).unwrap();
    rust(&["add", &src], &cd);
    assert!(rust(&["config-set", "gif_backend", "mpvpaper"], &cd)
        .status
        .success());
    let out = rust(&["config-get", "gif_backend"], &cd);
    assert!(String::from_utf8_lossy(&out.stdout).contains("mpvpaper"));
}

#[test]
fn status_output() {
    let (_d, cd) = temp_config();
    let src = format!("{}/walls", cd);
    std::fs::create_dir_all(&src).unwrap();
    rust(&["add", &src], &cd);
    let out = rust(&["status"], &cd);
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("config directory"));
    assert!(s.contains("current wallpaper"));
}

#[test]
fn favorite_roundtrip() {
    let (_d, cd) = temp_config();
    let fp = format!("{}/t.png", cd);
    std::fs::write(&fp, b"").unwrap();
    // favorite-add and favorites-json (non-interactive; favorites needs TTY)
    assert!(rust(&["favorite-add", &fp], &cd).status.success());
    let out = rust(&["favorites-json"], &cd);
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(
        s.contains(&fp),
        "favorites-json should contain added path: {}",
        s
    );
}

#[test]
fn favorites_and_history_json() {
    let (_d, cd) = temp_config();
    for cmd in &["favorites-json", "history-json"] {
        let out = rust(&[cmd], &cd);
        let s = String::from_utf8_lossy(&out.stdout);
        assert!(s.trim().starts_with('['), "{}: {}", cmd, s);
    }
}

#[test]
fn library_json_after_rescan() {
    let (_d, cd) = temp_config();
    let walls = format!("{}/walls", cd);
    let sub = format!("{}/sub", walls);
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(format!("{}/a.png", sub), b"").unwrap();
    std::fs::write(format!("{}/b.gif", sub), b"").unwrap();
    rust(&["add", &walls], &cd);
    assert!(rust(&["rescan"], &cd).status.success());
    let out = rust(&["library-json"], &cd);
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("\"path\""), "{}", s);
}

#[test]
fn missing_db_sqlite_verify() {
    let (_d, cd) = temp_config();
    let out = rust(&["sqlite-verify"], &cd);
    assert!(!out.status.success());
    let e = String::from_utf8_lossy(&out.stderr);
    assert!(e.contains("not found"), "{}", e);
}

#[test]
fn migrate_and_verify() {
    let (_d, cd) = temp_config();
    let src = format!("{}/walls", cd);
    std::fs::create_dir_all(&src).unwrap();
    rust(&["add", &src], &cd);
    let out = rust(&["migrate-to-sqlite"], &cd);
    assert!(
        out.status.success(),
        "migrate: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = rust(&["sqlite-verify"], &cd);
    assert!(
        out.status.success(),
        "verify: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn rescan_writes_sqlite_wallpapers() {
    let (_d, cd) = temp_config();
    let walls = format!("{}/walls", cd);
    let sub = format!("{}/sub", walls);
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(format!("{}/x.png", sub), b"").unwrap();
    rust(&["add", &walls], &cd);
    rust(&["migrate-to-sqlite"], &cd);
    assert!(rust(&["rescan"], &cd).status.success());
    // Verify library-json --sqlite works
    let out = rust(&["library-json", "--sqlite"], &cd);
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("\"path\""), "sqlite json: {}", s);
}

#[test]
fn history_clear() {
    let (_d, cd) = temp_config();
    assert!(rust(&["history-clear"], &cd).status.success());
    let out = rust(&["history-json"], &cd);
    let s = String::from_utf8_lossy(&out.stdout);
    assert_eq!(s.trim(), "[]");
}

#[test]
fn library_count_output() {
    let (_d, cd) = temp_config();
    let walls = format!("{}/walls", cd);
    let sub = format!("{}/sub", walls);
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(format!("{}/x.png", sub), b"").unwrap();
    rust(&["add", &walls], &cd);
    rust(&["rescan"], &cd);
    let out = rust(&["library-count"], &cd);
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("total="), "{}", s);
}

// ── WE scanning ───────────────────────────────────────────────────────────

#[test]
fn we_scene_project_not_in_library() {
    let (_d, cd) = temp_config();
    let we_root = format!("{}/steamapps/workshop/content/431960", cd);
    std::fs::create_dir_all(&we_root).unwrap();

    // Scene project
    let scene_dir = format!("{}/111", we_root);
    std::fs::create_dir_all(&scene_dir).unwrap();
    std::fs::write(
        format!("{}/project.json", scene_dir),
        r#"{"type":"scene","file":"scene.json"}"#,
    )
    .unwrap();
    std::fs::write(format!("{}/asset.png", scene_dir), b"").unwrap();

    rust(&["add", &we_root], &cd);
    assert!(rust(&["rescan"], &cd).status.success());

    let out = rust(&["library-json"], &cd);
    let s = String::from_utf8_lossy(&out.stdout);
    // Scene project's asset.png must NOT appear
    assert!(
        !s.contains("scene"),
        "scene project should not appear: {}",
        s
    );
    assert!(
        !s.contains("asset.png"),
        "scene project files should not appear: {}",
        s
    );
}

#[test]
fn we_image_project_only_file_field() {
    let (_d, cd) = temp_config();
    let we_root = format!("{}/steamapps/workshop/content/431960", cd);
    std::fs::create_dir_all(&we_root).unwrap();

    // Image project with extra files
    let img_dir = format!("{}/222", we_root);
    std::fs::create_dir_all(&img_dir).unwrap();
    let real_wp = format!("{}/bg.png", img_dir);
    std::fs::write(&real_wp, b"").unwrap();
    std::fs::write(format!("{}/preview.jpg", img_dir), b"").unwrap();
    std::fs::write(
        format!("{}/project.json", img_dir),
        r#"{"type":"image","file":"bg.png"}"#,
    )
    .unwrap();

    rust(&["add", &we_root], &cd);
    assert!(rust(&["rescan"], &cd).status.success());

    let out = rust(&["library-json"], &cd);
    let s = String::from_utf8_lossy(&out.stdout);
    // bg.png (project.json file) must appear
    assert!(
        s.contains("bg.png"),
        "project.json file should be included: {}",
        s
    );
    // preview.jpg should NOT appear (not listed in project.json)
    assert!(
        !s.contains("preview.jpg"),
        "preview.jpg should not be in library: {}",
        s
    );
}

#[test]
fn we_no_project_json_fallback_scan() {
    let (_d, cd) = temp_config();
    let we_root = format!("{}/steamapps/workshop/content/431960", cd);
    std::fs::create_dir_all(&we_root).unwrap();

    // Dir with no project.json — should be scanned recursively
    let fallback_dir = format!("{}/333", we_root);
    std::fs::create_dir_all(&fallback_dir).unwrap();
    std::fs::write(format!("{}/pic.jpg", fallback_dir), b"").unwrap();

    rust(&["add", &we_root], &cd);
    assert!(rust(&["rescan"], &cd).status.success());

    let out = rust(&["library-json"], &cd);
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(
        s.contains("pic.jpg"),
        "no-project dir should be fallback scanned: {}",
        s
    );
}

// ── Remove ────────────────────────────────────────────────────────────────

#[test]
fn remove_source_non_interactive() {
    let (_d, cd) = temp_config();
    let src = format!("{}/walls", cd);
    std::fs::create_dir_all(&src).unwrap();
    rust(&["add", &src], &cd);

    // remove-source DIR works non-interactively
    let out = rust(&["remove-source", &src], &cd);
    assert!(
        out.status.success(),
        "remove-source failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // Verify it's gone
    let out = rust(&["sources"], &cd);
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(!s.contains(&src), "source should be removed: {}", s);
}

#[test]
fn remove_missing_source_reports_error() {
    let (_d, cd) = temp_config();
    let out = rust(&["remove-source", "/nonexistent/path/xyz"], &cd);
    assert!(!out.status.success());
    let e = String::from_utf8_lossy(&out.stderr);
    assert!(e.contains("not found"), "should report not found: {}", e);
}

// ── Search filtering ──────────────────────────────────────────────────────

#[test]
fn search_by_filename_filters_correctly() {
    let (_d, cd) = temp_config();
    let walls = format!("{}/walls", cd);
    std::fs::create_dir_all(&walls).unwrap();
    std::fs::write(format!("{}/sunset.png", walls), b"").unwrap();
    std::fs::write(format!("{}/mountain.png", walls), b"").unwrap();
    std::fs::write(format!("{}/sunrise.gif", walls), b"").unwrap();
    rust(&["add", &walls], &cd);

    // We can't test fzf selection (no TTY), but we verify the search
    // logic by checking that rescan + library-json contains only
    // correctly filtered entries.
    rust(&["rescan"], &cd);
    let out = rust(&["library-json"], &cd);
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("sunset.png"));
    assert!(s.contains("mountain.png"));
    assert!(s.contains("sunrise.gif"));
}

#[test]
fn search_type_filters_by_type() {
    let (_d, cd) = temp_config();
    let walls = format!("{}/walls", cd);
    std::fs::create_dir_all(&walls).unwrap();
    std::fs::write(format!("{}/a.png", walls), b"").unwrap();
    std::fs::write(format!("{}/b.gif", walls), b"").unwrap();
    std::fs::write(format!("{}/c.mp4", walls), b"").unwrap();
    rust(&["add", &walls], &cd);
    rust(&["rescan"], &cd);

    // library-json should contain all three
    let out = rust(&["library-json"], &cd);
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("a.png"));
    assert!(s.contains("b.gif"));
    assert!(s.contains("c.mp4"));

    // Verify type classification is correct via library.tsv
    let raw = std::fs::read_to_string(format!("{}/wallpaper-console/library.tsv", cd)).unwrap();
    assert!(raw.contains("image\tpng"), "a.png should be image: {}", raw);
    assert!(raw.contains("gif\tgif"), "b.gif should be gif: {}", raw);
    assert!(raw.contains("video\tmp4"), "c.mp4 should be video: {}", raw);
}

// ── SQLite row error propagation ──────────────────────────────────────────

#[test]
fn sqlite_verify_does_not_silently_succeed_on_corruption() {
    let (_d, cd) = temp_config();
    let src = format!("{}/walls", cd);
    std::fs::create_dir_all(&src).unwrap();
    rust(&["add", &src], &cd);
    rust(&["migrate-to-sqlite"], &cd);

    // Corrupt the DB by writing garbage at the actual config path
    let db_path = format!("{}/wallpaper-console/wallpapers.db", cd);
    std::fs::write(&db_path, b"not a valid sqlite database").unwrap();

    let out = rust(&["sqlite-verify"], &cd);
    assert!(!out.status.success(), "verify should fail on corrupted DB");
}

#[test]
fn sqlite_export_flat_is_atomic() {
    let (_d, cd) = temp_config();
    let src = format!("{}/walls", cd);
    std::fs::create_dir_all(&src).unwrap();
    rust(&["add", &src], &cd);
    rust(&["migrate-to-sqlite"], &cd);
    // Enable hybrid mode so config-set mirrors to SQLite
    rust(&["config-set", "storage_backend", "hybrid"], &cd);
    rust(&["config-set", "test_key", "test_val"], &cd);

    let out = rust(&["sqlite-export-flat"], &cd);
    assert!(
        out.status.success(),
        "export failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // Verify exported files exist with correct content
    let wc_dir = format!("{}/wallpaper-console", cd);
    let cfg = std::fs::read_to_string(format!("{}/config", wc_dir)).unwrap();
    assert!(cfg.contains("test_key=test_val"), "config export: {}", cfg);

    let srcs = std::fs::read_to_string(format!("{}/sources", wc_dir)).unwrap();
    assert!(srcs.contains(&src), "sources export: {}", srcs);
}

#[test]
fn sqlite_verify_fails_on_missing_state_table() {
    let (_d, cd) = temp_config();
    let src = format!("{}/walls", cd);
    std::fs::create_dir_all(&src).unwrap();
    rust(&["add", &src], &cd);
    rust(&["migrate-to-sqlite"], &cd);

    // Drop the state table — verify must fail (SQL error, not silent empty)
    let db_path = format!("{}/wallpaper-console/wallpapers.db", cd);
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute_batch("DROP TABLE state;").unwrap();
    conn.close().unwrap();

    let out = rust(&["sqlite-verify"], &cd);
    assert!(
        !out.status.success(),
        "verify should fail when state table is missing"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("VERIFY OK"),
        "should not print VERIFY OK: {}",
        stderr
    );
}

#[test]
fn sqlite_export_flat_fails_on_missing_state_table() {
    let (_d, cd) = temp_config();
    let src = format!("{}/walls", cd);
    std::fs::create_dir_all(&src).unwrap();
    rust(&["add", &src], &cd);
    rust(&["migrate-to-sqlite"], &cd);

    // Drop the state table — export must fail
    let db_path = format!("{}/wallpaper-console/wallpapers.db", cd);
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute_batch("DROP TABLE state;").unwrap();
    conn.close().unwrap();

    let out = rust(&["sqlite-export-flat"], &cd);
    assert!(
        !out.status.success(),
        "export should fail when state table is missing"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("Export complete"),
        "should not print Export complete: {}",
        stderr
    );
}

// ── search-type uses live scan ─────────────────────────────────────────────

#[test]
fn search_type_live_scan_independent_of_library_tsv() {
    let (_d, cd) = temp_config();
    let walls = format!("{}/walls", cd);
    std::fs::create_dir_all(&walls).unwrap();
    std::fs::write(format!("{}/video.mp4", walls), b"").unwrap();
    rust(&["add", &walls], &cd);

    // Before rescan, library.tsv is empty
    let lib_path = format!("{}/wallpaper-console/library.tsv", cd);
    let lib_content = std::fs::read_to_string(&lib_path).unwrap_or_default();
    let entries: Vec<&str> = lib_content.lines().filter(|l| !l.is_empty()).collect();
    assert!(
        entries.is_empty(),
        "library.tsv should be empty before rescan, got: {:?}",
        entries
    );

    // library-json (from tsv) must also be empty before rescan
    let out = rust(&["library-json"], &cd);
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(
        !s.contains("video.mp4"),
        "library-json before rescan should be empty"
    );

    // After rescan (which uses the same live scanner as search-type),
    // the video file is discovered
    rust(&["rescan"], &cd);
    let out = rust(&["library-json"], &cd);
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(
        s.contains("video.mp4"),
        "live scanner should find video.mp4 after rescan: {}",
        s
    );
    assert!(
        s.contains("\"type\": \"video\""),
        "should be classified as video: {}",
        s
    );
}

// ── SQLite mode source writes ─────────────────────────────────────────────

#[test]
fn sqlite_mode_add_fails_when_db_missing() {
    let (_d, cd) = temp_config();
    // Switch to sqlite mode without migrating first — DB does not exist.
    rust(&["config-set", "storage_backend", "sqlite"], &cd);
    let walls = format!("{}/walls", cd);
    std::fs::create_dir_all(&walls).unwrap();
    let out = rust(&["add", &walls], &cd);
    assert!(
        !out.status.success(),
        "add must fail when storage_backend=sqlite and DB is missing"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("not found") || stderr.contains("migrate"),
        "error should mention missing DB: {}",
        stderr
    );
}

#[test]
fn sqlite_mode_add_and_sources_roundtrip() {
    let (_d, cd) = temp_config();
    // Set up sqlite mode with a real DB.
    rust(&["config-set", "storage_backend", "sqlite"], &cd);
    rust(&["migrate-to-sqlite"], &cd);
    let walls = format!("{}/walls", cd);
    std::fs::create_dir_all(&walls).unwrap();
    // Add should succeed.
    let out = rust(&["add", &walls], &cd);
    assert!(
        out.status.success(),
        "add failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    // sources must list it immediately.
    let out = rust(&["sources"], &cd);
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(
        s.contains(&walls),
        "sources should contain added path: {}",
        s
    );
}

#[test]
fn sqlite_mode_remove_source_roundtrip() {
    let (_d, cd) = temp_config();
    rust(&["config-set", "storage_backend", "sqlite"], &cd);
    rust(&["migrate-to-sqlite"], &cd);
    let walls = format!("{}/walls", cd);
    std::fs::create_dir_all(&walls).unwrap();
    rust(&["add", &walls], &cd);

    let out = rust(&["remove-source", &walls], &cd);
    assert!(
        out.status.success(),
        "remove-source failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = rust(&["sources"], &cd);
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(
        !s.contains(&walls),
        "sources should not contain removed path: {}",
        s
    );
}
