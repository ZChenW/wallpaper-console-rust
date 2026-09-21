use std::process::{Command, Output};

use wc_core::error::WcError;
use wc_storage::StorageApi;

pub use crate::mpvpaper::{classify_selector, MpvpaperOutputSelector, MpvpaperProcess};

const APPLY_COMMAND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(65);
const LAUNCH_COMMAND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const AWWW_QUERY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AwwwReadiness {
    Ready,
    SocketMissing,
    SocketPresentQueryFailed { stderr: String },
}

/// Observed host facts; the driver decides whether transparent release is supported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AwwwEnvironment {
    pub niri_socket_present: bool,
    pub version: Option<String>,
    pub daemon_commands: Vec<Vec<String>>,
}

pub fn awww_socket_path() -> Result<std::path::PathBuf, WcError> {
    let xdg = std::env::var("XDG_RUNTIME_DIR").map_err(|_| {
        WcError::Other("XDG_RUNTIME_DIR is not set; cannot locate awww-daemon socket".into())
    })?;
    let wayland = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".to_string());
    Ok(std::path::PathBuf::from(xdg).join(format!("{wayland}-awww-daemon.sock")))
}

/// Process I/O seam: spawn commands and probe renderer readiness.
///
/// Prefer this surface for new orchestration. Stop / apply policy belongs on
/// [`crate::driver::BackendDriver`]; see domain term **ProcessIo**.
pub trait ProcessIo {
    fn preflight_swaybg_observation(&mut self) -> Result<(), WcError> {
        Ok(())
    }
    fn is_niri_session(&mut self) -> Result<bool, WcError> {
        Ok(self.awww_environment(false)?.niri_socket_present)
    }
    fn launch_swaybg(&mut self, command: &mut Command) -> Result<(), WcError> {
        if self.command_status(command)?.success() {
            Ok(())
        } else {
            Err(WcError::Other("swaybg failed to launch".into()))
        }
    }
    fn renderer_identity_snapshot(&mut self) -> Result<String, WcError> {
        Ok(format!("{:?}", self.renderer_command_lines()?))
    }
    /// Fakes explicitly model successful readiness; the system requires live evidence.
    fn verify_recipe(
        &mut self,
        _output: &str,
        _recipe: &wc_core::display_assignment::RenderRecipe,
        _known_outputs: &[String],
    ) -> Result<(), WcError> {
        Ok(())
    }
    /// Fake runtimes may model image validation; the system uses a killable worker.
    fn preflight_image(
        &mut self,
        _path: &str,
    ) -> Result<Option<crate::image_media::ImageStamp>, WcError> {
        Ok(None)
    }
    /// Prepare media before any destructive stop. Fakes explicitly model success.
    fn prepare_mpvpaper_options(&mut self, options: &str, _path: &str) -> Result<String, WcError> {
        Ok(options.to_string())
    }
    fn command_output(&mut self, command: &mut Command) -> Result<Output, WcError>;
    fn command_status(
        &mut self,
        command: &mut Command,
    ) -> Result<std::process::ExitStatus, WcError>;
    fn mpvpaper_pids(&mut self) -> Result<Vec<u32>, WcError>;
    /// Live mpvpaper processes with cmdline-derived output selectors.
    fn mpvpaper_processes(&mut self) -> Result<Vec<MpvpaperProcess>, WcError> {
        Err(WcError::Other(
            "mpvpaper process inspection is unavailable for this runtime".into(),
        ))
    }
    fn wait_for_mpvpaper_ready(
        &mut self,
        previous_pids: &[u32],
        output: &str,
        path: &str,
    ) -> Result<u32, WcError>;
    fn mpvpaper_pid_running(&mut self, pid: u32) -> Result<bool, WcError>;
    /// Remove only renderer processes that appeared after a failed launch and
    /// whose process arguments exactly match this launch's output and path.
    fn cleanup_failed_mpvpaper_launch(
        &mut self,
        previous_pids: &[u32],
        output: &str,
        path: &str,
    ) -> Result<(), WcError>;
    fn swaybg_pids(&mut self) -> Result<Vec<u32>, WcError> {
        Err(WcError::Other(
            "swaybg process inspection is unavailable for this runtime".into(),
        ))
    }
    fn wait_for_swaybg_ready(
        &mut self,
        _previous_pids: &[u32],
        _path: &str,
        _scope: &crate::target_commands::ExecutionScope,
    ) -> Result<u32, WcError> {
        Err(WcError::Other(
            "swaybg readiness is unavailable for this runtime".into(),
        ))
    }
    fn swaybg_pid_running(&mut self, pid: u32) -> Result<bool, WcError> {
        Ok(self.swaybg_pids()?.contains(&pid))
    }
    fn cleanup_failed_swaybg_launch(
        &mut self,
        _previous_pids: &[u32],
        _path: &str,
        _scope: &crate::target_commands::ExecutionScope,
    ) -> Result<(), WcError> {
        Err(WcError::Other(
            "swaybg failed-launch cleanup is unavailable for this runtime".into(),
        ))
    }
    fn awww_socket_ready(&mut self) -> AwwwReadiness;
    /// Command lines of the current user's processes, for renderer ownership
    /// observation. Fails closed: implementations that cannot inspect report
    /// an error rather than an empty list.
    fn renderer_command_lines(
        &mut self,
    ) -> Result<Vec<crate::runtime_observation::ProcessCommandLine>, WcError> {
        Err(WcError::Other(
            "renderer process inspection is unavailable for this runtime".into(),
        ))
    }
    fn awww_process_running(&mut self) -> bool {
        false
    }
    fn awww_environment(&mut self, _inspect_daemons: bool) -> Result<AwwwEnvironment, WcError> {
        Err(WcError::Other(
            "awww environment inspection is unavailable for this runtime".into(),
        ))
    }
    fn awww_query_json(&mut self) -> Result<String, WcError> {
        let result = self.command_output(Command::new("awww").args(["query", "--json"]))?;
        if !result.status.success() {
            return Err(WcError::Other(
                "awww query failed during output release".into(),
            ));
        }
        String::from_utf8(result.stdout).map_err(|error| WcError::Other(error.to_string()))
    }
}

/// Testable backend seam: [`ProcessIo`] plus stop/apply hooks for fakes and legacy.
///
/// Checked stops and awww daemon/clear policy live on drivers, not this trait.
pub trait BackendRuntime: ProcessIo {
    fn supports_output_recovery(&self) -> bool {
        false
    }
    /// Preflight external renderer availability before any destructive handoff.
    /// Test runtimes default to available; the system runtime probes PATH.
    fn ensure_backend_available(
        &mut self,
        _backend: wc_core::types::Backend,
        _storage: &StorageApi,
    ) -> Result<(), WcError> {
        Ok(())
    }
    fn stop_awww(&mut self);
    fn stop_mpvpaper(&mut self);
    /// Kill only mpvpaper processes whose argv output is a Single match in `outputs`.
    /// Fails closed when process inspection cannot complete.
    fn stop_mpvpaper_outputs(&mut self, outputs: &[String]) -> Result<(), WcError>;
    fn stop_swaybg(&mut self) {}
    fn stop_swaybg_outputs(&mut self, _outputs: &[String]) -> Result<(), WcError> {
        Err(WcError::Other(
            "swaybg scoped stop is unavailable for this runtime".into(),
        ))
    }
    fn stop_lwe(&mut self, s: Option<&StorageApi>);
    fn stop_lwe_outputs(&mut self, _outputs: &[String]) -> Result<(), WcError> {
        Err(WcError::Other(
            "LWE output-scoped stop is unavailable for this runtime".into(),
        ))
    }
    /// Apply LWE to explicit outputs (readiness + handoff included).
    ///
    /// System runtime delegates to the real implementation. Fakes must not
    /// launch or kill real linux-wallpaperengine processes.
    fn apply_lwe_to_outputs(
        &mut self,
        s: &StorageApi,
        project: &crate::linux_wallpaperengine::LinuxWallpaperEngineProject,
        outputs: &[String],
    ) -> Result<(), WcError>;
}

pub struct SystemBackendRuntime;

pub(crate) fn build_awww_daemon_command() -> Command {
    let mut cmd = Command::new("setsid");
    cmd.args(["-f", "awww-daemon", "--no-cache", "--format", "argb"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    cmd
}

pub(crate) fn wait_for_awww_socket_ready(
    runtime: &mut dyn ProcessIo,
    user: &crate::ProcessUserScope,
) -> Result<(), WcError> {
    let mut last_stderr = String::new();
    for _ in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(50));
        match runtime.awww_socket_ready() {
            AwwwReadiness::Ready => return Ok(()),
            AwwwReadiness::SocketMissing => {}
            AwwwReadiness::SocketPresentQueryFailed { stderr } => {
                last_stderr = stderr;
            }
        }
    }
    let socket_path = awww_socket_path()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "<unknown>".to_string());
    let wayland = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".to_string());
    if crate::awww::is_awww_daemon_running(user) {
        Err(WcError::Other(format!(
            "awww-daemon is running but socket is not ready for WAYLAND_DISPLAY={} \
             (expected {}); last query stderr: {}",
            wayland, socket_path, last_stderr
        )))
    } else {
        Err(WcError::Other(
            "awww-daemon failed to start. Check 'awww-daemon' is installed and your \
             compositor supports wlr-layer-shell."
                .into(),
        ))
    }
}

pub(crate) fn new_mpvpaper_pid_for_target<M>(
    current_pids: &[u32],
    previous_pids: &[u32],
    output: &str,
    path: &str,
    matches_target: &mut M,
) -> Option<u32>
where
    M: FnMut(u32, &str, &str) -> bool,
{
    current_pids
        .iter()
        .copied()
        .find(|pid| !previous_pids.contains(pid) && matches_target(*pid, output, path))
}

pub(crate) fn wait_for_mpvpaper_ready_with<P, M, S>(
    previous_pids: &[u32],
    output: &str,
    path: &str,
    mut probe: P,
    mut matches_target: M,
    mut sleep: S,
) -> Result<u32, WcError>
where
    P: FnMut() -> Result<Vec<u32>, WcError>,
    M: FnMut(u32, &str, &str) -> bool,
    S: FnMut(std::time::Duration),
{
    for poll in 0..=40 {
        let current_pids = probe()?;
        if let Some(pid) = new_mpvpaper_pid_for_target(
            &current_pids,
            previous_pids,
            output,
            path,
            &mut matches_target,
        ) {
            return Ok(pid);
        }
        if poll < 40 {
            sleep(std::time::Duration::from_millis(50));
        }
    }
    Err(WcError::Other(
        "mpvpaper failed to become ready: no new process for the requested output and wallpaper \
         appeared within 2 seconds"
            .into(),
    ))
}

pub(crate) fn wait_for_mpvpaper_stopped_with<P, S>(
    mut probe: P,
    mut sleep: S,
) -> Result<(), WcError>
where
    P: FnMut() -> Result<Vec<u32>, WcError>,
    S: FnMut(std::time::Duration),
{
    poll_until_pids_absent(&mut probe, &mut sleep, |pids| {
        format!("mpvpaper still running after stop: pids={pids:?}")
    })
}

fn remaining_single_output_pids(processes: &[MpvpaperProcess], outputs: &[String]) -> Vec<u32> {
    processes
        .iter()
        .filter_map(|process| match &process.selector {
            MpvpaperOutputSelector::Single(output) if outputs.iter().any(|o| o == output) => {
                Some(process.pid)
            }
            _ => None,
        })
        .collect()
}

pub(crate) fn wait_for_mpvpaper_outputs_stopped_with<P, S>(
    outputs: &[String],
    mut probe: P,
    mut sleep: S,
) -> Result<(), WcError>
where
    P: FnMut() -> Result<Vec<MpvpaperProcess>, WcError>,
    S: FnMut(std::time::Duration),
{
    let outputs = outputs.to_vec();
    poll_until_pids_absent(
        &mut || Ok(remaining_single_output_pids(&probe()?, &outputs)),
        &mut sleep,
        |pids| format!("mpvpaper still running on {outputs:?}: pids={pids:?}"),
    )
}

/// Shared stop-wait loop used by global and scoped mpvpaper stop verification.
fn poll_until_pids_absent<P, S, M>(
    probe_remaining: &mut P,
    sleep: &mut S,
    timeout_message: M,
) -> Result<(), WcError>
where
    P: FnMut() -> Result<Vec<u32>, WcError>,
    S: FnMut(std::time::Duration),
    M: FnOnce(Vec<u32>) -> String,
{
    let mut last = Vec::new();
    for poll in 0..=40 {
        last = probe_remaining()?;
        if last.is_empty() {
            return Ok(());
        }
        if poll == 40 {
            return Err(WcError::Other(timeout_message(last)));
        }
        sleep(std::time::Duration::from_millis(50));
    }
    Err(WcError::Other(timeout_message(last)))
}

pub(crate) fn wait_for_swaybg_ready_with<P, M, S>(
    previous_pids: &[u32],
    path: &str,
    scope: &crate::target_commands::ExecutionScope,
    mut probe: P,
    mut matches_target: M,
    mut sleep: S,
) -> Result<u32, WcError>
where
    P: FnMut() -> Result<Vec<u32>, WcError>,
    M: FnMut(u32, &str, &crate::target_commands::ExecutionScope) -> bool,
    S: FnMut(std::time::Duration),
{
    let mut candidate = None;
    for poll in 0..=40 {
        let current_pids = probe()?;
        if let Some(pid) = candidate {
            if current_pids.contains(&pid) && matches_target(pid, path, scope) {
                return Ok(pid);
            }
        }
        candidate = current_pids
            .into_iter()
            .find(|pid| !previous_pids.contains(pid) && matches_target(*pid, path, scope));
        if poll < 40 {
            sleep(std::time::Duration::from_millis(50));
        }
    }
    Err(WcError::Other(
        "swaybg failed to become ready: no new process for the requested outputs and wallpaper \
         appeared within 2 seconds"
            .into(),
    ))
}

pub(crate) fn wait_for_swaybg_stopped_with<P, S>(mut probe: P, mut sleep: S) -> Result<(), WcError>
where
    P: FnMut() -> Result<Vec<u32>, WcError>,
    S: FnMut(std::time::Duration),
{
    for poll in 0..=40 {
        let pids = probe()?;
        if pids.is_empty() {
            return Ok(());
        }
        if poll == 40 {
            return Err(WcError::Other(format!(
                "swaybg still running after stop: pids={pids:?}"
            )));
        }
        sleep(std::time::Duration::from_millis(50));
    }
    unreachable!("the bounded polling loop always returns")
}

/// Niri exposes mapped layer surfaces; other compositors stay unverified until
/// their adapter can supply equivalent evidence (a living process is not enough).
pub(crate) fn swaybg_surface_outputs() -> Result<Vec<String>, WcError> {
    if std::env::var_os("NIRI_SOCKET").is_none() {
        return Err(WcError::Other(
            "swaybg surface readiness is unverified on this compositor; no wallpapers were stopped"
                .into(),
        ));
    }
    let result = crate::deadline_command::output(
        Command::new("niri").args(["msg", "--json", "layers"]),
        std::time::Duration::from_millis(1500),
    )?;
    if !result.status.success() {
        return Err(WcError::Other(
            "cannot inspect compositor background surfaces".into(),
        ));
    }
    let rows: Vec<serde_json::Value> = serde_json::from_slice(&result.stdout)
        .map_err(|e| WcError::Other(format!("invalid compositor surface snapshot: {e}")))?;
    Ok(rows
        .iter()
        .filter(|row| row["namespace"] == "wallpaper" && row["layer"] == "Background")
        .filter_map(|row| row["output"].as_str().map(str::to_owned))
        .collect())
}

fn recipe_matches_argv(
    output: &str,
    recipe: &wc_core::display_assignment::RenderRecipe,
    argv: &[String],
) -> bool {
    use wc_core::display_assignment::RenderOptions;
    match &recipe.options {
        RenderOptions::Mpvpaper { options } => {
            if crate::mpvpaper::parse_launch_identity(argv)
                != Some((output.into(), recipe.media_path.clone()))
            {
                return false;
            }
            argv.iter()
                .position(|a| a == "-o")
                .and_then(|i| argv.get(i + 1))
                .is_some_and(|actual| {
                    actual
                        .rsplit_once(" --input-ipc-server=")
                        .map(|(raw, _)| raw)
                        .unwrap_or(actual)
                        .trim()
                        == options.trim()
                })
        }
        RenderOptions::Swaybg { .. } => {
            if !argv
                .first()
                .is_some_and(|a| crate::process_control::token_is_swaybg_program(a))
            {
                return false;
            }
            let values = recipe.options.config_entries();
            let expected = crate::target_commands::build_swaybg_launch_command(
                &recipe.media_path,
                &values["awww_resize"],
                &crate::target_commands::ExecutionScope::Named(vec![output.into()]),
            );
            expected.is_ok_and(|cmd| {
                cmd.get_args()
                    .map(|a| a.to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    == argv[1..]
            })
        }
        RenderOptions::LinuxWallpaperengine {
            fps, muted, volume, ..
        } => {
            if !crate::runtime_observation::parse_lwe_command_line(argv)
                .is_some_and(|rows| rows.iter().any(|(name, _)| *name == output))
            {
                return false;
            }
            let value = |key: &str| {
                argv.iter()
                    .position(|a| a == key)
                    .and_then(|i| argv.get(i + 1))
                    .map(String::as_str)
            };
            let values = recipe.options.config_entries();
            let scaling = values["linux_wallpaperengine_scaling"].as_str();
            value("--scaling").unwrap_or("default") == scaling
                && value("--fps").and_then(|s| s.parse::<u16>().ok()) == Some(*fps)
                && value("--volume").and_then(|s| s.parse::<u8>().ok())
                    == Some(if *muted { 0 } else { *volume })
        }
        RenderOptions::Awww { .. } => true, // The shared daemon exposes content, not a per-apply argv.
        RenderOptions::Feh { .. } => false,
    }
}

impl ProcessIo for SystemBackendRuntime {
    fn preflight_swaybg_observation(&mut self) -> Result<(), WcError> {
        swaybg_surface_outputs().map(|_| ())
    }
    fn is_niri_session(&mut self) -> Result<bool, WcError> {
        Ok(std::env::var_os("NIRI_SOCKET")
            .is_some_and(|socket| std::path::Path::new(&socket).exists()))
    }
    fn launch_swaybg(&mut self, command: &mut Command) -> Result<(), WcError> {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
        let child = command.spawn()?;
        crate::process_control::detach_and_reap_child(child, "wc-swaybg-reaper");
        Ok(())
    }
    fn renderer_identity_snapshot(&mut self) -> Result<String, WcError> {
        let mut identities = Vec::new();
        for process in self.renderer_command_lines()? {
            let Some(program) = process
                .argv
                .first()
                .and_then(|a| std::path::Path::new(a).file_name())
                .and_then(|a| a.to_str())
            else {
                continue;
            };
            if !matches!(
                program,
                "mpvpaper" | "swaybg" | "awww-daemon" | "linux-wallpaperengine"
            ) {
                continue;
            }
            let identity =
                crate::process_control::ProcessIdentity::read(process.pid).ok_or_else(|| {
                    WcError::Other("renderer identity changed during inspection".into())
                })?;
            identities.push((process.pid, identity));
        }
        identities.sort_by_key(|(pid, _)| *pid);
        Ok(format!("{identities:?}"))
    }
    fn verify_recipe(
        &mut self,
        output: &str,
        recipe: &wc_core::display_assignment::RenderRecipe,
        known_outputs: &[String],
    ) -> Result<(), WcError> {
        let rows = [wc_storage::sqlite::DisplayStateRow {
            target: wc_storage::sqlite::DisplayStateTarget::Output(output.into()),
            wallpaper_path: recipe.media_path.clone(),
            backend: recipe.options.backend().as_str().into(),
            updated_at: String::new(),
        }];
        let observations =
            crate::runtime_observation::observe_runtime_wallpapers(known_outputs, &rows);
        let observed = observations.iter().find(|row| row.output == output);
        if observed.is_some_and(|row| {
            row.status == crate::runtime_observation::RuntimeObservationStatus::Confirmed
        }) {
            use wc_core::display_assignment::RenderOptions;
            if !matches!(recipe.options, RenderOptions::Awww { .. }) {
                let processes = self.renderer_command_lines()?;
                if !processes
                    .iter()
                    .any(|process| recipe_matches_argv(output, recipe, &process.argv))
                {
                    return Err(WcError::Other(format!("cannot confirm saved rendering parameters on {output}; the live renderer differs from its recipe")));
                }
            }
            if matches!(recipe.options, RenderOptions::Swaybg { .. })
                && !swaybg_surface_outputs()?.iter().any(|name| name == output)
            {
                return Err(WcError::Other(format!(
                    "swaybg has no mapped background surface on {output}"
                )));
            }
            Ok(())
        } else {
            Err(WcError::Other(format!(
                "cannot confirm {} on {output}: {}",
                recipe.media_path,
                observed
                    .and_then(|row| row.reason.as_deref())
                    .unwrap_or("no live evidence")
            )))
        }
    }
    fn preflight_image(
        &mut self,
        path: &str,
    ) -> Result<Option<crate::image_media::ImageStamp>, WcError> {
        crate::image_media::preflight(path).map(Some)
    }
    fn prepare_mpvpaper_options(&mut self, options: &str, path: &str) -> Result<String, WcError> {
        crate::mpvpaper_media::prepare_options(options, path)
    }
    fn awww_process_running(&mut self) -> bool {
        crate::awww::is_awww_daemon_running(&crate::current_process_user())
    }
    fn awww_environment(&mut self, inspect_daemons: bool) -> Result<AwwwEnvironment, WcError> {
        use crate::runtime_observation::{RuntimeObservationIo, SystemRuntimeObservationIo};
        let version = crate::deadline_command::output(
            Command::new("awww").arg("--version"),
            AWWW_QUERY_TIMEOUT,
        )?;
        let daemon_commands = if inspect_daemons {
            SystemRuntimeObservationIo
                .current_user_process_command_lines()
                .map_err(WcError::Other)?
                .into_iter()
                .map(|p| p.argv)
                .collect()
        } else {
            Vec::new()
        };
        Ok(AwwwEnvironment {
            niri_socket_present: std::env::var_os("NIRI_SOCKET").is_some(),
            version: version
                .status
                .success()
                .then(|| String::from_utf8_lossy(&version.stdout).trim().to_owned()),
            daemon_commands,
        })
    }
    fn awww_query_json(&mut self) -> Result<String, WcError> {
        let result = crate::deadline_command::output(
            Command::new("awww").args(["query", "--json"]),
            AWWW_QUERY_TIMEOUT,
        )?;
        if !result.status.success() {
            return Err(WcError::Other(
                "awww query failed during output release".into(),
            ));
        }
        String::from_utf8(result.stdout).map_err(|error| WcError::Other(error.to_string()))
    }

    fn command_output(&mut self, command: &mut Command) -> Result<Output, WcError> {
        crate::deadline_command::output(command, APPLY_COMMAND_TIMEOUT)
    }

    fn command_status(
        &mut self,
        command: &mut Command,
    ) -> Result<std::process::ExitStatus, WcError> {
        crate::deadline_command::status(command, LAUNCH_COMMAND_TIMEOUT)
    }

    fn mpvpaper_pids(&mut self) -> Result<Vec<u32>, WcError> {
        crate::mpvpaper::running_pids()
    }

    fn renderer_command_lines(
        &mut self,
    ) -> Result<Vec<crate::runtime_observation::ProcessCommandLine>, WcError> {
        crate::runtime_observation::read_current_user_process_command_lines()
            .map_err(WcError::Other)
    }

    fn mpvpaper_processes(&mut self) -> Result<Vec<MpvpaperProcess>, WcError> {
        crate::mpvpaper::running_processes()
    }

    fn wait_for_mpvpaper_ready(
        &mut self,
        previous_pids: &[u32],
        output: &str,
        path: &str,
    ) -> Result<u32, WcError> {
        wait_for_mpvpaper_ready_with(
            previous_pids,
            output,
            path,
            || self.mpvpaper_pids(),
            |pid, output, path| {
                crate::mpvpaper::pid_matches_target(pid, output, path)
                    && crate::mpvpaper_media::media_ready(pid, path)
            },
            std::thread::sleep,
        )
    }

    fn mpvpaper_pid_running(&mut self, pid: u32) -> Result<bool, WcError> {
        Ok(self.mpvpaper_pids()?.contains(&pid))
    }

    fn cleanup_failed_mpvpaper_launch(
        &mut self,
        previous_pids: &[u32],
        output: &str,
        path: &str,
    ) -> Result<(), WcError> {
        crate::mpvpaper::stop_pids_started_after(previous_pids, output, path)
    }

    fn swaybg_pids(&mut self) -> Result<Vec<u32>, WcError> {
        crate::swaybg::running_pids()
    }

    fn wait_for_swaybg_ready(
        &mut self,
        previous_pids: &[u32],
        path: &str,
        scope: &crate::target_commands::ExecutionScope,
    ) -> Result<u32, WcError> {
        wait_for_swaybg_ready_with(
            previous_pids,
            path,
            scope,
            || self.swaybg_pids(),
            |pid, path, scope| {
                crate::swaybg::pid_matches_target(pid, path, scope)
                    && swaybg_surface_outputs().is_ok_and(|mapped| match scope {
                        crate::target_commands::ExecutionScope::Named(outputs) => {
                            outputs.iter().all(|output| mapped.contains(output))
                        }
                        crate::target_commands::ExecutionScope::AllDisplays => false,
                    })
            },
            std::thread::sleep,
        )
    }

    fn swaybg_pid_running(&mut self, pid: u32) -> Result<bool, WcError> {
        Ok(self.swaybg_pids()?.contains(&pid))
    }

    fn cleanup_failed_swaybg_launch(
        &mut self,
        previous_pids: &[u32],
        path: &str,
        scope: &crate::target_commands::ExecutionScope,
    ) -> Result<(), WcError> {
        crate::swaybg::stop_pids_started_after(previous_pids, path, scope)
    }

    fn awww_socket_ready(&mut self) -> AwwwReadiness {
        let path = match awww_socket_path() {
            Ok(p) => p,
            Err(_) => return AwwwReadiness::SocketMissing,
        };
        if !path.exists() {
            return AwwwReadiness::SocketMissing;
        }
        let mut command = Command::new("awww");
        command.arg("query");
        let output = crate::deadline_command::output(&mut command, AWWW_QUERY_TIMEOUT);
        match output {
            Ok(o) if o.status.success() => AwwwReadiness::Ready,
            Ok(o) => AwwwReadiness::SocketPresentQueryFailed {
                stderr: String::from_utf8_lossy(&o.stderr).trim().to_string(),
            },
            Err(e) => AwwwReadiness::SocketPresentQueryFailed {
                stderr: e.to_string(),
            },
        }
    }
}

impl BackendRuntime for SystemBackendRuntime {
    fn stop_swaybg_outputs(&mut self, outputs: &[String]) -> Result<(), WcError> {
        crate::swaybg_process::stop_outputs(outputs)
    }
    fn supports_output_recovery(&self) -> bool {
        true
    }
    fn ensure_backend_available(
        &mut self,
        backend: wc_core::types::Backend,
        storage: &StorageApi,
    ) -> Result<(), WcError> {
        match crate::driver::driver_for(backend) {
            Some(driver) => driver.ensure_available(storage),
            None if backend == wc_core::types::Backend::Feh => Err(WcError::Other(
                wc_core::types::Backend::FEH_REMOVED_MESSAGE.into(),
            )),
            None => Ok(()),
        }
    }

    fn stop_awww(&mut self) {
        crate::awww::stop_awww();
    }

    fn stop_mpvpaper(&mut self) {
        crate::mpvpaper::stop_mpvpaper();
    }

    fn stop_mpvpaper_outputs(&mut self, outputs: &[String]) -> Result<(), WcError> {
        crate::mpvpaper::stop_outputs(outputs)
    }

    fn stop_swaybg(&mut self) {
        crate::swaybg::stop_swaybg();
    }

    fn stop_lwe(&mut self, s: Option<&StorageApi>) {
        crate::linux_wallpaperengine::stop(s);
    }

    fn stop_lwe_outputs(&mut self, outputs: &[String]) -> Result<(), WcError> {
        crate::lwe_process::stop_outputs(outputs)
    }

    fn apply_lwe_to_outputs(
        &mut self,
        s: &StorageApi,
        project: &crate::linux_wallpaperengine::LinuxWallpaperEngineProject,
        outputs: &[String],
    ) -> Result<(), WcError> {
        crate::linux_wallpaperengine::apply_to_outputs(s, project.clone(), outputs)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::time::Duration;

    use wc_core::error::WcError;

    use super::{
        new_mpvpaper_pid_for_target, wait_for_mpvpaper_outputs_stopped_with,
        wait_for_mpvpaper_ready_with, wait_for_mpvpaper_stopped_with, wait_for_swaybg_ready_with,
        MpvpaperOutputSelector, MpvpaperProcess,
    };
    use crate::target_commands::ExecutionScope;

    #[test]
    fn matching_path_with_different_mpv_options_is_not_recoverable_recipe_evidence() {
        use wc_core::display_assignment::{Presentation, RenderOptions, RenderRecipe};
        let recipe = RenderRecipe {
            schema_version: 1,
            source: "/video.mp4".into(),
            media_path: "/video.mp4".into(),
            presentation: Presentation::Original,
            options: RenderOptions::Mpvpaper {
                options: "--volume=20 --panscan=1".into(),
            },
        };
        let mut argv: Vec<String> = [
            "mpvpaper",
            "--fork",
            "-o",
            "--volume=20 --panscan=1 --input-ipc-server=/tmp/control.sock",
            "A",
            "--",
            "/video.mp4",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        assert!(super::recipe_matches_argv("A", &recipe, &argv));
        argv[3] = "--volume=90 --panscan=1 --input-ipc-server=/tmp/control.sock".into();
        assert!(!super::recipe_matches_argv("A", &recipe, &argv));
        assert!(!super::recipe_matches_argv("B", &recipe, &argv));
    }

    fn sample_process(pid: u32, output: &str) -> MpvpaperProcess {
        MpvpaperProcess {
            pid,
            selector: MpvpaperOutputSelector::Single(output.into()),
            path: "/walls/a.mp4".into(),
        }
    }

    #[test]
    fn mpvpaper_stop_waits_for_a_process_to_exit() {
        let mut probes: VecDeque<Result<Vec<u32>, WcError>> =
            VecDeque::from([Ok(vec![41]), Ok(vec![])]);
        let mut sleeps = Vec::new();

        wait_for_mpvpaper_stopped_with(
            || probes.pop_front().expect("unexpected extra PID probe"),
            |duration| sleeps.push(duration),
        )
        .unwrap();

        assert_eq!(sleeps, vec![Duration::from_millis(50)]);
    }

    #[test]
    fn scoped_mpvpaper_stop_succeeds_when_target_outputs_already_clear() {
        let mut probes: VecDeque<Result<Vec<MpvpaperProcess>, WcError>> =
            VecDeque::from([Ok(vec![sample_process(99, "DP-8")])]);
        let mut sleeps = Vec::new();

        wait_for_mpvpaper_outputs_stopped_with(
            &["eDP-1".into()],
            || probes.pop_front().expect("unexpected extra process probe"),
            |duration| sleeps.push(duration),
        )
        .unwrap();

        assert!(sleeps.is_empty());
    }

    #[test]
    fn scoped_mpvpaper_stop_waits_until_target_output_clears() {
        let mut probes: VecDeque<Result<Vec<MpvpaperProcess>, WcError>> = VecDeque::from([
            Ok(vec![
                sample_process(41, "eDP-1"),
                sample_process(99, "DP-8"),
            ]),
            Ok(vec![sample_process(99, "DP-8")]),
        ]);
        let mut sleeps = Vec::new();

        wait_for_mpvpaper_outputs_stopped_with(
            &["eDP-1".into()],
            || probes.pop_front().expect("unexpected extra process probe"),
            |duration| sleeps.push(duration),
        )
        .unwrap();

        assert_eq!(sleeps, vec![Duration::from_millis(50)]);
    }

    #[test]
    fn scoped_mpvpaper_stop_reports_remaining_target_pids_after_timeout() {
        let mut probe_count = 0;
        let mut sleeps = Vec::new();

        let error = wait_for_mpvpaper_outputs_stopped_with(
            &["eDP-1".into()],
            || {
                probe_count += 1;
                Ok(vec![
                    sample_process(41, "eDP-1"),
                    sample_process(99, "DP-8"),
                ])
            },
            |duration| sleeps.push(duration),
        )
        .unwrap_err();

        assert!(error.to_string().contains("pids=[41]"));
        assert!(!error.to_string().contains("99"));
        assert_eq!(probe_count, 41);
        assert_eq!(sleeps, vec![Duration::from_millis(50); 40]);
    }

    #[test]
    fn mpvpaper_stop_reports_remaining_pids_after_two_seconds() {
        let mut probe_count = 0;
        let mut sleeps = Vec::new();

        let error = wait_for_mpvpaper_stopped_with(
            || {
                probe_count += 1;
                Ok(vec![41])
            },
            |duration| sleeps.push(duration),
        )
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "mpvpaper still running after stop: pids=[41]"
        );
        assert_eq!(probe_count, 41);
        assert_eq!(sleeps, vec![Duration::from_millis(50); 40]);
    }

    #[test]
    fn mpvpaper_wait_returns_new_pid_after_an_immediate_old_only_probe() {
        let mut probes: VecDeque<Result<Vec<u32>, WcError>> =
            VecDeque::from([Ok(vec![41]), Ok(vec![41, 52])]);
        let mut sleeps = Vec::new();

        let pid = wait_for_mpvpaper_ready_with(
            &[41],
            "eDP-1",
            "/walls/target.mp4",
            || probes.pop_front().expect("unexpected extra PID probe"),
            |pid, output, path| pid == 52 && output == "eDP-1" && path == "/walls/target.mp4",
            |duration| sleeps.push(duration),
        )
        .unwrap();

        assert_eq!(pid, 52);
        assert_eq!(sleeps, vec![Duration::from_millis(50)]);
    }

    #[test]
    fn swaybg_wait_accepts_only_a_new_process_for_the_exact_scope_and_path() {
        let scope = ExecutionScope::named(vec!["eDP-1".into()]).unwrap();
        let mut probes: VecDeque<Result<Vec<u32>, WcError>> =
            VecDeque::from([Ok(vec![41]), Ok(vec![41, 52]), Ok(vec![41, 52])]);
        let mut sleeps = Vec::new();

        let pid = wait_for_swaybg_ready_with(
            &[41],
            "/walls/target.png",
            &scope,
            || probes.pop_front().expect("unexpected extra PID probe"),
            |pid, path, actual_scope| {
                pid == 52 && path == "/walls/target.png" && actual_scope == &scope
            },
            |duration| sleeps.push(duration),
        )
        .unwrap();

        assert_eq!(pid, 52);
        assert_eq!(sleeps, vec![Duration::from_millis(50); 2]);
    }

    #[test]
    fn mpvpaper_wait_times_out_after_two_seconds_of_old_only_probes() {
        let mut probe_count = 0;
        let mut sleeps = Vec::new();

        let error = wait_for_mpvpaper_ready_with(
            &[41],
            "eDP-1",
            "/walls/target.mp4",
            || {
                probe_count += 1;
                Ok(vec![41])
            },
            |_pid, _output, _path| true,
            |duration| sleeps.push(duration),
        )
        .unwrap_err();

        assert!(error.to_string().contains("within 2 seconds"));
        assert_eq!(probe_count, 41);
        assert_eq!(sleeps, vec![Duration::from_millis(50); 40]);
    }

    #[test]
    fn mpvpaper_wait_propagates_the_first_probe_error_without_sleeping() {
        let mut probe_count = 0;
        let mut sleeps = Vec::new();

        let error = wait_for_mpvpaper_ready_with(
            &[41],
            "eDP-1",
            "/walls/target.mp4",
            || {
                probe_count += 1;
                Err(WcError::Other("mpvpaper PID probe failed".into()))
            },
            |_pid, _output, _path| true,
            |duration| sleeps.push(duration),
        )
        .unwrap_err();

        assert_eq!(error.to_string(), "mpvpaper PID probe failed");
        assert_eq!(probe_count, 1);
        assert!(sleeps.is_empty());
    }

    #[test]
    fn mpvpaper_wait_does_not_accept_new_pids_for_another_output_or_path() {
        let mut probe_count = 0;

        let error = wait_for_mpvpaper_ready_with(
            &[41],
            "eDP-1",
            "/walls/target.mp4",
            || {
                probe_count += 1;
                Ok(vec![41, 52, 63])
            },
            |pid, output, path| {
                let observed = match pid {
                    52 => Some(("HDMI-A-1", "/walls/target.mp4")),
                    63 => Some(("eDP-1", "/walls/other.mp4")),
                    _ => None,
                };
                observed.is_some_and(|(observed_output, observed_path)| {
                    observed_output == output && observed_path == path
                })
            },
            |_| {},
        )
        .unwrap_err();

        assert!(error.to_string().contains("requested output and wallpaper"));
        assert_eq!(probe_count, 41);
    }

    #[test]
    fn new_mpvpaper_pid_selects_only_a_matching_pid_absent_from_previous_snapshot() {
        let mut matches_target = |pid, output: &str, path: &str| {
            pid == 52 && output == "eDP-1" && path == "/walls/target.mp4"
        };
        assert_eq!(
            new_mpvpaper_pid_for_target(
                &[41, 52, 63],
                &[41, 63],
                "eDP-1",
                "/walls/target.mp4",
                &mut matches_target,
            ),
            Some(52)
        );
    }

    #[test]
    fn new_mpvpaper_pid_returns_none_without_a_new_matching_pid() {
        let mut matches_target = |_pid, _output: &str, _path: &str| true;
        assert_eq!(
            new_mpvpaper_pid_for_target(
                &[41, 63],
                &[41, 63],
                "eDP-1",
                "/walls/target.mp4",
                &mut matches_target,
            ),
            None
        );
    }
}
