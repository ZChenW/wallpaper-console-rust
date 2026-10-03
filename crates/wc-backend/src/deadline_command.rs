//! Deadline-bound execution for short-lived renderer commands.
//!
//! Output commands continuously drain bounded stdout/stderr buffers. Every
//! child is isolated in a process group so timeout cleanup terminates helpers
//! as well as the direct process, then synchronously reaps the direct child.

use std::io::Read;
use std::process::{Command, ExitStatus, Output, Stdio};
use std::sync::{mpsc, Arc, Mutex};
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
            let drained = wait_for_drainers(&stdout, &stderr);
            kill_process_group(pid);
            if !drained {
                // Descendants can hold the pipes open after the direct child
                // exits. Cleanup gets one more bounded drain, then a snapshot.
                wait_for_drainers(&stdout, &stderr);
            }
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
    let mut poll_interval = Duration::from_millis(1);
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
                std::thread::sleep(poll_interval.min(timeout.saturating_sub(started.elapsed())));
                poll_interval = (poll_interval * 2).min(POLL_INTERVAL);
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

struct Drainer {
    captured: Arc<Mutex<Vec<u8>>>,
    finished: mpsc::Receiver<()>,
}

fn spawn_drainer(mut stream: impl Read + Send + 'static) -> Drainer {
    let captured = Arc::new(Mutex::new(Vec::with_capacity(OUTPUT_CAP)));
    let writer = Arc::clone(&captured);
    let (done, finished) = mpsc::channel();
    std::thread::spawn(move || {
        let mut chunk = [0_u8; 4096];
        loop {
            match stream.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    let mut captured = writer.lock().unwrap_or_else(|error| error.into_inner());
                    let remaining = OUTPUT_CAP.saturating_sub(captured.len());
                    captured.extend_from_slice(&chunk[..read.min(remaining)]);
                }
            }
        }
        let _ = done.send(());
    });
    Drainer { captured, finished }
}

fn wait_for_drainers(stdout: &Drainer, stderr: &Drainer) -> bool {
    // Both streams share the original 10 ms budget; completed drainers return
    // immediately. A disconnected channel also means the thread has ended.
    let deadline = Instant::now() + POLL_INTERVAL;
    for drainer in [stdout, stderr] {
        if matches!(
            drainer
                .finished
                .recv_timeout(deadline.saturating_duration_since(Instant::now())),
            Err(mpsc::RecvTimeoutError::Timeout)
        ) {
            return false;
        }
    }
    true
}

fn snapshot(drainer: &Drainer) -> Vec<u8> {
    drainer
        .captured
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
    fn short_command_has_complete_output_without_fixed_wait() {
        let mut samples = Vec::new();
        for _ in 0..20 {
            let started = Instant::now();
            let result = output(
                Command::new("/bin/sh").args(["-c", "printf hello"]),
                Duration::from_secs(1),
            )
            .unwrap();
            samples.push(started.elapsed());
            assert!(result.status.success());
            assert_eq!(result.stdout, b"hello");
            assert!(result.stderr.is_empty());
        }
        samples.sort();
        let median = (samples[9] + samples[10]) / 2;
        assert!(median < Duration::from_millis(10), "median {median:?}");
    }

    #[test]
    fn final_output_is_complete_on_both_streams_repeatedly() {
        for _ in 0..50 {
            let result = output(
                Command::new("/bin/sh").args([
                    "-c",
                    r"head -c 30000 /dev/zero | tr '\0' x; head -c 30000 /dev/zero | tr '\0' y >&2",
                ]),
                Duration::from_secs(2),
            )
            .unwrap();
            assert!(result.status.success());
            assert_eq!(result.stdout, vec![b'x'; 30000]);
            assert_eq!(result.stderr, vec![b'y'; 30000]);
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn descendant_holding_stdout_is_cleaned_without_blocking() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("child.pid");
        let started = Instant::now();
        let result = output(
            Command::new("/bin/sh")
                .args(["-c", "sleep 30 & echo $! > \"$1\"; echo done", "test"])
                .arg(&pid_file),
            Duration::from_secs(1),
        )
        .unwrap();
        let elapsed = started.elapsed();
        let pid: i32 = std::fs::read_to_string(pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let running = || {
            std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
                !stat
                    .rsplit_once(") ")
                    .is_some_and(|(_, fields)| fields.starts_with('Z') || fields.starts_with('X'))
            })
        };
        let cleanup_started = Instant::now();
        while running() && cleanup_started.elapsed() < Duration::from_millis(100) {
            std::thread::sleep(Duration::from_millis(1));
        }
        let survived = running();
        if survived {
            // SAFETY: cleanup targets only the PID written by this test's child.
            unsafe { libc::kill(pid, libc::SIGKILL) };
        }
        assert!(
            !survived,
            "descendant still running after process-group cleanup"
        );
        assert!(result.status.success());
        assert_eq!(result.stdout, b"done\n");
        assert!(elapsed < Duration::from_millis(100), "elapsed {elapsed:?}");
    }

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
