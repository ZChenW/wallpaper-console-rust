//! Shared FakeRuntime for wc-backend unit tests.
//!
//! Covers the field union used by `lib` apply tests and `display_executor`
//! tests. wc-app keeps its own FakeRuntime copies (cross-crate `cfg(test)`
//! sharing would require a `testing` feature on the normal dependency).

use std::cell::RefCell;
use std::process::Command;

use wc_core::error::WcError;
use wc_core::types::Backend;
use wc_storage::StorageApi;

use crate::runtime::{AwwwReadiness, BackendRuntime};

pub(crate) struct FakeRuntime {
    pub missing_backend: Option<Backend>,
    pub stop_awww_count: usize,
    pub stop_mpvpaper_count: usize,
    pub stop_lwe_count: usize,
    pub command_output_count: usize,
    pub command_status_count: usize,
    pub clear_awww_state_hint_count: usize,
    pub command_output_success: bool,
    pub command_status_success: bool,
    pub command_output_programs: Vec<String>,
    pub command_status_programs: Vec<String>,
    pub command_output_args: Vec<Vec<String>>,
    pub command_status_args: Vec<Vec<String>>,
    pub awww_readiness_sequence: RefCell<Vec<AwwwReadiness>>,
    /// When false, a non-ready socket fails immediately (display executor style).
    /// When true, attempts daemon autostart + wait (legacy apply tests).
    pub awww_autostart: bool,
    pub running_mpvpaper_pids: Vec<u32>,
    pub mpvpaper_pids_error: Option<String>,
    pub mpvpaper_pids_count: usize,
    pub mpvpaper_readiness_error: Option<String>,
    pub mpvpaper_wait_count: usize,
    pub mpvpaper_wait_previous_pids: Vec<Vec<u32>>,
    pub mpvpaper_wait_targets: Vec<(String, String)>,
    pub mpvpaper_ready_pid: Option<u32>,
    pub dead_mpvpaper_pids: Vec<u32>,
    pub mpvpaper_pid_running_error: Option<String>,
    pub mpvpaper_pid_running_checks: Vec<u32>,
    pub failed_mpvpaper_launch_cleanup_count: usize,
    pub failed_mpvpaper_launch_cleanup_calls: Vec<(Vec<u32>, String, String)>,
    pub lwe_apply_calls: Vec<(String, Vec<String>)>,
    pub lwe_apply_error: Option<String>,
}

impl Default for FakeRuntime {
    fn default() -> Self {
        Self {
            missing_backend: None,
            stop_awww_count: 0,
            stop_mpvpaper_count: 0,
            stop_lwe_count: 0,
            command_output_count: 0,
            command_status_count: 0,
            clear_awww_state_hint_count: 0,
            command_output_success: false,
            command_status_success: false,
            command_output_programs: Vec::new(),
            command_status_programs: Vec::new(),
            command_output_args: Vec::new(),
            command_status_args: Vec::new(),
            awww_readiness_sequence: RefCell::new(Vec::new()),
            awww_autostart: true,
            running_mpvpaper_pids: Vec::new(),
            mpvpaper_pids_error: None,
            mpvpaper_pids_count: 0,
            mpvpaper_readiness_error: None,
            mpvpaper_wait_count: 0,
            mpvpaper_wait_previous_pids: Vec::new(),
            mpvpaper_wait_targets: Vec::new(),
            mpvpaper_ready_pid: None,
            dead_mpvpaper_pids: Vec::new(),
            mpvpaper_pid_running_error: None,
            mpvpaper_pid_running_checks: Vec::new(),
            failed_mpvpaper_launch_cleanup_count: 0,
            failed_mpvpaper_launch_cleanup_calls: Vec::new(),
            lwe_apply_calls: Vec::new(),
            lwe_apply_error: None,
        }
    }
}

impl BackendRuntime for FakeRuntime {
    fn ensure_backend_available(
        &mut self,
        backend: Backend,
        _storage: &StorageApi,
    ) -> Result<(), WcError> {
        if self.missing_backend == Some(backend) {
            Err(WcError::BackendNotFound(backend.as_str().into()))
        } else {
            Ok(())
        }
    }

    fn command_output(&mut self, command: &mut Command) -> Result<std::process::Output, WcError> {
        self.command_output_count += 1;
        self.command_output_programs
            .push(command.get_program().to_string_lossy().to_string());
        self.command_output_args.push(
            command
                .get_args()
                .map(|arg| arg.to_string_lossy().to_string())
                .collect(),
        );
        let program = if self.command_output_success {
            "true"
        } else {
            "false"
        };
        Command::new(program)
            .output()
            .map_err(|e| WcError::Other(format!("fake command failed: {e}")))
    }

    fn command_status(
        &mut self,
        command: &mut Command,
    ) -> Result<std::process::ExitStatus, WcError> {
        self.command_status_count += 1;
        self.command_status_programs
            .push(command.get_program().to_string_lossy().to_string());
        self.command_status_args.push(
            command
                .get_args()
                .map(|arg| arg.to_string_lossy().to_string())
                .collect(),
        );
        let program = if self.command_status_success {
            "true"
        } else {
            "false"
        };
        Command::new(program)
            .status()
            .map_err(|e| WcError::Other(format!("fake command failed: {e}")))
    }

    fn mpvpaper_pids(&mut self) -> Result<Vec<u32>, WcError> {
        self.mpvpaper_pids_count += 1;
        match &self.mpvpaper_pids_error {
            Some(message) => Err(WcError::Other(message.clone())),
            None => Ok(self.running_mpvpaper_pids.clone()),
        }
    }

    fn wait_for_mpvpaper_ready(
        &mut self,
        previous_pids: &[u32],
        output: &str,
        path: &str,
    ) -> Result<u32, WcError> {
        self.mpvpaper_wait_count += 1;
        self.mpvpaper_wait_previous_pids
            .push(previous_pids.to_vec());
        self.mpvpaper_wait_targets
            .push((output.to_string(), path.to_string()));
        match &self.mpvpaper_readiness_error {
            Some(message) => Err(WcError::Other(message.clone())),
            None => Ok(self.mpvpaper_ready_pid.unwrap_or(1)),
        }
    }

    fn mpvpaper_pid_running(&mut self, pid: u32) -> Result<bool, WcError> {
        self.mpvpaper_pid_running_checks.push(pid);
        match &self.mpvpaper_pid_running_error {
            Some(message) => Err(WcError::Other(message.clone())),
            None => Ok(!self.dead_mpvpaper_pids.contains(&pid)),
        }
    }

    fn cleanup_failed_mpvpaper_launch(
        &mut self,
        previous_pids: &[u32],
        output: &str,
        path: &str,
    ) -> Result<(), WcError> {
        self.failed_mpvpaper_launch_cleanup_count += 1;
        self.failed_mpvpaper_launch_cleanup_calls.push((
            previous_pids.to_vec(),
            output.to_string(),
            path.to_string(),
        ));
        self.running_mpvpaper_pids
            .retain(|pid| previous_pids.contains(pid));
        Ok(())
    }

    fn stop_awww(&mut self) {
        self.stop_awww_count += 1;
    }

    fn stop_mpvpaper(&mut self) {
        self.stop_mpvpaper_count += 1;
        self.running_mpvpaper_pids.clear();
    }

    fn stop_lwe(&mut self, _s: Option<&StorageApi>) {
        self.stop_lwe_count += 1;
    }

    fn apply_lwe_to_outputs(
        &mut self,
        _s: &StorageApi,
        project: &crate::linux_wallpaperengine::LinuxWallpaperEngineProject,
        outputs: &[String],
    ) -> Result<(), WcError> {
        self.lwe_apply_calls
            .push((project.project_path.clone(), outputs.to_vec()));
        if let Some(message) = &self.lwe_apply_error {
            return Err(WcError::Other(message.clone()));
        }
        Ok(())
    }

    fn awww_socket_ready(&mut self) -> AwwwReadiness {
        let mut seq = self.awww_readiness_sequence.borrow_mut();
        if seq.len() > 1 {
            seq.remove(0)
        } else if !seq.is_empty() {
            seq[0].clone()
        } else {
            AwwwReadiness::Ready
        }
    }

    fn ensure_awww_daemon_running(&mut self) -> Result<(), WcError> {
        if matches!(self.awww_socket_ready(), AwwwReadiness::Ready) {
            return Ok(());
        }
        if !self.awww_autostart {
            return Err(WcError::Other("awww socket not ready".into()));
        }
        let user = crate::current_process_user();
        if !crate::awww::is_awww_daemon_running(&user) {
            let mut cmd = crate::runtime::build_awww_daemon_command();
            let _ = self.command_status(&mut cmd);
        }
        crate::runtime::wait_for_awww_socket_ready(self, &user)
    }

    fn clear_awww_state_hint(&mut self) {
        self.clear_awww_state_hint_count += 1;
    }
}
