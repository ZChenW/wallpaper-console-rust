//! Native Web wallpaper backend.
//!
//! This spawns `wallpaper-console-web-renderer`, a small GTK/WebKitGTK
//! layer-shell process that can become a real Wayland background layer.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use wc_core::error::WcError;
use wc_storage::StorageApi;

use crate::web_wallpaper::{project_from_path, WebProject};

const PID_CONFIG_KEY: &str = "web_renderer_pid";
const BINARY_NAME: &str = "wallpaper-console-web-renderer";

#[derive(Debug, Clone)]
pub struct WebRendererConfig {
    pub enabled: bool,
    pub path: String,
    pub audio: bool,
    pub width: u32,
    pub height: u32,
    pub output: Option<String>,
    pub debug: bool,
}

impl WebRendererConfig {
    pub fn from_storage(s: &StorageApi) -> Self {
        WebRendererConfig {
            enabled: s.config_get("web_renderer_enabled", "on") == "on",
            path: s.config_get("web_renderer_path", "auto"),
            audio: s.config_get("web_renderer_audio", "on") == "on",
            width: s
                .config_get("web_renderer_width", "1920")
                .parse()
                .unwrap_or(1920),
            height: s
                .config_get("web_renderer_height", "1080")
                .parse()
                .unwrap_or(1080),
            output: {
                let raw = s.config_get("web_renderer_output", "");
                let trimmed = raw.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            },
            debug: s.config_get("web_renderer_debug", "off") == "on",
        }
    }
}

pub struct Status {
    pub available: bool,
    pub path: Option<String>,
    pub message: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
}

pub fn status(config: &WebRendererConfig) -> Status {
    if !config.enabled {
        return Status {
            available: false,
            path: None,
            message: "Web renderer is disabled.".into(),
            detail: Some("Enable it in Settings → Web Wallpaper Renderer.".into()),
        };
    }
    match resolve_binary(config) {
        Ok(path) => Status {
            available: true,
            path: Some(path),
            message: "Native Web renderer ready.".into(),
            detail: Some(
                "Uses WebKitGTK + Wayland layer-shell for real Web wallpaper backgrounds.".into(),
            ),
        },
        Err(err) => Status {
            available: false,
            path: None,
            message: "Native Web renderer missing.".into(),
            detail: Some(err.to_string()),
        },
    }
}

pub fn is_available(s: &StorageApi) -> bool {
    let config = WebRendererConfig::from_storage(s);
    config.enabled && resolve_binary(&config).is_ok()
}

pub fn command_spec(
    config: &WebRendererConfig,
    project: &WebProject,
) -> Result<CommandSpec, WcError> {
    if !config.enabled {
        return Err(WcError::Other("native Web renderer is disabled".into()));
    }
    let program = resolve_binary(config)?;
    let mut args = vec![
        "--project".to_string(),
        project.project_path.to_string_lossy().to_string(),
        "--file".to_string(),
        project.file.clone(),
        "--width".to_string(),
        config.width.to_string(),
        "--height".to_string(),
        config.height.to_string(),
        "--audio".to_string(),
        if config.audio { "on" } else { "off" }.to_string(),
    ];
    if let Some(output) = &config.output {
        args.push("--output".into());
        args.push(output.clone());
    }
    if config.debug {
        args.push("--debug".into());
    }
    Ok(CommandSpec { program, args })
}

pub fn apply(s: &StorageApi, project_dir: &str) -> Result<(), WcError> {
    let project = project_from_path(project_dir)?;
    let config = WebRendererConfig::from_storage(s);
    let spec = command_spec(&config, &project)?;

    crate::stop_all_backends(Some(s))?;

    let log_path = s.cd.path.join("web-renderer-last.log");
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&log_path)
        .ok();
    let (stdout, stderr) = if let Some(file) = log_file {
        let err = file.try_clone().ok();
        (
            Stdio::from(file),
            err.map(Stdio::from).unwrap_or_else(Stdio::null),
        )
    } else {
        (Stdio::null(), Stdio::null())
    };

    let mut child = Command::new("setsid")
        .arg(&spec.program)
        .args(&spec.args)
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(stderr)
        .spawn()
        .map_err(|e| WcError::Other(format!("failed to launch native Web renderer: {}", e)))?;

    std::thread::sleep(Duration::from_millis(250));
    if let Some(status) = child
        .try_wait()
        .map_err(|e| WcError::Other(format!("failed to inspect native Web renderer: {}", e)))?
    {
        let detail = std::fs::read_to_string(&log_path).unwrap_or_default();
        return Err(WcError::Other(format!(
            "native Web renderer exited unexpectedly with status {}. {}",
            status,
            detail.trim()
        )));
    }

    s.config_set(PID_CONFIG_KEY, &child.id().to_string())?;
    s.current_write(project_dir)?;
    s.last_backend_write("webkit-layer-shell")?;
    s.history_add(project_dir, "webkit-layer-shell")?;
    Ok(())
}

pub fn stop(s: Option<&StorageApi>) {
    let Some(storage) = s else {
        return;
    };
    let pid = storage.config_get(PID_CONFIG_KEY, "");
    let Ok(pid) = pid.parse::<i32>() else {
        return;
    };
    let pgid = format!("-{}", pid);
    let _ = Command::new("kill")
        .args(["-TERM", &pgid])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    std::thread::sleep(Duration::from_millis(80));
    let _ = Command::new("kill")
        .args(["-KILL", &pgid])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = storage.config_set(PID_CONFIG_KEY, "");
}

fn resolve_binary(config: &WebRendererConfig) -> Result<String, WcError> {
    if config.path != "auto" && !config.path.trim().is_empty() {
        return validate_binary(&config.path);
    }
    if let Ok(current) = std::env::current_exe() {
        if let Some(parent) = current.parent() {
            let sibling = parent.join(BINARY_NAME);
            if sibling.exists() {
                return validate_binary(&sibling.to_string_lossy());
            }
        }
    }
    if let Ok(path) = which::which(BINARY_NAME) {
        return validate_binary(&path.to_string_lossy());
    }
    Err(WcError::Other(format!(
        "{} not found. Build/install the native Web renderer or set web_renderer_path.",
        BINARY_NAME
    )))
}

fn validate_binary(path: &str) -> Result<String, WcError> {
    let p = Path::new(path);
    if !p.exists() {
        return Err(WcError::Other(format!("web renderer not found: {}", path)));
    }
    if !p.is_file() {
        return Err(WcError::Other(format!(
            "web renderer path is not a file: {}",
            path
        )));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = p.metadata().map_err(WcError::Io)?;
        if meta.permissions().mode() & 0o111 == 0 {
            return Err(WcError::Other(format!(
                "web renderer is not executable: {}",
                path
            )));
        }
    }
    Ok(path.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wc_core::config::ConfigDir;

    fn temp_storage() -> (tempfile::TempDir, StorageApi) {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("config"),
        };
        cd.init().unwrap();
        wc_core::config::write_config_value(&cd.path, "storage_backend", "sqlite").unwrap();
        (tmp, StorageApi::new(cd))
    }

    fn web_project(root: &Path) -> std::path::PathBuf {
        let project = root.join("steamapps/workshop/content/431960/3650880224");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("index.html"), "<html></html>").unwrap();
        std::fs::write(
            project.join("project.json"),
            r#"{"type":"web","file":"index.html"}"#,
        )
        .unwrap();
        project
    }

    #[cfg(unix)]
    fn executable(path: &Path) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(path, "#!/bin/sh\nsleep 5\n").unwrap();
        let mut perms = std::fs::metadata(path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms).unwrap();
    }

    #[test]
    fn command_spec_uses_project_path_and_file() {
        let (tmp, s) = temp_storage();
        let bin = tmp.path().join("renderer");
        executable(&bin);
        s.config_set("web_renderer_path", &bin.to_string_lossy())
            .unwrap();
        s.config_set("web_renderer_output", "eDP-1").unwrap();
        let project = project_from_path(&web_project(tmp.path()).to_string_lossy()).unwrap();
        let spec = command_spec(&WebRendererConfig::from_storage(&s), &project).unwrap();
        assert_eq!(spec.program, bin.to_string_lossy());
        assert!(spec.args.contains(&"--project".to_string()));
        assert!(spec.args.contains(&"--file".to_string()));
        assert!(spec.args.contains(&"index.html".to_string()));
        assert!(spec.args.contains(&"eDP-1".to_string()));
    }

    #[test]
    fn missing_renderer_reports_unavailable() {
        let (_tmp, s) = temp_storage();
        s.config_set("web_renderer_path", "/no/such/renderer")
            .unwrap();
        let st = status(&WebRendererConfig::from_storage(&s));
        assert!(!st.available);
        assert!(st.message.contains("missing"));
    }

    #[test]
    fn disabled_renderer_is_not_available() {
        let (_tmp, s) = temp_storage();
        s.config_set("web_renderer_enabled", "off").unwrap();
        assert!(!is_available(&s));
    }
}
