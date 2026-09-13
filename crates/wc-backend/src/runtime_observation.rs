//! Read-only reconciliation of persisted display assignments with renderer state.
//!
//! Persistence is only an expectation. An assignment is confirmed only when
//! the corresponding renderer exposes matching runtime evidence.

use std::collections::HashMap;
use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use wc_storage::sqlite::{DisplayStateRow, DisplayStateTarget};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessCommandLine {
    pub pid: u32,
    pub argv: Vec<String>,
}

pub trait RuntimeObservationIo {
    fn mpvpaper_media_loaded(&self, _process: &ProcessCommandLine, _path: &str) -> bool {
        true
    }
    fn awww_query_json(&self) -> Result<String, String>;
    fn current_user_process_command_lines(&self) -> Result<Vec<ProcessCommandLine>, String>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemRuntimeObservationIo;

const AWWW_QUERY_TIMEOUT: Duration = Duration::from_secs(2);
const COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(10);

fn awww_query_arguments() -> [&'static str; 3] {
    ["query", "--all", "--json"]
}

impl RuntimeObservationIo for SystemRuntimeObservationIo {
    fn mpvpaper_media_loaded(&self, process: &ProcessCommandLine, path: &str) -> bool {
        crate::mpvpaper_media::media_ready(process.pid, path)
    }
    fn awww_query_json(&self) -> Result<String, String> {
        let mut command = Command::new("awww");
        command
            .args(awww_query_arguments())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let output = output_with_timeout(command, AWWW_QUERY_TIMEOUT, "awww query --json")?;
        let stdout = String::from_utf8(output.stdout)
            .map_err(|error| format!("awww query returned non-UTF-8 output: {error}"))?;
        if output.status.success() {
            Ok(stdout)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let detail = stderr.trim();
            Err(if detail.is_empty() {
                format!("awww query exited with {}", output.status)
            } else {
                detail.to_string()
            })
        }
    }

    fn current_user_process_command_lines(&self) -> Result<Vec<ProcessCommandLine>, String> {
        read_current_user_process_command_lines()
    }
}

fn output_with_timeout(
    mut command: Command,
    timeout: Duration,
    label: &str,
) -> Result<Output, String> {
    let mut child = command
        .spawn()
        .map_err(|error| format!("could not execute {label}: {error}"))?;
    let deadline = Instant::now() + timeout;

    loop {
        let status = match child.try_wait() {
            Ok(status) => status,
            Err(error) => {
                let cleanup = terminate_timed_out_child(child, label);
                return Err(format!("could not wait for {label}: {error}{cleanup}"));
            }
        };
        match status {
            Some(_) => {
                return child
                    .wait_with_output()
                    .map_err(|error| format!("could not collect {label} output: {error}"));
            }
            None => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    let cleanup = terminate_timed_out_child(child, label);
                    return Err(format!(
                        "{label} timed out after {} ms{cleanup}",
                        timeout.as_millis()
                    ));
                }
                thread::sleep(remaining.min(COMMAND_POLL_INTERVAL));
            }
        }
    }
}

fn terminate_timed_out_child(mut child: Child, label: &str) -> String {
    match child.kill() {
        Ok(()) => child
            .wait()
            .err()
            .map(|error| format!("; could not reap it: {error}"))
            .unwrap_or_default(),
        Err(kill_error) => match child.try_wait() {
            Ok(Some(_)) => format!("; kill raced with {label} exit: {kill_error}"),
            wait_result => {
                let wait_error = wait_result
                    .err()
                    .map(|error| format!("; follow-up status failed: {error}"))
                    .unwrap_or_default();
                match thread::Builder::new()
                    .name("wallpaper-console-probe-reaper".into())
                    .spawn(move || {
                        let _ = child.wait();
                    })
                {
                    Ok(_) => format!(
                        "; could not kill it: {kill_error}{wait_error}; reaping in background"
                    ),
                    Err(spawn_error) => format!(
                        "; could not kill it: {kill_error}{wait_error}; could not start reaper: {spawn_error}"
                    ),
                }
            }
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeObservationStatus {
    Confirmed,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeWallpaperObservation {
    pub output: String,
    pub wallpaper_path: Option<String>,
    pub status: RuntimeObservationStatus,
    pub reason: Option<String>,
}

pub fn observe_runtime_wallpapers(
    connected_outputs: &[String],
    persisted: &[DisplayStateRow],
) -> Vec<RuntimeWallpaperObservation> {
    observe_runtime_wallpapers_with(connected_outputs, persisted, &SystemRuntimeObservationIo)
}

#[derive(Debug, Clone)]
struct ExpectedAssignment<'a> {
    wallpaper_path: &'a str,
    backend: &'a str,
}

pub fn observe_runtime_wallpapers_with(
    connected_outputs: &[String],
    persisted: &[DisplayStateRow],
    io: &dyn RuntimeObservationIo,
) -> Vec<RuntimeWallpaperObservation> {
    let expected = expected_assignments(connected_outputs, persisted);
    let awww = match io.awww_query_json() {
        Ok(raw) if raw.trim().is_empty() => {
            AwwwEvidence::Unavailable("awww returned no namespace evidence".into())
        }
        Ok(raw) => match parse_awww_query_json(&raw) {
            Ok(evidence) => AwwwEvidence::Ready(evidence),
            Err(error) => AwwwEvidence::Ambiguous(error),
        },
        Err(error) => AwwwEvidence::Unavailable(error),
    };
    let processes = io.current_user_process_command_lines();
    let awww_daemon_running = processes.as_ref().is_ok_and(|processes| {
        processes
            .iter()
            .any(|process| program_is(&process.argv, "awww-daemon"))
    });
    let awww = if processes.as_ref().is_ok_and(|processes| {
        processes.iter().any(|process| {
            program_is(&process.argv, "awww-daemon")
                && !crate::awww::alpha_format_supported(&process.argv)
        })
    }) {
        match awww {
            AwwwEvidence::Ready(mut entries) => {
                for entry in entries.values_mut() {
                    if matches!(entry, AwwwOutputEvidence::Transparent) {
                        *entry = AwwwOutputEvidence::Color;
                    }
                }
                AwwwEvidence::Ready(entries)
            }
            other => other,
        }
    } else {
        awww
    };
    let mpvpaper = processes
        .as_ref()
        .map(|processes| collect_mpvpaper_evidence(processes, connected_outputs));
    let swaybg = processes
        .as_ref()
        .map(|processes| collect_swaybg_evidence(processes, connected_outputs));
    let lwe = processes
        .as_ref()
        .map(|processes| collect_lwe_evidence(processes));

    connected_outputs
        .iter()
        .map(|output| {
            let Some(saved) = expected.get(output.as_str()) else {
                return unknown(output, "No saved runtime assignment for this output.");
            };
            if let Some(reason) = runtime_ambiguity_reason(
                output,
                &awww,
                awww_daemon_running,
                &mpvpaper,
                &swaybg,
                &lwe,
            ) {
                return unknown(output, &reason);
            }
            match crate::driver::driver_for_persisted_name(saved.backend) {
                Some(driver) => match driver.backend() {
                    wc_core::types::Backend::Awww => {
                        let outputs = match &awww {
                            AwwwEvidence::Ready(outputs) => outputs,
                            AwwwEvidence::Unavailable(error) | AwwwEvidence::Ambiguous(error) => {
                                return unknown(
                                    output,
                                    &format!("awww runtime query failed: {error}"),
                                );
                            }
                        };
                        match outputs.get(output) {
                            Some(AwwwOutputEvidence::Image(path))
                                if path == saved.wallpaper_path =>
                            {
                                RuntimeWallpaperObservation {
                                    output: output.clone(),
                                    wallpaper_path: Some(path.clone()),
                                    status: RuntimeObservationStatus::Confirmed,
                                    reason: None,
                                }
                            }
                            Some(AwwwOutputEvidence::Color | AwwwOutputEvidence::Transparent) => {
                                unknown(output, "awww is displaying a color instead of an image.")
                            }
                            _ => unknown(output, "awww did not confirm the saved wallpaper path."),
                        }
                    }
                    wc_core::types::Backend::Mpvpaper => {
                        let observation = observe_mpvpaper(output, saved.wallpaper_path, &mpvpaper);
                        if observation.status == RuntimeObservationStatus::Confirmed {
                            let loaded = processes.as_ref().is_ok_and(|processes| processes.iter().any(|process| {
                                program_is(&process.argv, "mpvpaper")
                                    && parse_mpvpaper_command_line(&process.argv)
                                    .is_some_and(|(selector, path)| path == saved.wallpaper_path && mpvpaper_selected_outputs(selector, connected_outputs).is_some_and(|outputs| outputs.contains(output)))
                                    && io.mpvpaper_media_loaded(process, saved.wallpaper_path)
                            }));
                            if !loaded { return unknown(output, "mpv did not confirm loaded video frames for the saved wallpaper."); }
                        }
                        observation
                    }
                    wc_core::types::Backend::Swaybg => {
                        observe_swaybg(output, saved.wallpaper_path, &swaybg)
                    }
                    wc_core::types::Backend::Feh => unknown(
                        output,
                        "feh is a one-shot X root pixmap setter and exposes no process evidence.",
                    ),
                    wc_core::types::Backend::LinuxWallpaperEngine => {
                        observe_lwe(output, saved.wallpaper_path, &lwe)
                    }
                    wc_core::types::Backend::Unsupported => {
                        unknown(output, "Saved renderer has no matching runtime evidence.")
                    }
                },
                None => unknown(output, "Saved renderer has no matching runtime evidence."),
            }
        })
        .collect()
}

fn runtime_ambiguity_reason(
    output: &str,
    awww: &AwwwEvidence,
    awww_daemon_running: bool,
    mpvpaper: &Result<MpvpaperEvidence, &String>,
    swaybg: &Result<SwaybgEvidence, &String>,
    lwe: &Result<LweEvidence, &String>,
) -> Option<String> {
    if let AwwwEvidence::Ambiguous(error) = awww {
        return Some(format!("awww runtime evidence is ambiguous: {error}"));
    }
    if let AwwwEvidence::Unavailable(error) = awww {
        if awww_daemon_running {
            return Some(format!(
                "awww daemon is running but its runtime evidence is unavailable: {error}"
            ));
        }
    }
    if let Err(error) = mpvpaper {
        return Some(format!("Renderer process inspection failed: {error}"));
    }
    if mpvpaper
        .as_ref()
        .is_ok_and(|evidence| evidence.malformed_process)
    {
        return Some("A running mpvpaper process has an ambiguous command line.".into());
    }
    if swaybg
        .as_ref()
        .is_ok_and(|evidence| evidence.malformed_process)
    {
        return Some("A running swaybg process has an ambiguous command line.".into());
    }
    if lwe
        .as_ref()
        .is_ok_and(|evidence| evidence.malformed_process)
    {
        return Some("Running linux-wallpaperengine processes have ambiguous ownership.".into());
    }

    let renderer_count = usize::from(matches!(
        awww,
        AwwwEvidence::Ready(evidence) if evidence.get(output).is_some_and(|entry| !matches!(entry, AwwwOutputEvidence::Transparent))
    )) + usize::from(
        mpvpaper
            .as_ref()
            .is_ok_and(|evidence| evidence.by_output.contains_key(output)),
    ) + usize::from(
        swaybg
            .as_ref()
            .is_ok_and(|evidence| evidence.by_output.contains_key(output)),
    ) + usize::from(
        lwe.as_ref()
            .is_ok_and(|evidence| evidence.by_output.contains_key(output)),
    );
    (renderer_count > 1).then(|| format!("Conflicting renderer processes claim output {output}."))
}

#[derive(Debug, Default)]
struct SwaybgEvidence {
    by_output: HashMap<String, Vec<String>>,
    malformed_process: bool,
}

fn collect_swaybg_evidence(
    processes: &[ProcessCommandLine],
    connected_outputs: &[String],
) -> SwaybgEvidence {
    let mut evidence = SwaybgEvidence::default();
    for process in processes {
        if !program_is(&process.argv, "swaybg") {
            continue;
        }
        let Some(assignments) = parse_swaybg_command_line(&process.argv, connected_outputs) else {
            evidence.malformed_process = true;
            continue;
        };
        for (output, path) in assignments {
            evidence.by_output.entry(output).or_default().push(path);
        }
    }
    evidence
}

fn parse_swaybg_command_line(
    argv: &[String],
    connected_outputs: &[String],
) -> Option<Vec<(String, String)>> {
    let mut named = Vec::new();
    let mut global_image = None;
    let mut current_output: Option<&str> = None;
    let mut index = 1;
    while index < argv.len() {
        match argv[index].as_str() {
            "--output" | "-o" => {
                let output = argv.get(index + 1)?.trim();
                if output.is_empty()
                    || !connected_outputs
                        .iter()
                        .any(|connected| connected == output)
                {
                    return None;
                }
                current_output = Some(output);
                index += 2;
            }
            "--image" | "-i" => {
                let path = argv.get(index + 1)?.trim();
                if path.is_empty() {
                    return None;
                }
                if let Some(output) = current_output {
                    named.push((output.to_string(), path.to_string()));
                } else if global_image.replace(path.to_string()).is_some() {
                    return None;
                }
                index += 2;
            }
            "--mode" | "-m" | "--color" | "-c" => {
                argv.get(index + 1)?;
                index += 2;
            }
            _ => return None,
        }
    }
    if !named.is_empty() {
        if global_image.is_some() {
            return None;
        }
        let mut seen = std::collections::HashSet::new();
        if named.iter().any(|(output, _)| !seen.insert(output.clone())) {
            return None;
        }
        return Some(named);
    }
    global_image.map(|path| {
        connected_outputs
            .iter()
            .cloned()
            .map(|output| (output, path.clone()))
            .collect()
    })
}

fn observe_swaybg(
    output: &str,
    expected_path: &str,
    probe: &Result<SwaybgEvidence, &String>,
) -> RuntimeWallpaperObservation {
    let evidence = match probe {
        Ok(evidence) => evidence,
        Err(error) => return unknown(output, &format!("process inspection failed: {error}")),
    };
    if evidence.malformed_process {
        return unknown(output, "A running swaybg command line was ambiguous.");
    }
    let Some(paths) = evidence.by_output.get(output) else {
        return unknown(output, "No swaybg process owns this output.");
    };
    if paths.len() != 1 {
        return unknown(output, "Multiple swaybg processes claim this output.");
    }
    if paths[0] != expected_path {
        return unknown(output, "swaybg did not confirm the saved wallpaper path.");
    }
    RuntimeWallpaperObservation {
        output: output.to_string(),
        wallpaper_path: Some(paths[0].clone()),
        status: RuntimeObservationStatus::Confirmed,
        reason: None,
    }
}

#[derive(Debug)]
enum AwwwEvidence {
    Ready(HashMap<String, AwwwOutputEvidence>),
    Unavailable(String),
    Ambiguous(String),
}

#[derive(Debug)]
enum AwwwOutputEvidence {
    Image(String),
    Color,
    Transparent,
}

#[derive(Debug, Default)]
struct MpvpaperEvidence {
    by_output: HashMap<String, Vec<String>>,
    malformed_process: bool,
}

fn collect_mpvpaper_evidence(
    processes: &[ProcessCommandLine],
    connected_outputs: &[String],
) -> MpvpaperEvidence {
    let mut evidence = MpvpaperEvidence::default();
    for process in processes {
        if !program_is(&process.argv, "mpvpaper") {
            continue;
        }
        match parse_mpvpaper_command_line(&process.argv) {
            Some((selector, path)) => {
                let Some(outputs) = mpvpaper_selected_outputs(selector, connected_outputs) else {
                    evidence.malformed_process = true;
                    continue;
                };
                for output in outputs {
                    evidence
                        .by_output
                        .entry(output)
                        .or_default()
                        .push(path.to_string());
                }
            }
            None => evidence.malformed_process = true,
        }
    }
    evidence
}

fn mpvpaper_selected_outputs(selector: &str, connected_outputs: &[String]) -> Option<Vec<String>> {
    let selector = selector.trim();
    if selector.eq_ignore_ascii_case("ALL") {
        return (!connected_outputs.is_empty()).then(|| connected_outputs.to_vec());
    }
    if let Some(output) = connected_outputs
        .iter()
        .find(|output| output.as_str() == selector)
    {
        return Some(vec![output.clone()]);
    }

    let mut selected = Vec::new();
    for candidate in selector.split_whitespace() {
        let output = connected_outputs
            .iter()
            .find(|output| output.as_str() == candidate)?;
        if selected.contains(output) {
            return None;
        }
        selected.push(output.clone());
    }
    (!selected.is_empty()).then_some(selected)
}

fn observe_mpvpaper(
    output: &str,
    expected_path: &str,
    probe: &Result<MpvpaperEvidence, &String>,
) -> RuntimeWallpaperObservation {
    let evidence = match probe {
        Ok(evidence) => evidence,
        Err(error) => {
            return unknown(output, &format!("process inspection failed: {error}"));
        }
    };
    if evidence.malformed_process {
        return unknown(output, "A running mpvpaper command line was ambiguous.");
    }
    let Some(paths) = evidence.by_output.get(output) else {
        return unknown(output, "No mpvpaper process owns this output.");
    };
    if paths.len() != 1 {
        return unknown(output, "Multiple mpvpaper processes claim this output.");
    }
    if paths[0] != expected_path {
        return unknown(output, "mpvpaper did not confirm the saved wallpaper path.");
    }
    RuntimeWallpaperObservation {
        output: output.to_string(),
        wallpaper_path: Some(paths[0].clone()),
        status: RuntimeObservationStatus::Confirmed,
        reason: None,
    }
}

fn program_is(argv: &[String], expected: &str) -> bool {
    argv.first()
        .and_then(|program| Path::new(program).file_name())
        .is_some_and(|program| program == expected)
}

fn parse_mpvpaper_command_line(argv: &[String]) -> Option<(&str, &str)> {
    let mut separators = argv
        .iter()
        .enumerate()
        .filter(|(_, arg)| arg.as_str() == "--");
    let (separator, _) = separators.next()?;
    if separators.next().is_some() || separator < 2 || separator + 2 != argv.len() {
        return None;
    }
    let output = argv[separator - 1].trim();
    let path = argv[separator + 1].trim();
    (!output.is_empty() && !path.is_empty()).then_some((output, path))
}

fn decode_proc_cmdline(raw: &[u8]) -> Result<Vec<String>, String> {
    // Unix permits arbitrary bytes in arguments. Unrelated programs must not
    // make renderer observation fail just because they opened such a filename.
    // Renderer arguments remain strict because they determine output ownership.
    let program = raw.split(|byte| *byte == 0).next().unwrap_or_default();
    let basename = program
        .rsplit(|byte| *byte == b'/')
        .next()
        .unwrap_or_default();
    let renderer = matches!(
        basename,
        b"mpvpaper" | b"awww-daemon" | b"swaybg" | b"linux-wallpaperengine"
    );
    let mut arguments = Vec::new();
    let mut fields = raw.split(|byte| *byte == 0).peekable();
    while let Some(field) = fields.next() {
        if field.is_empty() && fields.peek().is_none() {
            break;
        }
        arguments.push(match std::str::from_utf8(field) {
            Ok(argument) => argument.to_string(),
            Err(error) if renderer => {
                return Err(format!("process command line is not UTF-8: {error}"));
            }
            Err(_) => String::from_utf8_lossy(field).into_owned(),
        });
    }
    Ok(arguments)
}

#[cfg(unix)]
pub(crate) fn read_current_user_process_command_lines() -> Result<Vec<ProcessCommandLine>, String> {
    use std::os::unix::fs::MetadataExt;

    let current_uid = std::fs::metadata("/proc/self")
        .map_err(|error| format!("could not inspect /proc/self: {error}"))?
        .uid();
    let entries =
        std::fs::read_dir("/proc").map_err(|error| format!("could not inspect /proc: {error}"))?;
    let mut processes = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| format!("could not enumerate /proc: {error}"))?;
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(format!("could not inspect process {pid}: {error}"));
            }
        };
        if metadata.uid() != current_uid {
            continue;
        }
        let raw = match std::fs::read(entry.path().join("cmdline")) {
            Ok(raw) => raw,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(format!(
                    "could not read process {pid} command line: {error}"
                ));
            }
        };
        if raw.is_empty() {
            continue;
        }
        let argv = decode_proc_cmdline(&raw)
            .map_err(|error| format!("could not decode process {pid}: {error}"))?;
        if !argv.is_empty() {
            processes.push(ProcessCommandLine { pid, argv });
        }
    }
    processes.sort_by_key(|process| process.pid);
    Ok(processes)
}

#[cfg(not(unix))]
pub(crate) fn read_current_user_process_command_lines() -> Result<Vec<ProcessCommandLine>, String> {
    Err("runtime renderer process inspection is supported only on Unix".into())
}

// ── Live output ownership (apply planning input) ───────────────────────────
//
// Persisted display_state rows are restore preferences, not proof of a running
// renderer. Apply planning instead uses this live snapshot, which separates
// "confirmed occupied" from "confirmed vacant" from "cannot tell". An
// observation failure is never read as an empty desktop.

/// Ownership of one connected output, derived from live evidence only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputOwnership {
    /// Exactly one backend verifiably owns a surface/process on this output.
    Occupied(wc_core::types::Backend),
    /// No known renderer owns this output.
    Vacant,
    /// Evidence is missing or contradictory; ownership cannot be determined.
    Uncertain(String),
}

/// One bounded, consistent observation pass over all connected outputs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OutputOwnershipSnapshot {
    /// One entry per connected output, in input order.
    pub outputs: Vec<(String, OutputOwnership)>,
    /// Backends with any live evidence, including ambiguous evidence. An
    /// all-displays replacement retires these even when per-output ownership
    /// could not be resolved.
    pub implicated_backends: Vec<wc_core::types::Backend>,
    /// The renderer set itself could not be enumerated. Even a global
    /// replacement cannot establish a complete retirement plan from this scan.
    pub process_inspection_error: Option<String>,
}

/// Observe which backend owns each connected output via one socket probe, at
/// most one awww query, and one process scan.
pub fn observe_output_ownership(
    connected_outputs: &[String],
    runtime: &mut dyn crate::runtime::ProcessIo,
) -> OutputOwnershipSnapshot {
    use wc_core::types::Backend;

    let mut claims: HashMap<String, Vec<Backend>> = HashMap::new();
    let mut uncertainty: Vec<String> = Vec::new();
    let mut implicated: Vec<Backend> = Vec::new();
    let mut implicate = |backend: Backend| {
        if !implicated.contains(&backend) {
            implicated.push(backend);
        }
    };

    let (processes, process_inspection_error) = match runtime.renderer_command_lines() {
        Ok(processes) => (processes, None),
        Err(error) => {
            let reason = format!("renderer process inspection failed: {error}");
            uncertainty.push(reason.clone());
            (Vec::new(), Some(reason))
        }
    };
    let process_scan_failed = process_inspection_error.is_some();

    // awww surfaces (shared daemon, per-output evidence).
    match runtime.awww_socket_ready() {
        crate::runtime::AwwwReadiness::SocketMissing => {
            if processes
                .iter()
                .any(|process| program_is(&process.argv, "awww-daemon"))
            {
                implicate(Backend::Awww);
                uncertainty.push(
                    "awww-daemon is running but its default socket is missing; output ownership cannot be verified"
                        .into(),
                );
            }
        }
        crate::runtime::AwwwReadiness::SocketPresentQueryFailed { stderr } => {
            implicate(Backend::Awww);
            uncertainty.push(format!(
                "awww socket is present but its query failed: {stderr}"
            ));
        }
        crate::runtime::AwwwReadiness::Ready => match runtime.awww_query_json() {
            Err(error) => {
                implicate(Backend::Awww);
                uncertainty.push(format!("awww runtime query failed: {error}"));
            }
            Ok(raw) => match parse_awww_query_json(&raw) {
                Err(error) => {
                    implicate(Backend::Awww);
                    uncertainty.push(format!("awww runtime evidence is ambiguous: {error}"));
                }
                Ok(mut entries) => {
                    // Without an alpha-capable daemon a "transparent" surface is
                    // actually opaque and still owns the output.
                    let opaque_daemon = processes.iter().any(|process| {
                        program_is(&process.argv, "awww-daemon")
                            && !crate::awww::alpha_format_supported(&process.argv)
                    });
                    if opaque_daemon {
                        for entry in entries.values_mut() {
                            if matches!(entry, AwwwOutputEvidence::Transparent) {
                                *entry = AwwwOutputEvidence::Color;
                            }
                        }
                    }
                    for (output, evidence) in entries {
                        if !connected_outputs.contains(&output) {
                            continue;
                        }
                        match evidence {
                            AwwwOutputEvidence::Image(_) | AwwwOutputEvidence::Color => {
                                implicate(Backend::Awww);
                                claims.entry(output).or_default().push(Backend::Awww);
                            }
                            AwwwOutputEvidence::Transparent => {}
                        }
                    }
                }
            },
        },
    }

    if !process_scan_failed {
        // mpvpaper: argv selector decides coverage; wildcards/multi-output
        // processes claim every output they cover, never a "safe single".
        for process in &processes {
            if !program_is(&process.argv, "mpvpaper") {
                continue;
            }
            implicate(Backend::Mpvpaper);
            let Some((selector, _path)) = parse_mpvpaper_command_line(&process.argv) else {
                uncertainty.push(format!(
                    "mpvpaper process {} has an ambiguous command line",
                    process.pid
                ));
                continue;
            };
            match crate::mpvpaper::classify_selector(selector) {
                crate::mpvpaper::MpvpaperOutputSelector::Single(output) => {
                    if connected_outputs.contains(&output) {
                        claims.entry(output).or_default().push(Backend::Mpvpaper);
                    }
                }
                crate::mpvpaper::MpvpaperOutputSelector::Wildcard => {
                    for output in connected_outputs {
                        claims
                            .entry(output.clone())
                            .or_default()
                            .push(Backend::Mpvpaper);
                    }
                }
                crate::mpvpaper::MpvpaperOutputSelector::Multi(outputs) => {
                    let mut claimed = false;
                    for output in outputs {
                        if connected_outputs.contains(&output) {
                            claims.entry(output).or_default().push(Backend::Mpvpaper);
                            claimed = true;
                        }
                    }
                    if !claimed {
                        uncertainty.push(format!(
                            "mpvpaper process {} covers no known output; its selector is ambiguous",
                            process.pid
                        ));
                    }
                }
                crate::mpvpaper::MpvpaperOutputSelector::Unparseable => {
                    uncertainty.push(format!(
                        "mpvpaper process {} has an unparseable output selector",
                        process.pid
                    ));
                }
            }
        }

        for process in &processes {
            if !program_is(&process.argv, "swaybg") {
                continue;
            }
            implicate(Backend::Swaybg);
            match parse_swaybg_command_line(&process.argv, connected_outputs) {
                Some(assignments) => {
                    for (output, _path) in assignments {
                        claims.entry(output).or_default().push(Backend::Swaybg);
                    }
                }
                None => uncertainty.push(format!(
                    "swaybg process {} has an ambiguous command line",
                    process.pid
                )),
            }
        }

        for process in processes
            .iter()
            .filter(|process| program_is(&process.argv, "linux-wallpaperengine"))
        {
            implicate(Backend::LinuxWallpaperEngine);
            match parse_lwe_command_line(&process.argv) {
                Some(assignments) => {
                    for (output, _) in assignments {
                        if connected_outputs.iter().any(|known| known == output) {
                            let owners = claims.entry(output.to_string()).or_default();
                            if owners.contains(&Backend::LinuxWallpaperEngine) {
                                uncertainty.push(format!("multiple linux-wallpaperengine assignments claim output {output}"));
                            }
                            owners.push(Backend::LinuxWallpaperEngine);
                        }
                    }
                }
                None => uncertainty.push(format!(
                    "linux-wallpaperengine process {} has an ambiguous command line",
                    process.pid
                )),
            }
        }
    }

    let outputs = connected_outputs
        .iter()
        .map(|output| {
            let ownership = if !uncertainty.is_empty() {
                OutputOwnership::Uncertain(uncertainty.join("; "))
            } else {
                let mut owners = claims.get(output).cloned().unwrap_or_default();
                owners.sort_by_key(|backend| backend.as_str());
                owners.dedup();
                match owners.as_slice() {
                    [] => OutputOwnership::Vacant,
                    [backend] => OutputOwnership::Occupied(*backend),
                    _ => OutputOwnership::Uncertain(format!(
                        "conflicting renderer processes claim output {output}"
                    )),
                }
            };
            (output.clone(), ownership)
        })
        .collect();

    OutputOwnershipSnapshot {
        outputs,
        implicated_backends: implicated,
        process_inspection_error,
    }
}

#[derive(Debug, Default)]
struct LweEvidence {
    by_output: HashMap<String, Vec<String>>,
    malformed_process: bool,
}

fn collect_lwe_evidence(processes: &[ProcessCommandLine]) -> LweEvidence {
    let mut evidence = LweEvidence::default();
    for process in processes {
        if !program_is(&process.argv, "linux-wallpaperengine") {
            continue;
        }
        match parse_lwe_command_line(&process.argv) {
            Some(assignments) => {
                for (output, renderer_id) in assignments {
                    evidence
                        .by_output
                        .entry(output.to_string())
                        .or_default()
                        .push(renderer_id.to_string());
                }
            }
            None => evidence.malformed_process = true,
        }
    }
    evidence
}

pub(crate) fn parse_lwe_command_line(argv: &[String]) -> Option<Vec<(&str, &str)>> {
    let mut assignments = Vec::new();
    let mut index = 1;
    while index < argv.len() {
        let screen_root = argv[index] == "--screen-root" || argv[index] == "-r";
        let screen_span = argv[index] == "--screen-span";
        if screen_root || screen_span {
            let selector = argv.get(index + 1)?.trim();
            if !matches!(argv.get(index + 2)?.as_str(), "--bg" | "-b") {
                return None;
            }
            let renderer_id = argv.get(index + 3)?.trim();
            if selector.is_empty() || renderer_id.is_empty() {
                return None;
            }
            if screen_root {
                assignments.push((selector, renderer_id));
            } else {
                let mut span_outputs = selector.split(',').map(str::trim).peekable();
                span_outputs.peek()?;
                for output in span_outputs {
                    if output.is_empty() {
                        return None;
                    }
                    assignments.push((output, renderer_id));
                }
            }
            index += 4;
        } else {
            match argv[index].as_str() {
                "--scaling"
                | "--clamp"
                | "--layer"
                | "--fps"
                | "-f"
                | "--volume"
                | "-v"
                | "--assets-dir"
                | "--screenshot"
                | "--screenshot-delay"
                | "--fullscreen-pause-ignore-appid"
                | "--set-property"
                | "--property"
                | "--render-debug" => {
                    argv.get(index + 1)?;
                    index += 2;
                }
                "--silent"
                | "-s"
                | "--noautomute"
                | "--no-audio-processing"
                | "--no-fullscreen-pause"
                | "--fullscreen-pause-only-active"
                | "--disable-particles"
                | "--disable-mouse"
                | "--disable-parallax"
                | "--list-properties"
                | "-l"
                | "--dump-structure"
                | "-z" => index += 1,
                _ => return None,
            }
        }
    }
    (!assignments.is_empty()).then_some(assignments)
}

fn observe_lwe(
    output: &str,
    expected_path: &str,
    probe: &Result<LweEvidence, &String>,
) -> RuntimeWallpaperObservation {
    let evidence = match probe {
        Ok(evidence) => evidence,
        Err(error) => {
            return unknown(output, &format!("process inspection failed: {error}"));
        }
    };
    if evidence.malformed_process {
        return unknown(
            output,
            "A running linux-wallpaperengine command line was ambiguous.",
        );
    }
    let Some(renderer_ids) = evidence.by_output.get(output) else {
        return unknown(output, "No linux-wallpaperengine process owns this output.");
    };
    if renderer_ids.len() != 1 {
        return unknown(
            output,
            "linux-wallpaperengine reported conflicting assignments for this output.",
        );
    }
    let expected_id = match crate::linux_wallpaperengine::project_from_path(expected_path) {
        Ok(project) => project.workshop_id.unwrap_or(project.project_path),
        Err(error) => {
            return unknown(
                output,
                &format!("Saved Wallpaper Engine project cannot be verified: {error}"),
            );
        }
    };
    if renderer_ids[0] != expected_id {
        return unknown(
            output,
            "linux-wallpaperengine did not confirm the saved scene identity.",
        );
    }
    RuntimeWallpaperObservation {
        output: output.to_string(),
        wallpaper_path: Some(expected_path.to_string()),
        status: RuntimeObservationStatus::Confirmed,
        reason: None,
    }
}

fn expected_assignments<'a>(
    connected_outputs: &[String],
    persisted: &'a [DisplayStateRow],
) -> HashMap<String, ExpectedAssignment<'a>> {
    let mut expected = HashMap::new();
    if let Some(all) = persisted
        .iter()
        .find(|row| matches!(row.target, DisplayStateTarget::AllDisplays))
    {
        for output in connected_outputs {
            expected.insert(
                output.clone(),
                ExpectedAssignment {
                    wallpaper_path: &all.wallpaper_path,
                    backend: &all.backend,
                },
            );
        }
    }
    for row in persisted {
        if let DisplayStateTarget::Output(output) = &row.target {
            if connected_outputs.contains(output) {
                expected.insert(
                    output.clone(),
                    ExpectedAssignment {
                        wallpaper_path: &row.wallpaper_path,
                        backend: &row.backend,
                    },
                );
            }
        }
    }
    expected
}

fn parse_awww_query_json(raw: &str) -> Result<HashMap<String, AwwwOutputEvidence>, String> {
    let root: serde_json::Value =
        serde_json::from_str(raw).map_err(|error| format!("invalid awww JSON: {error}"))?;
    let namespaces = root
        .as_object()
        .ok_or_else(|| "awww JSON root must be a namespace object".to_string())?;
    let mut evidence = HashMap::new();
    for outputs in namespaces.values() {
        let outputs = outputs
            .as_array()
            .ok_or_else(|| "awww namespace must contain an output array".to_string())?;
        for output in outputs {
            let name = output
                .get("name")
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| "awww output is missing a nonblank name".to_string())?;
            let displaying = output
                .get("displaying")
                .and_then(serde_json::Value::as_object)
                .ok_or_else(|| format!("awww output {name} has invalid display evidence"))?;
            let image = displaying
                .get("image")
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.trim().is_empty());
            let output_evidence = match (image, displaying.contains_key("color")) {
                (Some(path), false) => AwwwOutputEvidence::Image(path.to_string()),
                (None, true)
                    if displaying
                        .get("color")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(crate::awww::is_transparent_color) =>
                {
                    AwwwOutputEvidence::Transparent
                }
                (None, true) => AwwwOutputEvidence::Color,
                _ => {
                    return Err(format!("awww output {name} has ambiguous display evidence"));
                }
            };
            if evidence.insert(name.to_string(), output_evidence).is_some() {
                return Err(format!("awww returned duplicate output {name}"));
            }
        }
    }
    Ok(evidence)
}

fn unknown(output: &str, reason: &str) -> RuntimeWallpaperObservation {
    RuntimeWallpaperObservation {
        output: output.to_string(),
        wallpaper_path: None,
        status: RuntimeObservationStatus::Unknown,
        reason: Some(reason.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    use super::{
        awww_query_arguments, decode_proc_cmdline, output_with_timeout, parse_swaybg_command_line,
    };

    #[test]
    fn awww_query_covers_every_running_namespace() {
        assert_eq!(awww_query_arguments(), ["query", "--all", "--json"]);
    }

    #[test]
    fn proc_cmdline_decoder_preserves_spaces_inside_nul_delimited_arguments() {
        assert_eq!(
            decode_proc_cmdline(b"/usr/bin/mpvpaper\0HDMI-A-1\0--\0/walls/night sky.mp4\0")
                .unwrap(),
            vec![
                "/usr/bin/mpvpaper",
                "HDMI-A-1",
                "--",
                "/walls/night sky.mp4"
            ]
        );
    }

    #[test]
    fn unrelated_non_utf8_arguments_do_not_poison_renderer_observation() {
        let argv = decode_proc_cmdline(b"/usr/bin/python3\0script.py\0filename-\xff\0").unwrap();
        assert_eq!(argv[0], "/usr/bin/python3");
        assert_eq!(argv.len(), 3);
        assert!(argv[2].starts_with("filename-"));
        assert!(decode_proc_cmdline(b"/usr/bin/mpvpaper\0eDP-\xff\0--\0/a.mp4\0").is_err());
    }

    #[test]
    fn swaybg_command_line_maps_named_and_global_images_to_outputs() {
        let outputs = vec!["eDP-1".to_string(), "HDMI-A-1".to_string()];
        let named = [
            "swaybg",
            "--output",
            "eDP-1",
            "--image",
            "/walls/a.png",
            "--mode",
            "fill",
        ]
        .map(str::to_string);
        assert_eq!(
            parse_swaybg_command_line(&named, &outputs),
            Some(vec![("eDP-1".into(), "/walls/a.png".into())])
        );

        let global = ["swaybg", "--image", "/walls/a.png", "--mode", "fit"].map(str::to_string);
        assert_eq!(
            parse_swaybg_command_line(&global, &outputs),
            Some(vec![
                ("eDP-1".into(), "/walls/a.png".into()),
                ("HDMI-A-1".into(), "/walls/a.png".into()),
            ])
        );
    }

    #[cfg(unix)]
    #[test]
    fn command_output_is_collected_before_timeout() {
        let mut command = Command::new("sh");
        command
            .args(["-c", "printf ready"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let output = output_with_timeout(command, Duration::from_secs(1), "test probe")
            .expect("the short probe must finish");

        assert!(output.status.success());
        assert_eq!(output.stdout, b"ready");
    }

    #[cfg(unix)]
    #[test]
    fn command_output_timeout_kills_a_stuck_probe_promptly() {
        let mut command = Command::new("sh");
        command
            .args(["-c", "exec sleep 5"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let started = Instant::now();

        let error = output_with_timeout(command, Duration::from_millis(20), "test probe")
            .expect_err("the sleeping probe must time out");

        assert!(error.contains("timed out"), "unexpected error: {error}");
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "timeout helper did not return promptly"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn command_output_timeout_reaps_the_probe_process() {
        let temp = tempfile::tempdir().unwrap();
        let pid_file = temp.path().join("probe.pid");
        let mut command = Command::new("sh");
        command
            .args(["-c", "printf '%s' \"$$\" > \"$1\"; exec sleep 5", "probe"])
            .arg(&pid_file)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        output_with_timeout(command, Duration::from_millis(100), "test probe")
            .expect_err("the sleeping probe must time out");

        let pid = std::fs::read_to_string(pid_file).expect("probe must publish its pid");
        assert!(
            !std::path::Path::new("/proc").join(pid.trim()).exists(),
            "the timed-out child must be killed and reaped"
        );
    }

    mod ownership {
        use crate::runtime::AwwwReadiness;
        use crate::runtime_observation::{observe_output_ownership, OutputOwnership};
        use crate::test_support::FakeRuntime;
        use wc_core::types::Backend;

        fn dual() -> Vec<String> {
            vec!["eDP-1".to_string(), "DP-8".to_string()]
        }

        fn ownership_of<'a>(
            snapshot: &'a crate::runtime_observation::OutputOwnershipSnapshot,
            output: &str,
        ) -> &'a OutputOwnership {
            &snapshot
                .outputs
                .iter()
                .find(|(name, _)| name == output)
                .unwrap_or_else(|| panic!("missing output {output}"))
                .1
        }

        #[test]
        fn live_daemon_without_default_socket_is_uncertain_not_vacant() {
            let mut rt = FakeRuntime {
                extra_command_lines: vec![vec![
                    "awww-daemon".into(),
                    "--namespace".into(),
                    "custom".into(),
                ]],
                awww_readiness_sequence: std::cell::RefCell::new(vec![
                    AwwwReadiness::SocketMissing,
                ]),
                ..Default::default()
            };
            let snapshot = observe_output_ownership(&dual(), &mut rt);
            for output in dual() {
                assert!(
                    matches!(ownership_of(&snapshot, &output), OutputOwnership::Uncertain(reason) if reason.contains("socket"))
                );
            }
            assert_eq!(snapshot.implicated_backends, [Backend::Awww]);
        }

        #[test]
        fn vacant_desktop_after_stop_despite_saved_preferences() {
            // Nothing running: saved rows are irrelevant to observation.
            let mut rt = FakeRuntime::default();
            let snapshot = observe_output_ownership(&dual(), &mut rt);
            assert_eq!(ownership_of(&snapshot, "eDP-1"), &OutputOwnership::Vacant);
            assert_eq!(ownership_of(&snapshot, "DP-8"), &OutputOwnership::Vacant);
            assert!(snapshot.implicated_backends.is_empty());
        }

        #[test]
        fn live_renderers_are_occupied_even_without_saved_rows() {
            let mut rt = FakeRuntime {
                awww_displayed: vec![("eDP-1".into(), "/walls/a.jpg".into())],
                mpvpaper_process_table: vec![crate::runtime::MpvpaperProcess::for_output(
                    42,
                    "DP-8",
                    "/walls/b.mp4",
                )],
                ..Default::default()
            };
            let snapshot = observe_output_ownership(&dual(), &mut rt);
            assert_eq!(
                ownership_of(&snapshot, "eDP-1"),
                &OutputOwnership::Occupied(Backend::Awww)
            );
            assert_eq!(
                ownership_of(&snapshot, "DP-8"),
                &OutputOwnership::Occupied(Backend::Mpvpaper)
            );
        }

        #[test]
        fn wildcard_mpvpaper_occupies_every_connected_output() {
            let mut rt = FakeRuntime {
                mpvpaper_process_table: vec![crate::runtime::MpvpaperProcess::for_output(
                    9,
                    "*",
                    "/walls/all.mp4",
                )],
                ..Default::default()
            };
            let snapshot = observe_output_ownership(&dual(), &mut rt);
            for output in dual() {
                assert_eq!(
                    ownership_of(&snapshot, &output),
                    &OutputOwnership::Occupied(Backend::Mpvpaper)
                );
            }
        }

        #[test]
        fn unparseable_mpvpaper_makes_ownership_uncertain_not_vacant() {
            let mut rt = FakeRuntime {
                mpvpaper_process_table: vec![crate::runtime::MpvpaperProcess {
                    pid: 9,
                    selector: crate::runtime::MpvpaperOutputSelector::Unparseable,
                    path: "/walls/x.mp4".into(),
                }],
                ..Default::default()
            };
            let snapshot = observe_output_ownership(&dual(), &mut rt);
            for output in dual() {
                assert!(
                    matches!(
                        ownership_of(&snapshot, &output),
                        OutputOwnership::Uncertain(_)
                    ),
                    "unparseable selector must not read as vacant"
                );
            }
            assert!(snapshot.implicated_backends.contains(&Backend::Mpvpaper));
        }

        #[test]
        fn awww_socket_present_query_failed_is_uncertain_not_vacant() {
            let mut rt = FakeRuntime {
                awww_readiness_sequence: std::cell::RefCell::new(vec![
                    AwwwReadiness::SocketPresentQueryFailed {
                        stderr: "timeout".into(),
                    },
                ]),
                ..Default::default()
            };
            let snapshot = observe_output_ownership(&dual(), &mut rt);
            for output in dual() {
                assert!(matches!(
                    ownership_of(&snapshot, &output),
                    OutputOwnership::Uncertain(_)
                ));
            }
            assert!(snapshot.implicated_backends.contains(&Backend::Awww));
        }

        #[test]
        fn process_scan_failure_is_uncertain_for_every_output() {
            let mut rt = FakeRuntime {
                process_scan_error: Some("proc unreadable".into()),
                ..Default::default()
            };
            let snapshot = observe_output_ownership(&dual(), &mut rt);
            for output in dual() {
                assert!(matches!(
                    ownership_of(&snapshot, &output),
                    OutputOwnership::Uncertain(_)
                ));
            }
        }

        #[test]
        fn conflicting_renderers_on_one_output_are_uncertain() {
            let mut rt = FakeRuntime {
                awww_displayed: vec![("eDP-1".into(), "/walls/a.jpg".into())],
                mpvpaper_process_table: vec![crate::runtime::MpvpaperProcess::for_output(
                    42,
                    "eDP-1",
                    "/walls/b.mp4",
                )],
                ..Default::default()
            };
            let snapshot = observe_output_ownership(&dual(), &mut rt);
            assert!(matches!(
                ownership_of(&snapshot, "eDP-1"),
                OutputOwnership::Uncertain(_)
            ));
            assert_eq!(ownership_of(&snapshot, "DP-8"), &OutputOwnership::Vacant);
        }

        #[test]
        fn transparent_release_frees_output_only_with_alpha_daemon() {
            // Alpha-capable daemon: transparent surface means released.
            let mut rt = FakeRuntime {
                extra_command_lines: vec![vec![
                    "awww-daemon".into(),
                    "--no-cache".into(),
                    "--format".into(),
                    "argb".into(),
                ]],
                command_output_args: vec![vec![
                    "clear".into(),
                    "--outputs".into(),
                    "eDP-1".into(),
                    "00000000".into(),
                ]],
                ..Default::default()
            };
            let snapshot = observe_output_ownership(&dual(), &mut rt);
            assert_eq!(
                ownership_of(&snapshot, "eDP-1"),
                &OutputOwnership::Vacant,
                "transparent surface on an alpha daemon is released"
            );

            // Opaque daemon: the same "transparent" color displays as black.
            let mut rt = FakeRuntime {
                extra_command_lines: vec![vec!["awww-daemon".into()]],
                command_output_args: vec![vec![
                    "clear".into(),
                    "--outputs".into(),
                    "eDP-1".into(),
                    "00000000".into(),
                ]],
                ..Default::default()
            };
            let snapshot = observe_output_ownership(&dual(), &mut rt);
            assert_eq!(
                ownership_of(&snapshot, "eDP-1"),
                &OutputOwnership::Occupied(Backend::Awww),
                "without alpha the surface is opaque and still owns the output"
            );
        }

        #[test]
        fn independent_lwe_processes_claim_only_their_own_outputs() {
            let mut rt = FakeRuntime {
                extra_command_lines: dual()
                    .iter()
                    .map(|output| {
                        vec![
                            "linux-wallpaperengine".into(),
                            "--screen-root".into(),
                            output.clone(),
                            "--bg".into(),
                            "42".into(),
                        ]
                    })
                    .collect(),
                ..Default::default()
            };
            let snapshot = observe_output_ownership(&dual(), &mut rt);
            for output in dual() {
                assert_eq!(
                    ownership_of(&snapshot, &output),
                    &OutputOwnership::Occupied(Backend::LinuxWallpaperEngine)
                );
            }
            rt.extra_command_lines
                .push(rt.extra_command_lines[0].clone());
            let snapshot = observe_output_ownership(&dual(), &mut rt);
            assert!(matches!(
                ownership_of(&snapshot, "eDP-1"),
                OutputOwnership::Uncertain(_)
            ));
        }

        #[test]
        fn lwe_shared_process_claims_each_listed_output() {
            let mut rt = FakeRuntime {
                lwe_outputs: vec!["eDP-1".into(), "DP-8".into()],
                ..Default::default()
            };
            let snapshot = observe_output_ownership(&dual(), &mut rt);
            for output in dual() {
                assert_eq!(
                    ownership_of(&snapshot, &output),
                    &OutputOwnership::Occupied(Backend::LinuxWallpaperEngine)
                );
            }
        }
    }
}
