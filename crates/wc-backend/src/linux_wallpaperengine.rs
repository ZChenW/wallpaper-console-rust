use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use wc_core::error::WcError;
use wc_core::types::FileType;
use wc_storage::StorageApi;

const PID_CONFIG_KEY: &str = "linux_wallpaperengine_pid";

#[derive(Debug, Clone)]
pub struct LinuxWallpaperEngineConfig {
    pub enabled: bool,
    pub path: String,
    pub scaling: String,
    pub fps: u32,
    pub muted: bool,
    pub volume: u32,
    pub target_mode: String,
    pub target: String,
}

impl LinuxWallpaperEngineConfig {
    pub fn from_storage(s: &StorageApi) -> Self {
        LinuxWallpaperEngineConfig {
            enabled: s.config_get("linux_wallpaperengine_enabled", "on") == "on",
            path: s.config_get("linux_wallpaperengine_path", "auto"),
            scaling: s.config_get("linux_wallpaperengine_scaling", "default"),
            fps: s
                .config_get("linux_wallpaperengine_fps", "60")
                .parse()
                .unwrap_or(60),
            muted: s.config_get("linux_wallpaperengine_muted", "off") == "on",
            volume: s
                .config_get("linux_wallpaperengine_volume", "100")
                .parse()
                .unwrap_or(100),
            target_mode: s.config_get("linux_wallpaperengine_target_mode", "auto"),
            target: s.config_get("linux_wallpaperengine_target", ""),
        }
    }
}

pub struct Status {
    pub available: bool,
    pub path: Option<String>,
    pub message: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LinuxWallpaperEngineProject {
    pub project_path: String,
    pub workshop_id: Option<String>,
    pub project_type: FileType,
    pub title: Option<String>,
}

pub fn status(config: &LinuxWallpaperEngineConfig) -> Status {
    if !config.enabled {
        return Status {
            available: false,
            path: None,
            message: "linux-wallpaperengine is disabled.".into(),
            detail: Some("Enable it in Settings → Wallpaper Engine Backend.".into()),
        };
    }
    match resolve_binary(config) {
        Ok(path) => Status {
            available: true,
            path: Some(path),
            message: "linux-wallpaperengine ready.".into(),
            detail: Some("Scene wallpapers can be attempted with linux-wallpaperengine.".into()),
        },
        Err(err) => Status {
            available: false,
            path: None,
            message: "linux-wallpaperengine missing.".into(),
            detail: Some(err.to_string()),
        },
    }
}

pub fn project_from_path(path: &str) -> Result<LinuxWallpaperEngineProject, WcError> {
    let p = Path::new(path);
    let project_dir: PathBuf = if p.is_dir() {
        p.to_path_buf()
    } else {
        p.parent()
            .ok_or_else(|| WcError::UnsupportedFileType(path.to_string()))?
            .to_path_buf()
    };
    let info = wc_scan::read_we_project_info(&project_dir).ok_or_else(|| {
        WcError::Other("project.json missing or invalid for Wallpaper Engine project".into())
    })?;
    if info.entry_type != FileType::WeScene {
        return Err(WcError::Other(format!(
            "linux-wallpaperengine only supports scene projects here; got {:?}",
            info.entry_type
        )));
    }
    Ok(LinuxWallpaperEngineProject {
        project_path: project_dir.to_string_lossy().to_string(),
        workshop_id: info.workshop_id,
        project_type: info.entry_type,
        title: info.title,
    })
}

pub fn apply(s: &StorageApi, project: LinuxWallpaperEngineProject) -> Result<(), WcError> {
    let config = LinuxWallpaperEngineConfig::from_storage(s);
    if !config.enabled {
        return Err(WcError::Other("linux-wallpaperengine is disabled".into()));
    }
    let binary = resolve_binary(&config)?;
    let target = project
        .workshop_id
        .clone()
        .unwrap_or_else(|| project.project_path.clone());
    let mut args = Vec::new();
    match config.target_mode.as_str() {
        "screen-root" if !config.target.trim().is_empty() => {
            args.push("--screen-root".to_string());
            args.push(config.target.clone());
            args.push("--bg".to_string());
            args.push(target);
        }
        "screen-span" if !config.target.trim().is_empty() => {
            args.push("--screen-span".to_string());
            args.push(config.target.clone());
            args.push("--bg".to_string());
            args.push(target);
        }
        _ => args.push(target),
    }
    if config.scaling != "default" {
        args.push("--scaling".into());
        args.push(config.scaling.clone());
    }
    if config.fps > 0 {
        args.push("--fps".into());
        args.push(config.fps.to_string());
    }
    if config.muted {
        args.push("--volume".into());
        args.push("0".into());
    } else if config.volume <= 100 {
        args.push("--volume".into());
        args.push(config.volume.to_string());
    }

    crate::stop_all_backends(Some(s))?;
    let mut child = Command::new("setsid")
        .arg(&binary)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| WcError::Other(format!("linux-wallpaperengine failed to start: {}", e)))?;
    std::thread::sleep(Duration::from_millis(400));
    if let Some(status) = child.try_wait().map_err(WcError::Io)? {
        let output = child.wait_with_output().map_err(WcError::Io)?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(map_renderer_error(status.to_string(), &stderr));
    }
    s.config_set(PID_CONFIG_KEY, &child.id().to_string())?;
    s.current_write(&project.project_path)?;
    s.last_backend_write("linux-wallpaperengine")?;
    s.history_add(&project.project_path, "linux-wallpaperengine")?;
    Ok(())
}

pub fn stop(s: Option<&StorageApi>) {
    if let Some(s) = s {
        let pid = s.config_get(PID_CONFIG_KEY, "");
        if let Ok(pid) = pid.parse::<i32>() {
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
            let _ = s.config_set(PID_CONFIG_KEY, "");
        }
    }
}

fn resolve_binary(config: &LinuxWallpaperEngineConfig) -> Result<String, WcError> {
    if config.path != "auto" && !config.path.trim().is_empty() {
        let p = Path::new(&config.path);
        if !p.exists() {
            return Err(WcError::Other(format!(
                "linux-wallpaperengine not found at configured path: {}",
                config.path
            )));
        }
        if !p.is_file() {
            return Err(WcError::Other(format!(
                "linux-wallpaperengine path is not a file: {}",
                config.path
            )));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = p.metadata().map_err(WcError::Io)?;
            if meta.permissions().mode() & 0o111 == 0 {
                return Err(WcError::Other(format!(
                    "linux-wallpaperengine is not executable: {}",
                    config.path
                )));
            }
        }
        return Ok(config.path.clone());
    }
    which::which("linux-wallpaperengine")
        .map(|p| p.to_string_lossy().to_string())
        .map_err(|_| {
            WcError::Other(
                "Wallpaper Engine scene wallpapers require linux-wallpaperengine. Install it from AUR: yay -S linux-wallpaperengine-git"
                    .into(),
            )
        })
}

fn map_renderer_error(status: String, stderr: &str) -> WcError {
    let lower = stderr.to_lowercase();
    if lower.contains("projection must have a width") {
        return WcError::Other(format!(
            "This Wallpaper Engine scene uses projection data that linux-wallpaperengine cannot render. Use the preview GIF or choose another scene. linux-wallpaperengine exited unexpectedly with status {}. Renderer output: {}",
            status,
            stderr.trim()
        ));
    }
    if lower.contains("cannot find workshop directory") {
        return WcError::Other(format!(
            "linux-wallpaperengine could not find the Wallpaper Engine workshop directory. Check target mode/output and assets dir. Renderer output: {}",
            stderr.trim()
        ));
    }
    WcError::Other(format!(
        "linux-wallpaperengine exited unexpectedly with status {}. Renderer output: {}",
        status,
        stderr.trim()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn projection_error_is_mapped() {
        let err = map_renderer_error("exit status: 1".into(), "Projection must have a width");
        assert!(err.to_string().contains("projection data"));
    }

    fn config_with_path(path: PathBuf) -> LinuxWallpaperEngineConfig {
        LinuxWallpaperEngineConfig {
            enabled: true,
            path: path.to_string_lossy().to_string(),
            scaling: "default".into(),
            fps: 60,
            muted: false,
            volume: 100,
            target_mode: "auto".into(),
            target: String::new(),
        }
    }

    #[test]
    fn configured_binary_rejects_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let err = resolve_binary(&config_with_path(tmp.path().to_path_buf())).unwrap_err();
        assert!(err.to_string().contains("not a file"));
    }

    #[cfg(unix)]
    #[test]
    fn configured_binary_rejects_non_executable_file() {
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("linux-wallpaperengine");
        std::fs::write(&bin, "#!/bin/sh\n").unwrap();
        let err = resolve_binary(&config_with_path(bin)).unwrap_err();
        assert!(err.to_string().contains("not executable"));
    }
}
