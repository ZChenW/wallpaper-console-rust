//! Deadline-bound dimension probes with bounded output and process-group cleanup.
use std::{
    collections::HashSet,
    io::Read,
    process::{Command, Stdio},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};
const RESOLUTION_PROBE_TIMEOUT: Duration = Duration::from_secs(2);
const RESOLUTION_PROBE_OUTPUT_CAP: usize = 32 * 1024;
const RESOLUTION_PROBE_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Detect resolution using the right tool per file type:
///   - image/gif → identify (ImageMagick)
///   - video    → ffprobe (never runs identify on video)
pub(super) fn detect_resolution(path: &str, ftype: wc_core::types::FileType) -> String {
    match ftype {
        wc_core::types::FileType::Image | wc_core::types::FileType::Gif => {
            let first_frame = format!("{path}[0]");
            let args = ["-format", "%wx%h", "--", first_frame.as_str()];
            if let Some(output) = resolution_probe("identify", &args, path) {
                let s = String::from_utf8_lossy(&output).to_string();
                if !s.is_empty() && s.contains('x') {
                    return s;
                }
            }
        }
        wc_core::types::FileType::Video => {
            let args = [
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=width,height",
                "-of",
                "csv=p=0",
                path,
            ];
            if let Some(output) = resolution_probe("ffprobe", &args, path) {
                let s = String::from_utf8_lossy(&output).trim().to_string();
                let parts: Vec<&str> = s.split(',').collect();
                if parts.len() >= 2 {
                    return format!("{}x{}", parts[0], parts[1]);
                }
            }
        }
        wc_core::types::FileType::WeScene
        | wc_core::types::FileType::WeWeb
        | wc_core::types::FileType::WeApplication => {}
    }
    "?x?".to_string()
}

fn resolution_probe(program: &str, args: &[&str], path: &str) -> Option<Vec<u8>> {
    match run_command_with_deadline(program, args, RESOLUTION_PROBE_TIMEOUT) {
        Ok(output) if output.success => Some(output.stdout),
        Ok(output) => {
            let detail = String::from_utf8_lossy(&output.stderr);
            let detail = if detail.trim().is_empty() {
                String::new()
            } else {
                format!(": {}", detail.trim())
            };
            report_resolution_probe_issue(
                program,
                "unsuccessful",
                format!(
                    "could not inspect {path:?}: command exited unsuccessfully{detail}; \
                     further {program} exit failures will be suppressed"
                ),
            );
            None
        }
        Err(DeadlineCommandError::TimedOut) => {
            report_resolution_probe_issue(
                program,
                "timeout",
                format!(
                    "timed out after {} ms while inspecting {path:?}; verify the file and the \
                     installed {program} executable; further {program} timeouts will be suppressed",
                    RESOLUTION_PROBE_TIMEOUT.as_millis()
                ),
            );
            None
        }
        Err(DeadlineCommandError::Spawn(message)) => {
            report_resolution_probe_issue(
                program,
                "spawn",
                format!(
                    "could not start {program} while inspecting {path:?}: {message}; install \
                     {program} or configure a working executable; further spawn failures will be \
                     suppressed"
                ),
            );
            None
        }
        Err(DeadlineCommandError::Wait(message)) => {
            report_resolution_probe_issue(
                program,
                "wait",
                format!(
                    "{program} failed while inspecting {path:?}: {message}; verify the file and \
                     retry; further {program} wait failures will be suppressed"
                ),
            );
            None
        }
    }
}

fn report_resolution_probe_issue(program: &str, kind: &str, message: String) {
    static REPORTED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    if should_report_resolution_probe_issue(REPORTED.get_or_init(Mutex::default), program, kind) {
        eprintln!("wc-scan: {message}");
    }
}

fn should_report_resolution_probe_issue(
    reported: &Mutex<HashSet<String>>,
    program: &str,
    kind: &str,
) -> bool {
    reported
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(format!("{program}:{kind}"))
}

#[derive(Debug)]
struct DeadlineCommandOutput {
    success: bool,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DeadlineCommandError {
    Spawn(String),
    Wait(String),
    TimedOut,
}

/// Execute a metadata probe within a hard deadline.
///
/// Probes run in their own process group so a timeout also terminates helper
/// descendants. Stdout/stderr are continuously drained into bounded buffers,
/// preventing both pipe deadlocks and unbounded diagnostic memory.
fn run_command_with_deadline(
    program: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<DeadlineCommandOutput, DeadlineCommandError> {
    #[cfg(unix)]
    use std::os::unix::process::CommandExt;

    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    command.process_group(0);

    let mut child = command
        .spawn()
        .map_err(|error| DeadlineCommandError::Spawn(error.to_string()))?;
    let child_pid = child.id();
    let stdout = child
        .stdout
        .take()
        .map(spawn_bounded_output_drainer)
        .expect("piped stdout must be available");
    let stderr = child
        .stderr
        .take()
        .map(spawn_bounded_output_drainer)
        .expect("piped stderr must be available");
    let started = Instant::now();

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                kill_spawned_process_group(child_pid);
                std::thread::sleep(RESOLUTION_PROBE_POLL_INTERVAL);
                return Ok(DeadlineCommandOutput {
                    success: status.success(),
                    stdout: snapshot_drainer(&stdout),
                    stderr: snapshot_drainer(&stderr),
                });
            }
            Ok(None) if started.elapsed() >= timeout => {
                kill_spawned_process_group(child_pid);
                let _ = child.kill();
                let _ = child.wait();
                return Err(DeadlineCommandError::TimedOut);
            }
            Ok(None) => {
                std::thread::sleep(
                    RESOLUTION_PROBE_POLL_INTERVAL.min(timeout.saturating_sub(started.elapsed())),
                );
            }
            Err(error) => {
                kill_spawned_process_group(child_pid);
                let _ = child.kill();
                let _ = child.wait();
                return Err(DeadlineCommandError::Wait(error.to_string()));
            }
        }
    }
}

fn spawn_bounded_output_drainer(mut stream: impl Read + Send + 'static) -> Arc<Mutex<Vec<u8>>> {
    let captured = Arc::new(Mutex::new(Vec::with_capacity(RESOLUTION_PROBE_OUTPUT_CAP)));
    let writer = Arc::clone(&captured);
    std::thread::spawn(move || {
        let mut chunk = [0_u8; 4096];
        loop {
            match stream.read(&mut chunk) {
                Ok(0) | Err(_) => return,
                Ok(read) => {
                    let mut captured = writer.lock().unwrap_or_else(|error| error.into_inner());
                    let remaining = RESOLUTION_PROBE_OUTPUT_CAP.saturating_sub(captured.len());
                    captured.extend_from_slice(&chunk[..read.min(remaining)]);
                }
            }
        }
    });
    captured
}

fn snapshot_drainer(captured: &Arc<Mutex<Vec<u8>>>) -> Vec<u8> {
    captured
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

#[cfg(unix)]
fn kill_spawned_process_group(pid: u32) {
    let Ok(pid) = i32::try_from(pid) else {
        return;
    };
    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }
    // SAFETY: `run_command_with_deadline` creates a fresh process group whose
    // ID is the direct child PID. A negative PID targets only that group.
    unsafe {
        kill(-pid, 9);
    }
}

#[cfg(not(unix))]
fn kill_spawned_process_group(_pid: u32) {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    #[test]
    fn resolution_probe_runner_returns_small_successful_output() {
        let output = run_command_with_deadline(
            "/bin/sh",
            &["-c", "printf 1920x1080"],
            Duration::from_secs(1),
        )
        .unwrap();

        assert!(output.success);
        assert_eq!(output.stdout, b"1920x1080");
        assert!(output.stderr.is_empty());
    }

    #[test]
    fn resolution_probe_issues_are_rate_limited_per_tool_and_reason() {
        let reported = Mutex::new(HashSet::new());

        assert!(should_report_resolution_probe_issue(
            &reported, "identify", "spawn"
        ));
        assert!(!should_report_resolution_probe_issue(
            &reported, "identify", "spawn"
        ));
        assert!(should_report_resolution_probe_issue(
            &reported, "identify", "timeout"
        ));
        assert!(should_report_resolution_probe_issue(
            &reported, "ffprobe", "spawn"
        ));
    }

    #[cfg(unix)]
    #[test]
    fn resolution_probe_timeout_kills_and_reaps_its_process_group() {
        let temp = tempfile::tempdir().unwrap();
        let pid_file = temp.path().join("probe.pid");
        let pid_file_arg = pid_file.to_string_lossy().to_string();
        let started = Instant::now();

        let error = run_command_with_deadline(
            "/bin/sh",
            &[
                "-c",
                "echo $$ > \"$1\"; sleep 30",
                "resolution-probe",
                &pid_file_arg,
            ],
            Duration::from_millis(300),
        )
        .unwrap_err();

        assert_eq!(error, DeadlineCommandError::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(2));
        let pid = std::fs::read_to_string(pid_file).expect("probe must publish its PID");
        assert!(
            !Path::new("/proc").join(pid.trim()).exists(),
            "timed-out probe child must be synchronously reaped"
        );
    }

    #[cfg(unix)]
    #[test]
    fn resolution_probe_does_not_join_an_escaped_pipe_holder() {
        let temp = tempfile::tempdir().unwrap();
        let pid_file = temp.path().join("escaped.pid");
        let script = format!(
            "setsid sh -c 'echo $$ > \"{}\"; sleep 30' & exit 0",
            pid_file.display()
        );
        let started = Instant::now();

        let output =
            run_command_with_deadline("/bin/sh", &["-c", &script], Duration::from_secs(1)).unwrap();

        assert!(output.success);
        assert!(started.elapsed() < Duration::from_secs(2));
        if let Ok(raw) = std::fs::read_to_string(pid_file) {
            if let Ok(pid) = raw.trim().parse::<i32>() {
                unsafe extern "C" {
                    fn kill(pid: i32, signal: i32) -> i32;
                }
                // SAFETY: test cleanup targets the PID written by its child.
                unsafe {
                    kill(pid, 9);
                }
            }
        }
    }
}
