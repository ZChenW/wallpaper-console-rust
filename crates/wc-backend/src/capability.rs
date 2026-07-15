//! Typed backend capability declarations.
//!
//! Facts come from installed CLI help/behavior and from current stop/apply
//! implementation limits. Automated tests must not launch desktop renderers.
//! Cross-output runtime coexistence remains unverified on this host (only
//! eDP-1 connected), so the model marks coexistence as unknown until proven.

use wc_core::types::Backend;

/// How a capability fact was established.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Evidence {
    /// Observed from installed CLI help or documented CLI behavior.
    CliVerified,
    /// Derived from wallpaper-console's current command/stop paths.
    ImplementationLimit,
    /// Plausible from CLI shape but not proven on a multi-output runtime.
    Unverified,
    /// Must not be treated as safe by planners.
    Unknown,
}

/// How a backend addresses Wayland/X outputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputTargetMode {
    /// Named outputs via an explicit selector (awww `--outputs`).
    NamedOutputs,
    /// Exactly one output name per process invocation (mpvpaper).
    SingleOutputPerProcess,
    /// Repeated `--screen-root` / `--bg` pairs in one process (LWE).
    RepeatedScreenRootPairs,
}

/// How "All Displays" maps onto the backend CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllDisplaysTargeting {
    /// Omitting the output selector targets every connected output.
    OmitMeansAll,
    /// Requires one process (or invocation) per output.
    OneProcessPerOutput,
    /// One process can cover multiple outputs via repeated pairs.
    SingleProcessMultiOutput,
}

/// Scope of the backend's stop/cleanup path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopScope {
    /// Stopping the shared daemon clears wallpaper for all outputs.
    DaemonWide,
    /// Current stop kills every process matching the backend binary name.
    AllMatchingProcesses,
    /// Tracked PID/PGID owns exactly one output; stop is output-scoped.
    ///
    /// Being tracked alone does **not** imply this variant — shared ownership
    /// must use [`StopScope::TrackedProcessShared`]. Residual `pkill` must be
    /// modeled as [`StopScope::AllMatchingProcesses`].
    TrackedProcessPerOutput,
    /// Tracked PID/PGID may own multiple outputs; stopping it disturbs all of them.
    TrackedProcessShared,
}

/// How same-target replacement (same backend already on the target) is planned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SameTargetReplacement {
    /// Retarget within the existing daemon/process without an explicit Stop.
    InPlace,
    /// Backend manages process handoff as part of Apply (no planner Stop).
    ManagedHandoff,
    /// Must emit an explicit Stop before Apply to avoid duplicate processes.
    StopThenApply,
}

/// Whether multiple backend instances can own different outputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MultiInstanceSupport {
    /// One shared daemon owns all outputs (awww-daemon).
    SharedDaemon,
    /// Separate per-output processes are CLI-shaped but coexistence unproven.
    SeparateProcessesUnverified,
    /// Separate per-output processes verified safe at runtime.
    SeparateProcessesVerified,
    /// One process may own one or more outputs; extra instances unproven.
    SingleProcessUnverified,
}

/// Cross-output / cross-backend runtime coexistence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrossOutputCoexistence {
    /// Not proven on a multi-output session; planners must not rely on it.
    Unknown,
    /// Runtime-verified safe coexistence across outputs.
    Verified,
}

/// Capability description for one supported wallpaper backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendCapability {
    pub backend: Backend,
    pub output_target_mode: OutputTargetMode,
    pub output_target_evidence: Evidence,
    pub all_displays: AllDisplaysTargeting,
    pub all_displays_evidence: Evidence,
    pub stop_scope: StopScope,
    pub stop_scope_evidence: Evidence,
    pub multi_instance: MultiInstanceSupport,
    pub multi_instance_evidence: Evidence,
    pub same_target_replacement: SameTargetReplacement,
    pub same_target_replacement_evidence: Evidence,
    pub cross_output_coexistence: CrossOutputCoexistence,
    pub cross_output_coexistence_evidence: Evidence,
}

impl BackendCapability {
    /// True when CLI-verified targeting can address a specific named output.
    pub fn verified_named_output_targeting(&self) -> bool {
        matches!(self.output_target_evidence, Evidence::CliVerified)
            && matches!(
                self.output_target_mode,
                OutputTargetMode::NamedOutputs
                    | OutputTargetMode::SingleOutputPerProcess
                    | OutputTargetMode::RepeatedScreenRootPairs
            )
    }

    /// True when stopping this backend can disturb outputs beyond an explicit target.
    ///
    /// [`StopScope::TrackedProcessPerOutput`] is output-scoped.
    /// [`StopScope::TrackedProcessShared`], daemon-wide, and all-matching stops
    /// may affect non-target outputs.
    pub fn stop_may_affect_non_target_outputs(&self) -> bool {
        matches!(
            self.stop_scope,
            StopScope::DaemonWide
                | StopScope::AllMatchingProcesses
                | StopScope::TrackedProcessShared
        )
    }

    /// True when same-target replacement must emit an explicit Stop before Apply.
    pub fn requires_stop_before_same_target_apply(&self) -> bool {
        matches!(
            self.same_target_replacement,
            SameTargetReplacement::StopThenApply
        )
    }

    /// True only when cross-output coexistence has been runtime-verified.
    pub fn cross_output_coexistence_verified(&self) -> bool {
        matches!(
            self.cross_output_coexistence_evidence,
            Evidence::CliVerified
        ) && matches!(
            self.cross_output_coexistence,
            CrossOutputCoexistence::Verified
        )
    }
}

/// Group outputs into one CLI invocation group per planned Apply.
///
/// [`AllDisplaysTargeting::OneProcessPerOutput`] always yields one group per
/// output so a future verified multi-instance mpvpaper path cannot emit an
/// invalid multi-output single invocation.
pub fn apply_output_groups(
    targeting: AllDisplaysTargeting,
    outputs: &[String],
) -> Vec<Vec<String>> {
    match targeting {
        AllDisplaysTargeting::OneProcessPerOutput => {
            outputs.iter().map(|output| vec![output.clone()]).collect()
        }
        AllDisplaysTargeting::OmitMeansAll | AllDisplaysTargeting::SingleProcessMultiOutput => {
            if outputs.is_empty() {
                Vec::new()
            } else {
                vec![outputs.to_vec()]
            }
        }
    }
}

/// Capability matrix entry for a backend, if the backend is supported.
pub fn capability_for(backend: Backend) -> Option<BackendCapability> {
    crate::driver::driver_for(backend).map(|driver| driver.capability())
}

/// All supported backend capability declarations.
pub fn all_capabilities() -> Vec<BackendCapability> {
    [
        Backend::Awww,
        Backend::Mpvpaper,
        Backend::LinuxWallpaperEngine,
    ]
    .into_iter()
    .filter_map(capability_for)
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_has_no_capability() {
        assert!(capability_for(Backend::Unsupported).is_none());
    }

    #[test]
    fn matrix_covers_three_supported_backends() {
        let caps = all_capabilities();
        assert_eq!(caps.len(), 3);
        assert!(caps.iter().any(|c| c.backend == Backend::Awww));
        assert!(caps.iter().any(|c| c.backend == Backend::Mpvpaper));
        assert!(caps
            .iter()
            .any(|c| c.backend == Backend::LinuxWallpaperEngine));
    }

    #[test]
    fn awww_cli_verified_named_outputs_and_omit_means_all() {
        let cap = capability_for(Backend::Awww).expect("awww");
        assert_eq!(cap.output_target_mode, OutputTargetMode::NamedOutputs);
        assert_eq!(cap.output_target_evidence, Evidence::CliVerified);
        assert_eq!(cap.all_displays, AllDisplaysTargeting::OmitMeansAll);
        assert_eq!(cap.all_displays_evidence, Evidence::CliVerified);
        assert_eq!(cap.multi_instance, MultiInstanceSupport::SharedDaemon);
        assert_eq!(cap.multi_instance_evidence, Evidence::CliVerified);
        assert_eq!(cap.stop_scope, StopScope::DaemonWide);
        assert_eq!(cap.stop_scope_evidence, Evidence::ImplementationLimit);
        assert_eq!(cap.same_target_replacement, SameTargetReplacement::InPlace);
        assert!(cap.verified_named_output_targeting());
        assert!(cap.stop_may_affect_non_target_outputs());
        assert!(!cap.cross_output_coexistence_verified());
    }

    #[test]
    fn mpvpaper_cli_verified_single_output_per_process() {
        let cap = capability_for(Backend::Mpvpaper).expect("mpvpaper");
        assert_eq!(
            cap.output_target_mode,
            OutputTargetMode::SingleOutputPerProcess
        );
        assert_eq!(cap.output_target_evidence, Evidence::CliVerified);
        assert_eq!(cap.all_displays, AllDisplaysTargeting::OneProcessPerOutput);
        assert_eq!(
            cap.multi_instance,
            MultiInstanceSupport::SeparateProcessesUnverified
        );
        assert_eq!(cap.multi_instance_evidence, Evidence::Unverified);
        assert_eq!(cap.stop_scope, StopScope::AllMatchingProcesses);
        assert_eq!(
            cap.same_target_replacement,
            SameTargetReplacement::StopThenApply
        );
        assert!(cap.requires_stop_before_same_target_apply());
        assert!(cap.stop_may_affect_non_target_outputs());
        assert_eq!(
            cap.cross_output_coexistence,
            CrossOutputCoexistence::Unknown
        );
        assert!(!cap.cross_output_coexistence_verified());
    }

    #[test]
    fn lwe_current_stop_scope_is_all_matching_because_of_residual_pkill() {
        let cap = capability_for(Backend::LinuxWallpaperEngine).expect("lwe");
        assert_eq!(
            cap.output_target_mode,
            OutputTargetMode::RepeatedScreenRootPairs
        );
        assert_eq!(cap.output_target_evidence, Evidence::CliVerified);
        assert_eq!(
            cap.all_displays,
            AllDisplaysTargeting::SingleProcessMultiOutput
        );
        assert_eq!(cap.stop_scope, StopScope::AllMatchingProcesses);
        assert_eq!(cap.stop_scope_evidence, Evidence::ImplementationLimit);
        assert_eq!(
            cap.same_target_replacement,
            SameTargetReplacement::ManagedHandoff
        );
        assert!(!cap.requires_stop_before_same_target_apply());
        assert!(cap.stop_may_affect_non_target_outputs());
        assert_eq!(
            cap.multi_instance,
            MultiInstanceSupport::SingleProcessUnverified
        );
        assert_eq!(cap.multi_instance_evidence, Evidence::Unverified);
        assert!(!cap.cross_output_coexistence_verified());
    }

    #[test]
    fn tracked_process_per_output_stop_is_not_automatically_global() {
        let mut cap = capability_for(Backend::LinuxWallpaperEngine).expect("lwe");
        cap.stop_scope = StopScope::TrackedProcessPerOutput;
        assert!(!cap.stop_may_affect_non_target_outputs());
    }

    #[test]
    fn tracked_process_shared_stop_may_affect_non_target_outputs() {
        let mut cap = capability_for(Backend::LinuxWallpaperEngine).expect("lwe");
        cap.stop_scope = StopScope::TrackedProcessShared;
        assert!(cap.stop_may_affect_non_target_outputs());
    }

    #[test]
    fn same_target_replacement_is_capability_driven() {
        let awww = capability_for(Backend::Awww).expect("awww");
        assert_eq!(awww.same_target_replacement, SameTargetReplacement::InPlace);

        let mpv = capability_for(Backend::Mpvpaper).expect("mpvpaper");
        assert_eq!(
            mpv.same_target_replacement,
            SameTargetReplacement::StopThenApply
        );

        let lwe = capability_for(Backend::LinuxWallpaperEngine).expect("lwe");
        assert_eq!(
            lwe.same_target_replacement,
            SameTargetReplacement::ManagedHandoff
        );
    }

    #[test]
    fn one_process_per_output_groups_are_one_output_each() {
        let groups = apply_output_groups(
            AllDisplaysTargeting::OneProcessPerOutput,
            &["eDP-1".into(), "HDMI-1".into()],
        );
        assert_eq!(
            groups,
            vec![vec!["eDP-1".to_string()], vec!["HDMI-1".to_string()]]
        );
    }

    #[test]
    fn omit_means_all_and_single_process_keep_one_grouped_invocation() {
        let outputs = vec!["eDP-1".into(), "HDMI-1".into()];
        assert_eq!(
            apply_output_groups(AllDisplaysTargeting::OmitMeansAll, &outputs),
            vec![outputs.clone()]
        );
        assert_eq!(
            apply_output_groups(AllDisplaysTargeting::SingleProcessMultiOutput, &outputs),
            vec![outputs]
        );
    }

    #[test]
    fn no_backend_claims_verified_cross_output_coexistence() {
        for cap in all_capabilities() {
            assert_eq!(
                cap.cross_output_coexistence,
                CrossOutputCoexistence::Unknown,
                "{:?} must not claim verified coexistence",
                cap.backend
            );
            assert_eq!(cap.cross_output_coexistence_evidence, Evidence::Unknown);
            assert!(!cap.cross_output_coexistence_verified());
        }
    }
}
