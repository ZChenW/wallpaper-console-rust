//! Chromium-based backend for Wallpaper Engine web projects.
//!
//! Launches a Chromium app window pointing at the project's file (usually
//! index.html) with a dedicated user-data-dir.  Does not auto-configure the
//! compositor — the user must manage window rules (e.g. niri) separately.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use wc_core::config::ConfigDir;
use wc_core::error::WcError;
use wc_storage::StorageApi;

const PID_CONFIG_KEY: &str = "web_wallpaper_pid";

/// Config read once from StorageApi at apply time.
#[derive(Debug)]
pub struct WebWallpaperConfig {
    pub enabled: bool,
    pub browser_path: String,
    pub audio: bool,
    pub extra_args: Vec<String>,
    pub window_width: u32,
    pub window_height: u32,
}

impl WebWallpaperConfig {
    pub fn from_storage(s: &StorageApi) -> Self {
        let extra_args: Vec<String> = s
            .config_get("web_wallpaper_extra_args", "")
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();
        WebWallpaperConfig {
            enabled: s.config_get("web_wallpaper_enabled", "on") == "on",
            browser_path: s.config_get("web_wallpaper_browser", "auto"),
            audio: s.config_get("web_wallpaper_audio", "on") == "on",
            extra_args,
            window_width: s
                .config_get("web_wallpaper_window_width", "1920")
                .parse()
                .unwrap_or(1920),
            window_height: s
                .config_get("web_wallpaper_window_height", "1080")
                .parse()
                .unwrap_or(1080),
        }
    }
}

/// Resolved browser binary ready for execution.
#[derive(Debug, Clone)]
pub struct ResolvedBrowser {
    pub path: String,
}

const AUTO_BROWSERS: &[&str] = &[
    "chromium",
    "google-chrome-stable",
    "google-chrome",
    "brave",
    "brave-browser",
    "vivaldi",
];

fn resolve_browser(config: &WebWallpaperConfig) -> Result<ResolvedBrowser, WcError> {
    if config.browser_path != "auto" && !config.browser_path.is_empty() {
        let p = Path::new(&config.browser_path);
        if !p.exists() {
            return Err(WcError::Other(format!(
                "configured web browser not found: {}",
                config.browser_path
            )));
        }
        if !p.is_file() {
            return Err(WcError::Other(format!(
                "configured web browser path is not a file: {}",
                config.browser_path
            )));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = p.metadata() {
                if meta.permissions().mode() & 0o111 == 0 {
                    return Err(WcError::Other(format!(
                        "configured web browser is not executable: {}",
                        config.browser_path
                    )));
                }
            }
        }
        return Ok(ResolvedBrowser {
            path: config.browser_path.clone(),
        });
    }
    for name in AUTO_BROWSERS {
        if let Ok(full) = which::which(name) {
            return Ok(ResolvedBrowser {
                path: full.to_string_lossy().to_string(),
            });
        }
    }
    Err(WcError::Other(
        "no supported web browser found (auto-detected chromium, google-chrome, brave, or vivaldi). \
         Set web_wallpaper_browser in Settings to a custom path."
            .into(),
    ))
}

/// Information validated from a WE web project directory.
#[derive(Debug, Clone)]
pub struct WebProject {
    pub project_path: PathBuf,
    pub file: String, // relative path from project root, usually "index.html"
}

/// Read and validate a WE web project.  Rejects path traversal and missing files.
pub fn project_from_path(project_dir: &str) -> Result<WebProject, WcError> {
    let root = Path::new(project_dir);
    if !root.is_dir() {
        return Err(WcError::Other(
            "Wallpaper Engine Web wallpaper must point to a project folder.".into(),
        ));
    }
    let proj_json = root.join("project.json");
    if !proj_json.is_file() {
        return Err(WcError::Other(
            "project.json is missing from this Wallpaper Engine Web project.".into(),
        ));
    }
    let content = std::fs::read_to_string(&proj_json).map_err(WcError::Io)?;
    let proj: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| WcError::Other(e.to_string()))?;

    let proj_type = proj
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_lowercase();
    if proj_type != "web" {
        return Err(WcError::Other(format!(
            "project.json type is '{}', not 'web'. This project cannot be launched as a Web wallpaper.",
            proj_type
        )));
    }

    let file = proj
        .get("file")
        .and_then(|v| v.as_str())
        .unwrap_or("index.html")
        .to_string();

    // Reject path traversal BEFORE normalization.
    for comp in Path::new(file.trim()).components() {
        match comp {
            std::path::Component::ParentDir => {
                return Err(WcError::Other(format!(
                    "project.json file '{}' contains path traversal (..) and is rejected for security.",
                    file
                )));
            }
            std::path::Component::RootDir | std::path::Component::Prefix(_) => {
                return Err(WcError::Other(format!(
                    "project.json file '{}' is an absolute path and is rejected for security.",
                    file
                )));
            }
            _ => {}
        }
    }

    let normalized = normalize_relative(&file);
    let target = root.join(&normalized);
    if !target.is_file() {
        return Err(WcError::Other(format!(
            "Wallpaper Engine Web project file not found: {} (project.json file: '{}')",
            target.display(),
            file,
        )));
    }

    // Reject symlink escapes: the canonical target must be inside the canonical root.
    let root_canon = root.canonicalize().map_err(WcError::Io)?;
    let target_canon = target.canonicalize().map_err(WcError::Io)?;
    if !target_canon.starts_with(&root_canon) {
        return Err(WcError::Other(format!(
            "project.json file '{}' resolves outside the project root and is rejected for security.",
            file
        )));
    }

    Ok(WebProject {
        project_path: root.to_path_buf(),
        file: normalized,
    })
}

fn normalize_relative(raw: &str) -> String {
    let p = Path::new(raw.trim());
    if p.is_absolute() {
        return raw.to_string();
    }
    let mut out = String::new();
    for comp in p.components() {
        if let std::path::Component::Normal(s) = comp {
            if !out.is_empty() {
                out.push('/');
            }
            out.push_str(&s.to_string_lossy());
        }
    }
    if out.is_empty() {
        "index.html".to_string()
    } else {
        out
    }
}

pub struct Status {
    pub available: bool,
    pub path: Option<String>,
    pub message: String,
    pub detail: Option<String>,
}

pub fn status(config: &WebWallpaperConfig) -> Status {
    if !config.enabled {
        return Status {
            available: false,
            path: None,
            message: "Web wallpaper backend is disabled.".into(),
            detail: Some("Enable it in Settings → Web Wallpaper Backend.".into()),
        };
    }
    match resolve_browser(config) {
        Ok(b) => Status {
            available: true,
            path: Some(b.path),
            message: "Chromium web backend ready.".into(),
            detail: Some("Applies WE Web wallpapers in an isolated Chromium app window.".into()),
        },
        Err(e) => Status {
            available: false,
            path: None,
            message: "Web wallpaper browser not found.".into(),
            detail: Some(format!("{}", e)),
        },
    }
}

/// Result of preflight validation — project, config, and browser all verified
/// but no processes have been stopped or launched yet.
#[derive(Debug)]
pub struct PreflightResult {
    pub browser: ResolvedBrowser,
    pub file_url: String,
    pub config: WebWallpaperConfig,
}

/// Validate everything before stopping the current backend.
pub fn preflight(project_dir: &str, s: &StorageApi) -> Result<PreflightResult, WcError> {
    let project = project_from_path(project_dir)?;
    let config = WebWallpaperConfig::from_storage(s);
    if !config.enabled {
        return Err(WcError::Other(
            "Web wallpaper backend is disabled. Enable it in Settings → Web Wallpaper Backend."
                .into(),
        ));
    }
    let browser = resolve_browser(&config)?;

    let raw_file_path = format!("{}/{}", project.project_path.display(), project.file);
    let file_url = percent_encode_path(&raw_file_path);

    Ok(PreflightResult {
        browser,
        file_url,
        config,
    })
}

/// Find the PID of a browser process still using the given profile dir.
/// Returns None if no matching process is found.
fn find_browser_handoff_pid(profile_dir: &std::path::Path) -> Option<u32> {
    let pattern = format!("--user-data-dir={}", profile_dir.display());
    match std::process::Command::new("pgrep")
        .args(["-f", "--", &pattern])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
    {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let our_pid = std::process::id();
            stdout.lines().find_map(|line| {
                let pid: u32 = line.trim().parse().ok()?;
                if pid != our_pid {
                    Some(pid)
                } else {
                    None
                }
            })
        }
        _ => None,
    }
}

/// Launch Chromium using a previously validated preflight result.
/// Does NOT write state — the caller handles that.
pub fn apply_preflighted(s: &StorageApi, p: &PreflightResult) -> Result<(), WcError> {
    let profile_dir = s.cd.path.join("web-wallpaper-profile");
    let _ = std::fs::create_dir_all(&profile_dir);

    let mut cmd = Command::new("setsid");
    cmd.arg(&p.browser.path)
        .arg(format!("--app={}", p.file_url))
        .arg(format!("--user-data-dir={}", profile_dir.display()))
        .arg("--no-first-run")
        .arg("--disable-session-crashed-bubble")
        .arg("--autoplay-policy=no-user-gesture-required")
        .arg("--allow-file-access-from-files")
        .arg(format!(
            "--window-size={},{}",
            p.config.window_width, p.config.window_height
        ));

    if !p.config.audio {
        cmd.arg("--mute-audio");
    }

    for arg in &p.config.extra_args {
        if !arg.is_empty() {
            cmd.arg(arg);
        }
    }

    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    // Class for compositor window-rule matching (maps to app-id on Wayland).
    cmd.arg("--class=web-wallpaper-console");

    let mut child = cmd
        .spawn()
        .map_err(|e| WcError::Other(format!("failed to start web browser: {}", e)))?;

    let pid = child.id();
    let _ = s.config_set(PID_CONFIG_KEY, &pid.to_string());

    std::thread::sleep(Duration::from_millis(300));
    if let Ok(Some(status)) = child.try_wait() {
        if status.success() {
            if let Some(actual_pid) = find_browser_handoff_pid(&profile_dir) {
                let _ = s.config_set(PID_CONFIG_KEY, &actual_pid.to_string());
                return Ok(());
            }
        }
        let _ = s.config_set(PID_CONFIG_KEY, "");
        return Err(WcError::Other(format!(
            "Web wallpaper browser exited with status {}. \
             Check that the browser can access the project file and the display server is available.",
            status
        )));
    }

    Ok(())
}

/// Legacy convenience — uses preflight + apply_preflighted. Call sites
/// that need to stop the current backend between validation and launch
/// should call preflight and apply_preflighted directly.
pub fn apply(s: &StorageApi, project: &WebProject) -> Result<(), WcError> {
    let p = preflight(&project.project_path.to_string_lossy(), s)?;
    apply_preflighted(s, &p)
}

fn percent_encode_path(raw: &str) -> String {
    let encoded: String = raw
        .chars()
        .map(|c| match c {
            ' ' => "%20".to_string(),
            '#' => "%23".to_string(),
            '?' => "%3F".to_string(),
            '%' => "%25".to_string(),
            _ if c.is_ascii_graphic() || c == '/' => c.to_string(),
            _ => {
                let mut buf = [0u8; 4];
                let s = c.encode_utf8(&mut buf);
                s.bytes().map(|b| format!("%{:02X}", b)).collect::<String>()
            }
        })
        .collect();
    format!("file://{}", encoded)
}

/// Stop the Web wallpaper backend by killing the recorded PID and its process group,
/// then cleaning up any profile-matching processes.
pub fn stop(s: Option<&StorageApi>) {
    if let Some(storage) = s {
        let pid = storage.config_get(PID_CONFIG_KEY, "");
        if !pid.is_empty() && pid.chars().all(|c| c.is_ascii_digit()) {
            // Kill the process group (negative PID sends to the whole group).
            let _ = Command::new("kill")
                .args(["-TERM", &format!("-{}", pid)])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            std::thread::sleep(Duration::from_millis(80));
            let _ = Command::new("kill")
                .args(["-KILL", &format!("-{}", pid)])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            let _ = storage.config_set(PID_CONFIG_KEY, "");
        }
    }
    // Targeted cleanup: kill any remaining processes with our profile flag.
    if let Ok(cd) = ConfigDir::new() {
        let profile_flag = format!(
            "--user-data-dir={}",
            cd.path.join("web-wallpaper-profile").display()
        );
        if let Ok(out) = Command::new("pgrep")
            .args(["-f", &profile_flag])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
        {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let pids: Vec<&str> = stdout
                .lines()
                .map(|l| l.trim())
                .filter(|l| !l.is_empty())
                .collect();
            for pid in &pids {
                let _ = Command::new("kill")
                    .args(["-TERM", pid])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
            if !pids.is_empty() {
                std::thread::sleep(Duration::from_millis(80));
                for pid in &pids {
                    let _ = Command::new("kill")
                        .args(["-KILL", pid])
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .status();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn temp_project(json: &str) -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let proj = tmp.path().join("web_proj");
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(proj.join("project.json"), json).unwrap();
        (tmp, proj)
    }

    #[test]
    fn project_from_path_uses_index_html() {
        let (_tmp, proj) = temp_project(r#"{"type":"web","file":"index.html"}"#);
        std::fs::write(proj.join("index.html"), b"<html></html>").unwrap();
        let wp = project_from_path(&proj.to_string_lossy()).unwrap();
        assert_eq!(wp.file, "index.html");
    }

    #[test]
    fn project_from_path_defaults_to_index_html() {
        let (_tmp, proj) = temp_project(r#"{"type":"web"}"#);
        std::fs::write(proj.join("index.html"), b"<html></html>").unwrap();
        let wp = project_from_path(&proj.to_string_lossy()).unwrap();
        assert_eq!(wp.file, "index.html");
    }

    #[test]
    fn project_from_path_type_web_case_insensitive() {
        let (_tmp, proj) = temp_project(r#"{"type":"Web","file":"main.html"}"#);
        std::fs::write(proj.join("main.html"), b"<html></html>").unwrap();
        let wp = project_from_path(&proj.to_string_lossy()).unwrap();
        assert_eq!(wp.file, "main.html");
    }

    #[test]
    fn project_from_path_rejects_missing_file() {
        let (_tmp, proj) = temp_project(r#"{"type":"web","file":"missing.html"}"#);
        assert!(project_from_path(&proj.to_string_lossy()).is_err());
    }

    #[test]
    fn project_from_path_rejects_path_traversal() {
        let (_tmp, proj) = temp_project(r#"{"type":"web","file":"../etc/passwd"}"#);
        let err = project_from_path(&proj.to_string_lossy()).unwrap_err();
        assert!(
            err.to_string().contains("path traversal"),
            "should reject .. before file check, got: {}",
            err
        );
    }

    #[test]
    fn project_from_path_rejects_absolute_path() {
        let (_tmp, proj) = temp_project(r#"{"type":"web","file":"/etc/passwd"}"#);
        let err = project_from_path(&proj.to_string_lossy()).unwrap_err();
        assert!(
            err.to_string().contains("absolute path"),
            "should reject absolute path, got: {}",
            err
        );
    }

    #[test]
    fn project_from_path_rejects_not_web_type() {
        let (_tmp, proj) = temp_project(r#"{"type":"scene","file":"index.html"}"#);
        std::fs::write(proj.join("index.html"), b"<html></html>").ok();
        assert!(project_from_path(&proj.to_string_lossy()).is_err());
    }

    #[test]
    fn normalize_relative_strips_traversal() {
        assert_eq!(normalize_relative("subdir/index.html"), "subdir/index.html");
        assert_eq!(normalize_relative("index.html"), "index.html");
        assert_eq!(normalize_relative(""), "index.html");
    }

    #[test]
    fn resolve_browser_reports_missing_custom_path() {
        let config = WebWallpaperConfig {
            enabled: true,
            browser_path: "/nonexistent/browser".into(),
            audio: true,
            extra_args: vec![],
            window_width: 1920,
            window_height: 1080,
        };
        let err = resolve_browser(&config).unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn resolve_browser_rejects_directory_as_browser() {
        let tmp = tempfile::tempdir().unwrap();
        let config = WebWallpaperConfig {
            enabled: true,
            browser_path: tmp.path().to_string_lossy().to_string(),
            audio: true,
            extra_args: vec![],
            window_width: 1920,
            window_height: 1080,
        };
        let err = resolve_browser(&config).unwrap_err();
        assert!(err.to_string().contains("not a file"));
    }

    #[test]
    fn preflight_rejects_disabled_backend() {
        let (_tmp, proj) = temp_project(r#"{"type":"web","file":"index.html"}"#);
        std::fs::write(proj.join("index.html"), b"<html></html>").unwrap();

        let cd = wc_core::config::ConfigDir {
            path: proj.parent().unwrap().parent().unwrap().to_path_buf(),
        };
        let s = wc_storage::StorageApi::new(cd);
        s.config_set("web_wallpaper_enabled", "off").unwrap();

        let err = preflight(&proj.to_string_lossy(), &s).unwrap_err();
        assert!(err.to_string().contains("disabled"));
    }

    #[test]
    fn canonical_root_rejects_symlink_escape() {
        use std::os::unix::fs as unix_fs;
        let tmp = tempfile::tempdir().unwrap();
        let proj = tmp.path().join("web_proj");
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(
            proj.join("project.json"),
            r#"{"type":"web","file":"link.html"}"#,
        )
        .unwrap();

        let outside = tmp.path().join("outside.html");
        std::fs::write(&outside, b"<html>escaped</html>").unwrap();

        unix_fs::symlink(&outside, proj.join("link.html")).unwrap();

        let err = project_from_path(&proj.to_string_lossy()).unwrap_err();
        assert!(
            err.to_string()
                .contains("resolves outside the project root"),
            "should reject symlink escape, got: {}",
            err
        );
    }

    #[test]
    fn handoff_detects_running_pid() {
        let dir = tempfile::tempdir().unwrap();
        let profile = dir.path().join("web-wallpaper-profile");
        std::fs::create_dir_all(&profile).unwrap();

        let mut child = std::process::Command::new("python3")
            .arg("-c")
            .arg("import time; time.sleep(5)")
            .arg(format!("--user-data-dir={}", profile.display()))
            .spawn()
            .unwrap();
        let expected_pid = child.id();

        let found = find_browser_handoff_pid(&profile);
        assert!(
            found.is_some(),
            "should find a process with the profile pattern"
        );
        assert_eq!(found, Some(expected_pid));

        child.kill().ok();
        child.wait().ok();
    }

    #[test]
    fn handoff_returns_none_when_no_process() {
        let dir = tempfile::tempdir().unwrap();
        let profile = dir.path().join("nonexistent-profile-56789");
        std::fs::create_dir_all(&profile).unwrap();

        assert_eq!(find_browser_handoff_pid(&profile), None);
    }

    fn make_test_preflight(mock_browser_path: &str, file_url: &str) -> PreflightResult {
        PreflightResult {
            browser: ResolvedBrowser {
                path: mock_browser_path.to_string(),
            },
            file_url: file_url.to_string(),
            config: WebWallpaperConfig {
                enabled: true,
                browser_path: mock_browser_path.to_string(),
                audio: false,
                extra_args: vec![],
                window_width: 1920,
                window_height: 1080,
            },
        }
    }

    fn make_test_storage(dir: &std::path::Path) -> StorageApi {
        let cd = ConfigDir {
            path: dir.to_path_buf(),
        };
        cd.init().unwrap();
        wc_core::config::write_config_value(&cd.path, "storage_backend", "sqlite").unwrap();
        StorageApi::new(cd)
    }

    #[test]
    fn apply_exit_zero_with_handoff_records_actual_pid() {
        let dir = tempfile::tempdir().unwrap();
        let cfg_path = dir.path().join("wc-cfg");
        let profile_dir = cfg_path.join("web-wallpaper-profile");

        let mock_browser = dir.path().join("mock-browser.sh");
        std::fs::write(
            &mock_browser,
            format!(
                "#!/bin/sh\npython3 -c \"import time; time.sleep(5)\" --user-data-dir={} &\nexit 0\n",
                profile_dir.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&mock_browser, std::fs::Permissions::from_mode(0o755)).unwrap();

        let project_dir = dir.path().join("webproj");
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::write(project_dir.join("index.html"), b"<html></html>").unwrap();

        let s = make_test_storage(&cfg_path);
        let preflight = make_test_preflight(
            &mock_browser.to_string_lossy(),
            &format!("file://{}", project_dir.join("index.html").display()),
        );

        let result = apply_preflighted(&s, &preflight);
        assert!(
            result.is_ok(),
            "exit 0 with handoff should succeed, got: {:?}",
            result
        );

        let stored_pid: u32 = s.config_get("web_wallpaper_pid", "0").parse().unwrap_or(0);
        assert!(
            stored_pid > 0,
            "should have stored a valid PID, got: {}",
            stored_pid
        );

        // Cleanup background process.
        std::process::Command::new("kill")
            .args(["-TERM", &stored_pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .ok();
    }

    #[test]
    fn apply_exit_zero_no_handoff_is_error() {
        let dir = tempfile::tempdir().unwrap();

        let mock_browser = dir.path().join("mock-exit-0.sh");
        std::fs::write(&mock_browser, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&mock_browser, std::fs::Permissions::from_mode(0o755)).unwrap();

        let project_dir = dir.path().join("webproj2");
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::write(project_dir.join("index.html"), b"<html></html>").unwrap();

        let s = make_test_storage(&dir.path().join("wc-cfg-2"));
        let preflight = make_test_preflight(
            &mock_browser.to_string_lossy(),
            &format!("file://{}", project_dir.join("index.html").display()),
        );

        let result = apply_preflighted(&s, &preflight);
        assert!(result.is_err(), "exit 0 without handoff should be an error");
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("exited with status"),
            "error should mention exit status"
        );
    }

    #[test]
    fn apply_exit_nonzero_is_error() {
        let dir = tempfile::tempdir().unwrap();

        let mock_browser = dir.path().join("mock-exit-1.sh");
        std::fs::write(&mock_browser, "#!/bin/sh\nexit 1\n").unwrap();
        std::fs::set_permissions(&mock_browser, std::fs::Permissions::from_mode(0o755)).unwrap();

        let project_dir = dir.path().join("webproj3");
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::write(project_dir.join("index.html"), b"<html></html>").unwrap();

        let s = make_test_storage(&dir.path().join("wc-cfg-3"));
        let preflight = make_test_preflight(
            &mock_browser.to_string_lossy(),
            &format!("file://{}", project_dir.join("index.html").display()),
        );

        let result = apply_preflighted(&s, &preflight);
        assert!(result.is_err(), "exit non-zero should be an error");
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("exited with status"),
            "error should mention exit status"
        );
    }
}
