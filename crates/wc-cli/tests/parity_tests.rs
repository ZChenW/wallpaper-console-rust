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
    assert!(rust(&["favorite-add", &fp], &cd).status.success());
    let out = rust(&["favorites"], &cd);
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains(&fp));
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
