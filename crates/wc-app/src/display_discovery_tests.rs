use std::cell::RefCell;

use crate::display_discovery::{discover_connected_outputs_with, ProbeOutput};

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

    let outputs = discover_connected_outputs_with(
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

    let outputs = discover_connected_outputs_with(
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
        let err = discover_connected_outputs_with(
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

    let outputs = discover_connected_outputs_with(
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

    let outputs = discover_connected_outputs_with(env(&[]), |program, args| {
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

    let err = discover_connected_outputs_with(
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
        let outputs = discover_connected_outputs_with(env(&vars), |program, args| {
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

    let outputs = discover_connected_outputs_with(
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
