use std::process::{Command, Output, Stdio};

use wc_core::error::WcError;
use wc_storage::StorageApi;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AwwwReadiness {
    Ready,
    SocketMissing,
    SocketPresentQueryFailed { stderr: String },
}

pub fn awww_socket_path() -> Result<std::path::PathBuf, WcError> {
    let xdg = std::env::var("XDG_RUNTIME_DIR").map_err(|_| {
        WcError::Other("XDG_RUNTIME_DIR is not set; cannot locate awww-daemon socket".into())
    })?;
    let wayland = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".to_string());
    Ok(std::path::PathBuf::from(xdg).join(format!("{wayland}-awww-daemon.sock")))
}

pub trait BackendRuntime {
    fn command_output(&mut self, command: &mut Command) -> Result<Output, WcError>;
    fn command_status(
        &mut self,
        command: &mut Command,
    ) -> Result<std::process::ExitStatus, WcError>;
    fn stop_awww(&mut self);
    fn stop_mpvpaper(&mut self);
    fn stop_lwe(&mut self, s: Option<&StorageApi>);
    fn awww_socket_ready(&mut self) -> AwwwReadiness;
    fn ensure_awww_daemon_running(&mut self) -> Result<(), WcError>;
    fn clear_awww_state_hint(&mut self);
}

pub struct SystemBackendRuntime;

pub(crate) fn build_awww_daemon_command() -> Command {
    let mut cmd = Command::new("setsid");
    cmd.args(["-f", "awww-daemon", "--no-cache"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    cmd
}

pub(crate) fn wait_for_awww_socket_ready(
    runtime: &mut dyn BackendRuntime,
    user: &str,
) -> Result<(), WcError> {
    let mut last_stderr = String::new();
    for _ in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(50));
        match runtime.awww_socket_ready() {
            AwwwReadiness::Ready => return Ok(()),
            AwwwReadiness::SocketMissing => {}
            AwwwReadiness::SocketPresentQueryFailed { stderr } => {
                last_stderr = stderr;
            }
        }
    }
    let socket_path = awww_socket_path()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "<unknown>".to_string());
    let wayland = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".to_string());
    if crate::is_awww_daemon_running(user) {
        Err(WcError::Other(format!(
            "awww-daemon is running but socket is not ready for WAYLAND_DISPLAY={} \
             (expected {}); last query stderr: {}",
            wayland, socket_path, last_stderr
        )))
    } else {
        Err(WcError::Other(
            "awww-daemon failed to start. Check 'awww-daemon' is installed and your \
             compositor supports wlr-layer-shell."
                .into(),
        ))
    }
}

impl BackendRuntime for SystemBackendRuntime {
    fn command_output(&mut self, command: &mut Command) -> Result<Output, WcError> {
        command
            .output()
            .map_err(|e| WcError::Other(format!("command failed: {}", e)))
    }

    fn command_status(
        &mut self,
        command: &mut Command,
    ) -> Result<std::process::ExitStatus, WcError> {
        command
            .status()
            .map_err(|e| WcError::Other(format!("command failed: {}", e)))
    }

    fn stop_awww(&mut self) {
        crate::stop_awww();
    }

    fn stop_mpvpaper(&mut self) {
        crate::stop_mpvpaper();
    }

    fn stop_lwe(&mut self, s: Option<&StorageApi>) {
        crate::linux_wallpaperengine::stop(s);
    }

    fn awww_socket_ready(&mut self) -> AwwwReadiness {
        let path = match awww_socket_path() {
            Ok(p) => p,
            Err(_) => return AwwwReadiness::SocketMissing,
        };
        if !path.exists() {
            return AwwwReadiness::SocketMissing;
        }
        let output = Command::new("awww")
            .arg("query")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output();
        match output {
            Ok(o) if o.status.success() => AwwwReadiness::Ready,
            Ok(o) => AwwwReadiness::SocketPresentQueryFailed {
                stderr: String::from_utf8_lossy(&o.stderr).trim().to_string(),
            },
            Err(e) => AwwwReadiness::SocketPresentQueryFailed {
                stderr: format!("awww query failed to execute: {}", e),
            },
        }
    }

    fn ensure_awww_daemon_running(&mut self) -> Result<(), WcError> {
        if matches!(self.awww_socket_ready(), AwwwReadiness::Ready) {
            return Ok(());
        }
        let user = crate::whoami();
        let was_running = crate::is_awww_daemon_running(&user);
        if !was_running {
            let mut cmd = build_awww_daemon_command();
            let status = self.command_status(&mut cmd).map_err(|_| {
                WcError::Other(
                    "setsid not available — cannot launch awww-daemon. \
                     setsid is part of util-linux; install it with your package manager."
                        .into(),
                )
            })?;
            if !status.success() {
                return Err(WcError::Other(
                    "awww-daemon not found. Install awww (pip install awww or AUR).".into(),
                ));
            }
        }
        wait_for_awww_socket_ready(self, &user)
    }

    fn clear_awww_state_hint(&mut self) {
        let mut cmd = Command::new("awww");
        cmd.arg("clear")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let _ = self.command_status(&mut cmd);
    }
}
