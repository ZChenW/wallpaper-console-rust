use std::process::{Command, Output};

use wc_core::error::WcError;
use wc_storage::StorageApi;

const APPLY_COMMAND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(65);
const LAUNCH_COMMAND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const AWWW_QUERY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

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

/// Process I/O seam: spawn commands and probe renderer readiness.
///
/// Prefer this surface for new orchestration. Stop / apply policy belongs on
/// [`crate::driver::BackendDriver`]; see domain term **ProcessIo**.
pub trait ProcessIo {
    fn command_output(&mut self, command: &mut Command) -> Result<Output, WcError>;
    fn command_status(
        &mut self,
        command: &mut Command,
    ) -> Result<std::process::ExitStatus, WcError>;
    fn mpvpaper_pids(&mut self) -> Result<Vec<u32>, WcError>;
    fn wait_for_mpvpaper_ready(
        &mut self,
        previous_pids: &[u32],
        output: &str,
        path: &str,
    ) -> Result<u32, WcError>;
    fn mpvpaper_pid_running(&mut self, pid: u32) -> Result<bool, WcError>;
    /// Remove only renderer processes that appeared after a failed launch and
    /// whose process arguments exactly match this launch's output and path.
    fn cleanup_failed_mpvpaper_launch(
        &mut self,
        previous_pids: &[u32],
        output: &str,
        path: &str,
    ) -> Result<(), WcError>;
    fn swaybg_pids(&mut self) -> Result<Vec<u32>, WcError> {
        Err(WcError::Other(
            "swaybg process inspection is unavailable for this runtime".into(),
        ))
    }
    fn wait_for_swaybg_ready(
        &mut self,
        _previous_pids: &[u32],
        _path: &str,
        _scope: &crate::target_commands::ExecutionScope,
    ) -> Result<u32, WcError> {
        Err(WcError::Other(
            "swaybg readiness is unavailable for this runtime".into(),
        ))
    }
    fn swaybg_pid_running(&mut self, pid: u32) -> Result<bool, WcError> {
        Ok(self.swaybg_pids()?.contains(&pid))
    }
    fn cleanup_failed_swaybg_launch(
        &mut self,
        _previous_pids: &[u32],
        _path: &str,
        _scope: &crate::target_commands::ExecutionScope,
    ) -> Result<(), WcError> {
        Err(WcError::Other(
            "swaybg failed-launch cleanup is unavailable for this runtime".into(),
        ))
    }
    fn awww_socket_ready(&mut self) -> AwwwReadiness;
}

/// Testable backend seam: [`ProcessIo`] plus stop/apply hooks for fakes and legacy.
///
/// Checked stops and awww daemon/clear policy live on drivers, not this trait.
pub trait BackendRuntime: ProcessIo {
    /// Preflight external renderer availability before any destructive handoff.
    /// Test runtimes default to available; the system runtime probes PATH.
    fn ensure_backend_available(
        &mut self,
        _backend: wc_core::types::Backend,
        _storage: &StorageApi,
    ) -> Result<(), WcError> {
        Ok(())
    }
    fn stop_awww(&mut self);
    fn stop_mpvpaper(&mut self);
    fn stop_swaybg(&mut self) {}
    fn stop_lwe(&mut self, s: Option<&StorageApi>);
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
}

pub struct SystemBackendRuntime;

pub(crate) fn build_awww_daemon_command() -> Command {
    let mut cmd = Command::new("setsid");
    cmd.args(["-f", "awww-daemon", "--no-cache"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    cmd
}

pub(crate) fn wait_for_awww_socket_ready(
    runtime: &mut dyn ProcessIo,
    user: &crate::ProcessUserScope,
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

pub(crate) fn new_mpvpaper_pid_for_target<M>(
    current_pids: &[u32],
    previous_pids: &[u32],
    output: &str,
    path: &str,
    matches_target: &mut M,
) -> Option<u32>
where
    M: FnMut(u32, &str, &str) -> bool,
{
    current_pids
        .iter()
        .copied()
        .find(|pid| !previous_pids.contains(pid) && matches_target(*pid, output, path))
}

pub(crate) fn wait_for_mpvpaper_ready_with<P, M, S>(
    previous_pids: &[u32],
    output: &str,
    path: &str,
    mut probe: P,
    mut matches_target: M,
    mut sleep: S,
) -> Result<u32, WcError>
where
    P: FnMut() -> Result<Vec<u32>, WcError>,
    M: FnMut(u32, &str, &str) -> bool,
    S: FnMut(std::time::Duration),
{
    for poll in 0..=40 {
        let current_pids = probe()?;
        if let Some(pid) = new_mpvpaper_pid_for_target(
            &current_pids,
            previous_pids,
            output,
            path,
            &mut matches_target,
        ) {
            return Ok(pid);
        }
        if poll < 40 {
            sleep(std::time::Duration::from_millis(50));
        }
    }
    Err(WcError::Other(
        "mpvpaper failed to become ready: no new process for the requested output and wallpaper \
         appeared within 2 seconds"
            .into(),
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

pub(crate) fn wait_for_swaybg_ready_with<P, M, S>(
    previous_pids: &[u32],
    path: &str,
    scope: &crate::target_commands::ExecutionScope,
    mut probe: P,
    mut matches_target: M,
    mut sleep: S,
) -> Result<u32, WcError>
where
    P: FnMut() -> Result<Vec<u32>, WcError>,
    M: FnMut(u32, &str, &crate::target_commands::ExecutionScope) -> bool,
    S: FnMut(std::time::Duration),
{
    let mut candidate = None;
    for poll in 0..=40 {
        let current_pids = probe()?;
        if let Some(pid) = candidate {
            if current_pids.contains(&pid) && matches_target(pid, path, scope) {
                return Ok(pid);
            }
        }
        candidate = current_pids
            .into_iter()
            .find(|pid| !previous_pids.contains(pid) && matches_target(*pid, path, scope));
        if poll < 40 {
            sleep(std::time::Duration::from_millis(50));
        }
    }
    Err(WcError::Other(
        "swaybg failed to become ready: no new process for the requested outputs and wallpaper \
         appeared within 2 seconds"
            .into(),
    ))
}

pub(crate) fn wait_for_swaybg_stopped_with<P, S>(mut probe: P, mut sleep: S) -> Result<(), WcError>
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
                "swaybg still running after stop: pids={pids:?}"
            )));
        }
        sleep(std::time::Duration::from_millis(50));
    }
    unreachable!("the bounded polling loop always returns")
}

impl ProcessIo for SystemBackendRuntime {
    fn command_output(&mut self, command: &mut Command) -> Result<Output, WcError> {
        crate::deadline_command::output(command, APPLY_COMMAND_TIMEOUT)
    }

    fn command_status(
        &mut self,
        command: &mut Command,
    ) -> Result<std::process::ExitStatus, WcError> {
        crate::deadline_command::status(command, LAUNCH_COMMAND_TIMEOUT)
    }

    fn mpvpaper_pids(&mut self) -> Result<Vec<u32>, WcError> {
        crate::mpvpaper::running_pids()
    }

    fn wait_for_mpvpaper_ready(
        &mut self,
        previous_pids: &[u32],
        output: &str,
        path: &str,
    ) -> Result<u32, WcError> {
        wait_for_mpvpaper_ready_with(
            previous_pids,
            output,
            path,
            || self.mpvpaper_pids(),
            crate::mpvpaper::pid_matches_target,
            std::thread::sleep,
        )
    }

    fn mpvpaper_pid_running(&mut self, pid: u32) -> Result<bool, WcError> {
        Ok(self.mpvpaper_pids()?.contains(&pid))
    }

    fn cleanup_failed_mpvpaper_launch(
        &mut self,
        previous_pids: &[u32],
        output: &str,
        path: &str,
    ) -> Result<(), WcError> {
        crate::mpvpaper::stop_pids_started_after(previous_pids, output, path)
    }

    fn swaybg_pids(&mut self) -> Result<Vec<u32>, WcError> {
        crate::swaybg::running_pids()
    }

    fn wait_for_swaybg_ready(
        &mut self,
        previous_pids: &[u32],
        path: &str,
        scope: &crate::target_commands::ExecutionScope,
    ) -> Result<u32, WcError> {
        wait_for_swaybg_ready_with(
            previous_pids,
            path,
            scope,
            || self.swaybg_pids(),
            crate::swaybg::pid_matches_target,
            std::thread::sleep,
        )
    }

    fn swaybg_pid_running(&mut self, pid: u32) -> Result<bool, WcError> {
        Ok(self.swaybg_pids()?.contains(&pid))
    }

    fn cleanup_failed_swaybg_launch(
        &mut self,
        previous_pids: &[u32],
        path: &str,
        scope: &crate::target_commands::ExecutionScope,
    ) -> Result<(), WcError> {
        crate::swaybg::stop_pids_started_after(previous_pids, path, scope)
    }

    fn awww_socket_ready(&mut self) -> AwwwReadiness {
        let path = match awww_socket_path() {
            Ok(p) => p,
            Err(_) => return AwwwReadiness::SocketMissing,
        };
        if !path.exists() {
            return AwwwReadiness::SocketMissing;
        }
        let mut command = Command::new("awww");
        command.arg("query");
        let output = crate::deadline_command::output(&mut command, AWWW_QUERY_TIMEOUT);
        match output {
            Ok(o) if o.status.success() => AwwwReadiness::Ready,
            Ok(o) => AwwwReadiness::SocketPresentQueryFailed {
                stderr: String::from_utf8_lossy(&o.stderr).trim().to_string(),
            },
            Err(e) => AwwwReadiness::SocketPresentQueryFailed {
                stderr: e.to_string(),
            },
        }
    }
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

    fn stop_awww(&mut self) {
        crate::awww::stop_awww();
    }

    fn stop_mpvpaper(&mut self) {
        crate::mpvpaper::stop_mpvpaper();
    }

    fn stop_swaybg(&mut self) {
        crate::swaybg::stop_swaybg();
    }

    fn stop_lwe(&mut self, s: Option<&StorageApi>) {
        crate::linux_wallpaperengine::stop(s);
    }

    fn apply_lwe_to_outputs(
        &mut self,
        s: &StorageApi,
        project: &crate::linux_wallpaperengine::LinuxWallpaperEngineProject,
        outputs: &[String],
    ) -> Result<(), WcError> {
        crate::linux_wallpaperengine::apply_to_outputs(s, project.clone(), outputs)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::time::Duration;

    use wc_core::error::WcError;

    use super::{
        new_mpvpaper_pid_for_target, wait_for_mpvpaper_ready_with, wait_for_mpvpaper_stopped_with,
        wait_for_swaybg_ready_with,
    };
    use crate::target_commands::ExecutionScope;

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
            "eDP-1",
            "/walls/target.mp4",
            || probes.pop_front().expect("unexpected extra PID probe"),
            |pid, output, path| pid == 52 && output == "eDP-1" && path == "/walls/target.mp4",
            |duration| sleeps.push(duration),
        )
        .unwrap();

        assert_eq!(pid, 52);
        assert_eq!(sleeps, vec![Duration::from_millis(50)]);
    }

    #[test]
    fn swaybg_wait_accepts_only_a_new_process_for_the_exact_scope_and_path() {
        let scope = ExecutionScope::named(vec!["eDP-1".into()]).unwrap();
        let mut probes: VecDeque<Result<Vec<u32>, WcError>> =
            VecDeque::from([Ok(vec![41]), Ok(vec![41, 52]), Ok(vec![41, 52])]);
        let mut sleeps = Vec::new();

        let pid = wait_for_swaybg_ready_with(
            &[41],
            "/walls/target.png",
            &scope,
            || probes.pop_front().expect("unexpected extra PID probe"),
            |pid, path, actual_scope| {
                pid == 52 && path == "/walls/target.png" && actual_scope == &scope
            },
            |duration| sleeps.push(duration),
        )
        .unwrap();

        assert_eq!(pid, 52);
        assert_eq!(sleeps, vec![Duration::from_millis(50); 2]);
    }

    #[test]
    fn mpvpaper_wait_times_out_after_two_seconds_of_old_only_probes() {
        let mut probe_count = 0;
        let mut sleeps = Vec::new();

        let error = wait_for_mpvpaper_ready_with(
            &[41],
            "eDP-1",
            "/walls/target.mp4",
            || {
                probe_count += 1;
                Ok(vec![41])
            },
            |_pid, _output, _path| true,
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
            "eDP-1",
            "/walls/target.mp4",
            || {
                probe_count += 1;
                Err(WcError::Other("mpvpaper PID probe failed".into()))
            },
            |_pid, _output, _path| true,
            |duration| sleeps.push(duration),
        )
        .unwrap_err();

        assert_eq!(error.to_string(), "mpvpaper PID probe failed");
        assert_eq!(probe_count, 1);
        assert!(sleeps.is_empty());
    }

    #[test]
    fn mpvpaper_wait_does_not_accept_new_pids_for_another_output_or_path() {
        let mut probe_count = 0;

        let error = wait_for_mpvpaper_ready_with(
            &[41],
            "eDP-1",
            "/walls/target.mp4",
            || {
                probe_count += 1;
                Ok(vec![41, 52, 63])
            },
            |pid, output, path| {
                let observed = match pid {
                    52 => Some(("HDMI-A-1", "/walls/target.mp4")),
                    63 => Some(("eDP-1", "/walls/other.mp4")),
                    _ => None,
                };
                observed.is_some_and(|(observed_output, observed_path)| {
                    observed_output == output && observed_path == path
                })
            },
            |_| {},
        )
        .unwrap_err();

        assert!(error.to_string().contains("requested output and wallpaper"));
        assert_eq!(probe_count, 41);
    }

    #[test]
    fn new_mpvpaper_pid_selects_only_a_matching_pid_absent_from_previous_snapshot() {
        let mut matches_target = |pid, output: &str, path: &str| {
            pid == 52 && output == "eDP-1" && path == "/walls/target.mp4"
        };
        assert_eq!(
            new_mpvpaper_pid_for_target(
                &[41, 52, 63],
                &[41, 63],
                "eDP-1",
                "/walls/target.mp4",
                &mut matches_target,
            ),
            Some(52)
        );
    }

    #[test]
    fn new_mpvpaper_pid_returns_none_without_a_new_matching_pid() {
        let mut matches_target = |_pid, _output: &str, _path: &str| true;
        assert_eq!(
            new_mpvpaper_pid_for_target(
                &[41, 63],
                &[41, 63],
                "eDP-1",
                "/walls/target.mp4",
                &mut matches_target,
            ),
            None
        );
    }
}
