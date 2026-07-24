//! Deadline-bound execution for short-lived renderer commands.
//!
//! Output commands continuously drain bounded stdout/stderr buffers. Every
//! child is isolated in a process group so timeout cleanup terminates helpers
//! as well as the direct process, then synchronously reaps the direct child.

use std::io::Read;
use std::process::{Command, ExitStatus, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use wc_core::error::WcError;

const OUTPUT_CAP: usize = 32 * 1024;
const POLL_INTERVAL: Duration = Duration::from_millis(10);

pub(crate) fn output(command: &mut Command, timeout: Duration) -> Result<Output, WcError> {
    prepare_process_group(command);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let label = command.get_program().to_string_lossy().into_owned();
    let mut child = command
        .spawn()
        .map_err(|error| WcError::Other(format!("{label} failed to start: {error}")))?;
    let pid = child.id();
    let stdout = spawn_drainer(child.stdout.take().expect("stdout must be piped"));
    let stderr = spawn_drainer(child.stderr.take().expect("stderr must be piped"));
    let status = wait_until_deadline(&mut child, pid, &label, timeout);
    match status {
        Ok(status) => {
            kill_process_group(pid);
            std::thread::sleep(POLL_INTERVAL);
            Ok(Output {
                status,
                stdout: snapshot(&stdout),
                stderr: snapshot(&stderr),
            })
        }
        Err(error) => Err(error),
    }
}

pub(crate) fn status(command: &mut Command, timeout: Duration) -> Result<ExitStatus, WcError> {
    prepare_process_group(command);
    // Status-only callers do not consume output. Null streams prevent inherited
    // or high-volume renderer output from blocking the launcher.
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let label = command.get_program().to_string_lossy().into_owned();
    let mut child = command
        .spawn()
        .map_err(|error| WcError::Other(format!("{label} failed to start: {error}")))?;
    let pid = child.id();
    wait_until_deadline(&mut child, pid, &label, timeout)
}

fn wait_until_deadline(
    child: &mut std::process::Child,
    pid: u32,
    label: &str,
    timeout: Duration,
) -> Result<ExitStatus, WcError> {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if started.elapsed() >= timeout => {
                kill_process_group(pid);
                let _ = child.kill();
                let _ = child.wait();
                return Err(WcError::Other(format!(
                    "{label} timed out after {} ms; the command process group was terminated",
                    timeout.as_millis()
                )));
            }
            Ok(None) => {
                std::thread::sleep(POLL_INTERVAL.min(timeout.saturating_sub(started.elapsed())))
            }
            Err(error) => {
                kill_process_group(pid);
                let _ = child.kill();
                let _ = child.wait();
                return Err(WcError::Other(format!(
                    "failed while waiting for {label}: {error}; the command process group was terminated"
                )));
            }
        }
    }
}

fn spawn_drainer(mut stream: impl Read + Send + 'static) -> Arc<Mutex<Vec<u8>>> {
    let captured = Arc::new(Mutex::new(Vec::with_capacity(OUTPUT_CAP)));
    let writer = Arc::clone(&captured);
    std::thread::spawn(move || {
        let mut chunk = [0_u8; 4096];
        loop {
            match stream.read(&mut chunk) {
                Ok(0) | Err(_) => return,
                Ok(read) => {
                    let mut captured = writer.lock().unwrap_or_else(|error| error.into_inner());
                    let remaining = OUTPUT_CAP.saturating_sub(captured.len());
                    captured.extend_from_slice(&chunk[..read.min(remaining)]);
                }
            }
        }
    });
    captured
}

fn snapshot(captured: &Arc<Mutex<Vec<u8>>>) -> Vec<u8> {
    captured
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

#[cfg(unix)]
fn prepare_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(not(unix))]
fn prepare_process_group(_command: &mut Command) {}

#[cfg(unix)]
fn kill_process_group(pid: u32) {
    let Ok(pid) = i32::try_from(pid) else {
        return;
    };
    // SAFETY: `prepare_process_group` makes the child PID its process-group ID.
    unsafe {
        libc::kill(-pid, libc::SIGKILL);
    }
}

#[cfg(not(unix))]
fn kill_process_group(_pid: u32) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn large_output_is_drained_without_deadlock_and_is_bounded() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "yes x | head -c 1048576"]);
        let output = output(&mut command, Duration::from_secs(2)).unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout.len(), OUTPUT_CAP);
    }

    #[test]
    fn output_timeout_is_actionable_and_prompt() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "sleep 30"]);
        let started = Instant::now();
        let error = output(&mut command, Duration::from_millis(100)).unwrap_err();
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(error.to_string().contains("timed out after 100 ms"));
        assert!(error.to_string().contains("process group was terminated"));
    }

    #[test]
    fn status_timeout_is_actionable_and_prompt() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "sleep 30"]);
        let error = status(&mut command, Duration::from_millis(100)).unwrap_err();
        assert!(error.to_string().contains("timed out after 100 ms"));
    }

    #[cfg(unix)]
    #[test]
    fn escaped_descendant_holding_output_pipe_cannot_block_return() {
        let temp = tempfile::tempdir().unwrap();
        let pid_file = temp.path().join("escaped.pid");
        let script = format!(
            "setsid sh -c 'echo $$ > \"{}\"; sleep 30' & exit 0",
            pid_file.display()
        );
        let mut command = Command::new("/bin/sh");
        command.args(["-c", &script]);
        let started = Instant::now();
        let result = output(&mut command, Duration::from_secs(1));
        assert!(result.unwrap().status.success());
        assert!(started.elapsed() < Duration::from_secs(2));

        if let Ok(raw) = std::fs::read_to_string(pid_file) {
            if let Ok(pid) = raw.trim().parse::<i32>() {
                // SAFETY: test cleanup targets the PID written by its child.
                unsafe { libc::kill(pid, libc::SIGKILL) };
            }
        }
    }
}
