use std::process::{Command, Output};

use wc_core::error::WcError;

/// How an mpvpaper process selects outputs (from its argv output parameter).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MpvpaperOutputSelector {
    Single(String),
    Wildcard,
    Multi(Vec<String>),
    Unparseable,
}

/// A live mpvpaper process identified by cmdline (no PID persistence).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MpvpaperProcess {
    pub pid: u32,
    pub selector: MpvpaperOutputSelector,
    pub path: String,
}

impl MpvpaperProcess {
    /// Build a process row from a raw output selector string (tests / fakes).
    pub fn for_output(pid: u32, output: &str, path: impl Into<String>) -> Self {
        Self {
            pid,
            selector: classify_selector(output),
            path: path.into(),
        }
    }

    pub fn matches_stop_outputs(&self, outputs: &[String]) -> bool {
        match &self.selector {
            MpvpaperOutputSelector::Single(output) => outputs.iter().any(|o| o == output),
            _ => false,
        }
    }
}

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

/// Parse launch identity using the same positioning as [`cmdline_matches_target`]:
/// argv0 is mpvpaper, `-o` is third before `--`, and exactly one path follows `--`.
pub(crate) fn parse_launch_identity(tokens: &[String]) -> Option<(String, String)> {
    let argv0 = tokens.first()?;
    if !crate::process_control::token_is_mpvpaper_program(argv0) {
        return None;
    }
    (3..tokens.len()).find_map(|separator| {
        if tokens[separator] != "--" {
            return None;
        }
        if tokens.get(separator - 3).is_none_or(|token| token != "-o") {
            return None;
        }
        if separator + 2 != tokens.len() {
            return None;
        }
        let selector = tokens.get(separator - 1)?.clone();
        let path = tokens.get(separator + 1)?.clone();
        Some((selector, path))
    })
}

pub fn classify_selector(raw: &str) -> MpvpaperOutputSelector {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return MpvpaperOutputSelector::Unparseable;
    }
    if trimmed == "*" || trimmed.eq_ignore_ascii_case("ALL") {
        return MpvpaperOutputSelector::Wildcard;
    }
    let parts: Vec<String> = trimmed
        .split_whitespace()
        .map(str::to_string)
        .collect();
    match parts.as_slice() {
        [] => MpvpaperOutputSelector::Unparseable,
        [single] => MpvpaperOutputSelector::Single(single.clone()),
        multi => MpvpaperOutputSelector::Multi(multi.to_vec()),
    }
}

fn process_from_cmdline(pid: u32, tokens: Option<Vec<String>>) -> Option<MpvpaperProcess> {
    let tokens = tokens?;
    match parse_launch_identity(&tokens) {
        Some((raw, path)) => Some(MpvpaperProcess {
            pid,
            selector: classify_selector(&raw),
            path,
        }),
        None => Some(MpvpaperProcess {
            pid,
            selector: MpvpaperOutputSelector::Unparseable,
            path: String::new(),
        }),
    }
}

fn running_processes_with<F>(
    pids: &[u32],
    mut read_cmdline: F,
) -> Vec<MpvpaperProcess>
where
    F: FnMut(u32) -> Option<Vec<String>>,
{
    pids.iter()
        .copied()
        .filter_map(|pid| process_from_cmdline(pid, read_cmdline(pid)))
        .collect()
}

pub(crate) fn running_processes() -> Result<Vec<MpvpaperProcess>, WcError> {
    Ok(running_processes_with(&running_pids()?, read_mpvpaper_cmdline))
}

/// PIDs whose selectors cannot be safely targeted by a partial named stop.
pub(crate) fn scoped_stop_blockers(processes: &[MpvpaperProcess]) -> Vec<u32> {
    processes
        .iter()
        .filter(|process| {
            matches!(
                process.selector,
                MpvpaperOutputSelector::Wildcard
                    | MpvpaperOutputSelector::Multi(_)
                    | MpvpaperOutputSelector::Unparseable
            )
        })
        .map(|process| process.pid)
        .collect()
}

fn pids_matching_single_outputs_with<F>(
    processes: &[MpvpaperProcess],
    outputs: &[String],
    mut reverify: F,
) -> Vec<u32>
where
    F: FnMut(u32, &str) -> bool,
{
    processes
        .iter()
        .filter_map(|process| match &process.selector {
            MpvpaperOutputSelector::Single(output) if outputs.iter().any(|o| o == output) => {
                reverify(process.pid, output).then_some(process.pid)
            }
            _ => None,
        })
        .collect()
}

fn reverify_single_output(pid: u32, expected_output: &str) -> bool {
    if !crate::process_control::pid_looks_like_mpvpaper(pid as i32) {
        return false;
    }
    let Some(tokens) = read_mpvpaper_cmdline(pid) else {
        return false;
    };
    match parse_launch_identity(&tokens) {
        Some((raw, _)) => matches!(
            classify_selector(&raw),
            MpvpaperOutputSelector::Single(output) if output == expected_output
        ),
        None => false,
    }
}

pub(crate) fn stop_outputs(outputs: &[String]) -> Result<(), WcError> {
    let processes = running_processes()?;
    let target_pids = pids_matching_single_outputs_with(&processes, outputs, reverify_single_output);
    for pid in target_pids {
        crate::process_control::kill_pid_gracefully(pid);
    }
    Ok(())
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

    fn standard_tokens(output: &str, path: &str) -> Vec<String> {
        vec![
            "/usr/bin/mpvpaper".to_string(),
            "--fork".to_string(),
            "-o".to_string(),
            "--loop-file=inf --panscan=1.0".to_string(),
            output.to_string(),
            "--".to_string(),
            path.to_string(),
        ]
    }

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
        let target = standard_tokens("eDP-1", "/walls/target.mp4");
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
    fn parse_launch_identity_reads_standard_argv() {
        let tokens = standard_tokens("eDP-1", "/walls/a.mp4");
        assert_eq!(
            parse_launch_identity(&tokens),
            Some(("eDP-1".into(), "/walls/a.mp4".into()))
        );
    }

    #[test]
    fn parse_launch_identity_accepts_path_with_spaces() {
        let tokens = standard_tokens("DP-8", "/walls/my video.mp4");
        assert_eq!(
            parse_launch_identity(&tokens),
            Some(("DP-8".into(), "/walls/my video.mp4".into()))
        );
    }

    #[test]
    fn parse_launch_identity_rejects_missing_separator() {
        let tokens = vec![
            "mpvpaper".into(),
            "--fork".into(),
            "-o".into(),
            "opts".into(),
            "eDP-1".into(),
            "/walls/a.mp4".into(),
        ];
        assert_eq!(parse_launch_identity(&tokens), None);
    }

    #[test]
    fn classify_selector_covers_single_wildcard_multi_and_empty() {
        assert_eq!(
            classify_selector("eDP-1"),
            MpvpaperOutputSelector::Single("eDP-1".into())
        );
        assert_eq!(classify_selector("*"), MpvpaperOutputSelector::Wildcard);
        assert_eq!(classify_selector("ALL"), MpvpaperOutputSelector::Wildcard);
        assert_eq!(classify_selector("all"), MpvpaperOutputSelector::Wildcard);
        assert_eq!(
            classify_selector("eDP-1 DP-8"),
            MpvpaperOutputSelector::Multi(vec!["eDP-1".into(), "DP-8".into()])
        );
        assert_eq!(classify_selector("  "), MpvpaperOutputSelector::Unparseable);
    }

    #[test]
    fn scoped_stop_blockers_lists_unsafe_selectors() {
        let processes = vec![
            MpvpaperProcess {
                pid: 1,
                selector: MpvpaperOutputSelector::Single("eDP-1".into()),
                path: "/a".into(),
            },
            MpvpaperProcess {
                pid: 2,
                selector: MpvpaperOutputSelector::Wildcard,
                path: "/b".into(),
            },
            MpvpaperProcess {
                pid: 3,
                selector: MpvpaperOutputSelector::Multi(vec!["eDP-1".into(), "DP-8".into()]),
                path: "/c".into(),
            },
            MpvpaperProcess {
                pid: 4,
                selector: MpvpaperOutputSelector::Unparseable,
                path: String::new(),
            },
        ];
        assert_eq!(scoped_stop_blockers(&processes), vec![2, 3, 4]);
    }

    #[test]
    fn stop_outputs_selection_kills_only_matching_singles() {
        let processes = vec![
            MpvpaperProcess {
                pid: 10,
                selector: MpvpaperOutputSelector::Single("eDP-1".into()),
                path: "/a".into(),
            },
            MpvpaperProcess {
                pid: 20,
                selector: MpvpaperOutputSelector::Single("DP-8".into()),
                path: "/b".into(),
            },
            MpvpaperProcess {
                pid: 30,
                selector: MpvpaperOutputSelector::Wildcard,
                path: "/c".into(),
            },
        ];
        let killed = pids_matching_single_outputs_with(
            &processes,
            &["eDP-1".into()],
            |pid, output| pid == 10 && output == "eDP-1",
        );
        assert_eq!(killed, vec![10]);
    }

    #[test]
    fn running_processes_marks_unparseable_when_identity_missing() {
        let processes = running_processes_with(&[7, 8], |pid| match pid {
            7 => Some(standard_tokens("eDP-1", "/walls/a.mp4")),
            8 => Some(vec!["mpvpaper".into(), "weird".into()]),
            _ => None,
        });
        assert_eq!(processes.len(), 2);
        assert_eq!(
            processes[0].selector,
            MpvpaperOutputSelector::Single("eDP-1".into())
        );
        assert_eq!(processes[1].selector, MpvpaperOutputSelector::Unparseable);
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
