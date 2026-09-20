//! Select and stop complete swaybg instances without expanding the target set.
use crate::runtime_observation::{parse_swaybg_command_line, ProcessCommandLine};
use wc_core::error::WcError;

pub fn targeted_processes(
    processes: &[ProcessCommandLine],
    outputs: &[String],
) -> Result<Vec<ProcessCommandLine>, WcError> {
    let mut selected = Vec::new();
    for process in processes {
        if !process
            .argv
            .first()
            .is_some_and(|arg| crate::process_control::token_is_swaybg_program(arg))
        {
            continue;
        }
        if !process
            .argv
            .iter()
            .any(|arg| matches!(arg.as_str(), "--output" | "-o"))
        {
            return Err(WcError::Other(format!("swaybg PID {} uses implicit all-output ownership; explicitly stop the legacy shared renderer before migrating", process.pid)));
        }
        let assignments = parse_swaybg_command_line(&process.argv, outputs).ok_or_else(|| {
            WcError::Other(format!(
                "swaybg PID {} has ambiguous output ownership",
                process.pid
            ))
        })?;
        if !assignments.iter().any(|(name, _)| outputs.contains(name)) {
            continue;
        }
        if assignments.iter().any(|(name, _)| !outputs.contains(name)) {
            return Err(WcError::Other(format!("swaybg PID {} also owns a non-target output; select all of its outputs to migrate it", process.pid)));
        }
        selected.push(process.clone());
    }
    Ok(selected)
}

pub(crate) fn stop_outputs(outputs: &[String]) -> Result<(), WcError> {
    let read = || {
        crate::runtime_observation::read_current_user_process_command_lines()
            .map_err(WcError::Other)
    };
    let targets = targeted_processes(&read()?, outputs)?;
    for process in targets {
        if crate::process_control::read_proc_cmdline_tokens(process.pid as i32).as_ref()
            != Some(&process.argv)
        {
            return Err(WcError::Other("swaybg identity changed before stop".into()));
        }
        crate::process_control::kill_pid_matching_argv(process.pid, &process.argv);
    }
    for _ in 0..40 {
        if targeted_processes(&read()?, outputs)?.is_empty() {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    Err(WcError::Other(
        "swaybg still owns the selected outputs after stop".into(),
    ))
}
