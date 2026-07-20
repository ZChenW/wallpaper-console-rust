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

fn running_pids_for_user_with<F>(user: &str, mut run: F) -> Result<Vec<u32>, WcError>
where
    F: FnMut(&str) -> std::io::Result<Output>,
{
    let user = user.trim();
    if user.is_empty() {
        return Err(WcError::Other(
            "USER is not set; cannot query mpvpaper processes".into(),
        ));
    }
    let output = run(user).map_err(|error| {
        WcError::Other(format!("failed to execute pgrep for mpvpaper: {error}"))
    })?;
    parse_running_pids(output.status.code(), &output.stdout)
}

pub(crate) fn running_pids() -> Result<Vec<u32>, WcError> {
    let user = crate::whoami();
    running_pids_for_user_with(&user, |user| {
        Command::new("pgrep")
            .args(["-u", user, "-x", "mpvpaper"])
            .output()
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
    fn mpvpaper_pid_query_rejects_empty_user_before_spawning() {
        let error =
            running_pids_for_user_with("", |_| panic!("pgrep must not be spawned without a user"))
                .unwrap_err();

        assert!(error.to_string().contains("USER"));
    }

    #[test]
    fn mpvpaper_pid_query_propagates_pgrep_spawn_failure() {
        let error = running_pids_for_user_with("test-user", |_| {
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
