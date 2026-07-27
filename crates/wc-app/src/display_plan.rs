//! Side-effect-free per-display apply planner.
//!
//! Validates a requested display target against backend capabilities and the
//! current running assignment map. Does not execute backends, touch storage,
//! or expand a named output into All Displays.

use wc_backend::capability::{
    apply_output_groups, capability_for, AllDisplaysTargeting, BackendCapability,
    MultiInstanceSupport, SameTargetReplacement,
};
use wc_core::types::Backend;

/// User-selected apply target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisplayTarget {
    /// A concrete Wayland/X output name (for example `eDP-1`).
    Output(String),
    /// Explicit all-display operation; never inferred from a named output.
    AllDisplays,
}

/// Wallpaper currently associated with one concrete output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningAssignment {
    pub output: String,
    pub backend: Backend,
}

/// Pure planning input. No I/O.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayApplyRequest {
    pub target: DisplayTarget,
    pub backend: Backend,
    pub known_outputs: Vec<String>,
    pub running: Vec<RunningAssignment>,
}

/// One planned step. Execution is intentionally out of scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlannedAction {
    /// Stop `backend` on the listed outputs before a replacement Apply.
    Stop {
        backend: Backend,
        outputs: Vec<String>,
    },
    /// Apply `backend` to the listed outputs (one group = one CLI invocation).
    Apply {
        backend: Backend,
        outputs: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayApplyPlan {
    pub actions: Vec<PlannedAction>,
    pub capability: BackendCapability,
}

/// Why a display apply request was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RejectionReason {
    UnsupportedBackend,
    /// Named target string is empty or whitespace-only.
    EmptyNamedOutput,
    /// Named target is not present in `known_outputs`.
    UnknownNamedOutput {
        output: String,
    },
    /// `known_outputs` contains an empty or whitespace-only entry.
    BlankKnownOutput {
        output: String,
    },
    /// `known_outputs` contains the same name more than once.
    DuplicateKnownOutputs {
        output: String,
    },
    /// A running assignment output is empty or whitespace-only.
    BlankRunningAssignmentOutput {
        output: String,
    },
    /// `running` lists the same output more than once with the same backend.
    DuplicateRunningAssignment {
        output: String,
    },
    /// `running` lists the same output more than once with different backends.
    ConflictingRunningAssignment {
        output: String,
    },
    /// A running assignment refers to an output absent from `known_outputs`.
    RunningAssignmentUnknownOutput {
        output: String,
    },
    /// Named target would disturb another concrete output.
    WouldAffectNonTargetDisplay {
        non_target: String,
        explanation: String,
    },
    /// Plan depends on cross-output coexistence that is unknown/unverified.
    ReliesOnUnknownCoexistence {
        explanation: String,
    },
    /// Backend cannot express the requested target with verified facts alone.
    UnverifiedTargetScope {
        explanation: String,
    },
    /// Current stop scope would clear wallpaper on a non-target output.
    StopWouldAffectNonTarget {
        non_target: String,
        explanation: String,
    },
    /// Explicit All Displays requested but no outputs are known.
    NoKnownOutputs,
}

/// Plan a display-scoped apply without side effects.
pub fn plan_display_apply(
    request: &DisplayApplyRequest,
) -> Result<DisplayApplyPlan, RejectionReason> {
    let capability = capability_for(request.backend).ok_or(RejectionReason::UnsupportedBackend)?;
    plan_display_apply_with_capability(request, capability)
}

/// Injectable planning seam: callers (and tests) may supply a capability
/// declaration, including a future verified multi-instance mpvpaper matrix.
pub fn plan_display_apply_with_capability(
    request: &DisplayApplyRequest,
    capability: BackendCapability,
) -> Result<DisplayApplyPlan, RejectionReason> {
    if capability.backend != request.backend {
        return Err(RejectionReason::UnsupportedBackend);
    }

    validate_request_invariants(request)?;

    match &request.target {
        DisplayTarget::AllDisplays => plan_all_displays(request, capability),
        DisplayTarget::Output(output) => plan_named_output(request, capability, output),
    }
}

fn is_blank(value: &str) -> bool {
    value.trim().is_empty()
}

fn validate_request_invariants(request: &DisplayApplyRequest) -> Result<(), RejectionReason> {
    let mut seen_known = Vec::new();
    for output in &request.known_outputs {
        if is_blank(output) {
            return Err(RejectionReason::BlankKnownOutput {
                output: output.clone(),
            });
        }
        if seen_known.iter().any(|seen| seen == output) {
            return Err(RejectionReason::DuplicateKnownOutputs {
                output: output.clone(),
            });
        }
        seen_known.push(output.clone());
    }

    let mut seen_running: Vec<(&str, Backend)> = Vec::new();
    for assignment in &request.running {
        if is_blank(&assignment.output) {
            return Err(RejectionReason::BlankRunningAssignmentOutput {
                output: assignment.output.clone(),
            });
        }
        if !request
            .known_outputs
            .iter()
            .any(|o| o == &assignment.output)
        {
            return Err(RejectionReason::RunningAssignmentUnknownOutput {
                output: assignment.output.clone(),
            });
        }
        if let Some((_, previous)) = seen_running
            .iter()
            .find(|(output, _)| *output == assignment.output)
        {
            if *previous == assignment.backend {
                return Err(RejectionReason::DuplicateRunningAssignment {
                    output: assignment.output.clone(),
                });
            }
            return Err(RejectionReason::ConflictingRunningAssignment {
                output: assignment.output.clone(),
            });
        }
        seen_running.push((assignment.output.as_str(), assignment.backend));
    }

    match &request.target {
        DisplayTarget::Output(output) => {
            if is_blank(output) {
                return Err(RejectionReason::EmptyNamedOutput);
            }
            if !request.known_outputs.iter().any(|known| known == output) {
                return Err(RejectionReason::UnknownNamedOutput {
                    output: output.clone(),
                });
            }
        }
        DisplayTarget::AllDisplays => {
            if request.known_outputs.is_empty() {
                return Err(RejectionReason::NoKnownOutputs);
            }
        }
    }

    Ok(())
}

fn plan_all_displays(
    request: &DisplayApplyRequest,
    capability: BackendCapability,
) -> Result<DisplayApplyPlan, RejectionReason> {
    if request.known_outputs.len() > 1 {
        match capability.all_displays {
            AllDisplaysTargeting::OneProcessPerOutput => {
                if !matches!(
                    capability.multi_instance,
                    MultiInstanceSupport::SeparateProcessesVerified
                ) {
                    return Err(RejectionReason::ReliesOnUnknownCoexistence {
                        explanation: format!(
                            "{} All Displays needs one process per output, but multi-instance coexistence is unverified",
                            request.backend.as_str()
                        ),
                    });
                }
            }
            AllDisplaysTargeting::OmitMeansAll | AllDisplaysTargeting::SingleProcessMultiOutput => {
            }
        }
    }

    let mut actions = replacement_stops_for_all_displays(request, &capability)?;
    actions.extend(apply_actions_for_outputs(
        &capability,
        request.backend,
        &request.known_outputs,
    ));

    Ok(DisplayApplyPlan {
        actions,
        capability,
    })
}

fn plan_named_output(
    request: &DisplayApplyRequest,
    capability: BackendCapability,
    output: &str,
) -> Result<DisplayApplyPlan, RejectionReason> {
    if !capability.verified_named_output_targeting() {
        return Err(RejectionReason::UnverifiedTargetScope {
            explanation: format!(
                "{} lacks CLI-verified named-output targeting",
                request.backend.as_str()
            ),
        });
    }

    let current_on_target = request
        .running
        .iter()
        .find(|assignment| assignment.output == output);

    let mut actions = Vec::new();
    if let Some(current) = current_on_target {
        if current.backend != request.backend {
            actions.push(explicit_replacement_stop(request, current)?);
        } else if capability.requires_stop_before_same_target_apply() {
            actions.push(same_target_stop(request, &capability, current)?);
        }
    }

    let others: Vec<&RunningAssignment> = request
        .running
        .iter()
        .filter(|assignment| assignment.output != output)
        .collect();

    for other in &others {
        if other.backend != request.backend {
            if !capability.cross_output_coexistence_verified() {
                return Err(RejectionReason::ReliesOnUnknownCoexistence {
                    explanation: format!(
                        "applying {} to {} while {} runs on {} relies on unknown cross-output coexistence",
                        request.backend.as_str(),
                        output,
                        other.backend.as_str(),
                        other.output
                    ),
                });
            }
            continue;
        }

        reject_same_backend_on_other_output(request, &capability, output, other)?;
    }

    actions.extend(apply_actions_for_outputs(
        &capability,
        request.backend,
        &[output.to_string()],
    ));

    Ok(DisplayApplyPlan {
        actions,
        capability,
    })
}

fn reject_same_backend_on_other_output(
    request: &DisplayApplyRequest,
    capability: &BackendCapability,
    target: &str,
    other: &RunningAssignment,
) -> Result<(), RejectionReason> {
    match capability.multi_instance {
        MultiInstanceSupport::OneShot => Err(RejectionReason::WouldAffectNonTargetDisplay {
            non_target: other.output.clone(),
            explanation: format!(
                "{} is an all-displays one-shot setter and cannot change only {}",
                request.backend.as_str(),
                target
            ),
        }),
        MultiInstanceSupport::SharedDaemon => Ok(()),
        MultiInstanceSupport::SeparateProcessesVerified => {
            if capability.stop_may_affect_non_target_outputs() {
                return Err(RejectionReason::StopWouldAffectNonTarget {
                    non_target: other.output.clone(),
                    explanation: format!(
                        "{} stop would affect {}",
                        request.backend.as_str(),
                        other.output
                    ),
                });
            }
            Ok(())
        }
        MultiInstanceSupport::SeparateProcessesUnverified => {
            if capability.stop_may_affect_non_target_outputs() {
                return Err(RejectionReason::StopWouldAffectNonTarget {
                    non_target: other.output.clone(),
                    explanation: format!(
                        "{} stop is process-wide and would affect {}",
                        request.backend.as_str(),
                        other.output
                    ),
                });
            }
            Err(RejectionReason::ReliesOnUnknownCoexistence {
                explanation: format!(
                    "{} multi-instance coexistence with {} is unverified",
                    request.backend.as_str(),
                    other.output
                ),
            })
        }
        MultiInstanceSupport::SingleProcessUnverified => {
            Err(RejectionReason::WouldAffectNonTargetDisplay {
                non_target: other.output.clone(),
                explanation: format!(
                    "{} uses a shared process; changing {} would disturb {}",
                    request.backend.as_str(),
                    target,
                    other.output
                ),
            })
        }
    }
}

fn same_target_stop(
    request: &DisplayApplyRequest,
    capability: &BackendCapability,
    current: &RunningAssignment,
) -> Result<PlannedAction, RejectionReason> {
    if capability.stop_may_affect_non_target_outputs() {
        if let Some(other) = request.running.iter().find(|assignment| {
            assignment.output != current.output && assignment.backend == current.backend
        }) {
            return Err(RejectionReason::StopWouldAffectNonTarget {
                non_target: other.output.clone(),
                explanation: format!(
                    "{} stop is process-wide and would affect {}",
                    current.backend.as_str(),
                    other.output
                ),
            });
        }
    }

    Ok(PlannedAction::Stop {
        backend: current.backend,
        outputs: vec![current.output.clone()],
    })
}

fn explicit_replacement_stop(
    request: &DisplayApplyRequest,
    current: &RunningAssignment,
) -> Result<PlannedAction, RejectionReason> {
    let stop_cap = capability_for(current.backend).ok_or(RejectionReason::UnsupportedBackend)?;
    if stop_cap.stop_may_affect_non_target_outputs() {
        if let Some(other) = request.running.iter().find(|assignment| {
            assignment.output != current.output && assignment.backend == current.backend
        }) {
            return Err(RejectionReason::StopWouldAffectNonTarget {
                non_target: other.output.clone(),
                explanation: format!(
                    "{} stop is process-wide and would affect {}",
                    current.backend.as_str(),
                    other.output
                ),
            });
        }
    }

    Ok(PlannedAction::Stop {
        backend: current.backend,
        outputs: vec![current.output.clone()],
    })
}

fn replacement_stops_for_all_displays(
    request: &DisplayApplyRequest,
    capability: &BackendCapability,
) -> Result<Vec<PlannedAction>, RejectionReason> {
    let mut stops = Vec::new();
    let mut stopped = Vec::new();
    for assignment in &request.running {
        if assignment.backend == request.backend {
            continue;
        }
        if stopped.contains(&assignment.backend) {
            continue;
        }
        stopped.push(assignment.backend);
        stops.push(PlannedAction::Stop {
            backend: assignment.backend,
            outputs: request
                .running
                .iter()
                .filter(|a| a.backend == assignment.backend)
                .map(|a| a.output.clone())
                .collect(),
        });
    }

    if matches!(
        capability.same_target_replacement,
        SameTargetReplacement::StopThenApply
    ) {
        let same_backend_outputs: Vec<String> = request
            .running
            .iter()
            .filter(|assignment| assignment.backend == request.backend)
            .map(|assignment| assignment.output.clone())
            .collect();
        if !same_backend_outputs.is_empty() {
            stops.push(PlannedAction::Stop {
                backend: request.backend,
                outputs: same_backend_outputs,
            });
        }
    }

    Ok(stops)
}

fn apply_actions_for_outputs(
    capability: &BackendCapability,
    backend: Backend,
    outputs: &[String],
) -> Vec<PlannedAction> {
    apply_output_groups(capability.all_displays, outputs)
        .into_iter()
        .map(|group| PlannedAction::Apply {
            backend,
            outputs: group,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wc_backend::capability::{
        CrossOutputCoexistence, Evidence, MultiInstanceSupport, OutputTargetMode,
        SameTargetReplacement, StopScope,
    };

    fn edp() -> String {
        "eDP-1".into()
    }

    fn hdmi() -> String {
        "HDMI-1".into()
    }

    fn dual_outputs() -> Vec<String> {
        vec![edp(), hdmi()]
    }

    fn req(
        target: DisplayTarget,
        backend: Backend,
        known_outputs: Vec<String>,
        running: Vec<RunningAssignment>,
    ) -> DisplayApplyRequest {
        DisplayApplyRequest {
            target,
            backend,
            known_outputs,
            running,
        }
    }

    #[test]
    fn rejects_unsupported_backend() {
        let err = plan_display_apply(&req(
            DisplayTarget::Output(edp()),
            Backend::Unsupported,
            vec![edp()],
            vec![],
        ))
        .unwrap_err();
        assert_eq!(err, RejectionReason::UnsupportedBackend);
    }

    #[test]
    fn rejects_empty_named_output() {
        let err = plan_display_apply(&req(
            DisplayTarget::Output(String::new()),
            Backend::Awww,
            vec![edp()],
            vec![],
        ))
        .unwrap_err();
        assert_eq!(err, RejectionReason::EmptyNamedOutput);
    }

    #[test]
    fn rejects_whitespace_named_output() {
        let err = plan_display_apply(&req(
            DisplayTarget::Output("   ".into()),
            Backend::Awww,
            vec![edp()],
            vec![],
        ))
        .unwrap_err();
        assert_eq!(err, RejectionReason::EmptyNamedOutput);
    }

    #[test]
    fn rejects_blank_known_output() {
        let err = plan_display_apply(&req(
            DisplayTarget::Output(edp()),
            Backend::Awww,
            vec![edp(), "  ".into()],
            vec![],
        ))
        .unwrap_err();
        assert_eq!(
            err,
            RejectionReason::BlankKnownOutput {
                output: "  ".into()
            }
        );
    }

    #[test]
    fn rejects_blank_running_assignment_output() {
        let err = plan_display_apply(&req(
            DisplayTarget::Output(edp()),
            Backend::Awww,
            vec![edp()],
            vec![RunningAssignment {
                output: "\t".into(),
                backend: Backend::Awww,
            }],
        ))
        .unwrap_err();
        assert_eq!(
            err,
            RejectionReason::BlankRunningAssignmentOutput {
                output: "\t".into()
            }
        );
    }

    #[test]
    fn rejects_unknown_named_output() {
        let err = plan_display_apply(&req(
            DisplayTarget::Output("DP-9".into()),
            Backend::Awww,
            vec![edp()],
            vec![],
        ))
        .unwrap_err();
        assert_eq!(
            err,
            RejectionReason::UnknownNamedOutput {
                output: "DP-9".into()
            }
        );
    }

    #[test]
    fn rejects_duplicate_known_outputs() {
        let err = plan_display_apply(&req(
            DisplayTarget::Output(edp()),
            Backend::Awww,
            vec![edp(), edp()],
            vec![],
        ))
        .unwrap_err();
        assert_eq!(
            err,
            RejectionReason::DuplicateKnownOutputs { output: edp() }
        );
    }

    #[test]
    fn rejects_duplicate_running_assignment() {
        let err = plan_display_apply(&req(
            DisplayTarget::Output(edp()),
            Backend::Awww,
            vec![edp()],
            vec![
                RunningAssignment {
                    output: edp(),
                    backend: Backend::Awww,
                },
                RunningAssignment {
                    output: edp(),
                    backend: Backend::Awww,
                },
            ],
        ))
        .unwrap_err();
        assert_eq!(
            err,
            RejectionReason::DuplicateRunningAssignment { output: edp() }
        );
    }

    #[test]
    fn rejects_conflicting_running_assignment() {
        let err = plan_display_apply(&req(
            DisplayTarget::Output(edp()),
            Backend::Awww,
            vec![edp()],
            vec![
                RunningAssignment {
                    output: edp(),
                    backend: Backend::Awww,
                },
                RunningAssignment {
                    output: edp(),
                    backend: Backend::Mpvpaper,
                },
            ],
        ))
        .unwrap_err();
        assert_eq!(
            err,
            RejectionReason::ConflictingRunningAssignment { output: edp() }
        );
    }

    #[test]
    fn rejects_running_assignment_for_unknown_output() {
        let err = plan_display_apply(&req(
            DisplayTarget::Output(edp()),
            Backend::Awww,
            vec![edp()],
            vec![RunningAssignment {
                output: hdmi(),
                backend: Backend::Awww,
            }],
        ))
        .unwrap_err();
        assert_eq!(
            err,
            RejectionReason::RunningAssignmentUnknownOutput { output: hdmi() }
        );
    }

    #[test]
    fn accepts_named_output_when_only_that_output_is_idle() {
        let plan = plan_display_apply(&req(
            DisplayTarget::Output(edp()),
            Backend::Awww,
            vec![edp()],
            vec![],
        ))
        .expect("single idle output must be safe");
        assert_eq!(
            plan.actions,
            vec![PlannedAction::Apply {
                backend: Backend::Awww,
                outputs: vec![edp()],
            }]
        );
        assert_eq!(plan.capability.backend, Backend::Awww);
    }

    #[test]
    fn accepts_explicit_all_displays_for_awww_omit_means_all() {
        let plan = plan_display_apply(&req(
            DisplayTarget::AllDisplays,
            Backend::Awww,
            dual_outputs(),
            vec![],
        ))
        .expect("explicit all displays is intentional");
        assert_eq!(
            plan.actions,
            vec![PlannedAction::Apply {
                backend: Backend::Awww,
                outputs: dual_outputs(),
            }]
        );
    }

    #[test]
    fn accepts_explicit_all_displays_for_lwe_single_process_multi_output() {
        let plan = plan_display_apply(&req(
            DisplayTarget::AllDisplays,
            Backend::LinuxWallpaperEngine,
            dual_outputs(),
            vec![],
        ))
        .expect("LWE CLI supports repeated screen-root pairs");
        assert_eq!(
            plan.actions,
            vec![PlannedAction::Apply {
                backend: Backend::LinuxWallpaperEngine,
                outputs: dual_outputs(),
            }]
        );
    }

    #[test]
    fn feh_accepts_only_explicit_all_displays() {
        let plan = plan_display_apply(&req(
            DisplayTarget::AllDisplays,
            Backend::Feh,
            dual_outputs(),
            vec![],
        ))
        .expect("feh may intentionally update the X root for all displays");
        assert_eq!(
            plan.actions,
            vec![PlannedAction::Apply {
                backend: Backend::Feh,
                outputs: dual_outputs(),
            }]
        );

        let error = plan_display_apply(&req(
            DisplayTarget::Output(edp()),
            Backend::Feh,
            dual_outputs(),
            vec![],
        ))
        .unwrap_err();
        assert!(matches!(
            error,
            RejectionReason::UnverifiedTargetScope { .. }
        ));
    }

    #[test]
    fn accepts_mpvpaper_on_single_known_output_when_idle() {
        let plan = plan_display_apply(&req(
            DisplayTarget::Output(edp()),
            Backend::Mpvpaper,
            vec![edp()],
            vec![],
        ))
        .expect("one output + one mpvpaper process is CLI-verified");
        assert_eq!(
            plan.actions,
            vec![PlannedAction::Apply {
                backend: Backend::Mpvpaper,
                outputs: vec![edp()],
            }]
        );
    }

    #[test]
    fn rejects_all_displays_without_known_outputs() {
        let err = plan_display_apply(&req(
            DisplayTarget::AllDisplays,
            Backend::Awww,
            vec![],
            vec![],
        ))
        .unwrap_err();
        assert_eq!(err, RejectionReason::NoKnownOutputs);
    }

    #[test]
    fn rejects_mpvpaper_all_displays_when_multi_instance_unverified() {
        let err = plan_display_apply(&req(
            DisplayTarget::AllDisplays,
            Backend::Mpvpaper,
            dual_outputs(),
            vec![],
        ))
        .unwrap_err();
        match err {
            RejectionReason::ReliesOnUnknownCoexistence { explanation } => {
                assert!(
                    explanation.contains("mpvpaper"),
                    "explanation={explanation}"
                );
            }
            other => panic!("expected ReliesOnUnknownCoexistence, got {other:?}"),
        }
    }

    #[test]
    fn rejects_named_output_when_other_output_has_different_backend() {
        let err = plan_display_apply(&req(
            DisplayTarget::Output(edp()),
            Backend::Awww,
            dual_outputs(),
            vec![RunningAssignment {
                output: hdmi(),
                backend: Backend::Mpvpaper,
            }],
        ))
        .unwrap_err();
        match err {
            RejectionReason::ReliesOnUnknownCoexistence { explanation } => {
                assert!(explanation.contains("HDMI-1"), "explanation={explanation}");
            }
            other => panic!("expected ReliesOnUnknownCoexistence, got {other:?}"),
        }
    }

    #[test]
    fn injected_verified_cross_output_coexistence_allows_different_backend_on_other_output() {
        let mut capability = capability_for(Backend::Awww).expect("awww");
        capability.cross_output_coexistence = CrossOutputCoexistence::Verified;
        capability.cross_output_coexistence_evidence = Evidence::CliVerified;

        let plan = plan_display_apply_with_capability(
            &req(
                DisplayTarget::Output(edp()),
                Backend::Awww,
                dual_outputs(),
                vec![RunningAssignment {
                    output: hdmi(),
                    backend: Backend::Mpvpaper,
                }],
            ),
            capability,
        )
        .expect(
            "CliVerified cross-output coexistence allows a different backend on another output",
        );
        assert_eq!(
            plan.actions,
            vec![PlannedAction::Apply {
                backend: Backend::Awww,
                outputs: vec![edp()],
            }]
        );
    }

    #[test]
    fn rejects_mpvpaper_named_output_when_other_output_also_uses_mpvpaper() {
        let err = plan_display_apply(&req(
            DisplayTarget::Output(edp()),
            Backend::Mpvpaper,
            dual_outputs(),
            vec![RunningAssignment {
                output: hdmi(),
                backend: Backend::Mpvpaper,
            }],
        ))
        .unwrap_err();
        assert_eq!(
            err,
            RejectionReason::StopWouldAffectNonTarget {
                non_target: hdmi(),
                explanation: "mpvpaper stop is process-wide and would affect HDMI-1".into(),
            }
        );
    }

    #[test]
    fn rejects_lwe_named_output_when_other_output_uses_lwe_shared_process() {
        let err = plan_display_apply(&req(
            DisplayTarget::Output(edp()),
            Backend::LinuxWallpaperEngine,
            dual_outputs(),
            vec![RunningAssignment {
                output: hdmi(),
                backend: Backend::LinuxWallpaperEngine,
            }],
        ))
        .unwrap_err();
        assert_eq!(
            err,
            RejectionReason::WouldAffectNonTargetDisplay {
                non_target: hdmi(),
                explanation:
                    "linux-wallpaperengine uses a shared process; changing eDP-1 would disturb HDMI-1"
                        .into(),
            }
        );
    }

    #[test]
    fn accepts_awww_named_output_when_other_output_also_uses_awww() {
        let plan = plan_display_apply(&req(
            DisplayTarget::Output(edp()),
            Backend::Awww,
            dual_outputs(),
            vec![RunningAssignment {
                output: hdmi(),
                backend: Backend::Awww,
            }],
        ))
        .expect("awww named retarget within shared daemon is CLI-verified");
        assert_eq!(
            plan.actions,
            vec![PlannedAction::Apply {
                backend: Backend::Awww,
                outputs: vec![edp()],
            }]
        );
    }

    #[test]
    fn mpvpaper_same_target_replacement_emits_stop_then_apply() {
        let plan = plan_display_apply(&req(
            DisplayTarget::Output(edp()),
            Backend::Mpvpaper,
            vec![edp()],
            vec![RunningAssignment {
                output: edp(),
                backend: Backend::Mpvpaper,
            }],
        ))
        .expect("mpvpaper same-target replacement requires StopThenApply");
        assert_eq!(
            plan.actions,
            vec![
                PlannedAction::Stop {
                    backend: Backend::Mpvpaper,
                    outputs: vec![edp()],
                },
                PlannedAction::Apply {
                    backend: Backend::Mpvpaper,
                    outputs: vec![edp()],
                },
            ]
        );
    }

    #[test]
    fn awww_same_target_replacement_applies_in_place() {
        let plan = plan_display_apply(&req(
            DisplayTarget::Output(edp()),
            Backend::Awww,
            vec![edp()],
            vec![RunningAssignment {
                output: edp(),
                backend: Backend::Awww,
            }],
        ))
        .expect("awww retargets in place");
        assert_eq!(
            plan.actions,
            vec![PlannedAction::Apply {
                backend: Backend::Awww,
                outputs: vec![edp()],
            }]
        );
    }

    #[test]
    fn lwe_same_target_replacement_uses_managed_handoff_apply_only() {
        let plan = plan_display_apply(&req(
            DisplayTarget::Output(edp()),
            Backend::LinuxWallpaperEngine,
            vec![edp()],
            vec![RunningAssignment {
                output: edp(),
                backend: Backend::LinuxWallpaperEngine,
            }],
        ))
        .expect("LWE managed handoff is apply-only at plan level");
        assert_eq!(
            plan.actions,
            vec![PlannedAction::Apply {
                backend: Backend::LinuxWallpaperEngine,
                outputs: vec![edp()],
            }]
        );
        assert_eq!(
            plan.capability.same_target_replacement,
            SameTargetReplacement::ManagedHandoff
        );
    }

    #[test]
    fn cross_backend_target_replacement_requires_explicit_stop_before_apply() {
        let plan = plan_display_apply(&req(
            DisplayTarget::Output(edp()),
            Backend::Awww,
            vec![edp()],
            vec![RunningAssignment {
                output: edp(),
                backend: Backend::Mpvpaper,
            }],
        ))
        .expect("safe replacement must be explicit");
        assert_eq!(
            plan.actions,
            vec![
                PlannedAction::Stop {
                    backend: Backend::Mpvpaper,
                    outputs: vec![edp()],
                },
                PlannedAction::Apply {
                    backend: Backend::Awww,
                    outputs: vec![edp()],
                },
            ]
        );
    }

    #[test]
    fn cross_backend_replacement_rejects_when_stop_would_hit_non_target() {
        let err = plan_display_apply(&req(
            DisplayTarget::Output(edp()),
            Backend::Awww,
            dual_outputs(),
            vec![
                RunningAssignment {
                    output: edp(),
                    backend: Backend::Mpvpaper,
                },
                RunningAssignment {
                    output: hdmi(),
                    backend: Backend::Mpvpaper,
                },
            ],
        ))
        .unwrap_err();
        assert_eq!(
            err,
            RejectionReason::StopWouldAffectNonTarget {
                non_target: hdmi(),
                explanation: "mpvpaper stop is process-wide and would affect HDMI-1".into(),
            }
        );
    }

    #[test]
    fn never_silently_expands_named_output_to_all_displays() {
        let plan = plan_display_apply(&req(
            DisplayTarget::Output(edp()),
            Backend::Awww,
            dual_outputs(),
            vec![],
        ))
        .expect("idle dual-head named target");
        match &plan.actions[..] {
            [PlannedAction::Apply { outputs, .. }] => {
                assert_eq!(outputs, &vec![edp()]);
                assert_ne!(outputs, &dual_outputs());
            }
            other => panic!("unexpected actions: {other:?}"),
        }
    }

    #[test]
    fn injected_verified_one_process_per_output_emits_separate_apply_actions() {
        let mut capability = capability_for(Backend::Mpvpaper).expect("mpvpaper");
        capability.multi_instance = MultiInstanceSupport::SeparateProcessesVerified;
        capability.multi_instance_evidence = Evidence::CliVerified;

        let plan = plan_display_apply_with_capability(
            &req(
                DisplayTarget::AllDisplays,
                Backend::Mpvpaper,
                dual_outputs(),
                vec![],
            ),
            capability,
        )
        .expect("verified multi-instance may plan all displays");

        assert_eq!(
            plan.actions,
            vec![
                PlannedAction::Apply {
                    backend: Backend::Mpvpaper,
                    outputs: vec![edp()],
                },
                PlannedAction::Apply {
                    backend: Backend::Mpvpaper,
                    outputs: vec![hdmi()],
                },
            ]
        );
        assert_ne!(
            plan.actions,
            vec![PlannedAction::Apply {
                backend: Backend::Mpvpaper,
                outputs: dual_outputs(),
            }],
            "OneProcessPerOutput must not emit a grouped multi-output Apply"
        );
    }

    fn verified_per_output_mpvpaper() -> BackendCapability {
        let mut capability = capability_for(Backend::Mpvpaper).expect("mpvpaper");
        capability.multi_instance = MultiInstanceSupport::SeparateProcessesVerified;
        capability.multi_instance_evidence = Evidence::CliVerified;
        capability.stop_scope = StopScope::TrackedProcessPerOutput;
        capability.stop_scope_evidence = Evidence::CliVerified;
        capability.cross_output_coexistence = CrossOutputCoexistence::Verified;
        capability.cross_output_coexistence_evidence = Evidence::CliVerified;
        capability
    }

    #[test]
    fn injected_verified_mpvpaper_named_output_allows_other_output_mpvpaper() {
        let plan = plan_display_apply_with_capability(
            &req(
                DisplayTarget::Output(edp()),
                Backend::Mpvpaper,
                dual_outputs(),
                vec![RunningAssignment {
                    output: hdmi(),
                    backend: Backend::Mpvpaper,
                }],
            ),
            verified_per_output_mpvpaper(),
        )
        .expect("verified per-output mpvpaper may leave another output alone");
        assert_eq!(
            plan.actions,
            vec![PlannedAction::Apply {
                backend: Backend::Mpvpaper,
                outputs: vec![edp()],
            }]
        );
    }

    #[test]
    fn injected_verified_mpvpaper_same_target_replacement_still_stop_then_apply() {
        let plan = plan_display_apply_with_capability(
            &req(
                DisplayTarget::Output(edp()),
                Backend::Mpvpaper,
                dual_outputs(),
                vec![
                    RunningAssignment {
                        output: edp(),
                        backend: Backend::Mpvpaper,
                    },
                    RunningAssignment {
                        output: hdmi(),
                        backend: Backend::Mpvpaper,
                    },
                ],
            ),
            verified_per_output_mpvpaper(),
        )
        .expect("same-target mpvpaper replacement remains StopThenApply");
        assert_eq!(
            plan.actions,
            vec![
                PlannedAction::Stop {
                    backend: Backend::Mpvpaper,
                    outputs: vec![edp()],
                },
                PlannedAction::Apply {
                    backend: Backend::Mpvpaper,
                    outputs: vec![edp()],
                },
            ]
        );
    }

    #[test]
    fn injected_verified_mpvpaper_all_displays_with_existing_emits_one_stop_then_per_output_apply()
    {
        let plan = plan_display_apply_with_capability(
            &req(
                DisplayTarget::AllDisplays,
                Backend::Mpvpaper,
                dual_outputs(),
                vec![
                    RunningAssignment {
                        output: edp(),
                        backend: Backend::Mpvpaper,
                    },
                    RunningAssignment {
                        output: hdmi(),
                        backend: Backend::Mpvpaper,
                    },
                ],
            ),
            verified_per_output_mpvpaper(),
        )
        .expect("verified all-displays must not duplicate processes");
        assert_eq!(
            plan.actions,
            vec![
                PlannedAction::Stop {
                    backend: Backend::Mpvpaper,
                    outputs: vec![edp(), hdmi()],
                },
                PlannedAction::Apply {
                    backend: Backend::Mpvpaper,
                    outputs: vec![edp()],
                },
                PlannedAction::Apply {
                    backend: Backend::Mpvpaper,
                    outputs: vec![hdmi()],
                },
            ]
        );
    }

    #[test]
    fn table_driven_rejection_invariants() {
        let cases: Vec<(DisplayApplyRequest, RejectionReason)> = vec![
            (
                req(
                    DisplayTarget::Output(edp()),
                    Backend::Mpvpaper,
                    dual_outputs(),
                    vec![RunningAssignment {
                        output: hdmi(),
                        backend: Backend::Awww,
                    }],
                ),
                RejectionReason::ReliesOnUnknownCoexistence {
                    explanation: "applying mpvpaper to eDP-1 while awww runs on HDMI-1 relies on unknown cross-output coexistence".into(),
                },
            ),
            (
                req(
                    DisplayTarget::Output(edp()),
                    Backend::LinuxWallpaperEngine,
                    dual_outputs(),
                    vec![RunningAssignment {
                        output: hdmi(),
                        backend: Backend::Awww,
                    }],
                ),
                RejectionReason::ReliesOnUnknownCoexistence {
                    explanation: "applying linux-wallpaperengine to eDP-1 while awww runs on HDMI-1 relies on unknown cross-output coexistence".into(),
                },
            ),
            (
                req(
                    DisplayTarget::AllDisplays,
                    Backend::Mpvpaper,
                    dual_outputs(),
                    vec![RunningAssignment {
                        output: edp(),
                        backend: Backend::Awww,
                    }],
                ),
                RejectionReason::ReliesOnUnknownCoexistence {
                    explanation: "mpvpaper All Displays needs one process per output, but multi-instance coexistence is unverified".into(),
                },
            ),
        ];

        for (request, expected) in cases {
            let err = plan_display_apply(&request).expect_err("must reject");
            assert_eq!(err, expected, "request={request:?}");
        }
    }

    #[test]
    fn capability_seam_exports_remain_usable() {
        let _ = (
            OutputTargetMode::SingleOutputPerProcess,
            StopScope::TrackedProcessPerOutput,
            StopScope::TrackedProcessShared,
            SameTargetReplacement::StopThenApply,
            CrossOutputCoexistence::Verified,
            apply_output_groups,
        );
    }
}
