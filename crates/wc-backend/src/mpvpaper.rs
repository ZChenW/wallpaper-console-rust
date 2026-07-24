use std::process::{Command, Output};

use wc_core::error::WcError;

pub(crate) fn build_launch_command(options: &str, output: &str, path: &str) -> Command {
    let mut cmd = Command::new("setsid");
    cmd.args([
        "-f", "-w", "mpvpaper", "--fork", "-o", options, output, "--", path,
    ]);
    cmd
}

fn parse_running_pids(exit_code: Option<i32>, stdout: &[u8]) -> Result<Vec<u32>, WcError> {
    match exit_code {
        Some(1) => Ok(Vec::new()),
        Some(0) => {
            let stdout = std::str::from_utf8(stdout).map_err(|_| {
                WcError::Other("pgrep returned non-UTF-8 mpvpaper PID output".into())
            })?;
            let pids: Vec<u32> = stdout
                .lines()
                .map(|line| {
                    line.trim().parse::<u32>().map_err(|_| {
                        WcError::Other("pgrep returned invalid PID data for mpvpaper".into())
                    })
                })
                .collect::<Result<_, _>>()?;
            if pids.is_empty() {
                Err(WcError::Other(
                    "pgrep returned no PID data for mpvpaper despite success status".into(),
                ))
            } else {
                Ok(pids)
            }
        }
        Some(code) => Err(WcError::Other(format!(
            "pgrep for mpvpaper exited with status {code}"
        ))),
        None => Err(WcError::Other(
            "pgrep for mpvpaper terminated without an exit status".into(),
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
    cmd.args(["-x", "mpvpaper"]);
    let output = run(&mut cmd).map_err(|error| {
        WcError::Other(format!("failed to execute pgrep for mpvpaper: {error}"))
    })?;
    parse_running_pids(output.status.code(), &output.stdout)
}

pub(crate) fn running_pids() -> Result<Vec<u32>, WcError> {
    let user = crate::current_process_user();
    running_pids_for_scope_with(&user, |cmd| {
        crate::deadline_command::output(cmd, std::time::Duration::from_secs(2))
            .map_err(|error| std::io::Error::other(error.to_string()))
    })
}

pub(crate) fn stop_mpvpaper() {
    match running_pids() {
        Ok(pids) => {
            for pid in pids {
                if crate::process_control::pid_looks_like_mpvpaper(pid as i32) {
                    crate::process_control::kill_pid_gracefully(pid);
                }
            }
        }
        Err(err) => {
            log::warn!("failed to query mpvpaper PIDs for stop: {err}");
        }
    }
}

fn cmdline_matches_target(tokens: &[String], output: &str, path: &str) -> bool {
    let Some(argv0) = tokens.first() else {
        return false;
    };
    if !crate::process_control::token_is_mpvpaper_program(argv0) {
        return false;
    }
    (3..tokens.len()).any(|separator| {
        tokens[separator] == "--"
            && tokens.get(separator - 3).is_some_and(|token| token == "-o")
            && tokens
                .get(separator - 1)
                .is_some_and(|token| token == output)
            && tokens.get(separator + 1).is_some_and(|token| token == path)
            && separator + 2 == tokens.len()
    })
}

fn pids_started_after_matching_target_with<F>(
    current_pids: &[u32],
    previous_pids: &[u32],
    output: &str,
    path: &str,
    mut read_cmdline: F,
) -> Vec<u32>
where
    F: FnMut(u32) -> Option<Vec<String>>,
{
    current_pids
        .iter()
        .copied()
        .filter(|pid| !previous_pids.contains(pid))
        .filter(|pid| {
            read_cmdline(*pid).is_some_and(|tokens| cmdline_matches_target(&tokens, output, path))
        })
        .collect()
}

fn read_mpvpaper_cmdline(pid: u32) -> Option<Vec<String>> {
    let pid = i32::try_from(pid).ok()?;
    crate::process_control::read_proc_cmdline_tokens(pid)
}

pub(crate) fn pid_matches_target(pid: u32, output: &str, path: &str) -> bool {
    read_mpvpaper_cmdline(pid).is_some_and(|tokens| cmdline_matches_target(&tokens, output, path))
}

pub(crate) fn stop_pids_started_after(
    previous_pids: &[u32],
    output: &str,
    path: &str,
) -> Result<(), WcError> {
    let target_pids = pids_started_after_matching_target_with(
        &running_pids()?,
        previous_pids,
        output,
        path,
        read_mpvpaper_cmdline,
    );
    for pid in &target_pids {
        if !pid_matches_target(*pid, output, path) {
            return Err(WcError::Other(format!(
                "refusing to stop PID {pid} after failed mpvpaper launch because its target \
                 identity changed"
            )));
        }
        crate::process_control::kill_pid_gracefully(*pid);
    }

    for poll in 0..=40 {
        let still_running = running_pids()?
            .into_iter()
            .filter(|pid| target_pids.contains(pid))
            .filter(|pid| pid_matches_target(*pid, output, path))
            .collect::<Vec<_>>();
        if still_running.is_empty() {
            return Ok(());
        }
        if poll == 40 {
            return Err(WcError::Other(format!(
                "new mpvpaper processes survived failed-launch cleanup: pids={still_running:?}"
            )));
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    unreachable!("bounded cleanup loop always returns")
}

pub(crate) fn normalize_mpvpaper_options(raw: &str) -> &str {
    let trimmed = raw.trim();
    if trimmed == "no-audio --loop-file=inf" || trimmed == "--loop-file=inf" {
        "--loop-file=inf --panscan=1.0"
    } else {
        trimmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mpvpaper_pid_parser_returns_all_pids_for_success_status() {
        assert_eq!(
            parse_running_pids(Some(0), b"101\n202\n303\n").unwrap(),
            vec![101, 202, 303]
        );
    }

    #[test]
    fn mpvpaper_pid_parser_treats_status_one_as_no_matches() {
        assert_eq!(parse_running_pids(Some(1), b"").unwrap(), Vec::<u32>::new());
    }

    #[test]
    fn mpvpaper_pid_parser_rejects_malformed_success_output() {
        let error = parse_running_pids(Some(0), b"101\nnot-a-pid\n").unwrap_err();

        assert!(error.to_string().contains("invalid PID"));
    }

    #[test]
    fn mpvpaper_pid_parser_rejects_empty_success_output() {
        let error = parse_running_pids(Some(0), b"").unwrap_err();

        assert!(error.to_string().contains("no PID data"));
    }

    #[test]
    fn mpvpaper_pid_parser_rejects_abnormal_exit_status() {
        let error = parse_running_pids(Some(2), b"").unwrap_err();

        assert!(error.to_string().contains("status 2"));
    }

    #[test]
    fn failed_launch_cleanup_targets_only_new_renderer_for_exact_output_and_path() {
        let target = vec![
            "/usr/bin/mpvpaper".to_string(),
            "--fork".to_string(),
            "-o".to_string(),
            "--loop-file=inf --panscan=1.0".to_string(),
            "eDP-1".to_string(),
            "--".to_string(),
            "/walls/target.mp4".to_string(),
        ];
        let other_output = {
            let mut tokens = target.clone();
            tokens[4] = "HDMI-A-1".to_string();
            tokens
        };
        let other_path = {
            let mut tokens = target.clone();
            tokens[6] = "/walls/other.mp4".to_string();
            tokens
        };

        assert_eq!(
            pids_started_after_matching_target_with(
                &[10, 20, 30, 40],
                &[10],
                "eDP-1",
                "/walls/target.mp4",
                |pid| match pid {
                    20 => Some(target.clone()),
                    30 => Some(other_output.clone()),
                    40 => Some(other_path.clone()),
                    _ => None,
                },
            ),
            vec![20],
            "new renderers for another output or path must not be selected"
        );
    }

    #[test]
    fn mpvpaper_pgrep_scope_uses_uid_flag_for_numeric_scope() {
        let scope = crate::ProcessUserScope::Uid(1000);
        let mut cmd = Command::new("pgrep");
        crate::append_pgrep_user_scope(&mut cmd, &scope);
        cmd.args(["-x", "mpvpaper"]);
        let args: Vec<String> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(args.contains(&"-U".to_string()));
        assert!(args.contains(&"1000".to_string()));
    }

    #[test]
    fn mpvpaper_pid_query_propagates_pgrep_spawn_failure() {
        let scope = crate::ProcessUserScope::Name("test-user".to_string());
        let error = running_pids_for_scope_with(&scope, |_| {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "pgrep unavailable",
            ))
        })
        .unwrap_err();

        assert!(error.to_string().contains("failed to execute pgrep"));
    }

    #[test]
    fn mpvpaper_launch_command_waits_for_launcher_and_preserves_argument_order() {
        let cmd = build_launch_command(
            "--loop-file=inf --panscan=1.0",
            "DP-1",
            "/wallpapers/private/video.mp4",
        );

        assert_eq!(cmd.get_program(), "setsid");
        let args: Vec<String> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            vec![
                "-f",
                "-w",
                "mpvpaper",
                "--fork",
                "-o",
                "--loop-file=inf --panscan=1.0",
                "DP-1",
                "--",
                "/wallpapers/private/video.mp4",
            ]
        );
    }

    #[test]
    fn normalize_mpvpaper_options_migrates_legacy_silent_default() {
        assert_eq!(
            normalize_mpvpaper_options("no-audio --loop-file=inf"),
            "--loop-file=inf --panscan=1.0"
        );
        assert_eq!(
            normalize_mpvpaper_options("  no-audio --loop-file=inf  "),
            "--loop-file=inf --panscan=1.0"
        );
        assert_eq!(
            normalize_mpvpaper_options("no-audio --loop-file=inf --panscan=1"),
            "no-audio --loop-file=inf --panscan=1"
        );
    }

    #[test]
    fn normalize_mpvpaper_options_migrates_plain_loop_default_to_crop_fill() {
        assert_eq!(
            normalize_mpvpaper_options("--loop-file=inf"),
            "--loop-file=inf --panscan=1.0"
        );
        assert_eq!(
            normalize_mpvpaper_options("  --loop-file=inf  "),
            "--loop-file=inf --panscan=1.0"
        );
    }

    #[test]
    fn normalize_mpvpaper_options_preserves_custom_args() {
        assert_eq!(
            normalize_mpvpaper_options("--loop-file=inf --volume=60"),
            "--loop-file=inf --volume=60"
        );
        assert_eq!(
            normalize_mpvpaper_options("--loop-file=inf --volume=80 --mute=no"),
            "--loop-file=inf --volume=80 --mute=no"
        );
        assert_eq!(normalize_mpvpaper_options(""), "");
    }
}
