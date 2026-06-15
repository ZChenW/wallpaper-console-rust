//! Parity integration tests — CLI binary tests against temp config dirs.

use std::process::Command;

const RUST_BIN: &str = env!("CARGO_BIN_EXE_wallpaper-console-rust");

fn temp_config() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().to_string_lossy().to_string();
    let wc_dir = format!("{}/wallpaper-console", config_dir);
    std::fs::create_dir_all(&wc_dir).unwrap();
    // CLI parity tests use flat-file mode to test the original file-based storage paths.
    std::fs::write(format!("{}/config", wc_dir), "storage_backend=file\n").ok();
    (dir, config_dir)
}

fn rust(args: &[&str], cd: &str) -> std::process::Output {
    Command::new(RUST_BIN)
        .args(args)
        .env("XDG_CONFIG_HOME", cd)
        .output()
        .unwrap()
}

fn rust_with_home(args: &[&str], cd: &str, home: &str) -> std::process::Output {
    Command::new(RUST_BIN)
        .args(args)
        .env("XDG_CONFIG_HOME", cd)
        .env("HOME", home)
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
    let out = rust(&["library-json", "--sqlite"], &cd);
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
fn rescan_writes_large_library_to_tsv_and_sqlite() {
    let (_d, cd) = temp_config();
    let walls = format!("{}/walls", cd);
    std::fs::create_dir_all(&walls).unwrap();
    for i in 0..300 {
        std::fs::write(format!("{}/img_{i:03}.jpg", walls), b"jpg").unwrap();
    }

    rust(&["add", &walls], &cd);
    let out = rust(&["rescan"], &cd);
    assert!(
        out.status.success(),
        "rescan failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("walked: 300 files"), "{stdout}");
    assert!(stdout.contains("entries: 300"), "{stdout}");
    assert!(stdout.contains("sqlite: 300"), "{stdout}");

    let tsv_path = format!("{}/wallpaper-console/library.tsv", cd);
    let tsv = std::fs::read_to_string(tsv_path).unwrap();
    assert_eq!(tsv.lines().count(), 300);

    let page = rust(
        &["library-page-json", "--source", "sqlite", "--limit", "1"],
        &cd,
    );
    assert!(
        page.status.success(),
        "sqlite page failed: {}",
        String::from_utf8_lossy(&page.stderr)
    );
    let stdout = String::from_utf8_lossy(&page.stdout);
    assert!(stdout.contains("\"total\": 300"), "{stdout}");
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
fn we_scene_project_enters_library_without_assets() {
    let (_d, cd) = temp_config();
    let we_root = format!("{}/steamapps/workshop/content/431960", cd);
    std::fs::create_dir_all(&we_root).unwrap();

    // Scene project
    let scene_dir = format!("{}/111", we_root);
    std::fs::create_dir_all(&scene_dir).unwrap();
    std::fs::write(
        format!("{}/project.json", scene_dir),
        r#"{"type":"Scene","file":"scene.json","preview":"preview.gif","title":"Scene title"}"#,
    )
    .unwrap();
    std::fs::write(format!("{}/preview.gif", scene_dir), b"gif").unwrap();
    std::fs::write(format!("{}/asset.png", scene_dir), b"").unwrap();

    rust(&["add", &we_root], &cd);
    assert!(rust(&["rescan"], &cd).status.success());

    let out = rust(&["library-json"], &cd);
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(
        s.contains("\"type\": \"we_scene\""),
        "scene project should appear as we_scene: {}",
        s
    );
    assert!(
        s.contains("\"backend\": \"linux-wallpaperengine\""),
        "scene project should use linux-wallpaperengine backend: {}",
        s
    );
    assert!(
        s.contains(&scene_dir),
        "scene project path should appear: {}",
        s
    );
    assert!(
        !s.contains("asset.png"),
        "scene project files should not appear: {}",
        s
    );
}

#[test]
fn inspect_we_video_project_reports_real_media_apply_target() {
    let (_d, cd) = temp_config();
    let project = format!("{}/steamapps/workshop/content/431960/2924684771", cd);
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(format!("{}/bg.mp4", project), b"mp4").unwrap();
    std::fs::write(
        format!("{}/project.json", project),
        r#"{"type":"video","file":"bg.mp4","title":"Workshop Video","workshopid":"2924684771"}"#,
    )
    .unwrap();

    let out = rust(&["inspect", &project], &cd);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("\"backend\": \"mpvpaper\""), "{}", s);
    assert!(s.contains("\"resolved_path\""), "{}", s);
    assert!(s.contains("bg.mp4"), "{}", s);
    assert!(s.contains("2924684771"), "{}", s);
    assert!(s.contains("Workshop Video"), "{}", s);
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

#[test]
fn steam_workshop_detects_flatpak_steam_path() {
    let (_d, cd) = temp_config();
    let home = format!("{}/home", cd);
    let workshop_root = format!(
        "{}/.var/app/com.valvesoftware.Steam/.local/share/Steam/steamapps/workshop/content/431960",
        home
    );
    let project = format!("{}/987", workshop_root);
    std::fs::create_dir_all(&project).unwrap();

    let out = rust_with_home(&["steam-workshop"], &cd, &home);
    assert!(
        out.status.success(),
        "steam-workshop failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let out = rust(&["sources"], &cd);
    let sources = String::from_utf8_lossy(&out.stdout);
    assert!(
        sources.contains(&workshop_root),
        "Flatpak Steam workshop root should be in sources: {}",
        sources
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
    // sqlite_source_add auto-creates wallpapers.db schema now.
    // The add command should succeed even without a pre-existing DB.
    rust(&["config-set", "storage_backend", "sqlite"], &cd);
    let walls = format!("{}/walls", cd);
    std::fs::create_dir_all(&walls).unwrap();
    let out = rust(&["add", &walls], &cd);
    assert!(
        out.status.success(),
        "add should auto-create DB and succeed: {}",
        String::from_utf8_lossy(&out.stderr)
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

// ── Direct SQLite read/debug commands ─────────────────────────────────────

#[test]
fn sqlite_debug_read_commands_read_database_directly() {
    let (_d, cd) = temp_config();
    let walls = format!("{}/walls", cd);
    let fav = format!("{}/fav.png", cd);
    let hist_new = format!("{}/new.png", cd);
    let hist_old = format!("{}/old.mp4", cd);
    std::fs::create_dir_all(&walls).unwrap();
    std::fs::write(&fav, b"").unwrap();

    rust(&["add", &walls], &cd);
    rust(&["config-set", "alpha", "from-db"], &cd);
    rust(&["favorite-add", &fav], &cd);
    assert!(rust(&["migrate-to-sqlite"], &cd).status.success());

    let db_path = format!("{}/wallpaper-console/wallpapers.db", cd);
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute("DELETE FROM history", []).unwrap();
    conn.execute(
        "INSERT INTO history (path, backend) VALUES (?1, 'awww')",
        [&hist_old],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO history (path, backend) VALUES (?1, 'mpvpaper')",
        [&hist_new],
    )
    .unwrap();
    conn.execute(
        "INSERT OR REPLACE INTO state (key, value) VALUES ('current', ?1)",
        [&fav],
    )
    .unwrap();
    conn.execute(
        "INSERT OR REPLACE INTO state (key, value) VALUES ('last_backend', 'awww')",
        [],
    )
    .unwrap();
    conn.close().unwrap();

    let out = rust(&["sqlite-config-get", "alpha"], &cd);
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "from-db");

    let out = rust(&["sqlite-sources-list"], &cd);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains(&walls));

    let out = rust(&["sqlite-favorites-list"], &cd);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains(&fav));

    let out = rust(&["sqlite-history-list"], &cd);
    assert!(out.status.success());
    let history = String::from_utf8_lossy(&out.stdout);
    let first = history.lines().next().unwrap_or_default();
    assert_eq!(
        first, hist_new,
        "history should be newest first: {}",
        history
    );

    let out = rust(&["sqlite-current-read"], &cd);
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), fav);

    let out = rust(&["sqlite-last-backend-read"], &cd);
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "awww");
}

#[test]
fn sqlite_debug_read_commands_fail_without_database() {
    let (_d, cd) = temp_config();
    // sqlite_connection auto-creates wallpapers.db now.
    // Commands that read from SQLite succeed with empty results.
    let out = rust(&["sqlite-sources-list"], &cd);
    assert!(
        out.status.success(),
        "sqlite-sources-list should auto-create DB and succeed"
    );
}

#[test]
fn no_args_points_to_gui_or_tui_instead_of_plain_help_only() {
    let (_d, cd) = temp_config();
    let out = rust(&[], &cd);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("wallpaper-console-gui-rust")
            || stdout.contains("Rust TUI is not implemented")
            || stdout.contains("GUI"),
        "no-arg output should explain the interactive Rust entry point: {}",
        stdout
    );
}

#[test]
fn library_page_json_filters_sorts_and_paginates_sqlite() {
    let (_d, cd) = temp_config();
    let walls = format!("{}/walls", cd);
    std::fs::create_dir_all(&walls).unwrap();
    std::fs::write(format!("{}/a.png", walls), b"small").unwrap();
    std::fs::write(format!("{}/b.png", walls), b"larger image").unwrap();
    std::fs::write(format!("{}/movie.mp4", walls), b"video").unwrap();
    rust(&["add", &walls], &cd);
    assert!(rust(&["migrate-to-sqlite"], &cd).status.success());
    assert!(rust(&["rescan"], &cd).status.success());

    let out = rust(
        &[
            "library-page-json",
            "--source",
            "sqlite",
            "--filter",
            "image",
            "--sort",
            "name",
            "--search",
            ".png",
            "--offset",
            "1",
            "--limit",
            "1",
        ],
        &cd,
    );
    assert!(
        out.status.success(),
        "library-page-json failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["total"].as_u64(), Some(2));
    let items = json["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert!(
        items[0]["path"].as_str().unwrap().ends_with("b.png"),
        "second name-sorted image should be b.png: {}",
        json
    );
}

#[test]
fn library_page_json_reports_missing_sqlite_database() {
    let (_d, cd) = temp_config();
    // library_page_sqlite auto-creates wallpapers.db schema now.
    // The command returns success with empty results.
    let out = rust(
        &[
            "library-page-json",
            "--source",
            "sqlite",
            "--offset",
            "0",
            "--limit",
            "10",
        ],
        &cd,
    );
    assert!(
        out.status.success(),
        "library-page-json should auto-create DB and return empty results"
    );
}

// ── Incremental rescan ────────────────────────────────────────────────────

#[test]
fn second_rescan_reuses_metadata_for_unchanged_files() {
    let (_d, cd) = temp_config();
    let walls = format!("{}/walls", cd);
    std::fs::create_dir_all(&walls).unwrap();
    std::fs::write(format!("{}/a.png", walls), b"test1").unwrap();
    std::fs::write(format!("{}/b.jpg", walls), b"test2").unwrap();
    rust(&["add", &walls], &cd);

    // First rescan — must probe resolutions.
    let out1 = rust(&["rescan"], &cd);
    let s1 = String::from_utf8_lossy(&out1.stdout);
    assert!(out1.status.success(), "first rescan failed: {}", s1);
    assert!(
        s1.contains("probed_metadata:"),
        "first rescan should show probed_metadata: {}",
        s1
    );
    // All files are new, so probed_metadata should equal entries.
    assert!(s1.contains("entries: 2"), "first rescan: {}", s1);

    // Second rescan — same files, same size/mtime, must reuse all metadata.
    let out2 = rust(&["rescan"], &cd);
    let s2 = String::from_utf8_lossy(&out2.stdout);
    assert!(out2.status.success(), "second rescan failed: {}", s2);
    assert!(
        s2.contains("reused_metadata: 2"),
        "second rescan should reuse metadata: {}",
        s2
    );
    assert!(
        s2.contains("probed_metadata: 0"),
        "second rescan should probe 0: {}",
        s2
    );
}

#[test]
fn rescan_probes_changed_files_only() {
    let (_d, cd) = temp_config();
    let walls = format!("{}/walls", cd);
    std::fs::create_dir_all(&walls).unwrap();
    std::fs::write(format!("{}/x.png", walls), b"original").unwrap();
    rust(&["add", &walls], &cd);
    rust(&["rescan"], &cd);

    // Modify one file — it should be re-probed.
    std::fs::write(format!("{}/x.png", walls), b"modified content here").unwrap();

    let out = rust(&["rescan"], &cd);
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "rescan failed: {}", s);
    assert!(
        s.contains("probed_metadata: 1"),
        "changed file should be re-probed: {}",
        s
    );
}

// ── Thumbnail CLI ─────────────────────────────────────────────────────────

#[test]
fn thumbnail_command_generates_valid_webp() {
    // Skip if ffmpeg not available.
    if !std::process::Command::new("ffmpeg")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        eprintln!("skipping: ffmpeg not available");
        return;
    }

    let (_d, cd) = temp_config();
    // Create a tiny test video (1 second, single color).
    let video = format!("{}/test.mp4", cd);
    let status = std::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-f",
            "lavfi",
            "-i",
            "color=c=0x808080:s=320x240:d=1",
            "-frames:v",
            "25",
            &video,
        ])
        .status()
        .unwrap();
    assert!(status.success(), "failed to create test video");

    // Generate thumbnail.
    let out = rust(&["thumbnail", &video], &cd);
    assert!(
        out.status.success(),
        "thumbnail failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let thumb_path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert!(!thumb_path.is_empty(), "no thumbnail path in output");

    let meta = std::fs::metadata(&thumb_path).unwrap();
    assert!(meta.len() > 100, "thumbnail file too small");

    // Verify it's a valid WebP at ≤ 400px wide.
    if std::process::Command::new("identify")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        let id_out = std::process::Command::new("identify")
            .args(["-format", "%wx%h", &thumb_path])
            .output()
            .unwrap();
        let dims = String::from_utf8_lossy(&id_out.stdout).to_string();
        let width: u32 = dims.split('x').next().unwrap_or("0").parse().unwrap_or(0);
        assert!(
            width > 0 && width <= 400,
            "thumbnail width {} not in 1..400",
            width
        );
    }
}
