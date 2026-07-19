use std::cell::RefCell;
use std::time::Duration;

use crate::command_probe::ProbeOutput;
use crate::display_discovery::{
    discover_connected_outputs_with_mock, discover_with_script_probes_traced,
};

fn env<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
    move |name| {
        vars.iter()
            .find_map(|(key, value)| (*key == name).then(|| (*value).to_string()))
    }
}

fn success(stdout: &str) -> ProbeOutput {
    ProbeOutput {
        success: true,
        stdout: stdout.into(),
        stderr: String::new(),
    }
}

fn failure(stderr: &str) -> ProbeOutput {
    ProbeOutput {
        success: false,
        stdout: String::new(),
        stderr: stderr.into(),
    }
}

#[test]
fn niri_session_uses_niri_and_never_calls_awww_after_success() {
    let calls: RefCell<Vec<(String, Vec<String>)>> = RefCell::new(Vec::new());

    let outputs = discover_connected_outputs_with_mock(
        env(&[("NIRI_SOCKET", "/run/user/1000/niri.sock")]),
        |program, args| {
            calls.borrow_mut().push((
                program.to_string(),
                args.iter()
                    .map(|arg| (*arg).to_string())
                    .collect::<Vec<_>>(),
            ));
            assert_eq!(program, "niri", "successful niri must be the only probe");
            assert_eq!(args, ["msg", "-j", "outputs"]);
            Ok(success(
                r#"{"eDP-1":{"name":"eDP-1"},"HDMI-A-1":{"name":"HDMI-A-1"}}"#,
            ))
        },
    )
    .unwrap();

    assert_eq!(outputs, ["HDMI-A-1", "eDP-1"]);
    assert_eq!(calls.borrow().len(), 1);
}

#[test]
fn successful_empty_niri_result_is_authoritative_and_does_not_fallback() {
    let calls: RefCell<Vec<(String, Vec<String>)>> = RefCell::new(Vec::new());

    let outputs = discover_connected_outputs_with_mock(
        env(&[("XDG_CURRENT_DESKTOP", "niri")]),
        |program, args| {
            calls.borrow_mut().push((
                program.to_string(),
                args.iter()
                    .map(|arg| (*arg).to_string())
                    .collect::<Vec<_>>(),
            ));
            assert_eq!(program, "niri");
            Ok(success("{}"))
        },
    )
    .unwrap();

    assert!(outputs.is_empty());
    assert_eq!(calls.borrow().len(), 1);
}

#[test]
fn niri_blank_or_duplicate_names_fail_without_inventing_outputs() {
    for (json, expected) in [
        (r#"{"bad":{"name":"  "}}"#, "blank"),
        (
            r#"{"first":{"name":"eDP-1"},"second":{"name":"eDP-1"}}"#,
            "duplicate",
        ),
    ] {
        let err = discover_connected_outputs_with_mock(
            env(&[("NIRI_SOCKET", "/run/user/1000/niri.sock")]),
            |program, _args| match program {
                "niri" => Ok(success(json)),
                "awww" => Ok(failure("awww daemon unavailable")),
                other => panic!("unexpected probe: {other}"),
            },
        )
        .unwrap_err();

        assert_eq!(err.code, "display_discovery_failed");
        let detail = err.detail.unwrap_or_default();
        assert!(detail.contains(expected), "{detail}");
        assert!(detail.contains("niri"), "{detail}");
        assert!(detail.contains("awww"), "{detail}");
    }
}

#[test]
fn failed_primary_probe_falls_back_to_awww() {
    let calls: RefCell<Vec<(String, Vec<String>)>> = RefCell::new(Vec::new());

    let outputs = discover_connected_outputs_with_mock(
        env(&[("NIRI_SOCKET", "/run/user/1000/niri.sock")]),
        |program, args| {
            calls.borrow_mut().push((
                program.to_string(),
                args.iter()
                    .map(|arg| (*arg).to_string())
                    .collect::<Vec<_>>(),
            ));
            match program {
                "niri" => Ok(failure("niri socket unavailable")),
                "awww" => Ok(success(r#"{"legacy":[{"name":"eDP-1"}]}"#)),
                other => panic!("unexpected probe: {other}"),
            }
        },
    )
    .unwrap();

    assert_eq!(outputs, ["eDP-1"]);
    assert_eq!(
        calls.borrow().as_slice(),
        [
            (
                "niri".into(),
                vec!["msg".into(), "-j".into(), "outputs".into()],
            ),
            ("awww".into(), vec!["query".into(), "--json".into()]),
        ]
    );
}

#[test]
fn failed_awww_json_probe_falls_back_to_valid_plain_text() {
    let calls: RefCell<Vec<(String, Vec<String>)>> = RefCell::new(Vec::new());

    let outputs = discover_connected_outputs_with_mock(env(&[]), |program, args| {
        calls.borrow_mut().push((
            program.to_string(),
            args.iter()
                .map(|arg| (*arg).to_string())
                .collect::<Vec<_>>(),
        ));
        match (program, args) {
            ("awww", ["query", "--json"]) => Ok(failure("json unavailable")),
            ("awww", ["query"]) => Ok(success(
                ": eDP-1: 1920x1080, scale: 1, currently displaying: color: 000000\n",
            )),
            _ => panic!("unexpected probe: {program} {args:?}"),
        }
    })
    .unwrap();

    assert_eq!(outputs, ["eDP-1"]);
    assert_eq!(calls.borrow().len(), 2);
}

#[test]
fn all_probe_failures_return_structured_details_and_no_outputs() {
    let calls: RefCell<Vec<(String, Vec<String>)>> = RefCell::new(Vec::new());

    let err = discover_connected_outputs_with_mock(
        env(&[("NIRI_SOCKET", "/run/user/1000/niri.sock")]),
        |program, args| {
            calls.borrow_mut().push((
                program.to_string(),
                args.iter()
                    .map(|arg| (*arg).to_string())
                    .collect::<Vec<_>>(),
            ));
            match (program, args) {
                ("niri", _) => Err("niri executable unavailable".into()),
                ("awww", ["query", "--json"]) => Ok(failure("awww json socket failure")),
                ("awww", ["query"]) => Ok(failure("awww plain socket failure")),
                _ => panic!("unexpected probe: {program} {args:?}"),
            }
        },
    )
    .unwrap_err();

    assert_eq!(err.code, "display_discovery_failed");
    assert!(err.recoverable);
    let detail = err.detail.unwrap_or_default();
    assert!(detail.contains("niri executable unavailable"), "{detail}");
    assert!(detail.contains("awww plain socket failure"), "{detail}");
    assert_eq!(calls.borrow().len(), 3);
}

#[test]
fn explicit_session_markers_select_supported_compositor_probes() {
    let cases = [
        (
            vec![("HYPRLAND_INSTANCE_SIGNATURE", "instance")],
            "hyprctl",
            vec!["-j", "monitors"],
            r#"[{"name":"DP-1"},{"name":"eDP-1"}]"#,
            vec!["DP-1", "eDP-1"],
        ),
        (
            vec![("SWAYSOCK", "/run/user/1000/sway.sock")],
            "swaymsg",
            vec!["-t", "get_outputs", "-r"],
            r#"[{"name":"eDP-1","active":true},{"name":"DP-2","active":false}]"#,
            vec!["eDP-1"],
        ),
        (
            vec![("XDG_SESSION_TYPE", "x11")],
            "xrandr",
            vec!["--query"],
            "eDP-1 connected primary 1920x1080+0+0\nHDMI-1 disconnected\n",
            vec!["eDP-1"],
        ),
    ];

    for (vars, expected_program, expected_args, stdout, expected_outputs) in cases {
        let outputs = discover_connected_outputs_with_mock(env(&vars), |program, args| {
            assert_eq!(program, expected_program);
            assert_eq!(args, expected_args);
            Ok(success(stdout))
        })
        .unwrap();
        assert_eq!(outputs, expected_outputs);
    }
}

#[test]
fn unrelated_desktop_does_not_blindly_probe_installed_compositor_clis() {
    let calls: RefCell<Vec<(String, Vec<String>)>> = RefCell::new(Vec::new());

    let outputs = discover_connected_outputs_with_mock(
        env(&[("XDG_CURRENT_DESKTOP", "GNOME")]),
        |program, args| {
            calls.borrow_mut().push((
                program.to_string(),
                args.iter()
                    .map(|arg| (*arg).to_string())
                    .collect::<Vec<_>>(),
            ));
            assert_eq!(program, "awww");
            Ok(success(r#"{"legacy":[{"name":"eDP-1"}]}"#))
        },
    )
    .unwrap();

    assert_eq!(outputs, ["eDP-1"]);
    assert_eq!(
        calls.borrow().as_slice(),
        [("awww".into(), vec!["query".into(), "--json".into()])]
    );
}

#[test]
fn deadline_bounded_primary_times_out_without_hanging() {
    // Integration-style: the production discover_connected_outputs_with
    // uses real process deadlines. Test with a mock that simulates timeout.
    use std::time::Instant;

    // A hanging niri probe should time out and fall back to awww.
    let start = Instant::now();
    let result = discover_connected_outputs_with_mock(
        env(&[("NIRI_SOCKET", "/run/user/1000/niri.sock")]),
        |program, _args| match program {
            "niri" => Err("niri: timed out after 1.5s".into()),
            "awww" => Ok(success(r#"{"legacy":[{"name":"eDP-1"}]}"#)),
            other => panic!("unexpected probe: {other}"),
        },
    );

    assert!(
        start.elapsed() < Duration::from_secs(1),
        "timeout simulation must return quickly"
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), vec!["eDP-1"]);
}

#[test]
fn real_budget_primary_timeout_fallback_shares_remaining() {
    // Primary niri probe sleeps 2s → times out at ~1.5s.
    // Fallback awww json sleeps 1.2s then returns valid output.
    // The json probe uses the remaining budget (~1.5s); text is never
    // reached. Total elapsed must be ≤ 3.6s (OVERALL_DISPLAY_DEADLINE +
    // 2*DRAINER_JOIN_GRACE + CI tolerance).
    //
    // Wrapped in a channel recv_timeout so the test runner itself cannot
    // hang if the budget logic regresses.
    use std::sync::mpsc;
    use std::time::Instant;

    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let start = Instant::now();
        let mut budgets = Vec::new();
        let result = discover_with_script_probes_traced(
            env(&[("NIRI_SOCKET", "/run/user/1000/niri.sock")]),
            "sleep 2",                                               // primary hangs
            "sleep 1.2; echo '{\"legacy\":[{\"name\":\"eDP-1\"}]}'", // fallback json
            "echo unused",                                           // text (not reached)
            &mut budgets,
        );
        let _ = tx.send((start.elapsed(), result, budgets));
    });

    let (elapsed, result, budgets) = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("budget test hung — deadline logic regressed");

    // ── budget assertions ──────────────────────────────────────────

    // Primary budget must be ≤ PRIMARY_PROBE_DEADLINE (1500ms).
    let primary = budgets
        .iter()
        .find(|b| b.label == "niri")
        .expect("primary niri probe must be recorded");
    assert!(
        primary.budget <= Duration::from_millis(1500),
        "primary budget {:.0?} must be <= 1500ms",
        primary.budget
    );

    // awww json fallback budget must be ≤ overall remaining.
    let json = budgets
        .iter()
        .find(|b| b.label == "awww json")
        .expect("awww json fallback must be recorded");
    assert!(
        json.budget <= json.remaining_at_call,
        "json fallback budget {:.0?} must be <= remaining at call {:.0?}",
        json.budget,
        json.remaining_at_call
    );
    assert!(
        json.budget <= Duration::from_secs(3),
        "json fallback budget {:.0?} must be <= 3s overall deadline",
        json.budget
    );

    // awww text must NOT have been spawned (json succeeded).
    assert!(
        !budgets.iter().any(|b| b.label == "awww text"),
        "text fallback must not be spawned when json succeeds"
    );

    // ── elapsed assertions ─────────────────────────────────────────

    assert!(
        elapsed >= Duration::from_millis(2500),
        "expected >= 2.5s (1.5s primary timeout + 1.2s json), took {elapsed:?}"
    );
    assert!(
        elapsed <= Duration::from_millis(3600),
        "expected <= 3.6s (3s overall + 2*200ms drainer grace + CI), took {elapsed:?}"
    );
    assert!(result.is_ok(), "expected Ok, got {result:?}");
    assert_eq!(result.unwrap(), vec!["eDP-1"]);
}

#[test]
fn fast_primary_failure_hung_fallback_budget_exhausted_no_further_spawn() {
    // Primary fails immediately (exit 1).
    // Fallback json hangs (sleep 10) → times out after ~3s.
    // After json timeout the overall budget is exhausted — text must NOT
    // be spawned. Total elapsed ≤ 3.6s.
    use std::sync::mpsc;
    use std::time::Instant;

    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let start = Instant::now();
        let mut budgets = Vec::new();
        let result = discover_with_script_probes_traced(
            env(&[("NIRI_SOCKET", "/run/user/1000/niri.sock")]),
            "exit 1",              // primary fails immediately
            "sleep 10",            // fallback json hangs
            "echo should_not_run", // text must not be spawned
            &mut budgets,
        );
        let _ = tx.send((start.elapsed(), result, budgets));
    });

    let (elapsed, result, budgets) = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("budget test hung — deadline logic regressed");

    // ── budget assertions ──────────────────────────────────────────

    // Primary budget must be ≤ 1500ms.
    let primary = budgets
        .iter()
        .find(|b| b.label == "niri")
        .expect("primary niri probe must be recorded");
    assert!(
        primary.budget <= Duration::from_millis(1500),
        "primary budget {:.0?} must be <= 1500ms",
        primary.budget
    );

    // awww json fallback budget must be ≤ overall remaining at call time.
    let json = budgets
        .iter()
        .find(|b| b.label == "awww json")
        .expect("awww json fallback must be recorded");
    assert!(
        json.budget <= json.remaining_at_call,
        "json fallback budget {:.0?} must be <= remaining at call {:.0?}",
        json.budget,
        json.remaining_at_call
    );

    // awww text must NOT be in budgets — budget was exhausted before spawn.
    assert!(
        !budgets.iter().any(|b| b.label == "awww text"),
        "text fallback must not be spawned (budget exhausted), budgets: {budgets:?}"
    );

    // ── elapsed assertions ─────────────────────────────────────────

    assert!(
        elapsed <= Duration::from_millis(3600),
        "budget exhausted => total must stay under 3.6s, took {elapsed:?}"
    );

    // Result must be an error (both json and text failed/were skipped).
    let err = result.unwrap_err();
    assert_eq!(err.code, "display_discovery_failed");
    let detail = err.detail.unwrap_or_default();

    // Primary failure must be recorded.
    assert!(
        detail.contains("niri"),
        "must include primary failure, got: {detail}"
    );
    // Json timeout must be recorded.
    assert!(
        detail.contains("awww json"),
        "must include json fallback failure, got: {detail}"
    );
    // Text probe must NOT appear — budget was exhausted before it.
    assert!(
        !detail.contains("awww text"),
        "text probe must not be spawned (budget exhausted), got: {detail}"
    );
}
