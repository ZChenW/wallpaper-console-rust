//! Bounded compositor snapshots. Discovery is not evidence of renderer coexistence.
use std::{collections::HashSet, time::Duration};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub name: String,
    pub enabled: bool,
    /// Only populated for a unique make/model/serial tuple, never model alone.
    pub hardware_identity: Option<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Adapter {
    Niri,
    Sway,
    Hyprland,
    Xrandr,
}
impl Adapter {
    pub fn current() -> Option<Self> {
        if std::env::var_os("NIRI_SOCKET").is_some() {
            Some(Self::Niri)
        } else if std::env::var_os("SWAYSOCK").is_some() {
            Some(Self::Sway)
        } else if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
            Some(Self::Hyprland)
        } else if std::env::var("XDG_SESSION_TYPE").is_ok_and(|s| s == "x11") {
            Some(Self::Xrandr)
        } else {
            None
        }
    }
    pub fn snapshot(self) -> Result<Vec<Output>, String> {
        let (program, args): (&str, &[&str]) = match self {
            Self::Niri => ("niri", &["msg", "-j", "outputs"]),
            Self::Sway => ("swaymsg", &["-t", "get_outputs", "-r"]),
            Self::Hyprland => ("hyprctl", &["-j", "monitors", "all"]),
            Self::Xrandr => ("xrandr", &["--query"]),
        };
        let result = crate::command_probe::run_probe(program, args, Duration::from_millis(1500))
            .map_err(|e| format!("{program}: {e:?}"))?;
        if !result.success {
            return Err(format!("{program}: {}", result.stderr));
        }
        self.parse(&result.stdout)
    }
    pub fn parse(self, raw: &str) -> Result<Vec<Output>, String> {
        if self == Self::Xrandr {
            return Ok(raw
                .lines()
                .filter_map(|line| {
                    let words: Vec<_> = line.split_whitespace().collect();
                    if words.get(1) != Some(&"connected") {
                        return None;
                    }
                    Some(Output {
                        name: words[0].into(),
                        enabled: words
                            .iter()
                            .skip(2)
                            .any(|s| s.contains('x') && s.contains('+')),
                        hardware_identity: None,
                    })
                })
                .collect());
        }
        let root: serde_json::Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
        let entries: Vec<_> = if self == Self::Niri {
            root.as_object()
                .ok_or("expected output object")?
                .values()
                .collect()
        } else {
            root.as_array()
                .ok_or("expected output array")?
                .iter()
                .collect()
        };
        let mut seen = HashSet::new();
        let mut outputs = Vec::new();
        for entry in entries {
            let name = entry["name"]
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .ok_or("missing output name")?;
            if !seen.insert(name) {
                return Err("duplicate output connector".into());
            }
            let enabled = match self {
                Self::Niri => entry["current_mode"].is_number() && entry["logical"].is_object(),
                Self::Sway => entry["active"].as_bool() == Some(true),
                Self::Hyprland => {
                    entry["disabled"].as_bool() != Some(true)
                        && entry["width"].as_u64().unwrap_or(0) > 0
                }
                Self::Xrandr => unreachable!(),
            };
            let hardware_identity = entry["serial"]
                .as_str()
                .filter(|s| !s.trim().is_empty() && !matches!(*s, "Unknown" | "0"))
                .map(|serial| {
                    serde_json::json!([entry["make"], entry["model"], serial]).to_string()
                });
            outputs.push(Output {
                name: name.into(),
                enabled,
                hardware_identity,
            });
        }
        let duplicates: HashSet<_> = outputs
            .iter()
            .filter_map(|o| o.hardware_identity.as_ref())
            .filter(|id| {
                outputs
                    .iter()
                    .filter(|o| o.hardware_identity.as_ref() == Some(id))
                    .count()
                    > 1
            })
            .cloned()
            .collect();
        for output in &mut outputs {
            if output
                .hardware_identity
                .as_ref()
                .is_some_and(|id| duplicates.contains(id))
            {
                output.hardware_identity = None;
            }
        }
        outputs.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(outputs)
    }
}

/// Connector changes need a unique stable identity on both sides. A connector
/// with no serial can only retain its own name; it cannot borrow another's recipe.
pub fn matching_previous<'a>(current: &Output, previous: &'a [Output]) -> Option<&'a Output> {
    if let Some(id) = &current.hardware_identity {
        let mut matches = previous
            .iter()
            .filter(|o| o.hardware_identity.as_ref() == Some(id));
        let first = matches.next()?;
        if matches.next().is_none() {
            return Some(first);
        }
        None
    } else {
        previous
            .iter()
            .find(|o| o.name == current.name && o.hardware_identity.is_none())
    }
}
