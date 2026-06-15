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

    // Save old PID before starting new process.
    // If the new LWE process fails to start, we keep the old wallpaper running.
    let old_pid_str = s.config_get(PID_CONFIG_KEY, "");
    let old_pid: Option<i32> = old_pid_str.parse().ok().filter(|&p| p > 0);

    // Write stdout/stderr to a log file instead of Stdio::piped() which
    // can deadlock if nobody drains the pipe on a long-running process.
    let log_path = s.cd.path.join("linux-wallpaperengine-last.log");
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

    // Persist diagnostics info for the debug command.
    let cmd_line = format!("{} {}", binary, args.join(" "));
    let _ = s.config_set("lwe_last_command_line", &cmd_line);
    let _ = s.config_set(
        "lwe_last_target_config",
        &format!(
            "target_mode={} target={} scaling={} fps={} muted={} volume={}",
            config.target_mode,
            config.target,
            config.scaling,
            config.fps,
            config.muted,
            config.volume
        ),
    );

    let mut child = Command::new("setsid")
        .arg(&binary)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(stderr)
        .spawn()
        .map_err(|e| WcError::Other(format!("linux-wallpaperengine failed to start: {}", e)))?;

    // Poll for up to 800ms to detect immediate crash, checking every 50ms.
    let poll_interval = Duration::from_millis(50);
    let deadline = Duration::from_millis(800);
    let start = std::time::Instant::now();
    loop {
        std::thread::sleep(poll_interval);
        match child.try_wait().map_err(WcError::Io)? {
            Some(status) => {
                // Read stderr from log file for diagnostics.
                let stderr_tail = std::fs::read_to_string(&log_path).unwrap_or_default();
                let _ = s.config_set("lwe_last_stderr", &stderr_tail);
                let _ = s.config_set("lwe_last_exit_status", &status.to_string());
                // New process failed — do NOT kill old LWE. Keep current wallpaper.
                return Err(map_renderer_error(status.to_string(), &stderr_tail));
            }
            None if start.elapsed() >= deadline => break,
            None => continue,
        }
    }

    // New process is alive. Safe to kill old LWE now.
    if let Some(old) = old_pid {
        let pgid = format!("-{}", old);
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
    }

    // Clean up any stale LWE processes that aren't the new one.
    // This handles cases where old_pid was stale/missing but residual
    // linux-wallpaperengine processes are still running.
    crate::process_control::cleanup_stale_lwe_processes_except(child.id(), old_pid);

    // Clear all diagnostics on success.
    let _ = s.config_set("lwe_last_command_line", "");
    let _ = s.config_set("lwe_last_target_config", "");
    let _ = s.config_set("lwe_last_stderr", "");
    let _ = s.config_set("lwe_last_exit_status", "");

    s.config_set(PID_CONFIG_KEY, &child.id().to_string())?;
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
    stop_tracked_processes();
}

/// Kill any residual linux-wallpaperengine processes owned by current user.
/// Uses -f (match full command line) instead of -x because Linux truncates
/// /proc/.../comm to 15 chars.
pub fn stop_tracked_processes() {
    if let Ok(user) = std::env::var("USER") {
        let _ = Command::new("pkill")
            .args(["-u", &user, "-f", r"(^|/)linux-wallpaperengine\b"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
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
            "This scene uses projection data that linux-wallpaperengine cannot render. Use the preview GIF or choose another scene. (status {})",
            status
        ));
    }
    if lower.contains("cannot find workshop directory") {
        return WcError::Other(format!(
            "linux-wallpaperengine could not find the Wallpaper Engine workshop directory. Check target mode/output settings. (status {})",
            status
        ));
    }
    if lower.contains("failed to create window")
        || lower.contains("no suitable output")
        || lower.contains("no display")
    {
        return WcError::Other(format!(
            "linux-wallpaperengine could not create a window/display output. For Wayland/Niri, set target_mode=screen-root and target=<output name> in Settings. (status {}. Output: {})",
            status,
            stderr.trim()
        ));
    }
    WcError::Other(format!(
        "linux-wallpaperengine exited unexpectedly with status {}. Output: {}",
        status,
        stderr.trim()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use wc_core::config::ConfigDir;

    #[test]
    fn projection_error_is_mapped() {
        let err = map_renderer_error("exit status: 1".into(), "Projection must have a width");
        assert!(err.to_string().contains("projection data"));
    }

    #[test]
    fn target_output_error_is_mapped() {
        let err = map_renderer_error("exit status: 1".into(), "failed to create window");
        assert!(err.to_string().contains("could not create a window"));
        assert!(err.to_string().contains("screen-root"));

        let err = map_renderer_error("exit status: 1".into(), "no suitable output");
        assert!(err.to_string().contains("could not create a window"));

        let err = map_renderer_error("exit status: 1".into(), "no display");
        assert!(err.to_string().contains("could not create a window"));
    }

    #[test]
    fn workshop_directory_error_is_mapped() {
        let err = map_renderer_error("exit status: 1".into(), "cannot find workshop directory");
        assert!(err
            .to_string()
            .contains("could not find the Wallpaper Engine workshop directory"));
    }

    #[test]
    fn generic_lwe_error_includes_stderr() {
        let err = map_renderer_error("exit status: 1".into(), "unknown OpenGL error: framebuffer");
        assert!(err.to_string().contains("exited unexpectedly"));
        assert!(err.to_string().contains("OpenGL error"));
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

    #[cfg(unix)]
    #[test]
    fn handoff_preserves_old_pid_when_new_process_fails_immediately() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("config"),
        };
        cd.init().unwrap();
        wc_core::config::write_config_value(&cd.path, "storage_backend", "sqlite").unwrap();
        let s = StorageApi::new(cd);

        // Set a known old PID.
        let old_pid = 99999;
        s.config_set(PID_CONFIG_KEY, &old_pid.to_string()).unwrap();
        s.last_backend_write(crate::LWE_BACKEND_NAME).unwrap();

        // Mock LWE binary that fails immediately.
        let bin = tmp.path().join("test-lwe-mock");
        std::fs::write(&bin, "#!/bin/sh\nexit 1\n").unwrap();
        let mut perms = std::fs::metadata(&bin).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&bin, perms).unwrap();
        s.config_set("linux_wallpaperengine_path", &bin.to_string_lossy())
            .unwrap();

        // Create a fake scene project.
        let scene = tmp.path().join("scene");
        std::fs::create_dir_all(&scene).unwrap();
        std::fs::write(
            scene.join("project.json"),
            r#"{"type":"scene","file":"scene.pkg","workshopid":"123"}"#,
        )
        .unwrap();

        let project = project_from_path(&scene.to_string_lossy()).unwrap();
        let result = apply(&s, project);

        // Should fail (mock exits 1).
        assert!(result.is_err());

        // Old PID must NOT have been cleared — we keep the wallpaper.
        let pid_after = s.config_get(PID_CONFIG_KEY, "");
        assert_eq!(
            pid_after,
            old_pid.to_string(),
            "old PID should be preserved when new process fails immediately"
        );
    }

    #[cfg(unix)]
    #[test]
    fn handoff_kills_old_pid_when_new_process_survives() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("config"),
        };
        cd.init().unwrap();
        wc_core::config::write_config_value(&cd.path, "storage_backend", "sqlite").unwrap();
        let s = StorageApi::new(cd);

        // Set a known old PID.
        let old_pid = 88888;
        s.config_set(PID_CONFIG_KEY, &old_pid.to_string()).unwrap();
        s.last_backend_write(crate::LWE_BACKEND_NAME).unwrap();

        // Mock LWE binary that sleeps long enough to survive the poll.
        let bin = tmp.path().join("test-lwe-mock-survive");
        std::fs::write(&bin, "#!/bin/sh\nsleep 5\n").unwrap();
        let mut perms = std::fs::metadata(&bin).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&bin, perms).unwrap();
        s.config_set("linux_wallpaperengine_path", &bin.to_string_lossy())
            .unwrap();

        // Create a fake scene project.
        let scene = tmp.path().join("scene2");
        std::fs::create_dir_all(&scene).unwrap();
        std::fs::write(
            scene.join("project.json"),
            r#"{"type":"scene","file":"scene.pkg","workshopid":"456"}"#,
        )
        .unwrap();

        let project = project_from_path(&scene.to_string_lossy()).unwrap();
        let result = apply(&s, project);

        // Should succeed (mock sleeps, survives poll).
        assert!(result.is_ok());

        // New PID should be recorded, different from old.
        let pid_after = s.config_get(PID_CONFIG_KEY, "");
        assert_ne!(
            pid_after,
            old_pid.to_string(),
            "old PID should be replaced by new PID after successful handoff"
        );
        assert!(!pid_after.is_empty(), "new PID should be recorded");

        // Cleanup: kill the sleeping mock process.
        if let Ok(pid) = pid_after.parse::<i32>() {
            let _ = Command::new("kill")
                .args(["-TERM", &format!("-{}", pid)])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }

    #[cfg(unix)]
    #[test]
    fn cross_backend_switch_cleans_non_lwe_after_success() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("config"),
        };
        cd.init().unwrap();
        wc_core::config::write_config_value(&cd.path, "storage_backend", "sqlite").unwrap();
        let s = StorageApi::new(cd);

        // Simulate current backend being mpvpaper (not LWE).
        s.last_backend_write("mpvpaper").unwrap();

        // Mock LWE binary that succeeds.
        let bin = tmp.path().join("test-lwe-mock-cross");
        std::fs::write(&bin, "#!/bin/sh\nsleep 60\n").unwrap();
        let mut perms = std::fs::metadata(&bin).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&bin, perms).unwrap();
        s.config_set("linux_wallpaperengine_path", &bin.to_string_lossy())
            .unwrap();

        let scene = tmp.path().join("scene3");
        std::fs::create_dir_all(&scene).unwrap();
        std::fs::write(
            scene.join("project.json"),
            r#"{"type":"scene","file":"scene.pkg","workshopid":"789"}"#,
        )
        .unwrap();

        let project = project_from_path(&scene.to_string_lossy()).unwrap();
        let result = apply(&s, project);

        // Should succeed — cross-backend switch from mpvpaper to LWE.
        assert!(result.is_ok());

        // After successful apply, backend state should still be mpvpaper
        // because linux_wallpaperengine::apply only starts the renderer;
        // unified apply_wallpaper writes backend state.
        let backend = s.last_backend_read().unwrap().unwrap_or_default();
        assert_eq!(
            backend,
            "mpvpaper",
            "linux_wallpaperengine::apply only starts the renderer; unified apply_wallpaper writes backend state"
        );

        // Cleanup.
        let pid = s.config_get(PID_CONFIG_KEY, "");
        if let Ok(pid) = pid.parse::<i32>() {
            let _ = Command::new("kill")
                .args(["-TERM", &format!("-{}", pid)])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
}
