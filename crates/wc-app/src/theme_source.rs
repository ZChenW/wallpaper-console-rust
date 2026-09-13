//! Theme-source policy for per-output theme manifests and focus-follow.

use std::time::Duration;

use crate::command_probe::run_probe;

const FOCUS_PROBE_DEADLINE: Duration = Duration::from_millis(1500);

/// Which output supplies the primary theme still / active palette.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeSourcePolicy {
    LastApplied,
    Focused,
    Output(String),
}

impl ThemeSourcePolicy {
    pub fn parse(raw: &str) -> Self {
        let trimmed = raw.trim();
        match trimmed {
            "focused" => Self::Focused,
            other if other.starts_with("output:") => {
                let name = other["output:".len()..].trim();
                if name.is_empty() {
                    Self::LastApplied
                } else {
                    Self::Output(name.to_string())
                }
            }
            _ => Self::LastApplied,
        }
    }

    pub fn as_config_str(&self) -> String {
        match self {
            Self::LastApplied => "last_applied".to_string(),
            Self::Focused => "focused".to_string(),
            Self::Output(name) => format!("output:{name}"),
        }
    }
}

/// Select the output that owns the theme-source still for this apply/restore.
///
/// - `last_applied`: last of `changed_outputs` that is in `known_outputs`, else last known
/// - `output:NAME`: NAME if known, else last_applied fallback
/// - `focused`: `focused` if known, else last_applied fallback
pub fn select_theme_source(
    policy: &ThemeSourcePolicy,
    changed_outputs: &[String],
    known_outputs: &[String],
    focused: Option<&str>,
) -> Option<String> {
    let last_applied = || last_applied_output(changed_outputs, known_outputs);

    match policy {
        ThemeSourcePolicy::LastApplied => last_applied(),
        ThemeSourcePolicy::Output(name) => {
            if known_outputs.iter().any(|known| known == name) {
                Some(name.clone())
            } else {
                last_applied()
            }
        }
        ThemeSourcePolicy::Focused => {
            if let Some(name) = focused {
                if known_outputs.iter().any(|known| known == name) {
                    return Some(name.to_string());
                }
            }
            last_applied()
        }
    }
}

fn last_applied_output(changed_outputs: &[String], known_outputs: &[String]) -> Option<String> {
    changed_outputs
        .iter()
        .rev()
        .find(|changed| known_outputs.iter().any(|known| known == *changed))
        .cloned()
        .or_else(|| known_outputs.last().cloned())
}

/// Probe the currently focused output via compositor helpers.
///
/// Tries niri, then Hyprland, then Sway. Failures / timeouts → `None`.
pub fn probe_focused_output() -> Option<String> {
    if let Some(name) = probe_niri_focused() {
        return Some(name);
    }
    if let Some(name) = probe_hyprland_focused() {
        return Some(name);
    }
    probe_sway_focused()
}

fn probe_niri_focused() -> Option<String> {
    let output = run_probe(
        "niri",
        &["msg", "-j", "focused-output"],
        FOCUS_PROBE_DEADLINE,
    )
    .ok()?;
    if !output.success {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(output.stdout.trim()).ok()?;
    value
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

fn probe_hyprland_focused() -> Option<String> {
    let output = run_probe("hyprctl", &["-j", "monitors"], FOCUS_PROBE_DEADLINE).ok()?;
    if !output.success {
        return None;
    }
    let monitors: Vec<serde_json::Value> = serde_json::from_str(output.stdout.trim()).ok()?;
    monitors.into_iter().find_map(|monitor| {
        let focused = monitor
            .get("focused")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !focused {
            return None;
        }
        monitor
            .get("name")
            .and_then(|v| v.as_str())
            .map(str::to_string)
    })
}

fn probe_sway_focused() -> Option<String> {
    let output = run_probe("swaymsg", &["-t", "get_outputs"], FOCUS_PROBE_DEADLINE).ok()?;
    if !output.success {
        return None;
    }
    let outputs: Vec<serde_json::Value> = serde_json::from_str(output.stdout.trim()).ok()?;
    outputs.into_iter().find_map(|entry| {
        let focused = entry
            .get("focused")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !focused {
            return None;
        }
        entry
            .get("name")
            .and_then(|v| v.as_str())
            .map(str::to_string)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_policy_variants() {
        assert_eq!(
            ThemeSourcePolicy::parse("last_applied"),
            ThemeSourcePolicy::LastApplied
        );
        assert_eq!(
            ThemeSourcePolicy::parse("focused"),
            ThemeSourcePolicy::Focused
        );
        assert_eq!(
            ThemeSourcePolicy::parse("output:DP-8"),
            ThemeSourcePolicy::Output("DP-8".into())
        );
        assert_eq!(
            ThemeSourcePolicy::parse("output:"),
            ThemeSourcePolicy::LastApplied
        );
        assert_eq!(
            ThemeSourcePolicy::parse("bogus"),
            ThemeSourcePolicy::LastApplied
        );
    }

    #[test]
    fn last_applied_prefers_last_changed_in_known() {
        let known = vec!["eDP-1".into(), "DP-8".into()];
        let changed = vec!["eDP-1".into(), "DP-8".into()];
        assert_eq!(
            select_theme_source(&ThemeSourcePolicy::LastApplied, &changed, &known, None),
            Some("DP-8".into())
        );
    }

    #[test]
    fn last_applied_falls_back_to_last_known() {
        let known = vec!["eDP-1".into(), "DP-8".into()];
        let changed = vec!["HDMI-A-1".into()];
        assert_eq!(
            select_theme_source(&ThemeSourcePolicy::LastApplied, &changed, &known, None),
            Some("DP-8".into())
        );
    }

    #[test]
    fn output_policy_uses_named_when_known() {
        let known = vec!["eDP-1".into(), "DP-8".into()];
        let changed = vec!["eDP-1".into()];
        assert_eq!(
            select_theme_source(
                &ThemeSourcePolicy::Output("DP-8".into()),
                &changed,
                &known,
                None
            ),
            Some("DP-8".into())
        );
    }

    #[test]
    fn output_policy_falls_back_when_unknown() {
        let known = vec!["eDP-1".into(), "DP-8".into()];
        let changed = vec!["eDP-1".into()];
        assert_eq!(
            select_theme_source(
                &ThemeSourcePolicy::Output("HDMI-A-1".into()),
                &changed,
                &known,
                None
            ),
            Some("eDP-1".into())
        );
    }

    #[test]
    fn focused_policy_uses_focused_when_known() {
        let known = vec!["eDP-1".into(), "DP-8".into()];
        let changed = vec!["eDP-1".into()];
        assert_eq!(
            select_theme_source(&ThemeSourcePolicy::Focused, &changed, &known, Some("DP-8")),
            Some("DP-8".into())
        );
    }

    #[test]
    fn focused_policy_falls_back_when_missing_or_unknown() {
        let known = vec!["eDP-1".into(), "DP-8".into()];
        let changed = vec!["DP-8".into()];
        assert_eq!(
            select_theme_source(&ThemeSourcePolicy::Focused, &changed, &known, None),
            Some("DP-8".into())
        );
        assert_eq!(
            select_theme_source(
                &ThemeSourcePolicy::Focused,
                &changed,
                &known,
                Some("HDMI-A-1")
            ),
            Some("DP-8".into())
        );
    }

    #[test]
    fn empty_known_returns_none() {
        assert_eq!(
            select_theme_source(&ThemeSourcePolicy::LastApplied, &["a".into()], &[], None),
            None
        );
    }
}
