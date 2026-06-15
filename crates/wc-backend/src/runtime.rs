use std::process::{Command, Output, Stdio};

use wc_core::error::WcError;
use wc_storage::StorageApi;

pub trait BackendRuntime {
    fn command_output(&mut self, command: &mut Command) -> Result<Output, WcError>;
    fn command_status(
        &mut self,
        command: &mut Command,
    ) -> Result<std::process::ExitStatus, WcError>;
    fn stop_awww(&mut self);
    fn stop_mpvpaper(&mut self);
    fn stop_lwe(&mut self, s: Option<&StorageApi>);
    fn ensure_awww_daemon_running(&mut self) -> Result<(), WcError>;
}

pub struct SystemBackendRuntime;

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

    fn ensure_awww_daemon_running(&mut self) -> Result<(), WcError> {
        let user = crate::whoami();
        if crate::is_awww_daemon_running(&user) {
            return Ok(());
        }
        let mut cmd = Command::new("setsid");
        cmd.args(["-f", "awww-daemon"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
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
        for _ in 0..30 {
            std::thread::sleep(std::time::Duration::from_millis(50));
            if crate::is_awww_daemon_running(&user) {
                return Ok(());
            }
        }
        Err(WcError::Other(
            "awww-daemon failed to start. Check 'awww-daemon' is installed and your compositor supports wlr-layer-shell."
                .into(),
        ))
    }
}
