//! Explicit output-group command construction for display-aware apply.
//!
//! Pure builders only — no process launch, readiness waits, or state writes.
//! [`ExecutionScope`] is the single validated addressing mode for Apply/Stop.

use std::collections::HashSet;
use std::process::Command;

use wc_core::error::WcError;

use crate::awww::{
    build_awww_img_command as build_awww_img_command_base,
    build_awww_instant_command as build_awww_instant_command_base,
};
use crate::mpvpaper::build_launch_command as build_mpvpaper_launch_command_base;

/// How a display-aware Stop/Apply addresses outputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionScope {
    /// Explicit all-display request (awww omits `--outputs`).
    AllDisplays,
    /// Named output group: nonempty, nonblank, unique names.
    Named(Vec<String>),
}

impl ExecutionScope {
    /// Construct a validated named scope.
    pub fn named(outputs: Vec<String>) -> Result<Self, WcError> {
        validate_named_outputs(&outputs)?;
        Ok(Self::Named(outputs))
    }

    /// Validate an already-built scope (builders call this defensively).
    pub fn validate(&self) -> Result<(), WcError> {
        match self {
            Self::AllDisplays => Ok(()),
            Self::Named(outputs) => validate_named_outputs(outputs),
        }
    }

    pub fn named_outputs(&self) -> Option<&[String]> {
        match self {
            Self::AllDisplays => None,
            Self::Named(outputs) => Some(outputs.as_slice()),
        }
    }
}

fn validate_named_outputs(outputs: &[String]) -> Result<(), WcError> {
    if outputs.is_empty() {
        return Err(WcError::Other(
            "named execution scope requires at least one output".into(),
        ));
    }
    let mut seen = HashSet::with_capacity(outputs.len());
    for output in outputs {
        if output.trim().is_empty() {
            return Err(WcError::Other(format!(
                "named execution scope contains blank output: {output:?}"
            )));
        }
        if !seen.insert(output.as_str()) {
            return Err(WcError::Other(format!(
                "named execution scope contains duplicate output: {output}"
            )));
        }
    }
    Ok(())
}

/// Attach awww `--outputs` only for named scopes.
pub fn apply_awww_output_scope(cmd: &mut Command, scope: &ExecutionScope) -> Result<(), WcError> {
    scope.validate()?;
    match scope {
        ExecutionScope::AllDisplays => Ok(()),
        ExecutionScope::Named(outputs) => {
            cmd.arg("--outputs").arg(outputs.join(","));
            Ok(())
        }
    }
}

pub fn build_awww_img_command_for_scope(
    path: &str,
    resize: &str,
    transition_type: &str,
    duration: &str,
    fps: &str,
    scope: &ExecutionScope,
) -> Result<Command, WcError> {
    let mut cmd = build_awww_img_command_base(path, resize, transition_type, duration, fps);
    apply_awww_output_scope(&mut cmd, scope)?;
    Ok(cmd)
}

pub fn build_awww_instant_command_for_scope(
    path: &str,
    resize: &str,
    fps: &str,
    scope: &ExecutionScope,
) -> Result<Command, WcError> {
    let mut cmd = build_awww_instant_command_base(path, resize, fps);
    apply_awww_output_scope(&mut cmd, scope)?;
    Ok(cmd)
}

/// One mpvpaper invocation per output (CLI accepts exactly one output name).
pub fn build_mpvpaper_launch_command_for_output(
    options: &str,
    output: &str,
    path: &str,
) -> Result<Command, WcError> {
    if output.trim().is_empty() {
        return Err(WcError::Other(
            "mpvpaper output name must not be blank".into(),
        ));
    }
    Ok(build_mpvpaper_launch_command_base(options, output, path))
}

/// Build linux-wallpaperengine argv body (without binary) for planned outputs.
///
/// Emits repeated `--screen-root` / `--bg` pairs. The same wallpaper id is used
/// for every output (All Displays with one wallpaper across screens).
pub fn build_lwe_screen_root_args(
    outputs: &[String],
    wallpaper_id: &str,
) -> Result<Vec<String>, WcError> {
    validate_named_outputs(outputs)?;
    let mut args = Vec::with_capacity(outputs.len() * 4);
    for output in outputs {
        args.push("--screen-root".to_string());
        args.push(output.clone());
        args.push("--bg".to_string());
        args.push(wallpaper_id.to_string());
    }
    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args_of(cmd: &Command) -> Vec<String> {
        cmd.get_args()
            .map(|s| s.to_string_lossy().to_string())
            .collect()
    }

    #[test]
    fn named_scope_rejects_empty_outputs() {
        let err = ExecutionScope::named(vec![]).unwrap_err();
        assert!(err.to_string().contains("at least one output"));
    }

    #[test]
    fn named_scope_rejects_blank_output() {
        let err = ExecutionScope::named(vec!["eDP-1".into(), "  ".into()]).unwrap_err();
        assert!(err.to_string().contains("blank"));
    }

    #[test]
    fn named_scope_rejects_duplicate_outputs() {
        let err = ExecutionScope::named(vec!["eDP-1".into(), "eDP-1".into()]).unwrap_err();
        assert!(err.to_string().contains("duplicate"));
    }

    #[test]
    fn awww_builder_rejects_invalid_named_scope() {
        let invalid = ExecutionScope::Named(vec![]);
        let err =
            build_awww_img_command_for_scope("/tmp/a.jpg", "crop", "fade", "1", "60", &invalid)
                .unwrap_err();
        assert!(err.to_string().contains("at least one output"));
    }

    #[test]
    fn awww_named_scope_adds_comma_separated_outputs() {
        let scope = ExecutionScope::named(vec!["eDP-1".into(), "HDMI-1".into()]).unwrap();
        let cmd = build_awww_img_command_for_scope("/tmp/a.jpg", "crop", "fade", "1", "60", &scope)
            .unwrap();
        let args = args_of(&cmd);
        let idx = args
            .iter()
            .position(|a| a == "--outputs")
            .expect("--outputs");
        assert_eq!(args.get(idx + 1).map(String::as_str), Some("eDP-1,HDMI-1"));
    }

    #[test]
    fn awww_all_displays_omits_outputs_flag() {
        let cmd = build_awww_instant_command_for_scope(
            "/tmp/a.jpg",
            "crop",
            "60",
            &ExecutionScope::AllDisplays,
        )
        .unwrap();
        let args = args_of(&cmd);
        assert!(
            !args.iter().any(|a| a == "--outputs"),
            "AllDisplays must omit --outputs, got {args:?}"
        );
    }

    #[test]
    fn awww_single_named_output_uses_outputs() {
        let scope = ExecutionScope::named(vec!["eDP-1".into()]).unwrap();
        let cmd = build_awww_img_command_for_scope("/tmp/a.jpg", "crop", "fade", "1", "60", &scope)
            .unwrap();
        let args = args_of(&cmd);
        let idx = args
            .iter()
            .position(|a| a == "--outputs")
            .expect("--outputs");
        assert_eq!(args.get(idx + 1).map(String::as_str), Some("eDP-1"));
    }

    #[test]
    fn mpvpaper_command_targets_exact_output_argument() {
        let cmd = build_mpvpaper_launch_command_for_output(
            "--loop-file=inf --panscan=1.0",
            "HDMI-1",
            "/tmp/v.mp4",
        )
        .unwrap();
        let args = args_of(&cmd);
        assert!(args.iter().any(|a| a == "HDMI-1"), "args={args:?}");
        assert!(
            !args.iter().any(|a| a == "*"),
            "must not fall back to wildcard output"
        );
    }

    #[test]
    fn mpvpaper_rejects_blank_output() {
        let err =
            build_mpvpaper_launch_command_for_output("--loop", "  ", "/tmp/v.mp4").unwrap_err();
        assert!(err.to_string().contains("blank"));
    }

    #[test]
    fn lwe_builds_repeated_screen_root_bg_pairs_same_wallpaper() {
        let args =
            build_lwe_screen_root_args(&["eDP-1".into(), "HDMI-1".into()], "1234567890").unwrap();
        assert_eq!(
            args,
            vec![
                "--screen-root".to_string(),
                "eDP-1".to_string(),
                "--bg".to_string(),
                "1234567890".to_string(),
                "--screen-root".to_string(),
                "HDMI-1".to_string(),
                "--bg".to_string(),
                "1234567890".to_string(),
            ]
        );
    }

    #[test]
    fn lwe_single_output_is_one_pair() {
        let args = build_lwe_screen_root_args(&["eDP-1".into()], "/walls/scene").unwrap();
        assert_eq!(
            args,
            vec![
                "--screen-root".to_string(),
                "eDP-1".to_string(),
                "--bg".to_string(),
                "/walls/scene".to_string(),
            ]
        );
    }

    #[test]
    fn lwe_rejects_empty_outputs() {
        let err = build_lwe_screen_root_args(&[], "id").unwrap_err();
        assert!(err.to_string().contains("at least one output"));
    }
}
