//! Session-scoped compositor output recovery. Only previously observed assignments
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
use wc_storage::sqlite::{DisplayStateRow, DisplayStateTarget};
use wc_storage::StorageApi;

use crate::display_apply::DisplayApplyRuntimeOpts;
use crate::{AppError, AppService, ApplyRequest, ApplyRequestKind, DisplayTarget};

thread_local! { static HELD: RefCell<HashSet<PathBuf>> = RefCell::new(HashSet::new()); }

/// Serialize watcher, CLI and GUI mutations; nested AppService calls reuse the
/// current thread's lock. Read-only observations do not acquire this lock.
///
/// Reentrant only on the acquiring thread: nested apply/restore/operation guards
/// borrow the outer lock. They must be dropped before that owning guard; the
/// outermost lexical scope keeps the file lock through planning and execution.
pub struct RendererMutationGuard {
    file: Option<File>,
    path: PathBuf,
}
impl RendererMutationGuard {
    pub fn acquire(_storage: &StorageApi) -> Result<Self, WcError> {
        let path = session_lock_path()?;
        if HELD.with(|held| held.borrow().contains(&path)) {
            return Ok(Self { file: None, path });
        }
        let file = lock_file(&path).map_err(io_error)?;
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        loop {
            match file.try_lock_exclusive() {
                Ok(()) => break,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if std::time::Instant::now() >= deadline {
                        return Err(WcError::Other(
                            "another wallpaper operation is still running in this desktop session"
                                .into(),
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(25));
                }
                Err(error) => return Err(io_error(error)),
            }
        }
        HELD.with(|held| held.borrow_mut().insert(path.clone()));
        Ok(Self {
            file: Some(file),
            path,
        })
    }
}

/// Independent configuration directories still operate on the same desktop.
pub fn renderer_session_id() -> String {
    use std::hash::{Hash, Hasher};
    let mut identity = std::collections::hash_map::DefaultHasher::new();
    // Optional terminal-only variables (DISPLAY/XDG_SESSION_ID) must not split
    // a compositor session from its systemd-started GUI.
    for key in [
        "NIRI_SOCKET",
        "SWAYSOCK",
        "HYPRLAND_INSTANCE_SIGNATURE",
        "WAYLAND_DISPLAY",
        "DISPLAY",
    ] {
        if let Some(value) = std::env::var_os(key).filter(|value| !value.is_empty()) {
            key.hash(&mut identity);
            value.hash(&mut identity);
            break;
        }
    }
    std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .ok()
        .hash(&mut identity);
    format!("{:016x}", identity.finish())
}

fn session_lock_path() -> Result<PathBuf, WcError> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    // SAFETY: getuid has no preconditions or side effects.
    let uid = unsafe { libc::getuid() };
    let root = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join(format!("wallpaper-console-{uid}")));
    if !root.exists() {
        let mut builder = std::fs::DirBuilder::new();
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700).create(&root).map_err(io_error)?;
    }
    let metadata = std::fs::symlink_metadata(&root).map_err(io_error)?;
    if !metadata.is_dir() || metadata.uid() != uid || metadata.permissions().mode() & 0o022 != 0 {
        return Err(WcError::Other(
            "renderer session runtime directory has unsafe ownership or permissions".into(),
        ));
    }
    Ok(root.join(format!("wallpaper-console-{}.lock", renderer_session_id())))
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
    write_session_intents(&["*".into()], true)?;
    let epoch = format!("{:?}", std::time::SystemTime::now());
    std::fs::write(storage.cd.path.join("output-recovery-stop"), epoch).map_err(io_error)
}

pub(crate) fn set_output_stopped(
    storage: &StorageApi,
    outputs: &[String],
    stopped: bool,
) -> Result<(), WcError> {
    // The lock and stop intent have the same session scope: a watcher using a
    // different configuration directory must also observe this user's Stop.
    write_session_intents(outputs, stopped)?;
    let conn = wc_storage::sqlite::open_runtime_connection(&storage.cd)?;
    wc_storage::sqlite::display_operations::set_stopped(
        &conn,
        &renderer_session_id(),
        outputs,
        stopped,
    )
}

fn session_intents() -> Result<HashMap<String, (u64, bool)>, WcError> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    let path = session_lock_path()?.with_extension("intents.json");
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(error) => return Err(io_error(error)),
    };
    let mut bytes = Vec::new();
    file.take(65537).read_to_end(&mut bytes).map_err(io_error)?;
    if bytes.len() > 65536 {
        return Err(WcError::Other(
            "session stop intents exceed size limit".into(),
        ));
    }
    serde_json::from_slice(&bytes)
        .map_err(|e| WcError::Other(format!("invalid session stop intents: {e}")))
}

/// Caller holds RendererMutationGuard. Atomic rename prevents readers from
/// observing a truncated intent if the writing process exits unexpectedly.
fn write_session_intents(outputs: &[String], stopped: bool) -> Result<(), WcError> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut intents = session_intents()?;
    let epoch = intents
        .values()
        .map(|(epoch, _)| *epoch)
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| WcError::Other("session intent epoch overflow".into()))?;
    for output in outputs {
        intents.insert(output.clone(), (epoch, stopped));
    }
    let bytes = serde_json::to_vec(&intents).map_err(|e| WcError::Other(e.to_string()))?;
    if bytes.len() > 65536 {
        return Err(WcError::Other("too many session stop intents".into()));
    }
    let path = session_lock_path()?.with_extension("intents.json");
    let temporary = path.with_extension(format!("{}-{epoch}.tmp", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&temporary)
        .map_err(io_error)?;
    let result = (|| {
        file.write_all(&bytes)?;
        file.sync_all()?;
        std::fs::rename(&temporary, &path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result.map_err(io_error)
}

pub(crate) fn ensure_watcher(storage: &StorageApi) {
    if !matches!(
        crate::compositor::Adapter::current(),
        Some(
            crate::compositor::Adapter::Niri
                | crate::compositor::Adapter::Sway
                | crate::compositor::Adapter::Hyprland
        )
    ) || std::env::var_os("WCR_DISABLE_OUTPUT_WATCH").is_some()
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
    version: String,
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
                    version: String::new(),
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
    fn remember_disconnected(&mut self, enabled: &HashSet<String>) {
        for output in self.enabled.difference(enabled) {
            if let Some(assignment) = self.armed.remove(output) {
                self.pending.insert(output.clone(), assignment);
            }
        }
    }
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
        self.remember_disconnected(&enabled);
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

fn assignment_version(storage: &StorageApi, output: &str) -> Result<String, WcError> {
    let conn = wc_storage::sqlite::open_runtime_connection(&storage.cd)?;
    conn.query_row("SELECT assignment_revision,recipe_json,wallpaper_path FROM display_state WHERE target_key IN (?1,'__all_displays__') ORDER BY target_key=?1 DESC LIMIT 1", [output], |row| {
        use std::os::unix::fs::MetadataExt;
        let path = row.get::<_,String>(2)?;
        let stamp = std::fs::metadata(&path).ok().map(|m| (m.dev(), m.ino(), m.len(), m.mtime(), m.mtime_nsec(), m.ctime(), m.ctime_nsec()));
        Ok(format!("{}:{}:{stamp:?}",row.get::<_,i64>(0)?,row.get::<_,String>(1)?))
    }).map_err(|e|WcError::Other(e.to_string()))
}

struct Retry {
    assignment: Assignment,
    due: std::time::Instant,
    attempt: usize,
}
const RETRY_DELAYS: [u64; 5] = [500, 1000, 2000, 4000, 8000];

const WATCH_ACTIVE_INTERVAL: Duration = Duration::from_millis(300);
const WATCH_IDLE_INTERVAL: Duration = Duration::from_millis(2000);

/// Facts already collected by a complete recovery round; no extra probes.
#[derive(Clone, Default, PartialEq, Eq)]
struct RoundFacts {
    snapshot: Option<Vec<crate::compositor::Output>>,
    pending_empty: bool,
    retries_empty: bool,
    confirmed: HashSet<String>,
    intents: HashMap<String, (u64, bool)>,
    saved: HashMap<String, Assignment>,
}

#[derive(Default)]
struct WatchPacing {
    unchanged_rounds: u8,
    previous: Option<RoundFacts>,
}

impl WatchPacing {
    fn wake(&mut self) {
        self.unchanged_rounds = 0;
        self.previous = None;
    }

    fn observe(&mut self, round: &RoundFacts) -> Duration {
        if round.snapshot.is_some()
            && round.pending_empty
            && round.retries_empty
            && self.previous.as_ref() == Some(round)
        {
            self.unchanged_rounds = self.unchanged_rounds.saturating_add(1).min(10);
        } else {
            self.unchanged_rounds = 0;
        }
        self.previous = round.snapshot.as_ref().map(|_| round.clone());
        if self.unchanged_rounds == 10 {
            WATCH_IDLE_INTERVAL
        } else {
            WATCH_ACTIVE_INTERVAL
        }
    }
}

type WatchFingerprint = [Option<(std::time::SystemTime, u64)>; 3];

fn watch_fingerprint(paths: &[PathBuf; 3]) -> std::io::Result<WatchFingerprint> {
    let mut fingerprint = [None, None, None];
    for (stamp, path) in fingerprint.iter_mut().zip(paths) {
        // Like session_intents' O_NOFOLLOW, never inspect a symlink target.
        // Only stat metadata is read; no database or file contents are opened.
        match std::fs::symlink_metadata(path) {
            Ok(metadata) if metadata.is_file() => {
                *stamp = Some((metadata.modified()?, metadata.len()));
            }
            Ok(_) => return Err(std::io::Error::other("watch signal is not a regular file")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(fingerprint)
}

fn wait_for_watch(
    interval: Duration,
    mut sleep: impl FnMut(Duration),
    mut fingerprint: impl FnMut() -> std::io::Result<WatchFingerprint>,
) -> bool {
    if interval != WATCH_IDLE_INTERVAL {
        sleep(interval);
        return false;
    }
    let Ok(before) = fingerprint() else {
        return true;
    };
    let mut remaining = interval;
    while !remaining.is_zero() {
        let slice = remaining.min(WATCH_ACTIVE_INTERVAL);
        sleep(slice);
        if !matches!(fingerprint(), Ok(current) if current == before) {
            return true;
        }
        remaining -= slice;
    }
    false
}

fn report_watch_pacing(interval: Duration, reason: &str) {
    if std::env::var_os("WCR_PERF").is_some() {
        eprintln!(
            "WCR_PERF {}",
            serde_json::json!({
                "event": "watch_pacing",
                "mode": if interval == WATCH_IDLE_INTERVAL { "idle" } else { "active" },
                "reason": reason,
            })
        );
    }
}

fn transient_recovery_error(error: &AppError) -> bool {
    let text = format!(
        "{} {} {}",
        error.code,
        error.message,
        error.detail.as_deref().unwrap_or("")
    )
    .to_lowercase();
    ![
        "unsupported",
        "unverified",
        "decode",
        "corrupt",
        "not a regular file",
        "invalid",
        "not found",
        "assignment changed",
    ]
    .iter()
    .any(|part| text.contains(part))
}

impl AppService {
    fn restore_returned_output(
        &self,
        output: &str,
        assignment: &Assignment,
        known: &[String],
    ) -> Result<(), AppError> {
        let _guard =
            RendererMutationGuard::acquire(&self.storage).map_err(AppError::from_wc_error)?;
        let target = wc_storage::sqlite::DisplayStateTarget::Output(output.into());
        let recipe = match self
            .storage
            .display_recipe(&target)
            .map_err(AppError::from_wc_error)?
        {
            Some(recipe) => Some(recipe),
            None => self
                .storage
                .display_recipe(&wc_storage::sqlite::DisplayStateTarget::AllDisplays)
                .map_err(AppError::from_wc_error)?,
        }
        .ok_or_else(|| {
            AppError::from_wc_error(WcError::Other("saved output recipe missing".into()))
        })?;
        if recipe.media_path != assignment.path
            || recipe.options.backend().as_str() != assignment.backend
            || assignment_version(&self.storage, output).map_err(AppError::from_wc_error)?
                != assignment.version
        {
            return Err(AppError::from_wc_error(WcError::Other(
                "output assignment changed during recovery".into(),
            )));
        }
        let resolved = self.resolve_recipe_target(&recipe)?;
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
            resolved,
            crate::display_apply::AssignmentUpdate::Restore,
            None,
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
        let Some(adapter) = crate::compositor::Adapter::current() else {
            return Ok(());
        };
        if adapter == crate::compositor::Adapter::Xrandr {
            return Ok(());
        }
        let mut retries: HashMap<String, Retry> = HashMap::new();
        let mut last_outputs: Vec<crate::compositor::Output> = Vec::new();
        let mut previous_intents = HashMap::new();
        let mut failures = 0;
        let mut pacing = WatchPacing::default();
        let watch_paths = [
            session_lock_path()
                .map_err(AppError::from_wc_error)?
                .with_extension("intents.json"),
            self.storage.cd.path.join("wallpapers.db"),
            self.storage.cd.path.join("wallpapers.db-wal"),
        ];
        let mut previous_interval = WATCH_ACTIVE_INTERVAL;
        loop {
            let mut round = RoundFacts::default();
            if let Ok(snapshot) = adapter.snapshot() {
                failures = 0;
                round.snapshot = Some(snapshot.clone());
                let _guard = RendererMutationGuard::acquire(&self.storage)
                    .map_err(AppError::from_wc_error)?;
                let known: Vec<_> = snapshot
                    .iter()
                    .filter(|o| o.enabled)
                    .map(|o| o.name.clone())
                    .collect();
                let enabled: HashSet<_> = known.iter().cloned().collect();
                let intents = session_intents().map_err(AppError::from_wc_error)?;
                let global_epoch = intents
                    .get("*")
                    .filter(|(_, stopped)| *stopped)
                    .map(|(epoch, _)| *epoch)
                    .unwrap_or(0);
                let is_stopped = |name: &str| match intents.get(name) {
                    Some((epoch, stopped)) => *stopped || *epoch < global_epoch,
                    None => global_epoch > 0,
                };
                tracker.pending.retain(|output, _| !is_stopped(output));
                tracker.armed.retain(|output, _| !is_stopped(output));
                tracker.remember_disconnected(&enabled);
                // Unique hardware identities permit a connector rename. Preserve
                // the old preference and never overwrite an existing new target.
                for output in &snapshot {
                    if let Some(previous) =
                        crate::compositor::matching_previous(output, &last_outputs)
                    {
                        if previous.name != output.name
                            && !snapshot.iter().any(|o| o.name == previous.name)
                        {
                            if let Some(assignment) = tracker.pending.get(&previous.name).cloned() {
                                if is_stopped(&output.name)
                                    || assignment_version(&self.storage, &previous.name)
                                        .map_err(AppError::from_wc_error)?
                                        != assignment.version
                                {
                                    tracker.pending.remove(&previous.name);
                                    continue;
                                }
                                let target = DisplayStateTarget::Output(output.name.clone());
                                if self
                                    .storage
                                    .display_recipe(&target)
                                    .map_err(AppError::from_wc_error)?
                                    .is_none()
                                {
                                    let recipe = self
                                        .storage
                                        .display_recipe(&DisplayStateTarget::Output(
                                            previous.name.clone(),
                                        ))
                                        .map_err(AppError::from_wc_error)?;
                                    let recipe = match recipe {
                                        Some(recipe) => Some(recipe),
                                        None => self
                                            .storage
                                            .display_recipe(&DisplayStateTarget::AllDisplays)
                                            .map_err(AppError::from_wc_error)?,
                                    };
                                    if let Some(recipe) = recipe {
                                        let conn = wc_storage::sqlite::open_runtime_connection(
                                            &self.storage.cd,
                                        )
                                        .map_err(AppError::from_wc_error)?;
                                        wc_storage::sqlite::display_state_commit_distinct_recipes(
                                            &conn,
                                            &[(target, recipe)],
                                        )
                                        .map_err(AppError::from_wc_error)?;
                                        tracker.pending.remove(&previous.name);
                                        tracker.pending.insert(
                                            output.name.clone(),
                                            Assignment {
                                                version: assignment_version(
                                                    &self.storage,
                                                    &output.name,
                                                )
                                                .map_err(AppError::from_wc_error)?,
                                                ..assignment
                                            },
                                        );
                                    }
                                }
                            }
                        }
                    } else if last_outputs.iter().any(|o| o.name == output.name) {
                        tracker.pending.remove(&output.name);
                        tracker.armed.remove(&output.name);
                        retries.remove(&output.name);
                    }
                }
                for output in snapshot {
                    if let Some(id) = &output.hardware_identity {
                        last_outputs.retain(|old| {
                            old.name == output.name || old.hardware_identity.as_ref() != Some(id)
                        });
                    }
                    if let Some(old) = last_outputs.iter_mut().find(|o| o.name == output.name) {
                        *old = output;
                    } else {
                        last_outputs.push(output);
                    }
                }
                let rows = self
                    .storage
                    .display_state_list()
                    .map_err(AppError::from_wc_error)?;
                let mut names = known.clone();
                names.extend(tracker.pending.keys().cloned());
                names.extend(retries.keys().cloned());
                let mut saved = assignments(&rows, &names);
                for (output, assignment) in &mut saved {
                    assignment.version = assignment_version(&self.storage, output)
                        .map_err(AppError::from_wc_error)?;
                }
                let intents = session_intents().map_err(AppError::from_wc_error)?;
                let global_epoch = intents
                    .get("*")
                    .filter(|(_, stopped)| *stopped)
                    .map(|(epoch, _)| *epoch)
                    .unwrap_or(0);
                for (output, (epoch, _)) in &intents {
                    if previous_intents
                        .get(output)
                        .is_some_and(|previous| previous != epoch)
                    {
                        tracker.armed.remove(output);
                        tracker.pending.remove(output);
                        retries.remove(output);
                    }
                    previous_intents.insert(output.clone(), *epoch);
                }
                let stopped: HashSet<String> = names
                    .iter()
                    .filter(|output| match intents.get(*output) {
                        Some((epoch, stopped)) => *stopped || *epoch < global_epoch,
                        None => global_epoch > 0,
                    })
                    .cloned()
                    .collect();
                tracker.armed.retain(|output, _| !stopped.contains(output));
                tracker
                    .pending
                    .retain(|output, _| !stopped.contains(output));
                let active: Vec<_> = enabled.iter().cloned().collect();
                let observed = observe_runtime_wallpapers(&active, &rows);
                let confirmed: HashSet<_> = observed
                    .into_iter()
                    .filter(|row| {
                        row.status == RuntimeObservationStatus::Confirmed
                            && !stopped.contains(&row.output)
                    })
                    .map(|row| row.output)
                    .collect();
                round.confirmed = confirmed.clone();
                let epoch = global_epoch.to_string();
                if epoch != tracker.stop_epoch {
                    retries.clear();
                }
                for (output, assignment) in
                    tracker.advance(enabled.clone(), &saved, confirmed, epoch)
                {
                    retries.insert(
                        output,
                        Retry {
                            assignment,
                            due: std::time::Instant::now() + Duration::from_millis(300),
                            attempt: 0,
                        },
                    );
                }
                retries.retain(|output, retry| {
                    !stopped.contains(output)
                        && enabled.contains(output)
                        && saved.get(output) == Some(&retry.assignment)
                });
                let due: Vec<_> = retries
                    .iter()
                    .filter(|(_, r)| r.due <= std::time::Instant::now())
                    .map(|(o, _)| o.clone())
                    .collect();
                for output in due {
                    let mut retry = retries.remove(&output).expect("scheduled recovery");
                    match self.restore_returned_output(&output, &retry.assignment, &known) {
                        Ok(()) => eprintln!(
                            "Restored returned output {output} using {}",
                            retry.assignment.backend
                        ),
                        Err(error) => {
                            if transient_recovery_error(&error)
                                && retry.attempt < RETRY_DELAYS.len()
                            {
                                retry.due = std::time::Instant::now()
                                    + Duration::from_millis(RETRY_DELAYS[retry.attempt]);
                                retry.attempt += 1;
                                retries.insert(output.clone(), retry);
                            }
                            eprintln!("Output recovery failed for {output}: {error:?}");
                        }
                    }
                }
                round.intents = intents;
                round.saved = saved;
            } else {
                failures += 1;
                if failures >= 10 {
                    return Ok(());
                }
            }
            round.pending_empty = tracker.pending.is_empty();
            round.retries_empty = retries.is_empty();
            let interval = pacing.observe(&round);
            if interval != previous_interval {
                report_watch_pacing(interval, "round_facts");
            }
            previous_interval = interval;
            if wait_for_watch(interval, std::thread::sleep, || {
                watch_fingerprint(&watch_paths)
            }) {
                pacing.wake();
                previous_interval = WATCH_ACTIVE_INTERVAL;
                report_watch_pacing(WATCH_ACTIVE_INTERVAL, "file_changed");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_wait_uses_bounded_slices_without_waking_for_unchanged_files() {
        let mut sleeps = Vec::new();
        let woke = wait_for_watch(
            Duration::from_millis(2000),
            |duration| sleeps.push(duration),
            || Ok([None, None, None]),
        );
        assert!(!woke);
        assert_eq!(
            sleeps,
            vec![
                Duration::from_millis(300),
                Duration::from_millis(300),
                Duration::from_millis(300),
                Duration::from_millis(300),
                Duration::from_millis(300),
                Duration::from_millis(300),
                Duration::from_millis(200)
            ]
        );
    }

    fn assert_file_signal_wakes(index: usize, initially_present: bool) {
        let temp = tempfile::tempdir().unwrap();
        let paths = [
            temp.path().join("session.intents.json"),
            temp.path().join("wallpapers.db"),
            temp.path().join("wallpapers.db-wal"),
        ];
        for (i, path) in paths.iter().enumerate() {
            if i != index || initially_present {
                std::fs::write(path, b"old").unwrap();
            }
        }
        let mut sleeps = Vec::new();
        let woke = wait_for_watch(
            Duration::from_millis(2000),
            |duration| {
                sleeps.push(duration);
                if sleeps.len() == 3 {
                    std::fs::write(&paths[index], b"new and longer").unwrap();
                }
            },
            || watch_fingerprint(&paths),
        );
        assert!(woke);
        assert_eq!(sleeps, vec![Duration::from_millis(300); 3]);
    }

    #[test]
    fn intent_file_changes_wake_on_the_first_changed_slice() {
        assert_file_signal_wakes(0, true);
        assert_file_signal_wakes(0, false);
    }

    #[test]
    fn database_file_changes_wake_on_the_first_changed_slice() {
        assert_file_signal_wakes(1, true);
        assert_file_signal_wakes(1, false);
    }

    #[test]
    fn wal_file_changes_wake_on_the_first_changed_slice() {
        assert_file_signal_wakes(2, true);
        assert_file_signal_wakes(2, false);
    }

    #[test]
    fn modification_time_alone_wakes_the_wait() {
        let temp = tempfile::tempdir().unwrap();
        let paths = [
            temp.path().join("intent"),
            temp.path().join("db"),
            temp.path().join("wal"),
        ];
        std::fs::write(&paths[0], b"same size").unwrap();
        let mut sleeps = 0;
        assert!(wait_for_watch(
            Duration::from_millis(2000),
            |_| {
                sleeps += 1;
                File::open(&paths[0])
                    .unwrap()
                    .set_modified(std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(123))
                    .unwrap();
            },
            || watch_fingerprint(&paths)
        ));
        assert_eq!(sleeps, 1);
    }

    #[test]
    fn removed_file_wakes_the_wait() {
        let temp = tempfile::tempdir().unwrap();
        let paths = [
            temp.path().join("intent"),
            temp.path().join("db"),
            temp.path().join("wal"),
        ];
        std::fs::write(&paths[2], b"wal").unwrap();
        let mut sleeps = 0;
        assert!(wait_for_watch(
            Duration::from_millis(2000),
            |_| {
                sleeps += 1;
                std::fs::remove_file(&paths[2]).unwrap();
            },
            || watch_fingerprint(&paths)
        ));
        assert_eq!(sleeps, 1);
    }

    #[cfg(unix)]
    #[test]
    fn fingerprint_does_not_follow_symlinks() {
        let temp = tempfile::tempdir().unwrap();
        let paths = [
            temp.path().join("intent"),
            temp.path().join("db"),
            temp.path().join("wal"),
        ];
        let target = temp.path().join("other-session");
        std::fs::write(&target, b"unrelated").unwrap();
        std::os::unix::fs::symlink(target, &paths[0]).unwrap();
        assert!(watch_fingerprint(&paths).is_err());
        assert!(wait_for_watch(
            Duration::from_millis(2000),
            |_| panic!("unknown signals must wake immediately"),
            || watch_fingerprint(&paths)
        ));
    }

    #[test]
    fn active_wait_does_not_probe_file_metadata() {
        let mut sleeps = Vec::new();
        assert!(!wait_for_watch(
            Duration::from_millis(300),
            |duration| sleeps.push(duration),
            || panic!("active wait should not stat files")
        ));
        assert_eq!(sleeps, vec![Duration::from_millis(300)]);
    }

    #[test]
    fn waking_pacing_requires_fresh_stable_rounds() {
        let mut pacing = WatchPacing::default();
        let round = stable_round();
        for _ in 0..11 {
            pacing.observe(&round);
        }
        pacing.wake();
        assert_eq!(pacing.observe(&round), Duration::from_millis(300));
        for _ in 0..9 {
            assert_eq!(pacing.observe(&round), Duration::from_millis(300));
        }
        assert_eq!(pacing.observe(&round), Duration::from_millis(2000));
    }

    fn stable_round() -> RoundFacts {
        RoundFacts {
            snapshot: Some(vec![crate::compositor::Output {
                name: "DP-8".into(),
                enabled: true,
                hardware_identity: Some("monitor-1".into()),
            }]),
            pending_empty: true,
            retries_empty: true,
            confirmed: names(&["DP-8"]),
            intents: HashMap::from([("DP-8".into(), (1, false))]),
            saved: HashMap::from([("DP-8".into(), assignment())]),
        }
    }

    #[test]
    fn pacing_requires_ten_unchanged_rounds_before_idle() {
        let mut pacing = WatchPacing::default();
        let round = stable_round();
        // First observation establishes the comparison baseline.
        assert_eq!(pacing.observe(&round), Duration::from_millis(300));
        for _ in 0..9 {
            assert_eq!(pacing.observe(&round), Duration::from_millis(300));
        }
        assert_eq!(pacing.observe(&round), Duration::from_millis(2000));
        assert_eq!(pacing.observe(&round), Duration::from_millis(2000));
    }

    fn assert_pacing_restarts(change: impl FnOnce(&mut RoundFacts)) {
        let mut pacing = WatchPacing::default();
        let mut round = stable_round();
        for _ in 0..11 {
            pacing.observe(&round);
        }
        assert_eq!(pacing.observe(&round), Duration::from_millis(2000));
        change(&mut round);
        assert_eq!(pacing.observe(&round), Duration::from_millis(300));
        if !round.pending_empty || !round.retries_empty {
            for _ in 0..20 {
                assert_eq!(pacing.observe(&round), Duration::from_millis(300));
            }
            round.pending_empty = true;
            round.retries_empty = true;
            assert_eq!(pacing.observe(&round), Duration::from_millis(300));
        }
        for _ in 0..9 {
            assert_eq!(pacing.observe(&round), Duration::from_millis(300));
        }
        assert_eq!(pacing.observe(&round), Duration::from_millis(2000));
    }

    #[test]
    fn pacing_output_name_change_restarts_active_rounds() {
        assert_pacing_restarts(|r| r.snapshot.as_mut().unwrap()[0].name = "DP-9".into());
    }

    #[test]
    fn pacing_output_enabled_change_restarts_active_rounds() {
        assert_pacing_restarts(|r| r.snapshot.as_mut().unwrap()[0].enabled = false);
    }

    #[test]
    fn pacing_output_identity_change_restarts_active_rounds() {
        assert_pacing_restarts(|r| {
            r.snapshot.as_mut().unwrap()[0].hardware_identity = Some("monitor-2".into())
        });
    }

    #[test]
    fn pacing_pending_recovery_prevents_idle() {
        assert_pacing_restarts(|r| r.pending_empty = false);
    }

    #[test]
    fn pacing_retry_prevents_idle() {
        assert_pacing_restarts(|r| r.retries_empty = false);
    }

    #[test]
    fn pacing_confirmed_change_restarts_active_rounds() {
        assert_pacing_restarts(|r| r.confirmed.clear());
    }

    #[test]
    fn pacing_session_intent_change_restarts_active_rounds() {
        assert_pacing_restarts(|r| {
            r.intents.insert("DP-8".into(), (2, true));
        });
    }

    #[test]
    fn pacing_saved_assignment_change_restarts_active_rounds() {
        assert_pacing_restarts(|r| r.saved.get_mut("DP-8").unwrap().path = "/other.mp4".into());
    }

    #[test]
    fn pacing_assignment_version_change_restarts_active_rounds() {
        assert_pacing_restarts(|r| r.saved.get_mut("DP-8").unwrap().version = "revision-2".into());
    }

    #[test]
    fn pacing_snapshot_failure_clears_previous_stability() {
        let mut pacing = WatchPacing::default();
        let round = stable_round();
        for _ in 0..11 {
            pacing.observe(&round);
        }
        assert_eq!(
            pacing.observe(&RoundFacts::default()),
            Duration::from_millis(300)
        );
        // A successful snapshot establishes a fresh baseline after failure.
        assert_eq!(pacing.observe(&round), Duration::from_millis(300));
        for _ in 0..9 {
            assert_eq!(pacing.observe(&round), Duration::from_millis(300));
        }
        assert_eq!(pacing.observe(&round), Duration::from_millis(2000));
    }

    fn assignment() -> Assignment {
        Assignment {
            backend: "mpvpaper".into(),
            path: "/video.mp4".into(),
            version: String::new(),
        }
    }

    fn names(values: &[&str]) -> HashSet<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn direct_connector_rename_retains_pending_before_identity_migration() {
        let mut tracker = RecoveryTracker::default();
        let saved = HashMap::from([("A".into(), assignment())]);
        tracker.advance(names(&["A"]), &saved, names(&["A"]), "".into());
        tracker.remember_disconnected(&names(&["B"]));
        let value = tracker
            .pending
            .remove("A")
            .expect("direct rename must retain old live assignment");
        tracker.pending.insert("B".into(), value.clone());
        assert_eq!(
            tracker.advance(
                names(&["B"]),
                &HashMap::from([("B".into(), value.clone())]),
                names(&[]),
                "".into()
            ),
            vec![("B".into(), value)]
        );
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
