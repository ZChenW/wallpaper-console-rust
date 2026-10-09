//! Process control abstraction — trait + real implementation + cleanup logic.
//!
//! The trait allows tests to inject a FakeProcessControl so they never
//! touch real OS processes.
//!
//! Diagnostics (pgrep failures) are emitted to stderr. In a Tauri context
//! stderr may not be captured; this is best-effort diagnostic output.

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use std::time::Duration;

pub(crate) const LWE_PROGRAM_NAME: &str = "linux-wallpaperengine";
pub(crate) const MPVPAPER_PROGRAM_NAME: &str = "mpvpaper";
pub(crate) const SWAYBG_PROGRAM_NAME: &str = "swaybg";

/// True when a command-line token looks like an argv0 invocation of `linux-wallpaperengine`.
pub(crate) fn token_is_lwe_program(token: &str) -> bool {
    Path::new(token.trim())
        .file_name()
        .is_some_and(|name| name == LWE_PROGRAM_NAME)
}

/// True when a command-line token looks like an argv0 invocation of `mpvpaper`.
pub(crate) fn token_is_mpvpaper_program(token: &str) -> bool {
    Path::new(token.trim())
        .file_name()
        .is_some_and(|name| name == MPVPAPER_PROGRAM_NAME)
}

pub(crate) fn token_is_swaybg_program(token: &str) -> bool {
    Path::new(token.trim())
        .file_name()
        .is_some_and(|name| name == SWAYBG_PROGRAM_NAME)
}

/// Safer pgrep -f pattern for linux-wallpaperengine (argv0 anchored).
pub(crate) fn lwe_pgrep_pattern() -> &'static str {
    r"^(\S*/)?linux-wallpaperengine( |$)"
}

#[cfg(unix)]
pub(crate) fn read_proc_cmdline_tokens(pid: i32) -> Option<Vec<String>> {
    let raw = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    if raw.is_empty() {
        return None;
    }
    let mut tokens = Vec::new();
    for field in raw.split(|byte| *byte == 0) {
        if field.is_empty() {
            continue;
        }
        let token = std::str::from_utf8(field).ok()?.to_string();
        tokens.push(token);
    }
    Some(tokens)
}

#[cfg(not(unix))]
pub(crate) fn read_proc_cmdline_tokens(_pid: i32) -> Option<Vec<String>> {
    None
}

/// argv0 is `linux-wallpaperengine`, or argv0 is `setsid` and the next token is.
/// Later arguments that merely mention the binary are not a match.
fn cmdline_is_lwe(tokens: &[String]) -> bool {
    let Some(argv0) = tokens.first() else {
        return false;
    };
    if Path::new(argv0.trim())
        .file_name()
        .is_some_and(|name| name == "setsid")
    {
        return tokens
            .get(1)
            .is_some_and(|token| token_is_lwe_program(token));
    }
    token_is_lwe_program(argv0)
}

/// True when `/proc/<pid>` still looks like linux-wallpaperengine (argv0 or exe).
pub(crate) fn pid_looks_like_lwe(pid: i32) -> bool {
    pid_looks_like_lwe_with(pid, read_proc_cmdline_tokens)
}

pub(crate) fn pid_looks_like_lwe_with<F>(pid: i32, read_tokens: F) -> bool
where
    F: Fn(i32) -> Option<Vec<String>>,
{
    if pid <= 0 {
        return false;
    }
    if read_tokens(pid).is_some_and(|tokens| cmdline_is_lwe(&tokens)) {
        return true;
    }
    #[cfg(unix)]
    {
        if let Ok(link) = std::fs::read_link(format!("/proc/{pid}/exe")) {
            if link
                .file_name()
                .is_some_and(|name| name == LWE_PROGRAM_NAME)
            {
                return true;
            }
        }
    }
    false
}

pub(crate) fn pid_looks_like_mpvpaper(pid: i32) -> bool {
    if pid <= 0 {
        return false;
    }
    read_proc_cmdline_tokens(pid)
        .and_then(|tokens| tokens.first().cloned())
        .is_some_and(|argv0| token_is_mpvpaper_program(&argv0))
}

pub(crate) fn pid_looks_like_swaybg(pid: i32) -> bool {
    if pid <= 0 {
        return false;
    }
    read_proc_cmdline_tokens(pid)
        .and_then(|tokens| tokens.first().cloned())
        .is_some_and(|argv0| token_is_swaybg_program(&argv0))
}

pub(crate) fn argv0_matches_name(argv0: &str, name: &str) -> bool {
    Path::new(argv0.trim())
        .file_name()
        .is_some_and(|file| file == name)
}

pub(crate) fn argv0_file_name_is(pid: u32, name: &str) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    if pid <= 0 {
        return false;
    }
    read_proc_cmdline_tokens(pid)
        .and_then(|tokens| tokens.into_iter().next())
        .is_some_and(|argv0| argv0_matches_name(&argv0, name))
}

/// Keep PIDs that are both in this session and whose argv0 is exactly `name`.
pub(crate) fn select_named_session_pids(
    pids: impl IntoIterator<Item = u32>,
    mut in_session: impl FnMut(u32) -> bool,
    mut argv0_matches: impl FnMut(u32) -> bool,
) -> Vec<u32> {
    pids.into_iter()
        .filter(|pid| in_session(*pid) && argv0_matches(*pid))
        .collect()
}

pub(crate) fn exact_name_pgrep_command(user: &crate::ProcessUserScope, name: &str) -> Command {
    let mut cmd = Command::new("pgrep");
    crate::append_pgrep_user_scope(&mut cmd, user);
    cmd.arg("-x").arg(name);
    cmd
}

/// Send SIGTERM then SIGKILL to `pid`'s process group when it still looks like LWE.
pub(crate) fn kill_lwe_process_group(pid: i32) {
    if !pid_looks_like_lwe(pid) {
        return;
    }
    let pgid = format!("-{pid}");
    let _ = Command::new("kill")
        .args(["-TERM", &pgid])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    std::thread::sleep(Duration::from_millis(80));
    let _ = Command::new("kill")
        .args(["-KILL", &pgid])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Send SIGTERM then SIGKILL to a single PID.
pub(crate) fn kill_pid_gracefully(pid: u32) {
    let Some(identity) = ProcessIdentity::read(pid) else {
        return;
    };
    kill_identity_gracefully(&identity);
}

pub(crate) fn kill_pid_matching_argv(pid: u32, expected: &[String]) {
    let Some(identity) = ProcessIdentity::read(pid).filter(|identity| identity.argv == expected)
    else {
        return;
    };
    kill_identity_gracefully(&identity);
}

pub(crate) fn kill_identity_gracefully(identity: &ProcessIdentity) {
    let pid = identity.pid;
    use std::os::fd::{AsRawFd, FromRawFd};
    // A pidfd pins this kernel process, so later PID reuse cannot redirect SIGKILL.
    // SAFETY: pidfd_open accepts a numeric PID and flags=0 and returns an owned fd.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    if fd < 0 {
        return;
    }
    // SAFETY: successful pidfd_open returned a new owned descriptor.
    let handle = unsafe { std::fs::File::from_raw_fd(fd as i32) };
    if ProcessIdentity::read(pid).as_ref() != Some(identity) {
        return;
    }
    let signal = |value: libc::c_int| {
        // SAFETY: the owned pidfd remains valid; null siginfo and flags=0 are supported.
        unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                handle.as_raw_fd(),
                value,
                std::ptr::null::<libc::siginfo_t>(),
                0,
            )
        }
    };
    signal(libc::SIGTERM);
    std::thread::sleep(Duration::from_millis(80));
    if ProcessIdentity::read(pid).as_ref() != Some(identity) {
        return;
    }
    signal(libc::SIGKILL);
    if identity
        .argv
        .first()
        .is_some_and(|p| token_is_mpvpaper_program(p))
    {
        crate::mpvpaper_media::cleanup_socket(&identity.argv);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProcessIdentity {
    uid: u32,
    pid: u32,
    start_ticks: u64,
    argv: Vec<String>,
}
impl ProcessIdentity {
    pub(crate) fn read(pid: u32) -> Option<Self> {
        use std::os::unix::fs::MetadataExt;
        let directory = format!("/proc/{pid}");
        let uid = std::fs::metadata(&directory).ok()?.uid();
        let stat = std::fs::read_to_string(format!("{directory}/stat")).ok()?;
        let mut fields = stat[stat.rfind(')')? + 1..].split_whitespace();
        let start_ticks = fields.nth(19)?.parse().ok()?;
        let argv = read_proc_cmdline_tokens(pid as i32)?;
        // Detect reuse while the uid/stat/argv snapshot itself was assembled.
        if std::fs::read_to_string(format!("{directory}/stat"))
            .ok()?
            .rsplit_once(')')?
            .1
            .split_whitespace()
            .nth(19)?
            .parse::<u64>()
            .ok()?
            != start_ticks
        {
            return None;
        }
        Some(Self {
            uid,
            pid,
            start_ticks,
            argv,
        })
    }
}

#[cfg(unix)]
pub(crate) fn find_lwe_pids_for_current_user() -> Vec<u32> {
    use std::os::unix::fs::MetadataExt;

    let current_uid = match std::fs::metadata("/proc/self") {
        Ok(metadata) => metadata.uid(),
        Err(_) => return Vec::new(),
    };
    let entries = match std::fs::read_dir("/proc") {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };
    let mut pids = Vec::new();
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        if metadata.uid() != current_uid
            || !crate::runtime_observation::process_in_current_session(pid)
        {
            continue;
        }
        if pid_looks_like_lwe(pid as i32) {
            pids.push(pid);
        }
    }
    pids.sort_unstable();
    pids
}

#[cfg(not(unix))]
pub(crate) fn find_lwe_pids_for_current_user() -> Vec<u32> {
    Vec::new()
}

/// Move a long-lived `Child` into a detached background thread that `wait`s
/// so the process is reaped when it exits (or is killed elsewhere).
///
/// Rust's `Child` drop does not wait for a still-running process; without this,
/// an exit leaves a zombie until the GUI process itself exits.
pub(crate) fn detach_and_reap_child(mut child: Child, thread_name: &str) {
    let pid = child.id();
    if let Err(error) = std::thread::Builder::new()
        .name(thread_name.to_string())
        .spawn(move || {
            let _ = child.wait();
        })
    {
        eprintln!("wc-backend: failed to start reaper thread {thread_name} for pid {pid}: {error}");
    }
}

/// Stop and synchronously reap a process group that this process just spawned.
///
/// Unlike [`kill_lwe_process_group`], this does not inspect `/proc`: the
/// caller still owns the exact `Child` handle, so PID identity cannot be
/// stale. This is used when startup succeeded but publishing ownership state
/// failed; returning while the untracked renderer kept running would make a
/// later stop/restore unable to manage it.
pub(crate) fn terminate_spawned_process_group(mut child: Child) {
    let Ok(pid) = i32::try_from(child.id()) else {
        let _ = child.kill();
        let _ = child.wait();
        return;
    };

    #[cfg(unix)]
    {
        // SAFETY: the renderer is launched through `setsid`, so its PID is
        // also the process-group ID. The Child handle proves this is the
        // process we just spawned, rather than a persisted/stale PID.
        unsafe {
            libc::kill(-pid, libc::SIGTERM);
        }
        std::thread::sleep(Duration::from_millis(80));
        // Terminate descendants as well if the group leader already exited.
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
    }

    let _ = child.kill();
    let _ = child.wait();
}

/// Operations that interact with OS processes.
pub(crate) trait ProcessControl {
    /// Return PIDs of processes whose command line matches `pattern` (pgrep -f).
    fn find_processes(&self, pattern: &str) -> Vec<u32>;
    /// Return PIDs whose process name is exactly `name` (pgrep -x), in this session.
    fn find_named_processes(&self, _name: &str) -> Vec<u32> {
        Vec::new()
    }
    /// Return PIDs that look like linux-wallpaperengine for the current user.
    fn find_lwe_processes(&self) -> Vec<u32>;
    /// Return the process group ID of `pid`, if the process still exists.
    fn process_group_of(&self, pid: u32) -> Option<u32>;
    /// Send SIGTERM to `pid`.
    fn term_process(&self, pid: u32);
    /// Send SIGKILL to `pid`.
    fn kill_process(&self, pid: u32);
}

// ---------------------------------------------------------------------------
// RealProcessControl
// ---------------------------------------------------------------------------

/// Real OS process control via pgrep / ps / kill.
pub(crate) struct RealProcessControl {
    user: crate::ProcessUserScope,
}

impl RealProcessControl {
    pub(crate) fn new() -> Self {
        Self {
            user: crate::current_process_user(),
        }
    }
}

impl ProcessControl for RealProcessControl {
    fn find_processes(&self, pattern: &str) -> Vec<u32> {
        let mut cmd = Command::new("pgrep");
        crate::append_pgrep_user_scope(&mut cmd, &self.user);
        let out = cmd
            .arg("-f")
            .arg(pattern)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output();
        match out {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter_map(|l| l.trim().parse::<u32>().ok())
                .collect(),
            Ok(o) => {
                // pgrep ran but exited non-zero — probably not installed or no matches.
                // Exit code 1 = no matches (normal); anything else = infrastructure issue.
                if o.status.code() != Some(1) {
                    eprintln!(
                        "wc-backend: pgrep exited with status {} — stale LWE cleanup skipped",
                        o.status
                    );
                }
                Vec::new()
            }
            Err(e) => {
                eprintln!(
                    "wc-backend: pgrep unavailable ({}) — stale LWE cleanup skipped",
                    e
                );
                Vec::new()
            }
        }
    }

    fn find_named_processes(&self, name: &str) -> Vec<u32> {
        let mut cmd = exact_name_pgrep_command(&self.user, name);
        let listed = match crate::deadline_command::output(&mut cmd, Duration::from_secs(2)) {
            Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout)
                .lines()
                .filter_map(|line| line.trim().parse::<u32>().ok())
                .collect(),
            Ok(output) => {
                if output.status.code() != Some(1) {
                    eprintln!(
                        "wc-backend: pgrep -x {name} exited with status {} — daemon discovery skipped",
                        output.status
                    );
                }
                Vec::new()
            }
            Err(error) => {
                eprintln!(
                    "wc-backend: pgrep -x {name} unavailable ({error}) — daemon discovery skipped"
                );
                Vec::new()
            }
        };
        select_named_session_pids(
            listed,
            crate::runtime_observation::process_in_current_session,
            |pid| argv0_file_name_is(pid, name),
        )
    }

    fn find_lwe_processes(&self) -> Vec<u32> {
        self.find_processes(lwe_pgrep_pattern())
            .into_iter()
            .filter(|&pid| pid_looks_like_lwe(pid as i32))
            .collect()
    }

    fn process_group_of(&self, pid: u32) -> Option<u32> {
        let out = Command::new("ps")
            .args(["-o", "pgid=", "-p", &pid.to_string()])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output();
        match out {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
                .trim()
                .parse::<u32>()
                .ok(),
            _ => None,
        }
    }

    fn term_process(&self, pid: u32) {
        let _ = Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }

    fn kill_process(&self, pid: u32) {
        let _ = Command::new("kill")
            .args(["-KILL", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

// ---------------------------------------------------------------------------
// Cleanup logic
// ---------------------------------------------------------------------------

/// Clean up stale linux-wallpaperengine processes that are NOT related to new_pid.
/// `new_pid` is the setsid parent PID; we must protect both it and any children
/// in its process group (the actual linux-wallpaperengine child).
///
/// Production entry point — uses a cached RealProcessControl.
pub(crate) fn cleanup_stale_lwe_processes_except(new_pid: u32, old_pid: Option<i32>) {
    static REAL_PC: OnceLock<RealProcessControl> = OnceLock::new();
    cleanup_stale_lwe_processes_except_with(
        REAL_PC.get_or_init(RealProcessControl::new),
        new_pid,
        old_pid,
    );
}

/// Same logic, with injectable ProcessControl for testing.
pub(crate) fn cleanup_stale_lwe_processes_except_with(
    pc: &dyn ProcessControl,
    new_pid: u32,
    old_pid: Option<i32>,
) {
    // Since we spawned with setsid, new_pid IS the process group leader
    // and all children inherit this PGID.
    let new_pgid = new_pid;

    let candidates = pc.find_lwe_processes();
    if candidates.is_empty() {
        return;
    }

    // Phase 1: SIGTERM to stale processes.
    let mut any_killed = false;
    for &candidate in &candidates {
        if candidate == new_pid {
            continue;
        }
        if old_pid == Some(candidate as i32) {
            continue;
        }
        match pc.process_group_of(candidate) {
            // In the new process group → protect it.
            Some(pgid) if pgid == new_pgid => continue,
            // Process no longer exists → skip (safe: don't kill unidentifiable).
            None => continue,
            // Different PGID → stale, kill it.
            _ => {}
        }
        pc.term_process(candidate);
        any_killed = true;
    }

    if !any_killed {
        return;
    }

    std::thread::sleep(Duration::from_millis(100));

    // Phase 2: SIGKILL any that didn't respond.
    for &candidate in &candidates {
        if candidate == new_pid {
            continue;
        }
        if old_pid == Some(candidate as i32) {
            continue;
        }
        match pc.process_group_of(candidate) {
            Some(pgid) if pgid == new_pgid => continue,
            None => continue,
            _ => {}
        }
        pc.kill_process(candidate);
    }
}

// ---------------------------------------------------------------------------
// FakeProcessControl for unit tests
// ---------------------------------------------------------------------------
#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::process::Command;

    /// Parse the single-character state field from `/proc/<pid>/stat`.
    #[cfg(unix)]
    fn proc_state(pid: u32) -> Option<char> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        // Format: `pid (comm) state ...` — comm may contain spaces/parens.
        let close = stat.rfind(')')?;
        stat[close + 1..].trim_start().chars().next()
    }

    #[cfg(unix)]
    #[test]
    fn detach_and_reap_child_prevents_zombie() {
        // Use a short-lived but not-instant process. `Child::drop` only
        // `try_wait`s; dropping while still running leaves a zombie after exit.
        let child = Command::new("/bin/sleep")
            .arg("0.2")
            .spawn()
            .expect("/bin/sleep should spawn");
        let pid = child.id();
        // Still running when detached — Drop alone would not wait.
        assert!(
            matches!(proc_state(pid), Some(state) if state != 'Z'),
            "child must still be alive when handed to the reaper"
        );
        detach_and_reap_child(child, "wc-test-reaper");

        // Poll until the process is fully gone (reaped), not stuck as zombie.
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            match proc_state(pid) {
                None => return,
                Some('Z') => {
                    if std::time::Instant::now() >= deadline {
                        panic!("pid {pid} remained a zombie after detach_and_reap_child");
                    }
                }
                Some(state) => {
                    if std::time::Instant::now() >= deadline {
                        panic!("pid {pid} still present with unexpected state {state:?}");
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// In-memory process registry for testing without real OS processes.
    pub(crate) struct FakeProcessControl {
        /// PIDs that would be returned by find_processes.
        pub find_results: RefCell<Vec<u32>>,
        /// Map of PID → process group ID for process_group_of queries.
        pub pgids: RefCell<HashMap<u32, u32>>,
        /// PIDs that received SIGTERM (recorded in order).
        pub termed: RefCell<Vec<u32>>,
        /// PIDs that received SIGKILL (recorded in order).
        pub killed: RefCell<Vec<u32>>,
    }

    impl FakeProcessControl {
        pub fn new() -> Self {
            Self {
                find_results: RefCell::new(Vec::new()),
                pgids: RefCell::new(HashMap::new()),
                termed: RefCell::new(Vec::new()),
                killed: RefCell::new(Vec::new()),
            }
        }

        pub fn set_results(&self, pids: Vec<u32>) {
            *self.find_results.borrow_mut() = pids;
        }

        pub fn set_pgid(&self, pid: u32, pgid: u32) {
            self.pgids.borrow_mut().insert(pid, pgid);
        }

        pub fn termed(&self) -> Vec<u32> {
            self.termed.borrow().clone()
        }

        pub fn killed(&self) -> Vec<u32> {
            self.killed.borrow().clone()
        }
    }

    impl ProcessControl for FakeProcessControl {
        fn find_processes(&self, _pattern: &str) -> Vec<u32> {
            self.find_results.borrow().clone()
        }

        fn find_lwe_processes(&self) -> Vec<u32> {
            self.find_results.borrow().clone()
        }

        fn process_group_of(&self, pid: u32) -> Option<u32> {
            self.pgids.borrow().get(&pid).copied()
        }

        fn term_process(&self, pid: u32) {
            self.termed.borrow_mut().push(pid);
        }

        fn kill_process(&self, pid: u32) {
            self.killed.borrow_mut().push(pid);
        }
    }

    #[test]
    fn skip_child_in_new_process_group() {
        let pc = FakeProcessControl::new();
        let new_pid = 100u32;
        pc.set_results(vec![100, 101, 200]);
        pc.set_pgid(101, 100);
        pc.set_pgid(200, 200);

        super::cleanup_stale_lwe_processes_except_with(&pc, new_pid, None);

        assert!(!pc.termed().contains(&101));
        assert!(!pc.killed().contains(&101));
        assert!(pc.termed().contains(&200));
        assert!(pc.killed().contains(&200));
        assert!(!pc.termed().contains(&100));
    }

    #[test]
    fn kill_stale_pid_not_in_new_group() {
        let pc = FakeProcessControl::new();
        let new_pid = 10u32;
        pc.set_results(vec![10, 50]);
        pc.set_pgid(50, 50);

        super::cleanup_stale_lwe_processes_except_with(&pc, new_pid, None);

        assert_eq!(*pc.termed(), vec![50]);
        assert_eq!(*pc.killed(), vec![50]);
    }

    #[test]
    fn skip_old_pid() {
        let pc = FakeProcessControl::new();
        let new_pid = 10u32;
        let old_pid = Some(50i32);
        pc.set_results(vec![10, 50]);

        super::cleanup_stale_lwe_processes_except_with(&pc, new_pid, old_pid);

        assert!(pc.termed().is_empty());
        assert!(pc.killed().is_empty());
    }

    #[test]
    fn old_pid_none_with_residual_stale() {
        let pc = FakeProcessControl::new();
        let new_pid = 1u32;
        pc.set_results(vec![1, 42]);
        pc.set_pgid(42, 99);

        super::cleanup_stale_lwe_processes_except_with(&pc, new_pid, None);

        assert_eq!(*pc.termed(), vec![42]);
        assert_eq!(*pc.killed(), vec![42]);
    }

    #[test]
    fn process_group_of_none_safe_skip() {
        let pc = FakeProcessControl::new();
        let new_pid = 1u32;
        pc.set_results(vec![1, 99]);

        super::cleanup_stale_lwe_processes_except_with(&pc, new_pid, None);

        assert!(pc.termed().is_empty());
        assert!(pc.killed().is_empty());
    }

    #[test]
    fn empty_find_results_returns_early() {
        let pc = FakeProcessControl::new();

        super::cleanup_stale_lwe_processes_except_with(&pc, 100, None);

        assert!(pc.termed().is_empty());
        assert!(pc.killed().is_empty());
    }

    #[test]
    fn all_candidates_skipped_no_kill_phase() {
        let pc = FakeProcessControl::new();
        let new_pid = 1u32;
        pc.set_results(vec![1, 2, 3]);
        pc.set_pgid(2, 1);
        pc.set_pgid(3, 1);

        super::cleanup_stale_lwe_processes_except_with(&pc, new_pid, None);

        assert!(pc.termed().is_empty());
        assert!(pc.killed().is_empty());
    }

    #[test]
    fn pid_looks_like_lwe_with_injected_cmdline() {
        // PIDs that do not exist, so the /proc exe check cannot accept them.
        let absent = 2_100_000_000;
        assert!(super::pid_looks_like_lwe_with(absent, |pid| match pid {
            2100000000 => Some(vec![
                "setsid".to_string(),
                "/usr/bin/linux-wallpaperengine".to_string(),
            ]),
            _ => None,
        }));
        assert!(super::pid_looks_like_lwe_with(absent, |pid| match pid {
            2100000000 => Some(vec![
                "/usr/bin/setsid".to_string(),
                "linux-wallpaperengine".to_string(),
                "--bg".to_string(),
                "1".to_string(),
            ]),
            _ => None,
        }));
        assert!(super::pid_looks_like_lwe_with(absent, |pid| match pid {
            2100000000 => Some(vec![
                "/usr/local/bin/linux-wallpaperengine".to_string(),
                "--screen-root".to_string(),
                "eDP-1".to_string(),
            ]),
            _ => None,
        }));
        assert!(!super::pid_looks_like_lwe_with(absent, |pid| match pid {
            2100000000 => Some(vec!["bash".to_string()]),
            _ => None,
        }));
        assert!(!super::pid_looks_like_lwe_with(absent, |pid| match pid {
            2100000000 => Some(vec!["rg".to_string(), "linux-wallpaperengine".to_string(),]),
            _ => None,
        }));
        assert!(!super::pid_looks_like_lwe_with(absent, |pid| match pid {
            2100000000 => Some(vec![
                "vim".to_string(),
                "/usr/bin/linux-wallpaperengine".to_string(),
            ]),
            _ => None,
        }));
        assert!(!super::pid_looks_like_lwe_with(absent, |pid| match pid {
            2100000000 => Some(vec![
                "setsid".to_string(),
                "-f".to_string(),
                "linux-wallpaperengine".to_string(),
            ]),
            _ => None,
        }));
        assert!(!super::pid_looks_like_lwe_with(0, |_pid| None));
    }

    #[test]
    fn argv0_match_is_the_exact_file_name() {
        assert!(super::argv0_matches_name("awww-daemon", "awww-daemon"));
        assert!(super::argv0_matches_name(
            "/usr/bin/awww-daemon",
            "awww-daemon"
        ));
        assert!(!super::argv0_matches_name(
            "/usr/bin/awww-daemon-helper",
            "awww-daemon"
        ));
        assert!(!super::argv0_matches_name("rg", "awww-daemon"));
        assert!(!super::argv0_matches_name("awww", "awww-daemon"));
        assert!(!super::argv0_file_name_is(u32::MAX, "awww-daemon"));
    }

    #[test]
    fn select_named_session_pids_keeps_only_in_session_argv0_matches() {
        let selected = super::select_named_session_pids(
            [1, 2, 3, 4],
            |pid| pid != 2,
            |pid| pid == 1 || pid == 4 || pid == 2,
        );
        assert_eq!(selected, vec![1, 4]);
    }
}
