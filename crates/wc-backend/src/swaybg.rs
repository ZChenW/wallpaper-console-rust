use std::process::{Command, Output};

use wc_core::error::WcError;

use crate::target_commands::ExecutionScope;

fn parse_running_pids(exit_code: Option<i32>, stdout: &[u8]) -> Result<Vec<u32>, WcError> {
    match exit_code {
        Some(1) => Ok(Vec::new()),
        Some(0) => {
            let stdout = std::str::from_utf8(stdout)
                .map_err(|_| WcError::Other("pgrep returned non-UTF-8 swaybg PID output".into()))?;
            let pids = stdout
                .lines()
                .map(|line| {
                    line.trim().parse::<u32>().map_err(|_| {
                        WcError::Other("pgrep returned invalid PID data for swaybg".into())
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            if pids.is_empty() {
                Err(WcError::Other(
                    "pgrep returned no PID data for swaybg despite success status".into(),
                ))
            } else {
                Ok(pids)
            }
        }
        Some(code) => Err(WcError::Other(format!(
            "pgrep for swaybg exited with status {code}"
        ))),
        None => Err(WcError::Other(
            "pgrep for swaybg terminated without an exit status".into(),
        )),
    }
}

fn running_pids_for_scope_with<F>(
    user: &crate::ProcessUserScope,
    mut run: F,
) -> Result<Vec<u32>, WcError>
where
    F: FnMut(&mut Command) -> std::io::Result<Output>,
{
    let mut cmd = Command::new("pgrep");
    crate::append_pgrep_user_scope(&mut cmd, user);
    cmd.args(["-x", "swaybg"]);
    let output = run(&mut cmd)
        .map_err(|error| WcError::Other(format!("failed to execute pgrep for swaybg: {error}")))?;
    parse_running_pids(output.status.code(), &output.stdout)
}

pub(crate) fn running_pids() -> Result<Vec<u32>, WcError> {
    let user = crate::current_process_user();
    running_pids_for_scope_with(&user, |cmd| {
        crate::deadline_command::output(cmd, std::time::Duration::from_secs(2))
            .map_err(|error| std::io::Error::other(error.to_string()))
    })
}

pub(crate) fn stop_swaybg() {
    match running_pids() {
        Ok(pids) => {
            for pid in pids {
                if crate::process_control::pid_looks_like_swaybg(pid as i32) {
                    crate::process_control::kill_pid_gracefully(pid);
                }
            }
        }
        Err(error) => log::warn!("failed to query swaybg PIDs for stop: {error}"),
    }
}

fn option_value<'a>(tokens: &'a [String], option: &str) -> Option<&'a str> {
    tokens
        .windows(2)
        .find(|pair| pair[0] == option)
        .map(|pair| pair[1].as_str())
}

fn cmdline_matches_target(tokens: &[String], path: &str, scope: &ExecutionScope) -> bool {
    let Some(argv0) = tokens.first() else {
        return false;
    };
    if !crate::process_control::token_is_swaybg_program(argv0) {
        return false;
    }
    match scope {
        ExecutionScope::AllDisplays => {
            !tokens.iter().any(|token| token == "--output")
                && option_value(tokens, "--image") == Some(path)
        }
        ExecutionScope::Named(expected) => {
            let mut cursor = 1;
            let mut actual = Vec::new();
            while cursor < tokens.len() {
                if tokens.get(cursor).map(String::as_str) != Some("--output") {
                    cursor += 1;
                    continue;
                }
                let Some(output) = tokens.get(cursor + 1) else {
                    return false;
                };
                if tokens.get(cursor + 2).map(String::as_str) != Some("--image")
                    || tokens.get(cursor + 3).map(String::as_str) != Some(path)
                {
                    return false;
                }
                actual.push(output.clone());
                cursor += 4;
            }
            actual == *expected
        }
    }
}

pub(crate) fn pid_matches_target(pid: u32, path: &str, scope: &ExecutionScope) -> bool {
    crate::process_control::read_proc_cmdline_tokens(pid as i32)
        .is_some_and(|tokens| cmdline_matches_target(&tokens, path, scope))
}

pub(crate) fn stop_pids_started_after(
    previous_pids: &[u32],
    path: &str,
    scope: &ExecutionScope,
) -> Result<(), WcError> {
    let target_pids = running_pids()?
        .into_iter()
        .filter(|pid| !previous_pids.contains(pid))
        .filter(|pid| pid_matches_target(*pid, path, scope))
        .collect::<Vec<_>>();
    for pid in &target_pids {
        if !pid_matches_target(*pid, path, scope) {
            return Err(WcError::Other(format!(
                "refusing to stop PID {pid} after failed swaybg launch because its target \
                 identity changed"
            )));
        }
        crate::process_control::kill_pid_gracefully(*pid);
    }
    for poll in 0..=40 {
        let still_running = running_pids()?
            .into_iter()
            .filter(|pid| target_pids.contains(pid))
            .filter(|pid| pid_matches_target(*pid, path, scope))
            .collect::<Vec<_>>();
        if still_running.is_empty() {
            return Ok(());
        }
        if poll == 40 {
            return Err(WcError::Other(format!(
                "new swaybg processes survived failed-launch cleanup: pids={still_running:?}"
            )));
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    unreachable!("the bounded cleanup loop always returns")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_exact_named_output_groups() {
        let tokens = [
            "swaybg",
            "--output",
            "eDP-1",
            "--image",
            "/walls/a.png",
            "--mode",
            "fill",
            "--output",
            "HDMI-A-1",
            "--image",
            "/walls/a.png",
            "--mode",
            "fill",
        ]
        .map(str::to_string);
        let scope = ExecutionScope::named(vec!["eDP-1".into(), "HDMI-A-1".into()]).unwrap();

        assert!(cmdline_matches_target(&tokens, "/walls/a.png", &scope));
        assert!(!cmdline_matches_target(&tokens, "/walls/other.png", &scope));
    }

    #[test]
    fn all_displays_rejects_a_named_process() {
        let tokens = ["swaybg", "--output", "eDP-1", "--image", "/walls/a.png"].map(str::to_string);
        assert!(!cmdline_matches_target(
            &tokens,
            "/walls/a.png",
            &ExecutionScope::AllDisplays
        ));
    }
}
