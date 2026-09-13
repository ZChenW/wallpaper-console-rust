//! Output ownership and checked stops for independent LWE renderers.

use wc_core::error::WcError;

use crate::runtime_observation::{parse_lwe_command_line, ProcessCommandLine};

/// Resolve the complete stop set before sending any signal. A legacy shared
/// process can be stopped only when every output it owns is explicitly targeted.
pub(crate) fn targeted_processes(
    processes: &[ProcessCommandLine],
    outputs: &[String],
) -> Result<Vec<ProcessCommandLine>, WcError> {
    let mut targets = Vec::new();
    for process in processes {
        if !process
            .argv
            .first()
            .is_some_and(|arg| crate::process_control::token_is_lwe_program(arg))
        {
            continue;
        }
        let assignments = parse_lwe_command_line(&process.argv).ok_or_else(|| {
            WcError::Other(format!(
                "linux-wallpaperengine process {} has an ambiguous output selector; stop wallpapers first",
                process.pid
            ))
        })?;
        if !assignments
            .iter()
            .any(|(output, _)| outputs.iter().any(|o| o == output))
        {
            continue;
        }
        if let Some((other, _)) = assignments
            .iter()
            .find(|(output, _)| !outputs.iter().any(|o| o == output))
        {
            return Err(WcError::Other(format!(
                "linux-wallpaperengine process {} also owns non-target display {other}; \
                 apply to All Displays once to migrate the shared renderer to independent processes",
                process.pid
            )));
        }
        targets.push(process.clone());
    }
    Ok(targets)
}

pub(crate) fn inspect_targets(outputs: &[String]) -> Result<Vec<ProcessCommandLine>, WcError> {
    let processes = crate::runtime_observation::read_current_user_process_command_lines()
        .map_err(WcError::Other)?;
    targeted_processes(&processes, outputs)
}

pub(crate) fn stop_outputs(outputs: &[String]) -> Result<(), WcError> {
    let targets = inspect_targets(outputs)?;
    for process in targets {
        // Recheck the complete command identity immediately before signalling.
        // A vanished/reused PID must never turn into a signal to another owner.
        if crate::process_control::read_proc_cmdline_tokens(process.pid as i32).as_ref()
            == Some(&process.argv)
        {
            crate::process_control::kill_pid_gracefully(process.pid);
        }
    }
    verify_stopped_with(outputs, || inspect_targets(outputs), std::thread::sleep)
}

fn verify_stopped_with(
    outputs: &[String],
    mut inspect: impl FnMut() -> Result<Vec<ProcessCommandLine>, WcError>,
    mut sleep: impl FnMut(std::time::Duration),
) -> Result<(), WcError> {
    for poll in 0..=40 {
        if inspect()?.is_empty() {
            return Ok(());
        }
        if poll < 40 {
            sleep(std::time::Duration::from_millis(25));
        }
    }
    Err(WcError::Other(format!(
        "linux-wallpaperengine still owns requested outputs {outputs:?} after stop (1 second timeout)"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process(pid: u32, outputs: &[&str]) -> ProcessCommandLine {
        let mut argv = vec!["linux-wallpaperengine".into()];
        for output in outputs {
            argv.extend([
                "--screen-root".into(),
                (*output).into(),
                "--bg".into(),
                "42".into(),
            ]);
        }
        ProcessCommandLine { pid, argv }
    }

    #[test]
    fn scoped_selection_leaves_sibling_pid_untouched() {
        let processes = [process(10, &["DP-8"]), process(11, &["eDP-1"])];
        let selected = targeted_processes(&processes, &["eDP-1".into()]).unwrap();
        assert_eq!(selected, vec![processes[1].clone()]);
    }

    #[test]
    fn shared_process_is_rejected_before_any_stop_set_is_returned() {
        let processes = [process(10, &["eDP-1"]), process(11, &["eDP-1", "DP-8"])];
        let error = targeted_processes(&processes, &["eDP-1".into()]).unwrap_err();
        assert!(error.to_string().contains("non-target display DP-8"));
        assert_eq!(
            targeted_processes(&processes, &["eDP-1".into(), "DP-8".into()]).unwrap(),
            processes
        );
    }

    #[test]
    fn ambiguous_or_extra_target_arguments_are_never_stopped_partially() {
        for extra in [
            vec!["--window", "800x600"],
            vec!["--screen-root", "DP-8"],
            vec!["--unknown"],
        ] {
            let mut row = process(10, &["eDP-1"]);
            row.argv.extend(extra.into_iter().map(str::to_string));
            assert!(targeted_processes(&[row], &["eDP-1".into()]).is_err());
        }
    }

    #[test]
    fn option_values_cannot_impersonate_output_selectors() {
        let mut row = process(10, &["DP-8"]);
        row.argv
            .extend(["--assets-dir", "--screen-root", "eDP-1", "--bg", "42"].map(str::to_string));
        assert!(targeted_processes(&[row], &["eDP-1".into()]).is_err());
    }
}

#[cfg(test)]
mod stop_tests {
    use super::*;

    #[test]
    fn delayed_exit_after_signal_is_not_a_cleanup_failure() {
        let mut probes = 0;
        let result = verify_stopped_with(
            &["DP-8".into()],
            || {
                probes += 1;
                Ok(if probes < 3 {
                    vec![ProcessCommandLine {
                        pid: 123,
                        argv: vec![],
                    }]
                } else {
                    vec![]
                })
            },
            |_| {},
        );
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(probes, 3);
    }

    #[test]
    fn remaining_owner_is_still_rejected_after_bounded_wait() {
        let mut probes = 0;
        assert!(verify_stopped_with(
            &["DP-8".into()],
            || {
                probes += 1;
                Ok(vec![ProcessCommandLine {
                    pid: 123,
                    argv: vec![],
                }])
            },
            |_| {}
        )
        .is_err());
        assert!(probes <= 41);
    }

    #[test]
    fn inspection_failure_is_not_treated_as_success() {
        assert!(verify_stopped_with(
            &["DP-8".into()],
            || Err(WcError::Other("inspection failed".into())),
            |_| {}
        )
        .is_err());
    }
}
