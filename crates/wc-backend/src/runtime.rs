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
    /// Preflight external renderer availability before any destructive handoff.
    /// Test runtimes default to available; the system runtime probes PATH.
    fn ensure_backend_available(
        &mut self,
        _backend: wc_core::types::Backend,
        _storage: &StorageApi,
    ) -> Result<(), WcError> {
        Ok(())
    }
    fn command_output(&mut self, command: &mut Command) -> Result<Output, WcError>;
    fn command_status(
        &mut self,
        command: &mut Command,
    ) -> Result<std::process::ExitStatus, WcError>;
    fn mpvpaper_pids(&mut self) -> Result<Vec<u32>, WcError>;
    fn wait_for_mpvpaper_ready(&mut self, previous_pids: &[u32]) -> Result<u32, WcError>;
    fn mpvpaper_pid_running(&mut self, pid: u32) -> Result<bool, WcError>;
    fn stop_awww(&mut self);
    fn stop_mpvpaper(&mut self);
    fn stop_lwe(&mut self, s: Option<&StorageApi>);
    /// Stop awww and verify the daemon is gone. Display executor uses this.
    fn stop_awww_checked(&mut self) -> Result<(), WcError> {
        self.stop_awww();
        Ok(())
    }
    /// Stop mpvpaper and verify no processes remain. Display executor uses this.
    fn stop_mpvpaper_checked(&mut self) -> Result<(), WcError> {
        self.stop_mpvpaper();
        Ok(())
    }
    /// Stop LWE and verify termination. Display executor uses this.
    fn stop_lwe_checked(&mut self, s: Option<&StorageApi>) -> Result<(), WcError> {
        self.stop_lwe(s);
        Ok(())
    }
    /// Apply LWE to explicit outputs (readiness + handoff included).
    ///
    /// System runtime delegates to the real implementation. Fakes must not
    /// launch or kill real linux-wallpaperengine processes.
    fn apply_lwe_to_outputs(
        &mut self,
        s: &StorageApi,
        project: &crate::linux_wallpaperengine::LinuxWallpaperEngineProject,
        outputs: &[String],
    ) -> Result<(), WcError>;
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
    if crate::awww::is_awww_daemon_running(user) {
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

pub(crate) fn new_mpvpaper_pid(current_pids: &[u32], previous_pids: &[u32]) -> Option<u32> {
    current_pids
        .iter()
        .copied()
        .find(|pid| !previous_pids.contains(pid))
}

pub(crate) fn wait_for_mpvpaper_ready_with<P, S>(
    previous_pids: &[u32],
    mut probe: P,
    mut sleep: S,
) -> Result<u32, WcError>
where
    P: FnMut() -> Result<Vec<u32>, WcError>,
    S: FnMut(std::time::Duration),
{
    for poll in 0..=40 {
        let current_pids = probe()?;
        if let Some(pid) = new_mpvpaper_pid(&current_pids, previous_pids) {
            return Ok(pid);
        }
        if poll < 40 {
            sleep(std::time::Duration::from_millis(50));
        }
    }
    Err(WcError::Other(
        "mpvpaper failed to become ready: no new mpvpaper process appeared within 2 seconds".into(),
    ))
}

pub(crate) fn wait_for_mpvpaper_stopped_with<P, S>(
    mut probe: P,
    mut sleep: S,
) -> Result<(), WcError>
where
    P: FnMut() -> Result<Vec<u32>, WcError>,
    S: FnMut(std::time::Duration),
{
    for poll in 0..=40 {
        let pids = probe()?;
        if pids.is_empty() {
            return Ok(());
        }
        if poll == 40 {
            return Err(WcError::Other(format!(
                "mpvpaper still running after stop: pids={pids:?}"
            )));
        }
        sleep(std::time::Duration::from_millis(50));
    }
    Err(WcError::Other(
        "mpvpaper still running after stop: readiness poll exhausted".into(),
    ))
}

impl BackendRuntime for SystemBackendRuntime {
    fn ensure_backend_available(
        &mut self,
        backend: wc_core::types::Backend,
        storage: &StorageApi,
    ) -> Result<(), WcError> {
        match crate::driver::driver_for(backend) {
            Some(driver) => driver.ensure_available(storage),
            None => Ok(()),
        }
    }

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

    fn mpvpaper_pids(&mut self) -> Result<Vec<u32>, WcError> {
        crate::mpvpaper::running_pids()
    }

    fn wait_for_mpvpaper_ready(&mut self, previous_pids: &[u32]) -> Result<u32, WcError> {
        wait_for_mpvpaper_ready_with(previous_pids, || self.mpvpaper_pids(), std::thread::sleep)
    }

    fn mpvpaper_pid_running(&mut self, pid: u32) -> Result<bool, WcError> {
        Ok(self.mpvpaper_pids()?.contains(&pid))
    }

    fn stop_awww(&mut self) {
        crate::awww::stop_awww();
    }

    fn stop_mpvpaper(&mut self) {
        crate::mpvpaper::stop_mpvpaper();
    }

    fn stop_lwe(&mut self, s: Option<&StorageApi>) {
        crate::linux_wallpaperengine::stop(s);
    }

    fn stop_awww_checked(&mut self) -> Result<(), WcError> {
        self.stop_awww();
        let user = crate::whoami();
        if crate::awww::is_awww_daemon_running(&user) {
            return Err(WcError::Other(
                "awww-daemon still running after stop".into(),
            ));
        }
        // Socket may linger briefly; treat Ready as evidence the daemon survived.
        if matches!(self.awww_socket_ready(), AwwwReadiness::Ready) {
            return Err(WcError::Other(
                "awww socket still answers query after stop".into(),
            ));
        }
        Ok(())
    }

    fn stop_mpvpaper_checked(&mut self) -> Result<(), WcError> {
        self.stop_mpvpaper();
        wait_for_mpvpaper_stopped_with(|| self.mpvpaper_pids(), std::thread::sleep)
    }

    fn stop_lwe_checked(&mut self, s: Option<&StorageApi>) -> Result<(), WcError> {
        self.stop_lwe(s);
        if crate::linux_wallpaperengine::is_running_for_current_user() {
            return Err(WcError::Other(
                "linux-wallpaperengine still running after stop".into(),
            ));
        }
        if let Some(storage) = s {
            let pid = storage.config_get("linux_wallpaperengine_pid", "");
            if !pid.trim().is_empty() {
                return Err(WcError::Other(format!(
                    "linux-wallpaperengine pid tracking not cleared after stop: {pid}"
                )));
            }
        }
        Ok(())
    }

    fn apply_lwe_to_outputs(
        &mut self,
        s: &StorageApi,
        project: &crate::linux_wallpaperengine::LinuxWallpaperEngineProject,
        outputs: &[String],
    ) -> Result<(), WcError> {
        crate::linux_wallpaperengine::apply_to_outputs(s, project.clone(), outputs)
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
        let was_running = crate::awww::is_awww_daemon_running(&user);
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

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::time::Duration;

    use wc_core::error::WcError;

    use super::{new_mpvpaper_pid, wait_for_mpvpaper_ready_with, wait_for_mpvpaper_stopped_with};

    #[test]
    fn mpvpaper_stop_waits_for_a_process_to_exit() {
        let mut probes: VecDeque<Result<Vec<u32>, WcError>> =
            VecDeque::from([Ok(vec![41]), Ok(vec![])]);
        let mut sleeps = Vec::new();

        wait_for_mpvpaper_stopped_with(
            || probes.pop_front().expect("unexpected extra PID probe"),
            |duration| sleeps.push(duration),
        )
        .unwrap();

        assert_eq!(sleeps, vec![Duration::from_millis(50)]);
    }

    #[test]
    fn mpvpaper_stop_reports_remaining_pids_after_two_seconds() {
        let mut probe_count = 0;
        let mut sleeps = Vec::new();

        let error = wait_for_mpvpaper_stopped_with(
            || {
                probe_count += 1;
                Ok(vec![41])
            },
            |duration| sleeps.push(duration),
        )
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "mpvpaper still running after stop: pids=[41]"
        );
        assert_eq!(probe_count, 41);
        assert_eq!(sleeps, vec![Duration::from_millis(50); 40]);
    }

    #[test]
    fn mpvpaper_wait_returns_new_pid_after_an_immediate_old_only_probe() {
        let mut probes: VecDeque<Result<Vec<u32>, WcError>> =
            VecDeque::from([Ok(vec![41]), Ok(vec![41, 52])]);
        let mut sleeps = Vec::new();

        let pid = wait_for_mpvpaper_ready_with(
            &[41],
            || probes.pop_front().expect("unexpected extra PID probe"),
            |duration| sleeps.push(duration),
        )
        .unwrap();

        assert_eq!(pid, 52);
        assert_eq!(sleeps, vec![Duration::from_millis(50)]);
    }

    #[test]
    fn mpvpaper_wait_times_out_after_two_seconds_of_old_only_probes() {
        let mut probe_count = 0;
        let mut sleeps = Vec::new();

        let error = wait_for_mpvpaper_ready_with(
            &[41],
            || {
                probe_count += 1;
                Ok(vec![41])
            },
            |duration| sleeps.push(duration),
        )
        .unwrap_err();

        assert!(error.to_string().contains("within 2 seconds"));
        assert_eq!(probe_count, 41);
        assert_eq!(sleeps, vec![Duration::from_millis(50); 40]);
    }

    #[test]
    fn mpvpaper_wait_propagates_the_first_probe_error_without_sleeping() {
        let mut probe_count = 0;
        let mut sleeps = Vec::new();

        let error = wait_for_mpvpaper_ready_with(
            &[41],
            || {
                probe_count += 1;
                Err(WcError::Other("mpvpaper PID probe failed".into()))
            },
            |duration| sleeps.push(duration),
        )
        .unwrap_err();

        assert_eq!(error.to_string(), "mpvpaper PID probe failed");
        assert_eq!(probe_count, 1);
        assert!(sleeps.is_empty());
    }

    #[test]
    fn new_mpvpaper_pid_selects_only_a_pid_absent_from_previous_snapshot() {
        assert_eq!(new_mpvpaper_pid(&[41, 52, 63], &[41, 63]), Some(52));
    }

    #[test]
    fn new_mpvpaper_pid_returns_none_without_a_new_pid() {
        assert_eq!(new_mpvpaper_pid(&[41, 63], &[41, 63]), None);
    }
}
