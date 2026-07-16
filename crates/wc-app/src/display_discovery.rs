use std::collections::HashSet;
use std::time::{Duration, Instant};

#[cfg(test)]
use crate::command_probe::ProbeOutput;
use crate::command_probe::{run_probe, ProbeError, ProbeErrorKind};
use crate::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrimaryProbe {
    Niri,
    Hyprland,
    Sway,
    Xrandr,
}

/// Maximum time budget for the primary compositor probe.
const PRIMARY_PROBE_DEADLINE: Duration = Duration::from_millis(1500);

/// Shared time budget for all display probes combined.
const OVERALL_DISPLAY_DEADLINE: Duration = Duration::from_secs(3);

/// Discover the output names exposed by the active compositor/session.
///
/// Compositor IPC is preferred because it remains available when no wallpaper
/// renderer is running. `awww query` is retained only as a compatibility
/// fallback for sessions without a supported compositor probe.
///
/// The primary compositor probe gets at most 1.5 seconds. All probes combined
/// have a 3-second overall deadline. Timed-out children are killed and reaped.
pub fn discover_connected_outputs() -> Result<Vec<String>, AppError> {
    discover_connected_outputs_with_deadline(|name| std::env::var(name).ok())
}

fn discover_connected_outputs_with_deadline<E>(env: E) -> Result<Vec<String>, AppError>
where
    E: Fn(&str) -> Option<String>,
{
    let mut failures = Vec::new();
    let overall_deadline = Instant::now() + OVERALL_DISPLAY_DEADLINE;

    if let Some(probe) = select_primary_probe(&env) {
        let primary_budget = remaining_until(overall_deadline).min(PRIMARY_PROBE_DEADLINE);
        if primary_budget > Duration::ZERO {
            match run_primary_probe_with_deadline(probe, primary_budget) {
                Ok(outputs) => return Ok(outputs),
                Err(error) => failures.push(error),
            }
        }
    }

    match run_awww_probe_with_absolute_deadline(overall_deadline, &mut failures) {
        Ok(outputs) => Ok(outputs),
        Err(()) => Err(discovery_error(failures)),
    }
}

// ── test-only injection seam ──────────────────────────────────────────
//
// The production code uses `command_probe::run_probe` with real deadlines.
// Tests inject a mock `run` callback to exercise parsers and fallback logic
// without spawning real compositor CLIs.

#[cfg(test)]
pub(crate) fn discover_connected_outputs_with_mock<E, R>(
    env: E,
    mut run: R,
) -> Result<Vec<String>, AppError>
where
    E: Fn(&str) -> Option<String>,
    R: FnMut(&str, &[&str]) -> Result<ProbeOutput, String>,
{
    let mut failures = Vec::new();

    if let Some(probe) = select_primary_probe(&env) {
        match run_primary_probe_with_mock(probe, &mut run) {
            Ok(outputs) => return Ok(outputs),
            Err(error) => failures.push(error),
        }
    }

    match run_awww_probe_with_mock(&mut run, &mut failures) {
        Ok(outputs) => Ok(outputs),
        Err(()) => Err(discovery_error(failures)),
    }
}

#[cfg(test)]
fn run_primary_probe_with_mock<R>(probe: PrimaryProbe, run: &mut R) -> Result<Vec<String>, String>
where
    R: FnMut(&str, &[&str]) -> Result<ProbeOutput, String>,
{
    let (label, program, args): (&str, &str, &[&str]) = match probe {
        PrimaryProbe::Niri => ("niri", "niri", &["msg", "-j", "outputs"]),
        PrimaryProbe::Hyprland => ("hyprland", "hyprctl", &["-j", "monitors"]),
        PrimaryProbe::Sway => ("sway", "swaymsg", &["-t", "get_outputs", "-r"]),
        PrimaryProbe::Xrandr => ("xrandr", "xrandr", &["--query"]),
    };
    let output = run(program, args).map_err(|error| format!("{label}: {error}"))?;
    if !output.success {
        return Err(format!(
            "{label}: {}",
            command_failure_detail(&output.stderr)
        ));
    }
    let parsed = match probe {
        PrimaryProbe::Niri => parse_niri_outputs(&output.stdout),
        PrimaryProbe::Hyprland => parse_hyprland_outputs(&output.stdout),
        PrimaryProbe::Sway => parse_sway_outputs(&output.stdout),
        PrimaryProbe::Xrandr => parse_xrandr_outputs(&output.stdout),
    };
    parsed.map_err(|error| format!("{label}: {error}"))
}

#[cfg(test)]
fn run_awww_probe_with_mock<R>(run: &mut R, failures: &mut Vec<String>) -> Result<Vec<String>, ()>
where
    R: FnMut(&str, &[&str]) -> Result<ProbeOutput, String>,
{
    match run("awww", &["query", "--json"]) {
        Ok(output) if output.success => match parse_awww_query_json(&output.stdout) {
            Ok(outputs) => return Ok(outputs),
            Err(error) => failures.push(format!("awww json: {error}")),
        },
        Ok(output) => failures.push(format!(
            "awww json: {}",
            command_failure_detail(&output.stderr)
        )),
        Err(error) => failures.push(format!("awww json: {error}")),
    }

    match run("awww", &["query"]) {
        Ok(output) if output.success => match parse_awww_query_text(&output.stdout) {
            Ok(outputs) => return Ok(outputs),
            Err(error) => failures.push(format!("awww text: {error}")),
        },
        Ok(output) => failures.push(format!(
            "awww text: {}",
            command_failure_detail(&output.stderr)
        )),
        Err(error) => failures.push(format!("awww text: {error}")),
    }

    Err(())
}

// ── test-only budget seam that exercises real run_probe ───────────────
//
// Spawns `sh -c <script>` instead of real compositor CLIs, but honours the
// same absolute-deadline budget sharing as the production path.  Tests use
// this to verify timing behaviour without depending on installed compositors.

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn discover_with_script_probes<E>(
    env: E,
    primary_script: &str,
    awww_json_script: &str,
    awww_text_script: &str,
) -> Result<Vec<String>, AppError>
where
    E: Fn(&str) -> Option<String>,
{
    let mut failures = Vec::new();
    let overall_deadline = Instant::now() + OVERALL_DISPLAY_DEADLINE;

    if let Some(probe) = select_primary_probe(&env) {
        let primary_budget = remaining_until(overall_deadline).min(PRIMARY_PROBE_DEADLINE);
        if primary_budget > Duration::ZERO {
            let label = match probe {
                PrimaryProbe::Niri => "niri",
                PrimaryProbe::Hyprland => "hyprland",
                PrimaryProbe::Sway => "sway",
                PrimaryProbe::Xrandr => "xrandr",
            };
            match run_probe("sh", &["-c", primary_script], primary_budget) {
                Ok(output) if output.success => {
                    let parsed = match probe {
                        PrimaryProbe::Niri => parse_niri_outputs(&output.stdout),
                        PrimaryProbe::Hyprland => parse_hyprland_outputs(&output.stdout),
                        PrimaryProbe::Sway => parse_sway_outputs(&output.stdout),
                        PrimaryProbe::Xrandr => parse_xrandr_outputs(&output.stdout),
                    };
                    match parsed {
                        Ok(outputs) => return Ok(outputs),
                        Err(error) => failures.push(format!("{label}: {error}")),
                    }
                }
                Ok(output) => failures.push(format!(
                    "{label}: {}",
                    command_failure_detail(&output.stderr)
                )),
                Err(error) => failures.push(format!("{label}: {}", probe_error_detail(&error))),
            }
        }
    }

    // awww --json: use whatever remains of the overall deadline.
    let remaining = remaining_until(overall_deadline);
    if remaining > Duration::ZERO {
        match run_probe("sh", &["-c", awww_json_script], remaining) {
            Ok(output) if output.success => match parse_awww_query_json(&output.stdout) {
                Ok(outputs) => return Ok(outputs),
                Err(error) => failures.push(format!("awww json: {error}")),
            },
            Ok(output) => failures.push(format!(
                "awww json: {}",
                command_failure_detail(&output.stderr)
            )),
            Err(error) => failures.push(format!("awww json: {}", probe_error_detail(&error))),
        }
    }

    // awww plain text: recalculate remaining; skip spawn if budget exhausted.
    let remaining = remaining_until(overall_deadline);
    if remaining > Duration::ZERO {
        match run_probe("sh", &["-c", awww_text_script], remaining) {
            Ok(output) if output.success => match parse_awww_query_text(&output.stdout) {
                Ok(outputs) => return Ok(outputs),
                Err(error) => failures.push(format!("awww text: {error}")),
            },
            Ok(output) => failures.push(format!(
                "awww text: {}",
                command_failure_detail(&output.stderr)
            )),
            Err(error) => failures.push(format!("awww text: {}", probe_error_detail(&error))),
        }
    }

    Err(discovery_error(failures))
}

// ── test-only budget-traced seam ──────────────────────────────────────
//
// Like `discover_with_script_probes` but records every budget passed to
// `run_probe` so tests can verify budget-sharing invariants.

#[cfg(test)]
#[derive(Debug, Clone)]
pub(crate) struct ProbeBudgetRecord {
    pub label: &'static str,
    pub budget: Duration,
    pub remaining_at_call: Duration,
}

#[cfg(test)]
pub(crate) fn discover_with_script_probes_traced<E>(
    env: E,
    primary_script: &str,
    awww_json_script: &str,
    awww_text_script: &str,
    budgets: &mut Vec<ProbeBudgetRecord>,
) -> Result<Vec<String>, AppError>
where
    E: Fn(&str) -> Option<String>,
{
    let mut failures = Vec::new();
    let overall_deadline = Instant::now() + OVERALL_DISPLAY_DEADLINE;

    if let Some(probe) = select_primary_probe(&env) {
        let primary_budget = remaining_until(overall_deadline).min(PRIMARY_PROBE_DEADLINE);
        if primary_budget > Duration::ZERO {
            let label = match probe {
                PrimaryProbe::Niri => "niri",
                PrimaryProbe::Hyprland => "hyprland",
                PrimaryProbe::Sway => "sway",
                PrimaryProbe::Xrandr => "xrandr",
            };
            budgets.push(ProbeBudgetRecord {
                label,
                budget: primary_budget,
                remaining_at_call: remaining_until(overall_deadline),
            });
            match run_probe("sh", &["-c", primary_script], primary_budget) {
                Ok(output) if output.success => {
                    let parsed = match probe {
                        PrimaryProbe::Niri => parse_niri_outputs(&output.stdout),
                        PrimaryProbe::Hyprland => parse_hyprland_outputs(&output.stdout),
                        PrimaryProbe::Sway => parse_sway_outputs(&output.stdout),
                        PrimaryProbe::Xrandr => parse_xrandr_outputs(&output.stdout),
                    };
                    match parsed {
                        Ok(outputs) => return Ok(outputs),
                        Err(error) => failures.push(format!("{label}: {error}")),
                    }
                }
                Ok(output) => failures.push(format!(
                    "{label}: {}",
                    command_failure_detail(&output.stderr)
                )),
                Err(error) => failures.push(format!("{label}: {}", probe_error_detail(&error))),
            }
        }
    }

    // awww --json: use whatever remains of the overall deadline.
    let remaining = remaining_until(overall_deadline);
    if remaining > Duration::ZERO {
        budgets.push(ProbeBudgetRecord {
            label: "awww json",
            budget: remaining,
            remaining_at_call: remaining,
        });
        match run_probe("sh", &["-c", awww_json_script], remaining) {
            Ok(output) if output.success => match parse_awww_query_json(&output.stdout) {
                Ok(outputs) => return Ok(outputs),
                Err(error) => failures.push(format!("awww json: {error}")),
            },
            Ok(output) => failures.push(format!(
                "awww json: {}",
                command_failure_detail(&output.stderr)
            )),
            Err(error) => failures.push(format!("awww json: {}", probe_error_detail(&error))),
        }
    }

    // awww plain text: recalculate remaining; skip spawn if budget exhausted.
    let remaining = remaining_until(overall_deadline);
    if remaining > Duration::ZERO {
        budgets.push(ProbeBudgetRecord {
            label: "awww text",
            budget: remaining,
            remaining_at_call: remaining,
        });
        match run_probe("sh", &["-c", awww_text_script], remaining) {
            Ok(output) if output.success => match parse_awww_query_text(&output.stdout) {
                Ok(outputs) => return Ok(outputs),
                Err(error) => failures.push(format!("awww text: {error}")),
            },
            Ok(output) => failures.push(format!(
                "awww text: {}",
                command_failure_detail(&output.stderr)
            )),
            Err(error) => failures.push(format!("awww text: {}", probe_error_detail(&error))),
        }
    }

    Err(discovery_error(failures))
}

// ── production path with deadlines ────────────────────────────────────

fn run_primary_probe_with_deadline(
    probe: PrimaryProbe,
    deadline: Duration,
) -> Result<Vec<String>, String> {
    let (label, program, args): (&str, &str, &[&str]) = match probe {
        PrimaryProbe::Niri => ("niri", "niri", &["msg", "-j", "outputs"]),
        PrimaryProbe::Hyprland => ("hyprland", "hyprctl", &["-j", "monitors"]),
        PrimaryProbe::Sway => ("sway", "swaymsg", &["-t", "get_outputs", "-r"]),
        PrimaryProbe::Xrandr => ("xrandr", "xrandr", &["--query"]),
    };
    let output = run_probe(program, args, deadline)
        .map_err(|error| format!("{label}: {}", probe_error_detail(&error)))?;
    if !output.success {
        return Err(format!(
            "{label}: {}",
            command_failure_detail(&output.stderr)
        ));
    }
    let parsed = match probe {
        PrimaryProbe::Niri => parse_niri_outputs(&output.stdout),
        PrimaryProbe::Hyprland => parse_hyprland_outputs(&output.stdout),
        PrimaryProbe::Sway => parse_sway_outputs(&output.stdout),
        PrimaryProbe::Xrandr => parse_xrandr_outputs(&output.stdout),
    };
    parsed.map_err(|error| format!("{label}: {error}"))
}

fn run_awww_probe_with_absolute_deadline(
    overall_deadline: Instant,
    failures: &mut Vec<String>,
) -> Result<Vec<String>, ()> {
    // awww --json: use whatever remains of the overall deadline.
    let remaining = remaining_until(overall_deadline);
    if remaining > Duration::ZERO {
        match run_probe("awww", &["query", "--json"], remaining) {
            Ok(output) if output.success => match parse_awww_query_json(&output.stdout) {
                Ok(outputs) => return Ok(outputs),
                Err(error) => failures.push(format!("awww json: {error}")),
            },
            Ok(output) => failures.push(format!(
                "awww json: {}",
                command_failure_detail(&output.stderr)
            )),
            Err(error) => failures.push(format!("awww json: {}", probe_error_detail(&error))),
        }
    }

    // awww plain text: recalculate remaining; skip spawn if budget exhausted.
    let remaining = remaining_until(overall_deadline);
    if remaining > Duration::ZERO {
        match run_probe("awww", &["query"], remaining) {
            Ok(output) if output.success => match parse_awww_query_text(&output.stdout) {
                Ok(outputs) => return Ok(outputs),
                Err(error) => failures.push(format!("awww text: {error}")),
            },
            Ok(output) => failures.push(format!(
                "awww text: {}",
                command_failure_detail(&output.stderr)
            )),
            Err(error) => failures.push(format!("awww text: {}", probe_error_detail(&error))),
        }
    }

    Err(())
}

fn remaining_until(deadline: Instant) -> Duration {
    deadline.saturating_duration_since(Instant::now())
}

fn select_primary_probe<E>(env: &E) -> Option<PrimaryProbe>
where
    E: Fn(&str) -> Option<String>,
{
    let desktop = env("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_ascii_lowercase();
    if nonempty(env("NIRI_SOCKET")) || desktop_component(&desktop, "niri") {
        Some(PrimaryProbe::Niri)
    } else if nonempty(env("HYPRLAND_INSTANCE_SIGNATURE"))
        || desktop_component(&desktop, "hyprland")
    {
        Some(PrimaryProbe::Hyprland)
    } else if nonempty(env("SWAYSOCK")) || desktop_component(&desktop, "sway") {
        Some(PrimaryProbe::Sway)
    } else if env("XDG_SESSION_TYPE").is_some_and(|value| value.eq_ignore_ascii_case("x11")) {
        Some(PrimaryProbe::Xrandr)
    } else {
        None
    }
}

fn nonempty(value: Option<String>) -> bool {
    value.is_some_and(|value| !value.trim().is_empty())
}

fn desktop_component(desktop: &str, expected: &str) -> bool {
    desktop
        .split([':', ';'])
        .any(|part| part.trim() == expected)
}

/// Format a [`ProbeError`] for display in a failure chain.
///
/// The caller is responsible for adding any context label (e.g. "niri: ",
/// "awww json: "). This function produces the diagnostic payload without a
/// redundant label prefix.
fn probe_error_detail(error: &ProbeError) -> String {
    match &error.kind {
        ProbeErrorKind::TimedOut { deadline } => {
            format!(
                "timed out after {deadline:?}{}",
                error
                    .partial_stderr
                    .lines()
                    .next()
                    .map(|line| format!(": {line}"))
                    .unwrap_or_default()
            )
        }
        ProbeErrorKind::NonZeroExit { exit_code } => {
            format!(
                "exit code {exit_code}{}",
                error
                    .partial_stderr
                    .lines()
                    .next()
                    .map(|line| format!(": {line}"))
                    .unwrap_or_default()
            )
        }
        ProbeErrorKind::SpawnFailed { reason } => {
            format!("could not execute: {reason}")
        }
        ProbeErrorKind::InvalidUtf8 { stream } => {
            format!(
                "{stream} output is not valid UTF-8{}",
                error
                    .partial_stderr
                    .lines()
                    .next()
                    .map(|line| format!(" (stderr: {line})"))
                    .unwrap_or_default()
            )
        }
    }
}

fn command_failure_detail(stderr: &str) -> String {
    if stderr.trim().is_empty() {
        "command exited unsuccessfully without an error message".into()
    } else {
        stderr.trim().to_string()
    }
}

fn parse_niri_outputs(raw: &str) -> Result<Vec<String>, String> {
    let root: serde_json::Value =
        serde_json::from_str(raw).map_err(|error| format!("invalid JSON: {error}"))?;
    let outputs = root
        .as_object()
        .ok_or_else(|| "JSON root must be an output object".to_string())?;
    collect_json_names(outputs.values(), |_| true)
}

fn parse_hyprland_outputs(raw: &str) -> Result<Vec<String>, String> {
    let root: serde_json::Value =
        serde_json::from_str(raw).map_err(|error| format!("invalid JSON: {error}"))?;
    let outputs = root
        .as_array()
        .ok_or_else(|| "JSON root must be an output array".to_string())?;
    collect_json_names(outputs.iter(), |_| true)
}

fn parse_sway_outputs(raw: &str) -> Result<Vec<String>, String> {
    let root: serde_json::Value =
        serde_json::from_str(raw).map_err(|error| format!("invalid JSON: {error}"))?;
    let outputs = root
        .as_array()
        .ok_or_else(|| "JSON root must be an output array".to_string())?;
    collect_json_names(outputs.iter(), |output| {
        output.get("active").and_then(serde_json::Value::as_bool) == Some(true)
    })
}

fn collect_json_names<'a, I, F>(outputs: I, include: F) -> Result<Vec<String>, String>
where
    I: IntoIterator<Item = &'a serde_json::Value>,
    F: Fn(&serde_json::Value) -> bool,
{
    let mut names = Vec::new();
    let mut seen = HashSet::new();
    for output in outputs {
        if !include(output) {
            continue;
        }
        let name = output
            .get("name")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "output is missing a string name".to_string())?;
        push_output_name(name, &mut names, &mut seen)?;
    }
    names.sort();
    Ok(names)
}

fn parse_xrandr_outputs(raw: &str) -> Result<Vec<String>, String> {
    let mut names = Vec::new();
    let mut seen = HashSet::new();
    for line in raw.lines() {
        let mut fields = line.split_whitespace();
        let Some(name) = fields.next() else {
            continue;
        };
        if fields.next() == Some("connected") {
            push_output_name(name, &mut names, &mut seen)?;
        }
    }
    names.sort();
    Ok(names)
}

fn parse_awww_query_json(raw: &str) -> Result<Vec<String>, String> {
    let root: serde_json::Value =
        serde_json::from_str(raw).map_err(|error| format!("invalid JSON: {error}"))?;
    let namespaces = root
        .as_object()
        .ok_or_else(|| "JSON root must be a namespace object".to_string())?;
    let mut names = Vec::new();
    let mut seen = HashSet::new();
    for outputs in namespaces.values() {
        let outputs = outputs
            .as_array()
            .ok_or_else(|| "namespace must contain an output array".to_string())?;
        for output in outputs {
            let name = output
                .get("name")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| "output is missing a string name".to_string())?;
            push_output_name(name, &mut names, &mut seen)?;
        }
    }
    names.sort();
    Ok(names)
}

fn parse_awww_query_text(raw: &str) -> Result<Vec<String>, String> {
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut names = Vec::new();
    let mut seen = HashSet::new();
    for line in raw.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let parts: Vec<_> = line.splitn(3, ':').collect();
        let (name, detail) = match parts.as_slice() {
            [name, detail] => (name.trim(), *detail),
            [_namespace, name, detail] => (name.trim(), *detail),
            _ => continue,
        };
        if !has_resolution(detail) {
            continue;
        }
        push_output_name(name, &mut names, &mut seen)?;
    }
    if names.is_empty() {
        return Err("non-empty output contained no recognizable displays".into());
    }
    names.sort();
    Ok(names)
}

fn has_resolution(detail: &str) -> bool {
    let token = detail.split(',').next().unwrap_or_default().trim();
    let Some((width, height)) = token.split_once('x') else {
        return false;
    };
    width.trim().parse::<u32>().is_ok()
        && height
            .split_whitespace()
            .next()
            .is_some_and(|height| height.parse::<u32>().is_ok())
}

fn push_output_name(
    name: &str,
    names: &mut Vec<String>,
    seen: &mut HashSet<String>,
) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("blank display output name".into());
    }
    if !seen.insert(name.to_string()) {
        return Err(format!("duplicate display output: {name}"));
    }
    names.push(name.to_string());
    Ok(())
}

fn discovery_error(failures: Vec<String>) -> AppError {
    let detail = if failures.is_empty() {
        "no compositor or renderer output probe was available".to_string()
    } else {
        failures.join("; ")
    };
    AppError {
        code: "display_discovery_failed".into(),
        message: "Could not discover connected display outputs.".into(),
        detail: Some(detail),
        recoverable: true,
        suggestion: Some(
            "Run Wallpaper Console inside the active desktop session and verify compositor IPC access."
                .into(),
        ),
    }
}
