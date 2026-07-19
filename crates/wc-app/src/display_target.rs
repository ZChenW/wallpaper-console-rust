//! Shared display-target parsing and known-output validation for CLI and GUI.

use std::collections::HashSet;

use wc_storage::sqlite::ALL_DISPLAYS_TARGET_KEY;

use crate::DisplayTarget;

/// Parse an optional display target string.
///
/// `None` means All Displays (GUI default). Blank non-empty strings are rejected.
pub fn parse_display_target(raw: Option<&str>) -> Result<DisplayTarget, String> {
    let Some(raw) = raw else {
        return Ok(DisplayTarget::AllDisplays);
    };
    let value = raw.trim();
    if value.is_empty() {
        return Err("display target must not be blank".into());
    }
    if value.eq_ignore_ascii_case("all")
        || value.eq_ignore_ascii_case("all displays")
        || value == ALL_DISPLAYS_TARGET_KEY
    {
        return Ok(DisplayTarget::AllDisplays);
    }
    Ok(DisplayTarget::Output(value.to_string()))
}

/// Reject blank or duplicate output names.
pub fn validate_known_outputs(outputs: &[String]) -> Result<(), String> {
    let mut seen = HashSet::new();
    for output in outputs {
        if output.trim().is_empty() {
            return Err(format!("blank display output: {output:?}"));
        }
        if !seen.insert(output.as_str()) {
            return Err(format!("duplicate display output: {output}"));
        }
    }
    Ok(())
}

/// Discover connected outputs, validate them, and optionally require an exact
/// explicit set match (order-independent).
///
/// When `explicit_outputs` is `Some` and non-empty, it must match the discovered
/// set exactly. An empty explicit slice is treated like `None` (discovery only).
pub fn resolve_known_outputs_with<F>(
    explicit_outputs: Option<&[String]>,
    discover: F,
) -> Result<Vec<String>, String>
where
    F: FnOnce() -> Result<Vec<String>, String>,
{
    let discovered_outputs = discover()?;
    validate_known_outputs(&discovered_outputs)?;

    if let Some(outputs) = explicit_outputs {
        if !outputs.is_empty() {
            validate_known_outputs(outputs)?;
            let explicit: HashSet<_> = outputs.iter().map(String::as_str).collect();
            let discovered: HashSet<_> = discovered_outputs.iter().map(String::as_str).collect();
            if explicit != discovered {
                return Err(
                    "explicit display outputs must exactly match discovered connected outputs"
                        .into(),
                );
            }
        }
    }

    Ok(discovered_outputs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_display_target_defaults_none_to_all_displays() {
        assert_eq!(
            parse_display_target(None).unwrap(),
            DisplayTarget::AllDisplays
        );
        assert_eq!(
            parse_display_target(Some("all")).unwrap(),
            DisplayTarget::AllDisplays
        );
        assert_eq!(
            parse_display_target(Some("__all_displays__")).unwrap(),
            DisplayTarget::AllDisplays
        );
        assert_eq!(
            parse_display_target(Some(" eDP-1 ")).unwrap(),
            DisplayTarget::Output("eDP-1".into())
        );
        assert!(parse_display_target(Some("  "))
            .unwrap_err()
            .contains("blank"));
    }

    #[test]
    fn resolve_known_outputs_accepts_order_independent_explicit_sets() {
        let explicit = vec!["eDP-1".to_string(), "HDMI-A-1".to_string()];
        let resolved = resolve_known_outputs_with(Some(&explicit), || {
            Ok(vec!["HDMI-A-1".into(), "eDP-1".into()])
        })
        .unwrap();
        assert_eq!(resolved, ["HDMI-A-1", "eDP-1"]);
    }

    #[test]
    fn resolve_known_outputs_rejects_mismatched_explicit_sets() {
        let err = resolve_known_outputs_with(Some(&["eDP-1".into()]), || {
            Ok(vec!["eDP-1".into(), "HDMI-A-1".into()])
        })
        .unwrap_err();
        assert!(err.contains("match"), "{err}");
    }

    #[test]
    fn validate_known_outputs_rejects_blank_and_duplicate() {
        assert!(validate_known_outputs(&["  ".into()])
            .unwrap_err()
            .contains("blank"));
        assert!(validate_known_outputs(&["eDP-1".into(), "eDP-1".into()])
            .unwrap_err()
            .contains("duplicate"));
    }
}
