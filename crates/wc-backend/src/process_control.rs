//! Process control abstraction — trait + real implementation + cleanup logic.
//!
//! The trait allows tests to inject a FakeProcessControl so they never
//! touch real OS processes.
//!
//! Diagnostics (pgrep failures) are emitted to stderr. In a Tauri context
//! stderr may not be captured; this is best-effort diagnostic output.

use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::Duration;

/// Operations that interact with OS processes.
pub(crate) trait ProcessControl {
    /// Return PIDs of processes whose command line matches `pattern` (pgrep -f).
    fn find_processes(&self, pattern: &str) -> Vec<u32>;
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
    user: String,
}

impl RealProcessControl {
    pub(crate) fn new() -> Self {
        Self {
            user: crate::whoami(),
        }
    }
}

impl ProcessControl for RealProcessControl {
    fn find_processes(&self, pattern: &str) -> Vec<u32> {
        let out = Command::new("pgrep")
            .args(["-u", &self.user, "-f", pattern])
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

    let candidates = pc.find_processes(r"(^|/)linux-wallpaperengine\b");
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
}
