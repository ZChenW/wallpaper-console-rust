//! Session-scoped niri output recovery. Only previously observed assignments
//! are armed; saved preferences alone never resurrect a stopped wallpaper.
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use fs2::FileExt;
use wc_backend::apply_stage::NoopReporter;
use wc_backend::runtime::SystemBackendRuntime;
use wc_backend::runtime_observation::{observe_runtime_wallpapers, RuntimeObservationStatus};
use wc_core::error::WcError;
use wc_core::types::Backend;
use wc_storage::sqlite::{DisplayStateRow, DisplayStateTarget};
use wc_storage::StorageApi;

use crate::apply_execution::ApplyExecutionTarget;
use crate::display_apply::DisplayApplyRuntimeOpts;
use crate::{AppError, AppService, ApplyRequest, ApplyRequestKind, DisplayTarget};

thread_local! { static HELD: RefCell<HashSet<PathBuf>> = RefCell::new(HashSet::new()); }

/// Serialize watcher, CLI and GUI mutations; nested AppService calls reuse the
/// current thread's lock. Read-only observations do not acquire this lock.
pub struct RendererMutationGuard {
    file: Option<File>,
    path: PathBuf,
}
impl RendererMutationGuard {
    pub fn acquire(storage: &StorageApi) -> Result<Self, WcError> {
        let path = storage.cd.path.join("renderer-mutation.lock");
        if HELD.with(|held| held.borrow().contains(&path)) {
            return Ok(Self { file: None, path });
        }
        let file = lock_file(&path).map_err(io_error)?;
        file.lock_exclusive().map_err(io_error)?;
        HELD.with(|held| held.borrow_mut().insert(path.clone()));
        Ok(Self {
            file: Some(file),
            path,
        })
    }
}
impl Drop for RendererMutationGuard {
    fn drop(&mut self) {
        if self.file.is_some() {
            HELD.with(|held| held.borrow_mut().remove(&self.path));
        }
    }
}
fn io_error(error: std::io::Error) -> WcError {
    WcError::Other(error.to_string())
}
fn lock_file(path: &Path) -> std::io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
}

/// Call under the mutation lock before an explicit Stop, even if Stop fails.
pub fn disarm(storage: &StorageApi) -> Result<(), WcError> {
    let epoch = format!("{:?}", std::time::SystemTime::now());
    std::fs::write(storage.cd.path.join("output-recovery-stop"), epoch).map_err(io_error)
}

pub(crate) fn ensure_watcher(storage: &StorageApi) {
    if std::env::var_os("NIRI_SOCKET").is_none()
        || std::env::var_os("WCR_DISABLE_OUTPUT_WATCH").is_some()
    {
        return;
    }
    let Ok(lock) = lock_file(&storage.cd.path.join("output-recovery.lock")) else {
        return;
    };
    if lock.try_lock_exclusive().is_err() {
        return;
    }
    drop(lock);
    let executable = std::env::current_exe().ok();
    let cli = executable
        .as_ref()
        .and_then(|exe| {
            let parent = exe.parent()?;
            [
                parent.join("wallpaper-console-rust"),
                parent.join("../../bin/wallpaper-console-rust"),
            ]
            .into_iter()
            .find(|path| path.is_file())
        })
        .unwrap_or_else(|| PathBuf::from("wallpaper-console-rust"));
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(storage.cd.path.join("output-recovery.log"));
    let Ok(log) = log else {
        return;
    };
    let mut command = Command::new(cli);
    command
        .args(["watch-displays", "--config-dir"])
        .arg(&storage.cd.path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(log);
    use std::os::unix::process::CommandExt;
    command.process_group(0);
    match command.spawn() {
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(error) => log::warn!("Could not start output recovery: {error}"),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Assignment {
    backend: String,
    path: String,
}
fn assignments(rows: &[DisplayStateRow], outputs: &[String]) -> HashMap<String, Assignment> {
    outputs
        .iter()
        .filter_map(|output| {
            let row = rows
                .iter()
                .find(|row| row.target == DisplayStateTarget::Output(output.clone()))
                .or_else(|| {
                    rows.iter()
                        .find(|row| row.target == DisplayStateTarget::AllDisplays)
                })?;
            Some((
                output.clone(),
                Assignment {
                    backend: row.backend.clone(),
                    path: row.wallpaper_path.clone(),
                },
            ))
        })
        .collect()
}

#[derive(Default)]
struct RecoveryTracker {
    enabled: HashSet<String>,
    armed: HashMap<String, Assignment>,
    pending: HashMap<String, Assignment>,
    stop_epoch: String,
}
impl RecoveryTracker {
    fn advance(
        &mut self,
        enabled: HashSet<String>,
        saved: &HashMap<String, Assignment>,
        confirmed: HashSet<String>,
        epoch: String,
    ) -> Vec<(String, Assignment)> {
        if self.stop_epoch != epoch {
            self.armed.clear();
            self.pending.clear();
            self.stop_epoch = epoch;
        }
        for output in self.enabled.difference(&enabled) {
            if let Some(assignment) = self.armed.remove(output) {
                self.pending.insert(output.clone(), assignment);
            }
        }
        let mut restore = Vec::new();
        for output in enabled.difference(&self.enabled) {
            if let Some(assignment) = self.pending.remove(output) {
                if saved.get(output) == Some(&assignment) {
                    restore.push((output.clone(), assignment));
                }
            }
        }
        self.armed
            .retain(|output, _| enabled.contains(output) && confirmed.contains(output));
        for output in &confirmed {
            if let Some(assignment) = saved.get(output) {
                self.armed.insert(output.clone(), assignment.clone());
            }
        }
        self.enabled = enabled;
        restore
    }
}

fn niri_outputs() -> Result<(Vec<String>, HashSet<String>), String> {
    let output =
        crate::command_probe::run_probe("niri", &["msg", "-j", "outputs"], Duration::from_secs(1))
            .map_err(|e| format!("niri output probe failed: {e:?}"))?;
    if !output.success {
        return Err(output.stderr);
    }
    let data: serde_json::Value =
        serde_json::from_str(&output.stdout).map_err(|e| e.to_string())?;
    let rows = data.as_object().ok_or("niri outputs are not an object")?;
    let all = rows.keys().cloned().collect();
    let enabled = rows
        .iter()
        .filter(|(_, row)| row["current_mode"].is_number() && row["logical"].is_object())
        .map(|(name, _)| name.clone())
        .collect();
    Ok((all, enabled))
}

impl AppService {
    fn restore_returned_output(
        &self,
        output: &str,
        assignment: &Assignment,
        known: &[String],
    ) -> Result<(), AppError> {
        let backend = match assignment.backend.as_str() {
            "awww" => Backend::Awww,
            "mpvpaper" => Backend::Mpvpaper,
            "linux-wallpaperengine" => Backend::LinuxWallpaperEngine,
            "swaybg" => Backend::Swaybg,
            _ => return Ok(()),
        };
        let mut resolved = self.resolve_apply_request_target(&ApplyRequest {
            kind: ApplyRequestKind::Apply,
            path: assignment.path.clone(),
            request_id: None,
        })?;
        // Keep the assigned renderer even if current routing preferences changed.
        resolved.backend = backend;
        self.execute_resolved_display_apply(
            ApplyRequest {
                kind: ApplyRequestKind::Apply,
                path: assignment.path.clone(),
                request_id: None,
            },
            DisplayTarget::Output(output.into()),
            known,
            &mut SystemBackendRuntime,
            &mut NoopReporter,
            DisplayApplyRuntimeOpts::default(),
            None,
            ApplyExecutionTarget {
                backend,
                ..resolved
            },
            false,
        )
        .map(|_| ())
    }

    /// Long-lived companion to CLI and GUI. Exits with the compositor session.
    pub fn watch_displays(&self) -> Result<(), AppError> {
        let lock = lock_file(&self.storage.cd.path.join("output-recovery.lock"))
            .map_err(|e| AppError::from_wc_error(io_error(e)))?;
        if lock.try_lock_exclusive().is_err() {
            return Ok(());
        }
        let mut tracker = RecoveryTracker::default();
        let mut failures = 0;
        loop {
            if std::env::var_os("NIRI_SOCKET").is_none_or(|socket| !Path::new(&socket).exists()) {
                return Ok(());
            }
            let snapshot = niri_outputs();
            if let Ok((known, enabled)) = snapshot {
                failures = 0;
                let _guard = RendererMutationGuard::acquire(&self.storage)
                    .map_err(AppError::from_wc_error)?;
                let rows = self
                    .storage
                    .display_state_list()
                    .map_err(AppError::from_wc_error)?;
                let mut names = known.clone();
                names.extend(tracker.pending.keys().cloned());
                let saved = assignments(&rows, &names);
                let active: Vec<_> = enabled.iter().cloned().collect();
                let observed = observe_runtime_wallpapers(&active, &rows);
                let confirmed = observed
                    .into_iter()
                    .filter(|row| row.status == RuntimeObservationStatus::Confirmed)
                    .map(|row| row.output)
                    .collect();
                let epoch =
                    std::fs::read_to_string(self.storage.cd.path.join("output-recovery-stop"))
                        .unwrap_or_default();
                for (output, assignment) in tracker.advance(enabled, &saved, confirmed, epoch) {
                    // Let output/layer creation settle, then revalidate topology.
                    std::thread::sleep(Duration::from_millis(350));
                    let Ok((current, enabled)) = niri_outputs() else {
                        continue;
                    };
                    if !enabled.contains(&output) {
                        continue;
                    }
                    match self.restore_returned_output(&output, &assignment, &current) {
                        Ok(()) => eprintln!(
                            "Restored returned output {output} using {}",
                            assignment.backend
                        ),
                        Err(error) => eprintln!("Output recovery failed for {output}: {error:?}"),
                    }
                }
            } else {
                failures += 1;
                if failures >= 10 {
                    return Ok(());
                }
            }
            std::thread::sleep(Duration::from_millis(300));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assignment() -> Assignment {
        Assignment {
            backend: "mpvpaper".into(),
            path: "/video.mp4".into(),
        }
    }

    fn names(values: &[&str]) -> HashSet<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn returns_only_the_previously_playing_output() {
        let mut tracker = RecoveryTracker::default();
        let saved = HashMap::from([
            ("DP-8".into(), assignment()),
            ("eDP-1".into(), assignment()),
        ]);
        assert!(tracker
            .advance(
                names(&["DP-8", "eDP-1"]),
                &saved,
                names(&["DP-8", "eDP-1"]),
                "".into()
            )
            .is_empty());
        assert!(tracker
            .advance(names(&["eDP-1"]), &saved, names(&["eDP-1"]), "".into())
            .is_empty());
        assert_eq!(
            tracker.advance(
                names(&["DP-8", "eDP-1"]),
                &saved,
                names(&["eDP-1"]),
                "".into()
            ),
            vec![("DP-8".into(), assignment())]
        );
        assert!(tracker
            .advance(
                names(&["DP-8", "eDP-1"]),
                &saved,
                names(&["eDP-1"]),
                "".into()
            )
            .is_empty());
    }

    #[test]
    fn explicit_stop_while_disconnected_cancels_pending_restore() {
        let mut tracker = RecoveryTracker::default();
        let saved = HashMap::from([("DP-8".into(), assignment())]);
        tracker.advance(names(&["DP-8"]), &saved, names(&["DP-8"]), "".into());
        tracker.advance(names(&[]), &saved, names(&[]), "".into());
        assert!(tracker
            .advance(names(&["DP-8"]), &saved, names(&[]), "stopped".into())
            .is_empty());
    }

    #[test]
    fn saved_preferences_alone_never_restart_stopped_wallpaper() {
        let mut tracker = RecoveryTracker::default();
        let saved = HashMap::from([("DP-8".into(), assignment())]);
        tracker.advance(names(&["DP-8"]), &saved, names(&[]), "".into());
        tracker.advance(names(&[]), &saved, names(&[]), "".into());
        assert!(tracker
            .advance(names(&["DP-8"]), &saved, names(&[]), "".into())
            .is_empty());
    }

    #[test]
    fn assignment_changed_while_absent_is_not_overwritten() {
        let mut tracker = RecoveryTracker::default();
        let mut saved = HashMap::from([("DP-8".into(), assignment())]);
        tracker.advance(names(&["DP-8"]), &saved, names(&["DP-8"]), "".into());
        tracker.advance(names(&[]), &saved, names(&[]), "".into());
        saved.get_mut("DP-8").unwrap().path = "/new.mp4".into();
        assert!(tracker
            .advance(names(&["DP-8"]), &saved, names(&[]), "".into())
            .is_empty());
    }
}
