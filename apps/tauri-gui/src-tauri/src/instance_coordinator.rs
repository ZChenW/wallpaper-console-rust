//! Linux/Unix single-instance coordinator using an exclusive lock file and a
//! private Unix-domain socket.
//!
//! # Design
//!
//! - **Primary**: acquires an exclusive lock via [`claim_instance`], returning
//!   an RAII [`InstanceLease`]. A [`PrimarySocket`] is bound (after rejecting
//!   stale symlinks) and chmod'd 0600. After Tauri setup, the accept loop
//!   spawns; each "FOCUS" request invokes a [`FocusCallback`]. `Ok(())` →
//!   "OK\n", `Err(reason)` → "NACK {reason}\n".
//! - **Secondary**: [`claim_instance`] returns [`ClaimResult::Secondary`]. The
//!   caller calls [`try_focus_primary`] which shares one absolute 2s
//!   deadline across connect / write / read. On failure a native error dialog
//!   is shown via [`show_error`] (in addition to stderr); the secondary must
//!   **never** start a GUI.
//! - **CLI paths are completely unaffected** — the coordinator is only used by
//!   the Tauri GUI entry point.
//!
//! # Cleanup
//!
//! [`CoordinatorHandle::shutdown`] stops the accept loop, removes the socket
//! file, and drops the [`InstanceLease`] (releasing the lock).

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use fs2::FileExt;
use wc_core::config::ConfigDir;

const SOCKET_NAME: &str = ".instance.sock";
const LOCK_NAME: &str = ".instance.lock";
const FOCUS_REQUEST: &[u8] = b"FOCUS\n";
const FOCUS_ACK: &[u8] = b"OK\n";
/// Total deadline for secondary connect + write + read (one absolute instant,
/// ≤ 2s total).
const SECONDARY_TOTAL_DEADLINE: Duration = Duration::from_secs(2);

// ── Error dialog seam ──────────────────────────────────────────────────────

/// Signature for showing a desktop-visible error dialog.
pub type ErrorDialogFn = fn(title: &str, message: &str);

static ERROR_DIALOG_FN: OnceLock<Mutex<ErrorDialogFn>> = OnceLock::new();

fn error_dialog() -> ErrorDialogFn {
    let cell = ERROR_DIALOG_FN.get_or_init(|| Mutex::new(default_error_dialog));
    *cell.lock().unwrap_or_else(|p| p.into_inner())
}

/// Replace the error dialog callback for testing. Returns the previous
/// function. Not thread-safe by design — call only in single-threaded setup.
#[allow(dead_code)]
pub fn set_error_dialog_for_test(f: ErrorDialogFn) -> ErrorDialogFn {
    let cell = ERROR_DIALOG_FN.get_or_init(|| Mutex::new(default_error_dialog));
    let mut guard = cell.lock().unwrap_or_else(|p| p.into_inner());
    let prev = *guard;
    *guard = f;
    prev
}

/// Default error dialog: try zenity, fall back to kdialog, always print to
/// stderr. The helper is spawned rather than waited on: a broken desktop
/// portal must never keep the secondary process alive indefinitely.
fn default_error_dialog(title: &str, message: &str) {
    eprintln!("{title}: {message}");
    // Try zenity for a desktop-visible dialog.
    if std::process::Command::new("zenity")
        .args(["--error", "--title", title, "--text", message])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .is_err()
    {
        // Fall back to kdialog.
        let _ = std::process::Command::new("kdialog")
            .args(["--error", message, "--title", title])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
}

/// Show a desktop-visible error to the user. Always writes to stderr AND
/// attempts a native dialog. The dialog function can be replaced for testing
/// via [`set_error_dialog_for_test`].
pub fn show_error(title: &str, message: &str) {
    error_dialog()(title, message);
}

// ── Instance lease (RAII) ──────────────────────────────────────────────────

/// RAII guard that releases the exclusive instance lock on drop.
///
/// Unlike the previous `mem::forget` approach this properly cleans up
/// the file lock when the primary process exits.
#[derive(Debug)]
pub struct InstanceLease {
    file: std::fs::File,
    #[allow(dead_code)]
    lock_path: PathBuf,
}

impl Drop for InstanceLease {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

/// Outcome of [`claim_instance`].
#[derive(Debug)]
pub enum ClaimResult {
    /// This process holds the exclusive lock and must start the GUI.
    Primary(InstanceLease),
    /// Another process already holds the lock — this one is secondary.
    Secondary,
}

impl ClaimResult {
    /// Convenience: extract the [`InstanceLease`], panicking on Secondary.
    pub fn unwrap_primary(self) -> InstanceLease {
        match self {
            ClaimResult::Primary(lease) => lease,
            ClaimResult::Secondary => panic!("expected Primary, got Secondary"),
        }
    }
}

/// Claim the single-instance lock.
///
/// **Must be called before expensive initialisation** — only resolves the
/// config path and creates the private config directory with mode 0700.
///
/// Only `WouldBlock` is treated as "another instance is running". Any
/// other I/O error is fatal (returned as `Err`).
pub fn claim_instance(cd: &ConfigDir) -> Result<ClaimResult, String> {
    // Ensure the private config directory exists with mode 0700.
    std::fs::create_dir_all(&cd.path).map_err(|e| {
        format!(
            "cannot create config directory {}: {}",
            cd.path.display(),
            e
        )
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&cd.path, std::fs::Permissions::from_mode(0o700)).ok();
    }

    let lock_path = lock_path(cd);
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|e| format!("cannot open instance lock {}: {}", lock_path.display(), e))?;

    match file.try_lock_exclusive() {
        Ok(()) => Ok(ClaimResult::Primary(InstanceLease { file, lock_path })),
        Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
            drop(file);
            Ok(ClaimResult::Secondary)
        }
        Err(err) => Err(format!(
            "fatal error acquiring instance lock {}: {}",
            lock_path.display(),
            err
        )),
    }
}

// ── Primary socket ─────────────────────────────────────────────────────────

/// Bound Unix-domain socket for the primary instance.
#[derive(Debug)]
pub struct PrimarySocket {
    listener: UnixListener,
    socket_path: PathBuf,
}

impl PrimarySocket {
    /// Bind the socket, rejecting stale symlinks and non-socket inodes.
    /// Sets mode 0600 so only the owning user can connect.
    pub fn bind(cd: &ConfigDir) -> Result<Self, String> {
        let socket_path = socket_path(cd);

        // Inspect any existing inode at the socket path before removing it.
        if let Ok(meta) = std::fs::symlink_metadata(&socket_path) {
            let ft = meta.file_type();
            if ft.is_symlink() {
                return Err(format!(
                    "refusing to bind instance socket: {} is a symlink",
                    socket_path.display()
                ));
            }
            {
                use std::os::unix::fs::FileTypeExt;
                if !ft.is_socket() {
                    // Not a socket — can't be a live instance. Remove it.
                    let _ = std::fs::remove_file(&socket_path);
                }
            }
        }

        // Remove a stale socket left by a crashed previous instance.
        let _ = std::fs::remove_file(&socket_path);

        let listener = UnixListener::bind(&socket_path).map_err(|e| {
            format!(
                "cannot bind instance socket {}: {}",
                socket_path.display(),
                e
            )
        })?;

        // Restrict to owner-only access.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600))
                .map_err(|e| format!("cannot chmod socket: {}", e))?;
        }

        Ok(Self {
            listener,
            socket_path,
        })
    }

    /// Returns the socket path (useful for logging).
    #[allow(dead_code)]
    pub fn socket_path(&self) -> &std::path::Path {
        &self.socket_path
    }
}

// ── Coordinator handle ─────────────────────────────────────────────────────

/// Handle for the background accept loop.
///
/// Dropping without calling [`shutdown`](Self::shutdown) will leak the
/// thread (the loop runs for the process lifetime).
#[derive(Debug)]
pub struct CoordinatorHandle {
    thread: Option<std::thread::JoinHandle<()>>,
    socket_path: PathBuf,
    stop_flag: Arc<AtomicBool>,
}

impl CoordinatorHandle {
    /// Signal the accept loop to stop and wait for it to exit. Does NOT
    /// remove the socket file — call [`shutdown`](Self::shutdown) for that.
    pub fn stop(&mut self) {
        self.stop_flag.store(true, Ordering::SeqCst);
        // Connect to unblock a blocking accept.
        let _ = UnixStream::connect(&self.socket_path);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }

    /// Stop the accept loop, remove the socket file, and release the
    /// instance lease. Consumes both self and the lease for clean RAII
    /// teardown.
    pub fn shutdown(mut self, _lease: InstanceLease) {
        self.stop();
        let _ = std::fs::remove_file(&self.socket_path);
        // _lease dropped here → lock released.
    }
}

/// Callback signature for focus requests. Return `Ok(())` to ACK or
/// `Err(reason)` to NACK (the reason is sent to the secondary).
pub type FocusCallback = Arc<dyn Fn() -> Result<(), String> + Send + Sync + 'static>;

/// Start the accept loop in a background thread. Returns a
/// [`CoordinatorHandle`] that can stop and join the thread.
///
/// Each incoming connection is handled with a short timeout (100ms read)
/// so a malicious client that opens a connection without sending data
/// cannot consume the handler for the full secondary deadline.
pub fn start_accept_loop(primary: PrimarySocket, on_focus: FocusCallback) -> CoordinatorHandle {
    let socket_path = primary.socket_path.clone();
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop = stop_flag.clone();
    let listener = primary.listener;

    let _ = listener.set_nonblocking(true);

    let handle = std::thread::spawn(move || {
        let poll = Duration::from_millis(100);
        loop {
            if stop.load(Ordering::SeqCst) {
                break;
            }
            match listener.accept() {
                Ok((mut stream, _addr)) => {
                    // Short timeout per connection handler so a malicious
                    // client can't serial-occupy the secondary deadline.
                    let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));
                    let mut buf = [0u8; 64];
                    match stream.read(&mut buf) {
                        Ok(n) if buf[..n].starts_with(b"FOCUS") => match on_focus() {
                            Ok(()) => {
                                let _ = stream.write_all(FOCUS_ACK);
                            }
                            Err(reason) => {
                                let msg = format!("NACK {}\n", reason);
                                let _ = stream.write_all(msg.as_bytes());
                            }
                        },
                        _ => {} // Malformed, empty, or timed out.
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(poll);
                }
                Err(_) => break,
            }
        }
    });

    CoordinatorHandle {
        thread: Some(handle),
        socket_path,
        stop_flag,
    }
}

// ── Secondary focus request ────────────────────────────────────────────────

/// Outcome of a secondary instance's attempt to focus the primary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecondaryOutcome {
    /// Focus request succeeded — the primary acknowledged.
    Ack,
    /// The primary did not respond within the total deadline.
    Timeout,
    /// Connection, I/O, or NACK response.
    Failed(String),
}

/// Try to focus the primary instance as a secondary.
///
/// Connect, write, and read all share **one absolute deadline**
/// (`SECONDARY_TOTAL_DEADLINE` = 2s). Each stage gets the remaining
/// time budget, so a slow connect leaves less time for write/read.
pub fn try_focus_primary(cd: &ConfigDir) -> SecondaryOutcome {
    let deadline = Instant::now() + SECONDARY_TOTAL_DEADLINE;
    let socket_path = socket_path(cd);

    let mut stream = match connect_with_deadline(&socket_path, deadline) {
        Ok(s) => s,
        Err(e) => {
            if e.contains("deadline") || e.contains("timed out") {
                return SecondaryOutcome::Timeout;
            }
            return SecondaryOutcome::Failed(e);
        }
    };

    // Write FOCUS with remaining time budget.
    match deadline.checked_duration_since(Instant::now()) {
        Some(remaining) if !remaining.is_zero() => {
            let _ = stream.set_write_timeout(Some(remaining));
        }
        _ => return SecondaryOutcome::Timeout,
    }
    if let Err(e) = stream.write_all(FOCUS_REQUEST) {
        return classify_io_error(e, "write FOCUS");
    }

    // Read response with remaining time budget.
    match deadline.checked_duration_since(Instant::now()) {
        Some(remaining) if !remaining.is_zero() => {
            let _ = stream.set_read_timeout(Some(remaining));
        }
        _ => return SecondaryOutcome::Timeout,
    }
    let mut buf = [0u8; 128];
    match stream.read(&mut buf) {
        Ok(n) if n >= 2 && &buf[..2] == b"OK" => SecondaryOutcome::Ack,
        Ok(n) => SecondaryOutcome::Failed(format!(
            "unexpected response: {}",
            String::from_utf8_lossy(&buf[..n]).trim()
        )),
        Err(e) => classify_io_error(e, "read ack"),
    }
}

fn classify_io_error(e: std::io::Error, stage: &str) -> SecondaryOutcome {
    match e.kind() {
        std::io::ErrorKind::WouldBlock
        | std::io::ErrorKind::TimedOut
        | std::io::ErrorKind::Interrupted => SecondaryOutcome::Timeout,
        _ => SecondaryOutcome::Failed(format!("{}: {}", stage, e)),
    }
}

fn connect_with_deadline(path: &std::path::Path, deadline: Instant) -> Result<UnixStream, String> {
    let path = path.to_path_buf();
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .unwrap_or(Duration::ZERO);

    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result =
            UnixStream::connect(&path).map_err(|e| format!("connect {}: {}", path.display(), e));
        let _ = tx.send(result);
    });

    match rx.recv_timeout(remaining) {
        Ok(result) => result,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(format!(
            "connect deadline ({:?} total) expired",
            SECONDARY_TOTAL_DEADLINE
        )),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            Err("connect thread panicked".to_string())
        }
    }
}

// ── Path helpers ───────────────────────────────────────────────────────────

fn lock_path(cd: &ConfigDir) -> PathBuf {
    cd.path.join(LOCK_NAME)
}

fn socket_path(cd: &ConfigDir) -> PathBuf {
    cd.path.join(SOCKET_NAME)
}

// ── Cross-process test entry points ────────────────────────────────────────

/// Entry point for cross-process tests. The test binary calls this when
/// the `WC_INSTANCE_TEST` environment variable is set.
///
/// This function is `pub` so integration tests and the test binary can
/// call it. It must NOT be called in production.
#[doc(hidden)]
pub fn run_instance_test_entry_point() -> ! {
    let mode = std::env::var("WC_INSTANCE_TEST").unwrap_or_default();
    let cd_path = std::env::var("WC_CD_PATH").unwrap_or_default();
    let cd = ConfigDir {
        path: PathBuf::from(cd_path),
    };

    match mode.as_str() {
        "hold" => {
            // Hold the lock and signal via ready file.
            let claim = claim_instance(&cd).unwrap();
            let _lease = claim.unwrap_primary();
            // Signal ready by creating the ready file.
            if let Ok(path) = std::env::var("WC_READY_FILE") {
                let _ = std::fs::write(path, "ready");
            }
            // Wait until the release file appears.
            if let Ok(path) = std::env::var("WC_RELEASE_FILE") {
                let deadline = Instant::now() + Duration::from_secs(30);
                while !std::path::Path::new(&path).exists() {
                    if Instant::now() > deadline {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
            std::process::exit(0);
        }
        "claim" => {
            match claim_instance(&cd).unwrap() {
                ClaimResult::Primary(_) => println!("PRIMARY"),
                ClaimResult::Secondary => println!("SECONDARY"),
            }
            std::process::exit(0);
        }
        "serve" => {
            let claim = claim_instance(&cd).unwrap();
            let _lease = claim.unwrap_primary();
            let primary = PrimarySocket::bind(&cd).unwrap();
            let _handle = start_accept_loop(primary, Arc::new(|| Ok(())));
            // Signal ready by creating the ready file.
            if let Ok(path) = std::env::var("WC_READY_FILE") {
                let _ = std::fs::write(path, "ready");
            }
            // Wait until the release file appears.
            if let Ok(path) = std::env::var("WC_RELEASE_FILE") {
                let deadline = Instant::now() + Duration::from_secs(30);
                while !std::path::Path::new(&path).exists() {
                    if Instant::now() > deadline {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
            std::process::exit(0);
        }
        "serve-and-exit" => {
            let claim = claim_instance(&cd).unwrap();
            let lease = claim.unwrap_primary();
            let primary = PrimarySocket::bind(&cd).unwrap();
            let handle = start_accept_loop(primary, Arc::new(|| Ok(())));
            println!("SERVED");
            handle.shutdown(lease);
            std::process::exit(0);
        }
        "focus" => {
            match try_focus_primary(&cd) {
                SecondaryOutcome::Ack => println!("ACK"),
                SecondaryOutcome::Timeout => println!("TIMEOUT"),
                SecondaryOutcome::Failed(msg) => println!("FAILED: {msg}"),
            }
            std::process::exit(0);
        }
        _ => {
            eprintln!("unknown WC_INSTANCE_TEST mode: {mode}");
            std::process::exit(1);
        }
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use wc_config::ConfigDirExt;

    fn config_dir() -> (tempfile::TempDir, ConfigDir) {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        (tmp, cd)
    }

    // ── Claim / lease tests ────────────────────────────────────────────

    #[test]
    fn primary_claims_lock_and_releases_on_drop() {
        let (_tmp, cd) = config_dir();

        let claim = claim_instance(&cd).unwrap();
        let lease = match claim {
            ClaimResult::Primary(lease) => lease,
            ClaimResult::Secondary => panic!("expected Primary"),
        };

        // The lock file should exist.
        assert!(lock_path(&cd).exists());

        // Release the lease.
        drop(lease);

        // A fresh claim should succeed.
        let claim2 = claim_instance(&cd).unwrap();
        assert!(matches!(claim2, ClaimResult::Primary(_)));
    }

    #[test]
    fn secondary_detected_when_lock_is_held_by_another_process() {
        // flock() on Linux is per-open-file-description: two opens to the
        // same lock file get independent file descriptions, so the second
        // try_lock_exclusive sees WouldBlock from the first. The RAII
        // lease correctly detects this as Secondary.
        let (_tmp, cd) = config_dir();
        let _first = claim_instance(&cd).unwrap();
        // Second claim through a separate open() sees the lock → Secondary.
        let second = claim_instance(&cd).unwrap();
        assert!(
            matches!(second, ClaimResult::Secondary),
            "second claim in same process with separate open() must be Secondary"
        );
    }

    #[test]
    fn claim_creates_config_dir_with_0700() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        // Don't call cd.init() — claim must create the dir itself.

        let claim = claim_instance(&cd).unwrap();
        assert!(matches!(claim, ClaimResult::Primary(_)));
        assert!(cd.path.exists());
        assert!(cd.path.is_dir());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = std::fs::metadata(&cd.path).unwrap();
            let mode = meta.permissions().mode();
            assert_eq!(mode & 0o077, 0, "config dir should be 0700, got {mode:o}");
        }

        assert!(lock_path(&cd).exists());
    }

    // ── Socket tests ───────────────────────────────────────────────────

    #[test]
    fn primary_socket_binds_and_sets_0600() {
        let (_tmp, cd) = config_dir();
        let _claim = claim_instance(&cd).unwrap();

        let primary = PrimarySocket::bind(&cd).unwrap();
        assert!(primary.socket_path().exists());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = std::fs::metadata(primary.socket_path()).unwrap();
            let mode = meta.permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "socket mode is {mode:o}");
        }
    }

    #[test]
    fn primary_socket_rejects_symlink() {
        let (_tmp, cd) = config_dir();
        let _claim = claim_instance(&cd).unwrap();

        let sp = socket_path(&cd);
        let target = cd.path.join("some-target");
        std::fs::write(&target, b"not-a-socket").unwrap();
        std::os::unix::fs::symlink(&target, &sp).unwrap();

        let err = PrimarySocket::bind(&cd).unwrap_err();
        assert!(err.contains("symlink"), "should reject symlink, got: {err}");
    }

    #[test]
    fn primary_socket_removes_stale_non_socket_file() {
        let (_tmp, cd) = config_dir();
        let _claim = claim_instance(&cd).unwrap();

        let sp = socket_path(&cd);
        std::fs::write(&sp, b"stale-file-content").unwrap();

        let primary = PrimarySocket::bind(&cd).unwrap();
        assert!(primary.socket_path().exists());

        #[cfg(unix)]
        {
            use std::os::unix::fs::FileTypeExt;
            let meta = std::fs::metadata(primary.socket_path()).unwrap();
            assert!(meta.file_type().is_socket());
        }
    }

    // ── Accept loop / focus tests ──────────────────────────────────────

    #[test]
    fn secondary_handshake_with_primary_socket() {
        let (_tmp, cd) = config_dir();
        let claim = claim_instance(&cd).unwrap();
        let lease = match claim {
            ClaimResult::Primary(l) => l,
            ClaimResult::Secondary => panic!("expected Primary"),
        };
        let primary = PrimarySocket::bind(&cd).unwrap();

        let focused = Arc::new(AtomicBool::new(false));
        let f = focused.clone();
        let handle = start_accept_loop(
            primary,
            Arc::new(move || {
                f.store(true, Ordering::SeqCst);
                Ok(())
            }),
        );

        std::thread::sleep(Duration::from_millis(50));

        let outcome = try_focus_primary(&cd);
        assert_eq!(outcome, SecondaryOutcome::Ack);
        assert!(focused.load(Ordering::SeqCst));

        handle.shutdown(lease);
    }

    #[test]
    fn primary_sends_nack_when_focus_callback_fails() {
        let (_tmp, cd) = config_dir();
        let claim = claim_instance(&cd).unwrap();
        let lease = match claim {
            ClaimResult::Primary(l) => l,
            ClaimResult::Secondary => panic!("expected Primary"),
        };
        let primary = PrimarySocket::bind(&cd).unwrap();

        let handle = start_accept_loop(
            primary,
            Arc::new(|| Err("window not available".to_string())),
        );

        std::thread::sleep(Duration::from_millis(50));

        let outcome = try_focus_primary(&cd);
        match outcome {
            SecondaryOutcome::Failed(msg) => {
                assert!(
                    msg.contains("NACK") || msg.contains("window not available"),
                    "should contain NACK reason, got: {msg}"
                );
            }
            other => panic!("expected Failed with NACK, got {other:?}"),
        }

        handle.shutdown(lease);
    }

    #[test]
    fn secondary_timeout_when_no_primary_listening() {
        let (_tmp, cd) = config_dir();
        let outcome = try_focus_primary(&cd);
        assert!(
            matches!(
                outcome,
                SecondaryOutcome::Failed(_) | SecondaryOutcome::Timeout
            ),
            "must fail or timeout, got {outcome:?}"
        );
    }

    #[test]
    fn primary_accepts_multiple_focus_requests() {
        let (_tmp, cd) = config_dir();
        let claim = claim_instance(&cd).unwrap();
        let lease = match claim {
            ClaimResult::Primary(l) => l,
            ClaimResult::Secondary => panic!("expected Primary"),
        };
        let primary = PrimarySocket::bind(&cd).unwrap();

        let counter = Arc::new(AtomicUsize::new(0));
        let c = counter.clone();
        let handle = start_accept_loop(
            primary,
            Arc::new(move || {
                c.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }),
        );

        std::thread::sleep(Duration::from_millis(50));

        for _ in 0..3 {
            assert_eq!(try_focus_primary(&cd), SecondaryOutcome::Ack);
        }
        assert_eq!(counter.load(Ordering::SeqCst), 3);

        handle.shutdown(lease);
    }

    #[test]
    fn coordinator_shutdown_cleans_socket_and_releases_lock() {
        let (_tmp, cd) = config_dir();
        let claim = claim_instance(&cd).unwrap();
        let lease = claim.unwrap_primary();

        let primary = PrimarySocket::bind(&cd).unwrap();
        let sp = primary.socket_path().to_path_buf();
        assert!(sp.exists());

        let handle = start_accept_loop(primary, Arc::new(|| Ok(())));
        handle.shutdown(lease);

        assert!(!sp.exists(), "socket must be removed on shutdown");
        // Lock should be released — a new claim succeeds.
        assert!(matches!(
            claim_instance(&cd).unwrap(),
            ClaimResult::Primary(_)
        ));
    }

    #[test]
    fn error_dialog_seam_is_testable() {
        use std::sync::Mutex as StdMutex;
        static LAST_ERROR: StdMutex<Option<(String, String)>> = StdMutex::new(None);

        let _prev = set_error_dialog_for_test(|title, message| {
            *LAST_ERROR.lock().unwrap() = Some((title.to_string(), message.to_string()));
        });

        show_error("Test Title", "Test message body.");

        let captured = LAST_ERROR.lock().unwrap().take().unwrap();
        assert_eq!(captured.0, "Test Title");
        assert_eq!(captured.1, "Test message body.");

        // Restore the default (optional in test — the Mutex-based impl
        // means the next test that uses the default will just call
        // default_error_dialog again via the OnceLock init fallback
        // if the Mutex was never initialised... but we already did init.
        // For safety in test isolation, reset:
        let _ = set_error_dialog_for_test(default_error_dialog);
    }

    // ── Cross-process tests ────────────────────────────────────────────

    /// IPC child entry point. When the test binary is spawned with
    /// `WC_INSTANCE_TEST` set, this test detects it and runs the
    /// coordinator child logic instead of a normal test. The parent test
    /// passes `WC_INSTANCE_TEST` to the child process; when the test
    /// binary runs normally, the env var is absent and this is a no-op.
    #[test]
    fn ipc_child_entry() {
        if std::env::var("WC_INSTANCE_TEST").is_ok() {
            run_instance_test_entry_point();
        }
    }

    /// Cross-process: a child process holds the instance lock; a second
    /// child must observe `Secondary`.
    #[test]
    fn cross_process_second_claim_is_secondary() {
        let tmp = tempfile::tempdir().unwrap();
        let cd_path = tmp.path().join("wallpaper-console");
        std::fs::create_dir_all(&cd_path).unwrap();

        let exe = std::env::current_exe().unwrap();
        let cd_path_str = cd_path.to_string_lossy().to_string();
        let ready_file = tmp.path().join("ready");
        let ready_file_str = ready_file.to_string_lossy().to_string();
        let release_file = tmp.path().join("release");
        let release_file_str = release_file.to_string_lossy().to_string();

        // Spawn a child that holds the lock.
        let mut holder = std::process::Command::new(&exe)
            .args([
                "--exact",
                "instance_coordinator::tests::ipc_child_entry",
                "--nocapture",
            ])
            .env("WC_INSTANCE_TEST", "hold")
            .env("WC_CD_PATH", &cd_path_str)
            .env("WC_READY_FILE", &ready_file_str)
            .env("WC_RELEASE_FILE", &release_file_str)
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();

        // Wait for the ready file to appear.
        let deadline = Instant::now() + Duration::from_secs(5);
        while !ready_file.exists() {
            assert!(
                Instant::now() < deadline,
                "child did not become ready in time"
            );
            std::thread::sleep(Duration::from_millis(50));
        }

        // Spawn a second child to try claiming.
        let second_output = std::process::Command::new(&exe)
            .args([
                "--exact",
                "instance_coordinator::tests::ipc_child_entry",
                "--nocapture",
            ])
            .env("WC_INSTANCE_TEST", "claim")
            .env("WC_CD_PATH", &cd_path_str)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output()
            .unwrap();

        let second_stdout = String::from_utf8_lossy(&second_output.stdout);
        assert!(
            second_stdout.contains("SECONDARY"),
            "second process should be Secondary, got: {second_stdout}"
        );

        // Signal the holder to release by creating the release file.
        std::fs::write(&release_file, b"release").unwrap();
        let _ = holder.wait();
    }

    /// Cross-process: primary holds lock and socket; secondary focuses
    /// and receives ACK.
    #[test]
    fn cross_process_secondary_receives_ack() {
        let tmp = tempfile::tempdir().unwrap();
        let cd_path = tmp.path().join("wallpaper-console");
        std::fs::create_dir_all(&cd_path).unwrap();

        let exe = std::env::current_exe().unwrap();
        let cd_path_str = cd_path.to_string_lossy().to_string();
        let ready_file = tmp.path().join("ready");
        let ready_file_str = ready_file.to_string_lossy().to_string();
        let release_file = tmp.path().join("release");
        let release_file_str = release_file.to_string_lossy().to_string();

        // Spawn a child that binds and accepts with auto-ACK.
        let mut primary = std::process::Command::new(&exe)
            .args([
                "--exact",
                "instance_coordinator::tests::ipc_child_entry",
                "--nocapture",
            ])
            .env("WC_INSTANCE_TEST", "serve")
            .env("WC_CD_PATH", &cd_path_str)
            .env("WC_READY_FILE", &ready_file_str)
            .env("WC_RELEASE_FILE", &release_file_str)
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();

        // Wait for the ready file.
        let deadline = Instant::now() + Duration::from_secs(5);
        while !ready_file.exists() {
            assert!(
                Instant::now() < deadline,
                "primary did not become ready in time"
            );
            std::thread::sleep(Duration::from_millis(50));
        }

        // Spawn a secondary that focuses.
        let secondary_output = std::process::Command::new(&exe)
            .args([
                "--exact",
                "instance_coordinator::tests::ipc_child_entry",
                "--nocapture",
            ])
            .env("WC_INSTANCE_TEST", "focus")
            .env("WC_CD_PATH", &cd_path_str)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output()
            .unwrap();

        let secondary_stdout = String::from_utf8_lossy(&secondary_output.stdout);
        assert!(
            secondary_stdout.contains("ACK"),
            "secondary should receive ACK, got: {secondary_stdout}"
        );

        // Clean up.
        std::fs::write(&release_file, b"release").unwrap();
        let _ = primary.wait();
    }

    /// Cross-process: after primary exits, a new claim becomes Primary with
    /// no stale socket.
    #[test]
    fn cross_process_new_primary_after_original_exits() {
        let tmp = tempfile::tempdir().unwrap();
        let cd_path = tmp.path().join("wallpaper-console");
        std::fs::create_dir_all(&cd_path).unwrap();

        let exe = std::env::current_exe().unwrap();
        let cd_path_str = cd_path.to_string_lossy().to_string();

        // Primary holds lock + binds socket, then exits (serve-and-exit
        // prints "SERVED" to stdout and exits on its own).
        let output = std::process::Command::new(&exe)
            .args([
                "--exact",
                "instance_coordinator::tests::ipc_child_entry",
                "--nocapture",
            ])
            .env("WC_INSTANCE_TEST", "serve-and-exit")
            .env("WC_CD_PATH", &cd_path_str)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output()
            .unwrap();

        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("SERVED"),
            "primary should have served, got stdout: {stdout}"
        );

        // Now a new process should be able to claim Primary.
        let new_output = std::process::Command::new(&exe)
            .args([
                "--exact",
                "instance_coordinator::tests::ipc_child_entry",
                "--nocapture",
            ])
            .env("WC_INSTANCE_TEST", "claim")
            .env("WC_CD_PATH", &cd_path_str)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output()
            .unwrap();

        let new_stdout = String::from_utf8_lossy(&new_output.stdout);
        assert!(
            new_stdout.contains("PRIMARY"),
            "new claim should be Primary after original exits, got: {new_stdout}"
        );
    }
}
