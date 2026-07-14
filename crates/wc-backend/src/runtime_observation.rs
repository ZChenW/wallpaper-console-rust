//! Read-only reconciliation of persisted display assignments with renderer state.
//!
//! Persistence is only an expectation. An assignment is confirmed only when
//! the corresponding renderer exposes matching runtime evidence.

use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Stdio};

use wc_storage::sqlite::{DisplayStateRow, DisplayStateTarget};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessCommandLine {
    pub pid: u32,
    pub argv: Vec<String>,
}

pub trait RuntimeObservationIo {
    fn awww_query_json(&self) -> Result<String, String>;
    fn current_user_process_command_lines(&self) -> Result<Vec<ProcessCommandLine>, String>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemRuntimeObservationIo;

fn awww_query_arguments() -> [&'static str; 3] {
    ["query", "--all", "--json"]
}

impl RuntimeObservationIo for SystemRuntimeObservationIo {
    fn awww_query_json(&self) -> Result<String, String> {
        let output = Command::new("awww")
            .args(awww_query_arguments())
            .stdin(Stdio::null())
            .output()
            .map_err(|error| format!("could not execute awww query --json: {error}"))?;
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
    let mpvpaper = processes
        .as_ref()
        .map(|processes| collect_mpvpaper_evidence(processes, connected_outputs));
    let lwe = processes
        .as_ref()
        .map(|processes| collect_lwe_evidence(processes));

    connected_outputs
        .iter()
        .map(|output| {
            let Some(saved) = expected.get(output.as_str()) else {
                return unknown(output, "No saved runtime assignment for this output.");
            };
            if let Some(reason) =
                runtime_ambiguity_reason(output, &awww, awww_daemon_running, &mpvpaper, &lwe)
            {
                return unknown(output, &reason);
            }
            match saved.backend {
                "awww" => {
                    let outputs = match &awww {
                        AwwwEvidence::Ready(outputs) => outputs,
                        AwwwEvidence::Unavailable(error) | AwwwEvidence::Ambiguous(error) => {
                            return unknown(output, &format!("awww runtime query failed: {error}"));
                        }
                    };
                    match outputs.get(output) {
                        Some(AwwwOutputEvidence::Image(path)) if path == saved.wallpaper_path => {
                            RuntimeWallpaperObservation {
                                output: output.clone(),
                                wallpaper_path: Some(path.clone()),
                                status: RuntimeObservationStatus::Confirmed,
                                reason: None,
                            }
                        }
                        Some(AwwwOutputEvidence::Color) => {
                            unknown(output, "awww is displaying a color instead of an image.")
                        }
                        _ => unknown(output, "awww did not confirm the saved wallpaper path."),
                    }
                }
                "mpvpaper" => observe_mpvpaper(output, saved.wallpaper_path, &mpvpaper),
                "linux-wallpaperengine" => observe_lwe(output, saved.wallpaper_path, &lwe),
                _ => unknown(output, "Saved renderer has no matching runtime evidence."),
            }
        })
        .collect()
}

fn runtime_ambiguity_reason(
    output: &str,
    awww: &AwwwEvidence,
    awww_daemon_running: bool,
    mpvpaper: &Result<MpvpaperEvidence, &String>,
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
    if lwe
        .as_ref()
        .is_ok_and(|evidence| evidence.malformed_process || evidence.process_count > 1)
    {
        return Some("Running linux-wallpaperengine processes have ambiguous ownership.".into());
    }

    let renderer_count = usize::from(matches!(
        awww,
        AwwwEvidence::Ready(evidence) if evidence.contains_key(output)
    )) + usize::from(
        mpvpaper
            .as_ref()
            .is_ok_and(|evidence| evidence.by_output.contains_key(output)),
    ) + usize::from(
        lwe.as_ref()
            .is_ok_and(|evidence| evidence.by_output.contains_key(output)),
    );
    (renderer_count > 1).then(|| format!("Conflicting renderer processes claim output {output}."))
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
    let mut arguments = Vec::new();
    let mut fields = raw.split(|byte| *byte == 0).peekable();
    while let Some(field) = fields.next() {
        if field.is_empty() && fields.peek().is_none() {
            break;
        }
        arguments.push(
            std::str::from_utf8(field)
                .map_err(|error| format!("process command line is not UTF-8: {error}"))?
                .to_string(),
        );
    }
    Ok(arguments)
}

#[cfg(unix)]
fn read_current_user_process_command_lines() -> Result<Vec<ProcessCommandLine>, String> {
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
fn read_current_user_process_command_lines() -> Result<Vec<ProcessCommandLine>, String> {
    Err("runtime renderer process inspection is supported only on Unix".into())
}

#[derive(Debug, Default)]
struct LweEvidence {
    by_output: HashMap<String, Vec<String>>,
    process_count: usize,
    malformed_process: bool,
}

fn collect_lwe_evidence(processes: &[ProcessCommandLine]) -> LweEvidence {
    let mut evidence = LweEvidence::default();
    for process in processes {
        if !program_is(&process.argv, "linux-wallpaperengine") {
            continue;
        }
        evidence.process_count += 1;
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

fn parse_lwe_command_line(argv: &[String]) -> Option<Vec<(&str, &str)>> {
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
            index += 1;
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
    if evidence.process_count > 1 {
        return unknown(
            output,
            "Multiple linux-wallpaperengine processes make ownership ambiguous.",
        );
    }
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
    use super::{awww_query_arguments, decode_proc_cmdline};

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
}
