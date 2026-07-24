use std::fmt;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use fs2::FileExt;
use serde::{Deserialize, Serialize};
use wc_scan::{ScanControl, ScanSourceKind, SourceScanOutcome, SourceScanRequest};
use wc_storage::sqlite::{SnapshotPathPresence, ValidatedScanSnapshot};

use crate::scan_worker_snapshot::{
    read_private_worker_request, worker_request_arg, WorkerProtocolError, WorkerSourceKind,
    WORKER_MODE_ARG,
};

pub const WORKER_HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(30);
pub const WORKER_OUTPUT_CAP: usize = 32 * 1024;
const WORKER_PROTOCOL_VERSION: u32 = 1;
const WORKER_LINE_CAP: usize = 8 * 1024;
const WORKER_ARTIFACT_PREFIX: &str = "worker-";
static WORKER_ARTIFACT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub struct WorkerArtifactLease {
    directory: std::path::PathBuf,
    snapshot_path: std::path::PathBuf,
    request_path: std::path::PathBuf,
    lock: File,
}

impl WorkerArtifactLease {
    pub fn create(root: &Path) -> std::io::Result<Self> {
        fs::create_dir_all(root)?;
        let sequence = WORKER_ARTIFACT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let directory = root.join(format!(
            "{WORKER_ARTIFACT_PREFIX}{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&directory)?;
        let lock = File::options()
            .read(true)
            .write(true)
            .create_new(true)
            .open(directory.join("owner.lock"))?;
        lock.lock_exclusive()?;
        Ok(Self {
            snapshot_path: directory.join("scan.sqlite"),
            request_path: directory.join("request.json"),
            directory,
            lock,
        })
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn snapshot_path(&self) -> &Path {
        &self.snapshot_path
    }

    pub fn request_path(&self) -> &Path {
        &self.request_path
    }
}

impl Drop for WorkerArtifactLease {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.lock);
        let _ = fs::remove_dir_all(&self.directory);
    }
}

pub fn cleanup_stale_worker_artifact_dirs(root: &Path) -> std::io::Result<usize> {
    let mut removed = 0;
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_dir()
            || !entry
                .file_name()
                .to_string_lossy()
                .starts_with(WORKER_ARTIFACT_PREFIX)
        {
            continue;
        }
        let directory = entry.path();
        let stale = match File::options()
            .read(true)
            .write(true)
            .open(directory.join("owner.lock"))
        {
            Ok(lock) => lock.try_lock_exclusive().is_ok(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
            Err(error) => return Err(error),
        };
        if stale {
            fs::remove_dir_all(&directory)?;
            removed += 1;
        }
    }
    Ok(removed)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkerRecord {
    pub protocol_version: u32,
    pub epoch: u64,
    pub completed: bool,
    pub entries_visited: usize,
    pub candidates_found: usize,
    pub entries_indexed: usize,
    #[serde(default)]
    pub metadata_reused: usize,
    #[serde(default)]
    pub terminal: Option<WorkerTerminal>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkerTerminal {
    Complete,
    Offline,
    Incomplete,
    Cancelled,
}

#[derive(Debug)]
pub enum ScanWorkerError {
    Protocol(WorkerProtocolError),
    Snapshot(wc_storage::sqlite::ScanSnapshotError),
    Io(std::io::Error),
    ScanFailed {
        category: &'static str,
    },
    Cancelled,
    HeartbeatTimeout {
        after: Duration,
    },
    UncleanWorkerExit {
        code: Option<i32>,
        stdout: String,
        stderr: String,
    },
}

impl fmt::Display for ScanWorkerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Protocol(error) => write!(formatter, "{error}"),
            Self::Snapshot(error) => write!(formatter, "{error}"),
            Self::Io(_) => write!(formatter, "scan worker I/O failed"),
            Self::ScanFailed { category } => write!(formatter, "scan worker failed: {category}"),
            Self::Cancelled => write!(formatter, "scan worker cancelled"),
            Self::HeartbeatTimeout { after } => {
                write!(formatter, "scan worker heartbeat timed out after {after:?}")
            }
            Self::UncleanWorkerExit { code, .. } => {
                write!(
                    formatter,
                    "scan worker exited without a valid completion record ({code:?})"
                )
            }
        }
    }
}

impl std::error::Error for ScanWorkerError {}

impl From<WorkerProtocolError> for ScanWorkerError {
    fn from(value: WorkerProtocolError) -> Self {
        Self::Protocol(value)
    }
}

impl From<wc_storage::sqlite::ScanSnapshotError> for ScanWorkerError {
    fn from(value: wc_storage::sqlite::ScanSnapshotError) -> Self {
        Self::Snapshot(value)
    }
}

impl From<std::io::Error> for ScanWorkerError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

/// Run hidden worker mode before any GUI, CLI, or main-database initialization.
/// Returns `None` for ordinary invocations and an exit code for worker mode.
pub fn try_run_worker_mode(args: &[String]) -> Option<i32> {
    let request_path = match worker_request_arg(args) {
        Ok(Some(path)) => path,
        Ok(None) => return None,
        Err(error) => {
            eprintln!("{error}");
            return Some(2);
        }
    };
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    match run_worker_request(&request_path, &mut lock) {
        Ok(()) => Some(0),
        Err(error) => {
            eprintln!("{error}");
            Some(1)
        }
    }
}

pub fn run_worker_request(
    request_path: &Path,
    output: &mut impl Write,
) -> Result<(), ScanWorkerError> {
    let request = read_private_worker_request(request_path)?;
    let source_kind = match request.source_kind {
        WorkerSourceKind::Directory => "directory",
        WorkerSourceKind::WallpaperEngineWorkshop => "wallpaper_engine_workshop",
    };
    wc_storage::sqlite::create_incomplete_scan_snapshot_for_source(
        &request.snapshot_path,
        request.source_id,
        &request.source_path,
        source_kind,
        request.recursive,
    )?;
    let scan_request = SourceScanRequest {
        path: request.source_path.clone(),
        kind: match request.source_kind {
            WorkerSourceKind::Directory => ScanSourceKind::Directory,
            WorkerSourceKind::WallpaperEngineWorkshop => ScanSourceKind::WallpaperEngineWorkshop,
        },
        recursive: request.recursive,
    };
    let prior_metadata = request
        .prior_metadata
        .into_iter()
        .map(|entry| {
            let key = std::fs::canonicalize(entry.path.as_std_path())
                .unwrap_or_else(|_| entry.path.as_std_path().to_path_buf())
                .to_string_lossy()
                .into_owned();
            (key, entry)
        })
        .collect::<std::collections::HashMap<_, _>>();
    let mut epoch = 0u64;
    let outcome = wc_scan::scan_source_cached(&scan_request, &prior_metadata, |event| {
        epoch = epoch.saturating_add(1);
        let stats = match event {
            wc_scan::SourceScanEvent::SourceStarted { .. } => wc_scan::ScanStats::default(),
            wc_scan::SourceScanEvent::EntryVisited { stats, .. }
            | wc_scan::SourceScanEvent::CandidateFound { stats, .. } => *stats,
        };
        let record = WorkerRecord {
            protocol_version: WORKER_PROTOCOL_VERSION,
            epoch,
            completed: false,
            entries_visited: stats.entries_visited,
            candidates_found: stats.candidates_found,
            entries_indexed: stats.entries_indexed,
            metadata_reused: stats.metadata_reused,
            terminal: None,
        };
        if write_record(output, &record).is_err() {
            ScanControl::Cancel
        } else {
            ScanControl::Continue
        }
    });
    let SourceScanOutcome::Complete(snapshot) = outcome else {
        let (terminal, stats) = match outcome {
            SourceScanOutcome::Offline(failure) => (WorkerTerminal::Offline, failure.stats),
            SourceScanOutcome::Incomplete(failure) => (WorkerTerminal::Incomplete, failure.stats),
            SourceScanOutcome::Cancelled(stats) => (WorkerTerminal::Cancelled, stats),
            SourceScanOutcome::Complete(_) => unreachable!(),
        };
        epoch = epoch.saturating_add(1);
        write_record(
            output,
            &WorkerRecord {
                protocol_version: WORKER_PROTOCOL_VERSION,
                epoch,
                completed: true,
                entries_visited: stats.entries_visited,
                candidates_found: stats.candidates_found,
                entries_indexed: stats.entries_indexed,
                metadata_reused: stats.metadata_reused,
                terminal: Some(terminal),
            },
        )?;
        return Ok(());
    };
    let prior_presence = request
        .prior_paths
        .iter()
        .map(|path| {
            let presence = match path.try_exists() {
                Ok(true) => SnapshotPathPresence::Present,
                Ok(false) => SnapshotPathPresence::Missing,
                Err(_) => SnapshotPathPresence::Unknown,
            };
            (path.to_string_lossy().into_owned(), presence)
        })
        .collect::<Vec<_>>();
    wc_storage::sqlite::complete_scan_snapshot(
        &request.snapshot_path,
        request.source_id,
        snapshot.entries(),
        &prior_presence,
    )?;
    epoch = epoch.saturating_add(1);
    let stats = snapshot.stats();
    write_record(
        output,
        &WorkerRecord {
            protocol_version: WORKER_PROTOCOL_VERSION,
            epoch,
            completed: true,
            entries_visited: stats.entries_visited,
            candidates_found: stats.candidates_found,
            entries_indexed: stats.entries_indexed,
            metadata_reused: stats.metadata_reused,
            terminal: Some(WorkerTerminal::Complete),
        },
    )?;
    Ok(())
}

fn write_record(output: &mut impl Write, record: &WorkerRecord) -> Result<(), ScanWorkerError> {
    serde_json::to_writer(&mut *output, record).map_err(|_| ScanWorkerError::ScanFailed {
        category: "protocol_output",
    })?;
    output.write_all(b"\n")?;
    output.flush()?;
    Ok(())
}

#[derive(Debug)]
struct SupervisedOutput {
    status: ExitStatus,
    stats: wc_scan::ScanStats,
    terminal: WorkerTerminal,
}

pub struct IsolatedScanResult {
    pub snapshot: ValidatedScanSnapshot,
    pub stats: wc_scan::ScanStats,
}

/// Launch the matching executable in hidden worker mode, enforce heartbeat,
/// and accept the SQLite snapshot only after a clean protocol completion.
pub fn run_isolated_scan_worker(
    executable: &Path,
    request_path: &Path,
    expected_source_id: i64,
    snapshot_path: &Path,
) -> Result<IsolatedScanResult, ScanWorkerError> {
    run_isolated_scan_worker_with_cancel(
        executable,
        request_path,
        expected_source_id,
        snapshot_path,
        None,
    )
}

pub fn run_isolated_scan_worker_with_cancel(
    executable: &Path,
    request_path: &Path,
    expected_source_id: i64,
    snapshot_path: &Path,
    cancelled: Option<&AtomicBool>,
) -> Result<IsolatedScanResult, ScanWorkerError> {
    run_isolated_scan_worker_with_cancel_and_progress(
        executable,
        request_path,
        expected_source_id,
        snapshot_path,
        cancelled,
        None,
    )
}

pub fn run_isolated_scan_worker_with_progress(
    executable: &Path,
    request_path: &Path,
    expected_source_id: i64,
    snapshot_path: &Path,
    progress: &mut dyn FnMut(wc_scan::ScanStats) -> ScanControl,
) -> Result<IsolatedScanResult, ScanWorkerError> {
    run_isolated_scan_worker_with_cancel_and_progress(
        executable,
        request_path,
        expected_source_id,
        snapshot_path,
        None,
        Some(progress),
    )
}

fn run_isolated_scan_worker_with_cancel_and_progress(
    executable: &Path,
    request_path: &Path,
    expected_source_id: i64,
    snapshot_path: &Path,
    cancelled: Option<&AtomicBool>,
    progress: Option<&mut dyn FnMut(wc_scan::ScanStats) -> ScanControl>,
) -> Result<IsolatedScanResult, ScanWorkerError> {
    let mut command = Command::new(executable);
    command
        .arg(WORKER_MODE_ARG)
        .arg(request_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn()?;
    let result = supervise_child_with_group(
        &mut child,
        WORKER_HEARTBEAT_TIMEOUT,
        true,
        cancelled,
        progress,
    );
    let _ = std::fs::remove_file(request_path);
    let output = match result {
        Ok(output) => output,
        Err(error) => {
            let _ = std::fs::remove_file(snapshot_path);
            return Err(error);
        }
    };
    if output.terminal != WorkerTerminal::Complete {
        let category = match output.terminal {
            WorkerTerminal::Complete => unreachable!(),
            WorkerTerminal::Offline => "offline",
            WorkerTerminal::Incomplete => "incomplete",
            WorkerTerminal::Cancelled => "cancelled",
        };
        let _ = std::fs::remove_file(snapshot_path);
        return Err(ScanWorkerError::ScanFailed { category });
    }
    match wc_storage::sqlite::validate_scan_snapshot(
        snapshot_path,
        expected_source_id,
        output.status.success(),
    ) {
        Ok(snapshot) => Ok(IsolatedScanResult {
            snapshot,
            stats: output.stats,
        }),
        Err(error) => {
            let _ = std::fs::remove_file(snapshot_path);
            Err(error.into())
        }
    }
}

#[cfg(test)]
fn supervise_child(
    child: &mut Child,
    heartbeat_timeout: Duration,
) -> Result<SupervisedOutput, ScanWorkerError> {
    supervise_child_with_group(child, heartbeat_timeout, false, None, None)
}

#[cfg(test)]
fn supervise_child_with_cancel(
    child: &mut Child,
    heartbeat_timeout: Duration,
    cancelled: &AtomicBool,
) -> Result<SupervisedOutput, ScanWorkerError> {
    supervise_child_with_group(child, heartbeat_timeout, false, Some(cancelled), None)
}

fn supervise_child_with_group(
    child: &mut Child,
    heartbeat_timeout: Duration,
    kill_group: bool,
    cancelled: Option<&AtomicBool>,
    mut progress: Option<&mut dyn FnMut(wc_scan::ScanStats) -> ScanControl>,
) -> Result<SupervisedOutput, ScanWorkerError> {
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ScanWorkerError::Io(std::io::Error::other("worker stdout was not piped")))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| ScanWorkerError::Io(std::io::Error::other("worker stderr was not piped")))?;
    let stdout_capture = Arc::new(Mutex::new(Vec::new()));
    let stderr_capture = Arc::new(Mutex::new(Vec::new()));
    let (record_tx, record_rx) = mpsc::channel();
    let stdout_thread = spawn_stdout_drain(stdout, Arc::clone(&stdout_capture), record_tx);
    let stderr_thread = spawn_bounded_drain(stderr, Arc::clone(&stderr_capture));
    let mut last_heartbeat = Instant::now();
    let mut last_epoch = 0u64;
    let mut protocol_valid = true;
    let mut completion_seen = false;
    let mut latest_stats = wc_scan::ScanStats::default();
    let mut terminal = None;

    let status = loop {
        while let Ok(record) = record_rx.try_recv() {
            match record {
                Ok(record)
                    if record.protocol_version == WORKER_PROTOCOL_VERSION
                        && record.epoch > last_epoch
                        && !completion_seen
                        && (record.completed == record.terminal.is_some()) =>
                {
                    last_epoch = record.epoch;
                    last_heartbeat = Instant::now();
                    completion_seen |= record.completed;
                    latest_stats = wc_scan::ScanStats {
                        entries_visited: record.entries_visited,
                        candidates_found: record.candidates_found,
                        entries_indexed: record.entries_indexed,
                        metadata_reused: record.metadata_reused,
                    };
                    if progress
                        .as_mut()
                        .is_some_and(|callback| callback(latest_stats) == ScanControl::Cancel)
                    {
                        terminate_worker(child, kill_group);
                        let _ = stdout_thread.join();
                        let _ = stderr_thread.join();
                        return Err(ScanWorkerError::Cancelled);
                    }
                    if record.completed {
                        terminal = record.terminal;
                    }
                }
                _ => protocol_valid = false,
            }
        }
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
            terminate_worker(child, kill_group);
            let _ = stdout_thread.join();
            let _ = stderr_thread.join();
            return Err(ScanWorkerError::Cancelled);
        }
        if progress
            .as_mut()
            .is_some_and(|callback| callback(latest_stats) == ScanControl::Cancel)
        {
            terminate_worker(child, kill_group);
            let _ = stdout_thread.join();
            let _ = stderr_thread.join();
            return Err(ScanWorkerError::Cancelled);
        }
        if last_heartbeat.elapsed() >= heartbeat_timeout {
            terminate_worker(child, kill_group);
            let _ = stdout_thread.join();
            let _ = stderr_thread.join();
            return Err(ScanWorkerError::HeartbeatTimeout {
                after: heartbeat_timeout,
            });
        }
        thread::sleep(Duration::from_millis(10));
    };
    let _ = stdout_thread.join();
    let _ = stderr_thread.join();
    while let Ok(record) = record_rx.try_recv() {
        match record {
            Ok(record)
                if record.protocol_version == WORKER_PROTOCOL_VERSION
                    && record.epoch > last_epoch
                    && !completion_seen
                    && (record.completed == record.terminal.is_some()) =>
            {
                last_epoch = record.epoch;
                completion_seen |= record.completed;
                latest_stats = wc_scan::ScanStats {
                    entries_visited: record.entries_visited,
                    candidates_found: record.candidates_found,
                    entries_indexed: record.entries_indexed,
                    metadata_reused: record.metadata_reused,
                };
                if record.completed {
                    terminal = record.terminal;
                }
            }
            _ => protocol_valid = false,
        }
    }
    let stdout = capture_string(&stdout_capture);
    let stderr = capture_string(&stderr_capture);
    if !status.success() || !protocol_valid || !completion_seen || terminal.is_none() {
        return Err(ScanWorkerError::UncleanWorkerExit {
            code: status.code(),
            stdout,
            stderr,
        });
    }
    Ok(SupervisedOutput {
        status,
        stats: latest_stats,
        terminal: terminal.expect("terminal record checked above"),
    })
}

fn terminate_worker(child: &mut Child, kill_group: bool) {
    #[cfg(unix)]
    if kill_group {
        if let Ok(pid) = i32::try_from(child.id()) {
            // SAFETY: production workers are spawned into a process group led
            // by their own PID; a negative PID targets only that group.
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
            }
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn spawn_bounded_drain(
    mut reader: impl Read + Send + 'static,
    capture: Arc<Mutex<Vec<u8>>>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut chunk = [0u8; 4096];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(count) => append_bounded(&capture, &chunk[..count]),
            }
        }
    })
}

fn spawn_stdout_drain(
    mut reader: impl Read + Send + 'static,
    capture: Arc<Mutex<Vec<u8>>>,
    records: mpsc::Sender<Result<WorkerRecord, ()>>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut chunk = [0u8; 4096];
        let mut pending = Vec::new();
        loop {
            let count = match reader.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(count) => count,
            };
            append_bounded(&capture, &chunk[..count]);
            for byte in &chunk[..count] {
                if *byte == b'\n' {
                    let parsed = serde_json::from_slice::<WorkerRecord>(&pending).map_err(|_| ());
                    let _ = records.send(parsed);
                    pending.clear();
                } else if pending.len() < WORKER_LINE_CAP {
                    pending.push(*byte);
                } else {
                    let _ = records.send(Err(()));
                    pending.clear();
                }
            }
        }
        if !pending.is_empty() {
            let parsed = serde_json::from_slice::<WorkerRecord>(&pending).map_err(|_| ());
            let _ = records.send(parsed);
        }
    })
}

fn append_bounded(capture: &Mutex<Vec<u8>>, bytes: &[u8]) {
    let mut capture = capture
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let remaining = WORKER_OUTPUT_CAP.saturating_sub(capture.len());
    capture.extend_from_slice(&bytes[..bytes.len().min(remaining)]);
}

fn capture_string(capture: &Mutex<Vec<u8>>) -> String {
    let capture = capture
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    String::from_utf8_lossy(&capture).into_owned()
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::process::{Command, Stdio};
    use std::sync::atomic::AtomicBool;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::scan_worker_snapshot::{ScanWorkerRequest, WorkerSourceKind};

    #[test]
    fn worker_writes_completed_snapshot_and_path_free_progress() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("secret-source-name");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("wall.jpg"), b"fixture").unwrap();
        let snapshot = temp.path().join("wc-scan-success.sqlite");
        let request_path = temp.path().join("wc-scan-success.request.json");
        let request = ScanWorkerRequest {
            source_id: 12,
            source_path: source.clone(),
            source_kind: WorkerSourceKind::Directory,
            recursive: true,
            snapshot_path: snapshot.clone(),
            prior_paths: Vec::new(),
            prior_metadata: Vec::new(),
        };
        crate::scan_worker_snapshot::write_private_worker_request(&request_path, &request).unwrap();
        let mut output = Cursor::new(Vec::new());

        run_worker_request(&request_path, &mut output).unwrap();

        let output = String::from_utf8(output.into_inner()).unwrap();
        assert!(!output.contains("secret-source-name"));
        let records: Vec<WorkerRecord> = output
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert!(records.windows(2).all(|pair| pair[0].epoch < pair[1].epoch));
        assert!(records.last().unwrap().completed);
        assert!(wc_storage::sqlite::validate_scan_snapshot(&snapshot, 12, true).is_ok());
    }

    #[test]
    fn heartbeat_timeout_kills_and_reaps_child() {
        let temp = tempfile::tempdir().unwrap();
        let pid_file = temp.path().join("pid");
        let mut child = Command::new("sh")
            .args([
                "-c",
                &format!("echo $$ > '{}'; exec sleep 30", pid_file.display()),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let started = Instant::now();
        let error = supervise_child(&mut child, Duration::from_millis(100)).unwrap_err();
        assert!(matches!(error, ScanWorkerError::HeartbeatTimeout { .. }));
        assert!(started.elapsed() < Duration::from_secs(2));
        let pid: i32 = std::fs::read_to_string(pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
    }

    #[test]
    fn stdout_and_stderr_are_drained_concurrently_and_bounded() {
        let mut child = Command::new("sh")
            .args([
                "-c",
                "i=0; while [ $i -lt 4000 ]; do printf 'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx' >&1; printf 'yyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy' >&2; i=$((i+1)); done",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();

        let error = supervise_child(&mut child, Duration::from_secs(5)).unwrap_err();
        let ScanWorkerError::UncleanWorkerExit { stdout, stderr, .. } = error else {
            panic!("expected protocol/exit error after bounded drain");
        };
        assert!(stdout.len() <= WORKER_OUTPUT_CAP);
        assert!(stderr.len() <= WORKER_OUTPUT_CAP);
    }

    #[test]
    fn cancellation_kills_and_reaps_active_worker() {
        let temp = tempfile::tempdir().unwrap();
        let pid_file = temp.path().join("cancel-pid");
        let mut child = Command::new("sh")
            .args([
                "-c",
                &format!("echo $$ > '{}'; exec sleep 30", pid_file.display()),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let wait_deadline = Instant::now() + Duration::from_secs(1);
        while !pid_file.exists() {
            assert!(Instant::now() < wait_deadline, "worker did not publish PID");
            std::thread::sleep(Duration::from_millis(5));
        }
        let cancelled = AtomicBool::new(true);

        let error = supervise_child_with_cancel(&mut child, Duration::from_secs(30), &cancelled)
            .unwrap_err();

        assert!(matches!(error, ScanWorkerError::Cancelled));
        let pid: i32 = std::fs::read_to_string(pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
    }

    #[test]
    fn progress_callback_can_cancel_and_reap_active_worker() {
        let record = serde_json::to_string(&WorkerRecord {
            protocol_version: WORKER_PROTOCOL_VERSION,
            epoch: 1,
            completed: false,
            entries_visited: 7,
            candidates_found: 3,
            entries_indexed: 2,
            metadata_reused: 1,
            terminal: None,
        })
        .unwrap();
        let mut child = Command::new("sh")
            .args(["-c", &format!("printf '%s\\n' '{record}'; exec sleep 30")])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut observed = wc_scan::ScanStats::default();
        let error = supervise_child_with_group(
            &mut child,
            Duration::from_secs(30),
            false,
            None,
            Some(&mut |stats| {
                observed = stats;
                ScanControl::Cancel
            }),
        )
        .unwrap_err();

        assert!(matches!(error, ScanWorkerError::Cancelled));
        assert_eq!(observed, wc_scan::ScanStats::default());
        assert!(child.try_wait().unwrap().is_some());
    }

    #[test]
    fn stale_artifact_cleanup_skips_live_owner_and_removes_unlocked_directory() {
        let temp = tempfile::tempdir().unwrap();
        let active = WorkerArtifactLease::create(temp.path()).unwrap();
        std::fs::write(active.snapshot_path(), b"active").unwrap();
        let stale = temp.path().join("worker-stale");
        std::fs::create_dir(&stale).unwrap();
        std::fs::write(stale.join("owner.lock"), b"").unwrap();
        std::fs::write(stale.join("wc-scan-stale.sqlite"), b"partial").unwrap();

        assert_eq!(cleanup_stale_worker_artifact_dirs(temp.path()).unwrap(), 1);
        assert!(active.directory().exists());
        assert!(!stale.exists());
    }
}
