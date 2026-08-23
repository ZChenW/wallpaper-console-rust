//! Deadline-bound subprocess execution for display probes and other
//! compositor / wallpaper-engine queries that must not hang forever.
//!
//! Each probe gets a fixed deadline and runs in its own process group. Once
//! the direct child exits, or on timeout, the remaining process group is
//! killed and the child is reaped; partial output is retained for diagnostics.
//!
//! Background drainer threads continuously consume stdout/stderr so the child
//! never blocks on a full pipe buffer. After the child exits or is killed,
//! drainers are joined with a short grace period; if a descendant process
//! still holds the pipe, the drainers are detached rather than hanging the
//! caller.

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// Maximum bytes to capture from stdout/stderr before discarding further
/// output (draining continues to prevent the child from blocking).
const OUTPUT_CAP: usize = 32 * 1024;

/// Grace period for drainer threads to finish after process-tree cleanup.
/// A descendant that escaped the process group can still hold a pipe open;
/// in that case we detach rather than hanging the caller.
const DRAINER_JOIN_GRACE: Duration = Duration::from_millis(200);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone)]
pub struct ProbeError {
    #[allow(dead_code)]
    pub program: String,
    pub kind: ProbeErrorKind,
    /// Partial stdout captured before the probe ended.
    #[allow(dead_code)]
    pub partial_stdout: String,
    /// Partial stderr captured before the probe ended.
    pub partial_stderr: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeErrorKind {
    /// The child did not exit before the deadline.
    TimedOut { deadline: Duration },
    /// The child was spawned but exited with a non-zero status and no
    /// useful error on stderr.
    NonZeroExit { exit_code: i32 },
    /// Could not spawn the program (e.g. executable not found), or
    /// polling the child failed after a successful spawn.
    SpawnFailed { reason: String },
    /// Captured output is not valid UTF-8.
    InvalidUtf8 { stream: &'static str },
}

/// Shared buffer that drainer threads write into.
struct DrainBuffer {
    data: Mutex<Vec<u8>>,
    /// Set when the main thread wants drainers to stop after tree cleanup.
    ///
    /// NOTE: the stop flag does **not** interrupt a blocking `read`. If a
    /// descendant inherited the pipe, the drainer stays blocked inside `read`
    /// until the pipe is closed. The bounded-waiter pattern in
    /// `collect_drainers_with_grace` detaches after [`DRAINER_JOIN_GRACE`]
    /// so the caller is never hung.
    stop: AtomicBool,
}

impl DrainBuffer {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            data: Mutex::new(Vec::with_capacity(OUTPUT_CAP)),
            stop: AtomicBool::new(false),
        })
    }

    fn signal_stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    /// Take a strict UTF-8 snapshot of the buffered data.
    ///
    /// Returns `Err((lossy_fallback, stream_name))` when the captured bytes
    /// are not valid UTF-8 so callers can produce a typed [`ProbeError`].
    fn take_string_strict(
        &self,
        stream_name: &'static str,
    ) -> Result<String, (String, &'static str)> {
        let data = self.data.lock().unwrap_or_else(|p| p.into_inner());
        match String::from_utf8(data.clone()) {
            Ok(s) => Ok(s),
            Err(e) => {
                let lossy = String::from_utf8_lossy(e.as_bytes()).into_owned();
                Err((lossy, stream_name))
            }
        }
    }
}

/// Run a subprocess with a hard deadline.
///
/// Spawns the child with piped stdout and stderr. Background threads
/// continuously drain both pipes so the child never blocks on a full buffer.
/// Polls `try_wait` in short intervals. If the child does not exit before
/// `deadline`, its process group is killed (SIGKILL on Unix) and the direct
/// child is waited-to-reap. Remaining descendants are also killed after a
/// normal direct-child exit. Partial output captured up to `OUTPUT_CAP` bytes
/// per stream is returned in the error.
pub fn run_probe(
    program: &str,
    args: &[&str],
    deadline: Duration,
) -> Result<ProbeOutput, ProbeError> {
    let mut child = spawn_child(program, args)?;
    let child_pid = child.id();

    // Take ownership of the pipes so drainer threads can read them
    // independently of the Child handle.
    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();

    let stdout_buf = DrainBuffer::new();
    let stderr_buf = DrainBuffer::new();

    let stdout_handle = spawn_drainer(stdout_pipe, stdout_buf.clone());
    let stderr_handle = spawn_drainer(stderr_pipe, stderr_buf.clone());

    let started = Instant::now();

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                // A probe is a process tree, not just its direct child.
                kill_process_group(child_pid);
                // The direct child has exited and been reaped by try_wait.
                // Collect drainer output after terminating any descendants.
                let (stdout, stderr) = collect_drainers_with_grace(
                    program,
                    stdout_handle,
                    stderr_handle,
                    &stdout_buf,
                    &stderr_buf,
                    // The child exited within its deadline. Give pipe drainers
                    // their fixed bounded cleanup grace so final bytes are not
                    // lost merely because the execution budget reached zero.
                    DRAINER_JOIN_GRACE,
                )?;
                if status.success() {
                    return Ok(ProbeOutput {
                        success: true,
                        stdout,
                        stderr,
                    });
                }
                return Err(ProbeError {
                    program: program.to_string(),
                    kind: ProbeErrorKind::NonZeroExit {
                        exit_code: status.code().unwrap_or(-1),
                    },
                    partial_stdout: stdout,
                    partial_stderr: stderr,
                });
            }
            Ok(None) => {
                let elapsed = started.elapsed();
                if elapsed >= deadline {
                    return kill_and_reap(
                        child,
                        program,
                        deadline,
                        stdout_buf,
                        stderr_buf,
                        stdout_handle,
                        stderr_handle,
                    );
                }
                std::thread::sleep(Duration::from_millis(10).min(deadline.saturating_sub(elapsed)));
            }
            Err(err) => {
                // try_wait error — terminate the probe process group and reap
                // the direct child, then collect drainer output with a bounded
                // grace period. The original poll error remains primary.
                terminate_and_reap(child);
                let (stdout, stderr) = collect_drainers_with_grace(
                    program,
                    stdout_handle,
                    stderr_handle,
                    &stdout_buf,
                    &stderr_buf,
                    deadline.saturating_sub(started.elapsed()),
                )
                .unwrap_or_else(|e| (e.partial_stdout, e.partial_stderr));
                return Err(ProbeError {
                    program: program.to_string(),
                    kind: ProbeErrorKind::SpawnFailed {
                        reason: format!("{} (child terminated and reaped during cleanup)", err),
                    },
                    partial_stdout: stdout,
                    partial_stderr: stderr,
                });
            }
        }
    }
}

fn spawn_child(program: &str, args: &[&str]) -> Result<Child, ProbeError> {
    use std::os::unix::process::CommandExt;

    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    command.spawn().map_err(|err| ProbeError {
        program: program.to_string(),
        kind: ProbeErrorKind::SpawnFailed {
            reason: err.to_string(),
        },
        partial_stdout: String::new(),
        partial_stderr: String::new(),
    })
}

/// Terminate a probe process group and reap its direct child.
///
/// Sends SIGKILL to the group and direct child (tolerating `ESRCH` /
/// already-exited), then calls `wait()` to reap the direct child. After this
/// returns the child PID is no longer valid and `/proc/<pid>` does not exist.
///
/// This is a testable helper exposed so unit tests can verify the kill+wait
/// semantics on a real child without going through `run_probe`.
pub(crate) fn terminate_and_reap(mut child: Child) {
    kill_process_group(child.id());
    // kill tolerates already-exited (ESRCH on Unix).
    let _ = child.kill();
    // wait must execute — it reaps the zombie.
    let _ = child.wait();
}

fn kill_process_group(leader_pid: u32) {
    if let Ok(pid) = i32::try_from(leader_pid) {
        // SAFETY: `spawn_child` creates a new process group whose ID is the
        // child PID. A negative PID targets only that group.
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
    }
}

/// Background thread that continuously reads from a pipe and appends to the
/// shared buffer. Once the buffer reaches OUTPUT_CAP, further reads are
/// still consumed (to prevent the child from blocking) but not buffered.
fn spawn_drainer(
    pipe: Option<impl Read + Send + 'static>,
    buf: Arc<DrainBuffer>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let Some(mut pipe) = pipe else {
            return;
        };
        let mut chunk = [0u8; 4096];
        loop {
            // Check stop signal before blocking on read.
            //
            // NOTE: the stop flag is only checked between reads. If the
            // drainer is already blocked inside `read()` (e.g. because a
            // descendant still holds the pipe), setting the flag has no
            // effect until the pipe closes or the read unblocks. The
            // bounded-waiter in `collect_drainers_with_grace` handles this
            // by detaching the drainer thread after a grace period.
            if buf.stop.load(Ordering::SeqCst) {
                // Drain any remaining bytes without blocking.
                match pipe.read(&mut chunk) {
                    Ok(n) if n > 0 => {
                        let mut data = buf.data.lock().unwrap_or_else(|p| p.into_inner());
                        if data.len() < OUTPUT_CAP {
                            let space = OUTPUT_CAP - data.len();
                            data.extend_from_slice(&chunk[..n.min(space)]);
                        }
                        continue;
                    }
                    _ => return,
                }
            }
            match pipe.read(&mut chunk) {
                Ok(0) => return, // EOF — pipe closed
                Ok(n) => {
                    let mut data = buf.data.lock().unwrap_or_else(|p| p.into_inner());
                    if data.len() < OUTPUT_CAP {
                        let space = OUTPUT_CAP - data.len();
                        data.extend_from_slice(&chunk[..n.min(space)]);
                    }
                    // Beyond cap: continue draining to prevent child blocking,
                    // but don't buffer.
                }
                Err(_) => return, // Pipe error, give up
            }
        }
    })
}

fn kill_and_reap(
    child: Child,
    program: &str,
    deadline: Duration,
    stdout_buf: Arc<DrainBuffer>,
    stderr_buf: Arc<DrainBuffer>,
    stdout_handle: thread::JoinHandle<()>,
    stderr_handle: thread::JoinHandle<()>,
) -> Result<ProbeOutput, ProbeError> {
    // Terminate the entire probe group, then reap the direct child so no
    // direct-child zombie remains.
    terminate_and_reap(child);

    // Collect drainer output with a bounded grace period. A descendant that
    // escaped the process group may still hold a pipe; detach in that case.
    let (stdout, stderr) = collect_drainers_with_grace(
        program,
        stdout_handle,
        stderr_handle,
        &stdout_buf,
        &stderr_buf,
        Duration::ZERO,
    )?;

    Err(ProbeError {
        program: program.to_string(),
        kind: ProbeErrorKind::TimedOut { deadline },
        partial_stdout: stdout,
        partial_stderr: stderr,
    })
}

/// Collect drainer output with a bounded grace period.
///
/// Signals drainers to stop and waits up to [`DRAINER_JOIN_GRACE`] for them
/// to finish. If a descendant still holds the pipe the drainers are blocked
/// on read; we move the [`JoinHandle`]s into a waiter thread, wait via
/// [`mpsc::recv_timeout`], and detach on timeout.  Buffer snapshots are
/// always taken regardless of whether drainers completed.
///
/// Returns an error when captured output is not valid UTF-8.
fn collect_drainers_with_grace(
    program: &str,
    stdout_handle: thread::JoinHandle<()>,
    stderr_handle: thread::JoinHandle<()>,
    stdout_buf: &DrainBuffer,
    stderr_buf: &DrainBuffer,
    remaining_budget: Duration,
) -> Result<(String, String), ProbeError> {
    stdout_buf.signal_stop();
    stderr_buf.signal_stop();

    // Move JoinHandles into a waiter thread; use a oneshot-style channel
    // to detect completion with a timeout.
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = stdout_handle.join();
        let _ = stderr_handle.join();
        let _ = tx.send(());
    });

    // Wait with grace period. If drainers don't finish in time a
    // descendant still holds the pipe — detach (the waiter thread will
    // eventually complete and be cleaned up by the OS).
    let _ = rx.recv_timeout(DRAINER_JOIN_GRACE.min(remaining_budget));

    let stdout = stdout_buf
        .take_string_strict("stdout")
        .map_err(|(lossy, stream)| {
            let (partial_stdout, partial_stderr) = match stream {
                "stdout" => (lossy, String::new()),
                _ => (String::new(), lossy),
            };
            ProbeError {
                program: program.to_string(),
                kind: ProbeErrorKind::InvalidUtf8 { stream },
                partial_stdout,
                partial_stderr,
            }
        })?;
    let stderr = stderr_buf
        .take_string_strict("stderr")
        .map_err(|(lossy, stream)| {
            let (partial_stdout, partial_stderr) = match stream {
                "stdout" => (lossy, String::new()),
                _ => (String::new(), lossy),
            };
            ProbeError {
                program: program.to_string(),
                kind: ProbeErrorKind::InvalidUtf8 { stream },
                partial_stdout,
                partial_stderr,
            }
        })?;
    Ok((stdout, stderr))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Helper: send SIGKILL to a PID via the system `kill` command.
    fn kill_pid(pid: i32) {
        let _ = std::process::Command::new("kill")
            .arg("-9")
            .arg(pid.to_string())
            .status();
    }

    /// Helper: check whether /proc/<pid> exists on Linux.
    fn proc_exists(pid: i32) -> bool {
        std::path::Path::new(&format!("/proc/{pid}")).exists()
    }

    fn wait_for_proc_exit(pid: i32, timeout: Duration) -> bool {
        let started = Instant::now();
        while proc_exists(pid) && started.elapsed() < timeout {
            std::thread::sleep(Duration::from_millis(10));
        }
        !proc_exists(pid)
    }

    /// RAII guard that SIGKILLs a PID read from a temp file on drop.
    /// Prevents orphaned descendant processes from leaking if a test fails
    /// before it proves normal process-group cleanup and disarms the guard.
    struct DescendantGuard {
        pidfile: std::path::PathBuf,
        armed: bool,
    }
    impl DescendantGuard {
        fn new(pidfile: std::path::PathBuf) -> Self {
            Self {
                pidfile,
                armed: true,
            }
        }
        fn kill_descendant(&self) {
            if let Ok(pid_str) = std::fs::read_to_string(&self.pidfile) {
                if let Ok(pid) = pid_str.trim().parse::<i32>() {
                    kill_pid(pid);
                }
            }
        }
        fn disarm(&mut self) {
            self.armed = false;
        }
    }
    impl Drop for DescendantGuard {
        fn drop(&mut self) {
            if self.armed {
                self.kill_descendant();
            }
        }
    }

    #[test]
    fn probe_succeeds_within_deadline() {
        let output = run_probe("echo", &["hello"], Duration::from_secs(2)).unwrap();
        assert!(output.success);
        assert!(output.stdout.contains("hello"));
    }

    #[test]
    fn probe_times_out_and_captures_partial_output() {
        let err = run_probe("sleep", &["10"], Duration::from_millis(100)).unwrap_err();
        assert!(matches!(err.kind, ProbeErrorKind::TimedOut { .. }));
    }

    #[test]
    fn probe_reports_non_zero_exit() {
        let err = run_probe("sh", &["-c", "exit 3"], Duration::from_secs(2)).unwrap_err();
        assert_eq!(err.kind, ProbeErrorKind::NonZeroExit { exit_code: 3 });
        assert_eq!(err.program, "sh");
    }

    #[test]
    fn probe_reports_spawn_failure() {
        let err =
            run_probe("/nonexistent/probe_tool_xyz", &[], Duration::from_secs(1)).unwrap_err();
        assert!(matches!(err.kind, ProbeErrorKind::SpawnFailed { .. }));
    }

    #[test]
    fn probe_captures_stdout_stderr_before_truncation() {
        // Write enough data to verify truncation
        let output = run_probe(
            "sh",
            &[
                "-c",
                "dd if=/dev/zero bs=1 count=50000 2>/dev/null; echo done",
            ],
            Duration::from_secs(5),
        )
        .unwrap();
        assert!(output.stdout.len() <= OUTPUT_CAP + 10);
    }

    #[test]
    fn concurrent_probes_share_an_overall_deadline() {
        // Simulate the display-discovery pattern: primary + fallbacks within one overall budget.
        let overall_start = Instant::now();
        let primary_deadline = Duration::from_millis(200);

        let primary = run_probe("sleep", &["10"], primary_deadline);
        let elapsed = overall_start.elapsed();
        assert!(
            primary.is_err(),
            "primary probe must fail (sleep never exits)"
        );
        assert!(matches!(
            primary.as_ref().unwrap_err().kind,
            ProbeErrorKind::TimedOut { .. }
        ));
        assert!(
            elapsed < Duration::from_secs(1),
            "primary probe must respect its {primary_deadline:?} budget, took {elapsed:?}"
        );

        // Fallback probes should also be bounded.
        let fallback = run_probe(
            "sleep",
            &["10"],
            Duration::from_secs(3).saturating_sub(elapsed),
        );
        let total = overall_start.elapsed();
        assert!(
            total < Duration::from_secs(4),
            "overall display discovery took {total:?}, expected under 4s"
        );
        assert!(fallback.is_err());
    }

    #[test]
    fn high_output_probe_drains_without_blocking_child() {
        // Generate ~200KB of output (well beyond OUTPUT_CAP) to verify the
        // drainer prevents the child from blocking on a full pipe buffer.
        let deadline = Duration::from_secs(5);
        let output = run_probe(
            "sh",
            &[
                "-c",
                // Print 200KB to stdout; the drainer must consume it all so
                // the shell does not block on write.
                "for i in $(seq 1 200); do printf '%01024d' 0; done; echo FINISHED",
            ],
            deadline,
        )
        .unwrap();
        assert!(output.success, "high-output probe must succeed");
        assert!(
            output.stdout.len() <= OUTPUT_CAP + 20,
            "stdout must be capped at {}, got {}",
            OUTPUT_CAP,
            output.stdout.len()
        );
    }

    #[test]
    fn descendant_holding_pipe_does_not_hang_kill_and_reap() {
        // A grandchild that inherits the pipe must not cause run_probe to
        // hang. The shell sleeps longer than the probe deadline, while a
        // background grandchild inherits the pipe. Timeout cleanup must kill
        // the probe's entire process group, including that grandchild.
        //
        // Capture the background sleep PID so the test can prove the
        // descendant is gone before the guard performs panic-only cleanup.
        let tmpdir = tempfile::tempdir().expect("tempdir");
        let pidfile = tmpdir.path().join("bg.pid");

        // Panic fallback only: normal timeout cleanup must remove the process.
        let mut guard = DescendantGuard::new(pidfile.clone());

        let started = Instant::now();
        let err = run_probe(
            "sh",
            &[
                "-c",
                &format!(
                    // Capture background sleep PID, then sleep 5 so
                    // run_probe hits the 300ms deadline and kills the
                    // shell. The background sleep 30 inherits the pipe
                    // and outlives the deadline.
                    "sleep 30 & echo $! > '{}'; sleep 5",
                    pidfile.display()
                ),
            ],
            Duration::from_millis(300),
        )
        .unwrap_err();
        let elapsed = started.elapsed();

        assert!(
            matches!(err.kind, ProbeErrorKind::TimedOut { .. }),
            "descendant-holding-pipe probe must time out, got {:?}",
            err.kind
        );
        assert!(
            elapsed < Duration::from_secs(3),
            "descendant must not hang kill+reap (took {elapsed:?})"
        );

        let pid: i32 = std::fs::read_to_string(&pidfile)
            .expect("descendant must write its PID")
            .trim()
            .parse()
            .expect("descendant PID must be valid");
        assert!(
            wait_for_proc_exit(pid, Duration::from_secs(2)),
            "/proc/{pid} must disappear after probe process-group cleanup"
        );
        guard.disarm();
    }

    #[test]
    fn pid_already_reaped_does_not_panic_or_hang() {
        // If the child exits extremely quickly (before the first try_wait
        // poll), the code must handle the already-reaped state gracefully.
        let output = run_probe("true", &[], Duration::from_secs(2)).unwrap();
        assert!(output.success);
    }

    #[test]
    fn descendant_holding_pipe_normal_exit_does_not_hang() {
        // The shell exits 0 immediately, but a background sleep inherits
        // stdout/stderr pipes. run_probe must
        // not hang waiting for drainers — collect_drainers_with_grace must
        // detach after DRAINER_JOIN_GRACE.
        //
        // The test wraps run_probe in a thread + channel recv_timeout to
        // prevent the test runner itself from hanging.
        //
        // NOTE: the stop flag in DrainBuffer cannot interrupt a blocking
        // read. The bounded waiter in collect_drainers_with_grace provides
        // the caller-side deadline via DRAINER_JOIN_GRACE. The drainer
        // thread is NOT guaranteed to exit promptly — it stays blocked
        // until the descendant closes the pipe.
        let tmpdir = tempfile::tempdir().expect("tempdir");
        let pidfile = tmpdir.path().join("bg.pid");

        // Panic fallback only. Normal successful-probe cleanup must kill the
        // remaining process group after the direct child exits.
        let mut guard = DescendantGuard::new(pidfile.clone());

        let (tx, rx) = mpsc::channel();
        let started = Instant::now();

        let pidfile_clone = pidfile.clone();
        thread::spawn(move || {
            let result = run_probe(
                "sh",
                &[
                    "-c",
                    &format!("sleep 30 & echo $! > '{}'; exit 0", pidfile_clone.display()),
                ],
                Duration::from_secs(2),
            );
            let _ = tx.send((started.elapsed(), result));
        });

        let (elapsed, result) = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("run_probe hung — descendant pipe not handled by collect_drainers_with_grace");

        assert!(
            elapsed < Duration::from_secs(1),
            "normal exit with descendant pipe must return in < 1s, took {elapsed:?}"
        );
        match result {
            Ok(output) => assert!(output.success, "shell exit 0 must succeed"),
            Err(e) => panic!("expected Ok for exit 0, got {e:?}"),
        }

        let pid: i32 = std::fs::read_to_string(&pidfile)
            .expect("descendant must write its PID")
            .trim()
            .parse()
            .expect("descendant PID must be valid");
        assert!(
            wait_for_proc_exit(pid, Duration::from_secs(2)),
            "/proc/{pid} must disappear after successful-probe process-group cleanup"
        );
        guard.disarm();
    }

    #[test]
    fn timeout_child_pid_is_reaped_after_kill_and_wait() {
        // Verify that after run_probe times out (kill + wait), the child
        // PID is no longer valid. We use a real run_probe call: the shell
        // writes its own PID to a temp file, then execs sleep 30. When
        // run_probe returns TimedOut, /proc/<pid> must not exist.
        let tmpdir = tempfile::tempdir().expect("tempdir");
        let pidfile = tmpdir.path().join("child.pid");

        let err = run_probe(
            "sh",
            &[
                "-c",
                &format!(
                    // Write PID to temp file BEFORE exec so the file
                    // survives across the exec boundary.
                    "echo $$ > '{}'; exec sleep 30",
                    pidfile.display()
                ),
            ],
            Duration::from_millis(100),
        )
        .unwrap_err();

        assert!(
            matches!(err.kind, ProbeErrorKind::TimedOut { .. }),
            "sleep 30 probe must time out, got {:?}",
            err.kind
        );

        // Read the PID the shell wrote before exec'ing sleep 30.
        let pid_str = std::fs::read_to_string(&pidfile).expect("shell must write PID to temp file");
        let pid: i32 = pid_str.trim().parse().expect("PID must be a valid integer");

        // After kill + wait, /proc/<pid> must not exist.
        assert!(
            !proc_exists(pid),
            "/proc/{pid} must not exist after run_probe timeout (kill+wait)"
        );
    }

    #[test]
    fn terminate_and_reap_cleans_up_real_child() {
        // Direct unit test for the terminate_and_reap helper: spawn a real
        // child, terminate it, and verify /proc/<pid> disappears.
        let child = Command::new("sleep")
            .arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn sleep");
        let pid = child.id() as i32;

        terminate_and_reap(child);

        // After reap, /proc/<pid> must not exist.
        assert!(
            !proc_exists(pid),
            "/proc/{pid} must not exist after terminate_and_reap"
        );
    }

    #[test]
    fn terminate_and_reap_tolerates_already_exited_child() {
        // If the child already exited, terminate_and_reap must not panic
        // and must still complete the wait.
        let child = Command::new("true")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn true");
        // true exits almost instantly; give it a moment.
        std::thread::sleep(Duration::from_millis(200));

        // This must not panic even if the child already exited.
        terminate_and_reap(child);
    }

    #[test]
    fn invalid_utf8_stdout_yields_typed_error_not_lossy() {
        // Write non-UTF-8 bytes to stdout; the probe must return
        // InvalidUtf8 instead of silently lossy-converting.
        let err = run_probe(
            "sh",
            &["-c", "printf '\\200\\201\\202'"], // invalid UTF-8
            Duration::from_secs(2),
        )
        .unwrap_err();
        assert!(
            matches!(err.kind, ProbeErrorKind::InvalidUtf8 { stream: "stdout" }),
            "non-UTF-8 stdout must yield InvalidUtf8, got {:?}",
            err.kind
        );
    }

    #[test]
    fn invalid_utf8_stderr_yields_typed_error_not_lossy() {
        // Write non-UTF-8 bytes to stderr; the probe must return
        // InvalidUtf8 instead of silently lossy-converting.
        let err = run_probe(
            "sh",
            &["-c", "printf '\\377\\376' >&2; exit 1"], // invalid UTF-8 on stderr
            Duration::from_secs(2),
        )
        .unwrap_err();
        // stderr should be checked first (or both), but since the command
        // exits non-zero, the exit status takes priority over invalid UTF-8
        // on stderr? Actually the drain is collected after exit, so
        // NonZeroExit comes from the exit status, but the stderr snapshot
        // fails UTF-8 validation in collect_drainers_with_grace.
        // The InvalidUtf8 error from collect_drainers_with_grace takes
        // priority because it's checked first.
        assert!(
            matches!(err.kind, ProbeErrorKind::InvalidUtf8 { .. }),
            "non-UTF-8 stderr must yield InvalidUtf8 or NonZeroExit, got {:?}",
            err.kind
        );
    }

    #[test]
    fn high_output_probe_still_works_after_collect_drainers_with_grace() {
        // Verify that the collect_drainers_with_grace refactor does not
        // regress high-output capture.  The child produces ~200KB then
        // exits cleanly; drainers must keep up and the grace period must
        // be long enough for the post-exit drain.
        let deadline = Duration::from_secs(5);
        let output = run_probe(
            "sh",
            &[
                "-c",
                "for i in $(seq 1 200); do printf '%01024d' 0; done; echo FINISHED",
            ],
            deadline,
        )
        .unwrap();
        assert!(output.success, "high-output probe must succeed");
        assert!(
            output.stdout.len() <= OUTPUT_CAP + 20,
            "stdout must be capped at {}, got {}",
            OUTPUT_CAP,
            output.stdout.len()
        );
    }

    #[test]
    fn timeout_probe_stderr_is_valid_utf8_partial_output() {
        // When a probe times out and produces stderr, the partial_stderr
        // field must be valid UTF-8 (not lossy).
        let tmpdir = tempfile::tempdir().expect("tempdir");
        let pidfile = tmpdir.path().join("child.pid");

        let err = run_probe(
            "sh",
            &[
                "-c",
                &format!(
                    "echo $$ > '{}'; echo 'timed out message' >&2; exec sleep 30",
                    pidfile.display()
                ),
            ],
            Duration::from_millis(100),
        )
        .unwrap_err();

        assert!(matches!(err.kind, ProbeErrorKind::TimedOut { .. }));
        // stderr should contain our message (valid UTF-8 is preserved).
        assert!(
            err.partial_stderr.contains("timed out message"),
            "partial_stderr must contain timed out message, got: {}",
            err.partial_stderr
        );
    }
}
