use wc_backend::runtime_observation::{
    observe_runtime_wallpapers_with, ProcessCommandLine, RuntimeObservationIo,
    RuntimeObservationStatus,
};
use wc_storage::sqlite::{DisplayStateRow, DisplayStateTarget};

struct FakeIo {
    awww: Result<String, String>,
    processes: Result<Vec<ProcessCommandLine>, String>,
}

impl RuntimeObservationIo for FakeIo {
    fn awww_query_json(&self) -> Result<String, String> {
        self.awww.clone()
    }

    fn current_user_process_command_lines(&self) -> Result<Vec<ProcessCommandLine>, String> {
        self.processes.clone()
    }
}

fn saved(output: &str, path: &str, backend: &str) -> DisplayStateRow {
    DisplayStateRow {
        target: DisplayStateTarget::Output(output.into()),
        wallpaper_path: path.into(),
        backend: backend.into(),
        updated_at: "2026-07-14T00:00:00Z".into(),
    }
}

fn saved_all(path: &str, backend: &str) -> DisplayStateRow {
    DisplayStateRow {
        target: DisplayStateTarget::AllDisplays,
        wallpaper_path: path.into(),
        backend: backend.into(),
        updated_at: "2026-07-14T00:00:00Z".into(),
    }
}

#[test]
fn awww_json_confirms_only_the_exact_saved_path_for_a_connected_output() {
    let io = FakeIo {
        awww: Ok(r##"{
          "awww-daemon": [{
            "name": "eDP-1",
            "width": 1920,
            "height": 1080,
            "scale": 1,
            "displaying": {"image": "/walls/current.jpg"}
          }]
        }"##
        .into()),
        processes: Ok(vec![]),
    };

    let observed = observe_runtime_wallpapers_with(
        &["eDP-1".into()],
        &[saved("eDP-1", "/walls/current.jpg", "awww")],
        &io,
    );

    assert_eq!(observed.len(), 1);
    assert_eq!(observed[0].output, "eDP-1");
    assert_eq!(observed[0].status, RuntimeObservationStatus::Confirmed);
    assert_eq!(
        observed[0].wallpaper_path.as_deref(),
        Some("/walls/current.jpg")
    );
    assert_eq!(observed[0].reason, None);
}

#[test]
fn awww_color_on_one_output_does_not_hide_an_image_on_another() {
    let io = FakeIo {
        awww: Ok(r##"{
          "awww-daemon": [{
            "name": "eDP-1",
            "displaying": {"color": "#112233"}
          }, {
            "name": "HDMI-A-1",
            "displaying": {"image": "/walls/current.jpg"}
          }]
        }"##
        .into()),
        processes: Ok(vec![]),
    };

    let observed = observe_runtime_wallpapers_with(
        &["eDP-1".into(), "HDMI-A-1".into()],
        &[
            saved("eDP-1", "/walls/saved.jpg", "awww"),
            saved("HDMI-A-1", "/walls/current.jpg", "awww"),
        ],
        &io,
    );

    assert_eq!(observed[0].status, RuntimeObservationStatus::Unknown);
    assert_eq!(observed[1].status, RuntimeObservationStatus::Confirmed);
    assert_eq!(
        observed[1].wallpaper_path.as_deref(),
        Some("/walls/current.jpg")
    );
}

#[test]
fn awww_command_failure_keeps_the_saved_assignment_unknown() {
    let io = FakeIo {
        awww: Err("socket missing".into()),
        processes: Ok(vec![]),
    };

    let observed = observe_runtime_wallpapers_with(
        &["eDP-1".into()],
        &[saved("eDP-1", "/walls/saved.jpg", "awww")],
        &io,
    );

    assert_eq!(observed[0].status, RuntimeObservationStatus::Unknown);
    assert_eq!(observed[0].wallpaper_path, None);
    assert!(observed[0]
        .reason
        .as_deref()
        .is_some_and(|reason| reason.contains("socket missing")));
}

#[test]
fn mpvpaper_command_line_confirms_the_exact_output_and_path() {
    let io = FakeIo {
        awww: Ok(r#"{"awww-daemon": []}"#.into()),
        processes: Ok(vec![ProcessCommandLine {
            pid: 4242,
            argv: vec![
                "/usr/bin/mpvpaper".into(),
                "--fork".into(),
                "-o".into(),
                "--loop-file=inf --panscan=1.0".into(),
                "HDMI-A-1".into(),
                "--".into(),
                "/walls/animated night.mp4".into(),
            ],
        }]),
    };

    let observed = observe_runtime_wallpapers_with(
        &["HDMI-A-1".into()],
        &[saved("HDMI-A-1", "/walls/animated night.mp4", "mpvpaper")],
        &io,
    );

    assert_eq!(observed[0].status, RuntimeObservationStatus::Confirmed);
    assert_eq!(
        observed[0].wallpaper_path.as_deref(),
        Some("/walls/animated night.mp4")
    );
}

#[test]
fn mpvpaper_all_selector_claims_each_connected_output() {
    let io = FakeIo {
        awww: Ok(r#"{"awww-daemon": []}"#.into()),
        processes: Ok(vec![ProcessCommandLine {
            pid: 4242,
            argv: vec![
                "/usr/bin/mpvpaper".into(),
                "ALL".into(),
                "--".into(),
                "/walls/shared.mp4".into(),
            ],
        }]),
    };

    let observed = observe_runtime_wallpapers_with(
        &["eDP-1".into(), "HDMI-A-1".into()],
        &[saved_all("/walls/shared.mp4", "mpvpaper")],
        &io,
    );

    assert!(observed
        .iter()
        .all(|entry| entry.status == RuntimeObservationStatus::Confirmed));
}

#[test]
fn mpvpaper_space_separated_selector_claims_each_named_output() {
    let io = FakeIo {
        awww: Ok(r#"{"awww-daemon": []}"#.into()),
        processes: Ok(vec![ProcessCommandLine {
            pid: 4242,
            argv: vec![
                "/usr/bin/mpvpaper".into(),
                "eDP-1 HDMI-A-1".into(),
                "--".into(),
                "/walls/shared.mp4".into(),
            ],
        }]),
    };

    let observed = observe_runtime_wallpapers_with(
        &["eDP-1".into(), "HDMI-A-1".into()],
        &[saved_all("/walls/shared.mp4", "mpvpaper")],
        &io,
    );

    assert!(observed
        .iter()
        .all(|entry| entry.status == RuntimeObservationStatus::Confirmed));
}

#[test]
fn failed_awww_query_with_a_running_daemon_makes_mpvpaper_ownership_unknown() {
    let io = FakeIo {
        awww: Err("custom namespace could not be queried".into()),
        processes: Ok(vec![
            ProcessCommandLine {
                pid: 4000,
                argv: vec![
                    "/usr/bin/awww-daemon".into(),
                    "--namespace".into(),
                    "side".into(),
                ],
            },
            ProcessCommandLine {
                pid: 4242,
                argv: vec![
                    "/usr/bin/mpvpaper".into(),
                    "eDP-1".into(),
                    "--".into(),
                    "/walls/movie.mp4".into(),
                ],
            },
        ]),
    };

    let observed = observe_runtime_wallpapers_with(
        &["eDP-1".into()],
        &[saved("eDP-1", "/walls/movie.mp4", "mpvpaper")],
        &io,
    );

    assert_eq!(observed[0].status, RuntimeObservationStatus::Unknown);
    assert!(observed[0]
        .reason
        .as_deref()
        .is_some_and(|reason| reason.contains("awww")));
}

#[test]
fn unavailable_awww_without_a_daemon_does_not_hide_exact_mpvpaper_evidence() {
    let io = FakeIo {
        awww: Err("awww is not installed".into()),
        processes: Ok(vec![ProcessCommandLine {
            pid: 4242,
            argv: vec![
                "/usr/bin/mpvpaper".into(),
                "eDP-1".into(),
                "--".into(),
                "/walls/movie.mp4".into(),
            ],
        }]),
    };

    let observed = observe_runtime_wallpapers_with(
        &["eDP-1".into()],
        &[saved("eDP-1", "/walls/movie.mp4", "mpvpaper")],
        &io,
    );

    assert_eq!(observed[0].status, RuntimeObservationStatus::Confirmed);
}

#[test]
fn lwe_screen_root_and_workshop_id_confirm_the_saved_scene_path() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp
        .path()
        .join("steamapps/workshop/content/431960/3558034522");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("scene.pkg"), b"scene").unwrap();
    std::fs::write(
        project.join("project.json"),
        r#"{"type":"scene","file":"scene.pkg","workshopid":"3558034522"}"#,
    )
    .unwrap();
    let project = project.to_string_lossy().to_string();
    let io = FakeIo {
        awww: Ok(r#"{"awww-daemon": []}"#.into()),
        processes: Ok(vec![ProcessCommandLine {
            pid: 5151,
            argv: vec![
                "/usr/bin/linux-wallpaperengine".into(),
                "--screen-root".into(),
                "eDP-1".into(),
                "--bg".into(),
                "3558034522".into(),
                "--fps".into(),
                "60".into(),
            ],
        }]),
    };

    let observed = observe_runtime_wallpapers_with(
        &["eDP-1".into()],
        &[saved("eDP-1", &project, "linux-wallpaperengine")],
        &io,
    );

    assert_eq!(observed[0].status, RuntimeObservationStatus::Confirmed);
    assert_eq!(
        observed[0].wallpaper_path.as_deref(),
        Some(project.as_str())
    );
}

#[test]
fn lwe_screen_span_confirms_the_scene_on_each_spanned_output() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp
        .path()
        .join("steamapps/workshop/content/431960/3558034522");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("scene.pkg"), b"scene").unwrap();
    std::fs::write(
        project.join("project.json"),
        r#"{"type":"scene","file":"scene.pkg","workshopid":"3558034522"}"#,
    )
    .unwrap();
    let project = project.to_string_lossy().to_string();
    let io = FakeIo {
        awww: Ok(r#"{"awww-daemon": []}"#.into()),
        processes: Ok(vec![ProcessCommandLine {
            pid: 5151,
            argv: vec![
                "/usr/bin/linux-wallpaperengine".into(),
                "--screen-span".into(),
                "eDP-1,HDMI-A-1".into(),
                "--bg".into(),
                "3558034522".into(),
                "--fps".into(),
                "60".into(),
            ],
        }]),
    };

    let observed = observe_runtime_wallpapers_with(
        &["eDP-1".into(), "HDMI-A-1".into()],
        &[saved_all(&project, "linux-wallpaperengine")],
        &io,
    );

    assert!(observed
        .iter()
        .all(|entry| entry.status == RuntimeObservationStatus::Confirmed));
}

#[test]
fn conflicting_renderer_ownership_on_one_output_is_unknown() {
    let io = FakeIo {
        awww: Ok(r#"{
          "awww-daemon": [{
            "name": "eDP-1",
            "width": 1920,
            "height": 1080,
            "scale": 1,
            "displaying": {"image": "/walls/saved.jpg"}
          }]
        }"#
        .into()),
        processes: Ok(vec![ProcessCommandLine {
            pid: 6161,
            argv: vec![
                "mpvpaper".into(),
                "--fork".into(),
                "eDP-1".into(),
                "--".into(),
                "/walls/other.mp4".into(),
            ],
        }]),
    };

    let observed = observe_runtime_wallpapers_with(
        &["eDP-1".into()],
        &[saved("eDP-1", "/walls/saved.jpg", "awww")],
        &io,
    );

    assert_eq!(observed[0].status, RuntimeObservationStatus::Unknown);
    assert_eq!(observed[0].wallpaper_path, None);
    assert!(observed[0]
        .reason
        .as_deref()
        .is_some_and(|reason| reason.to_ascii_lowercase().contains("conflict")));
}

#[test]
fn process_inspection_failure_prevents_awww_from_being_reported_as_certain() {
    let io = FakeIo {
        awww: Ok(r#"{
          "awww-daemon": [{
            "name": "eDP-1",
            "width": 1920,
            "height": 1080,
            "scale": 1,
            "displaying": {"image": "/walls/saved.jpg"}
          }]
        }"#
        .into()),
        processes: Err("/proc is unavailable".into()),
    };

    let observed = observe_runtime_wallpapers_with(
        &["eDP-1".into()],
        &[saved("eDP-1", "/walls/saved.jpg", "awww")],
        &io,
    );

    assert_eq!(observed[0].status, RuntimeObservationStatus::Unknown);
    assert!(observed[0]
        .reason
        .as_deref()
        .is_some_and(|reason| reason.contains("/proc is unavailable")));
}

#[test]
fn ambiguous_awww_json_prevents_selecting_a_different_renderer_candidate() {
    let io = FakeIo {
        awww: Ok(r#"{
          "namespace-one": [{
            "name": "eDP-1",
            "width": 1920,
            "height": 1080,
            "scale": 1,
            "displaying": {"image": "/walls/first.jpg"}
          }],
          "namespace-two": [{
            "name": "eDP-1",
            "width": 1920,
            "height": 1080,
            "scale": 1,
            "displaying": {"image": "/walls/second.jpg"}
          }]
        }"#
        .into()),
        processes: Ok(vec![ProcessCommandLine {
            pid: 7171,
            argv: vec![
                "mpvpaper".into(),
                "--fork".into(),
                "eDP-1".into(),
                "--".into(),
                "/walls/saved.mp4".into(),
            ],
        }]),
    };

    let observed = observe_runtime_wallpapers_with(
        &["eDP-1".into()],
        &[saved("eDP-1", "/walls/saved.mp4", "mpvpaper")],
        &io,
    );

    assert_eq!(observed[0].status, RuntimeObservationStatus::Unknown);
    assert!(observed[0]
        .reason
        .as_deref()
        .is_some_and(|reason| reason.to_ascii_lowercase().contains("awww")));
}

#[test]
fn persistence_without_matching_renderer_evidence_is_never_confirmed() {
    let io = FakeIo {
        awww: Ok(r#"{"awww-daemon": []}"#.into()),
        processes: Ok(vec![]),
    };

    let observed = observe_runtime_wallpapers_with(
        &["eDP-1".into(), "HDMI-A-1".into()],
        &[
            saved("eDP-1", "/walls/saved.jpg", "awww"),
            saved("HDMI-A-1", "/walls/saved.mp4", "mpvpaper"),
        ],
        &io,
    );

    assert!(observed
        .iter()
        .all(|entry| entry.status == RuntimeObservationStatus::Unknown));
    assert!(observed.iter().all(|entry| entry.wallpaper_path.is_none()));
}

#[test]
fn duplicate_mpvpaper_processes_for_one_output_are_unknown() {
    let process = |pid, path: &str| ProcessCommandLine {
        pid,
        argv: vec![
            "mpvpaper".into(),
            "--fork".into(),
            "eDP-1".into(),
            "--".into(),
            path.into(),
        ],
    };
    let io = FakeIo {
        awww: Ok(r#"{"awww-daemon": []}"#.into()),
        processes: Ok(vec![
            process(8001, "/walls/saved.mp4"),
            process(8002, "/walls/saved.mp4"),
        ]),
    };

    let observed = observe_runtime_wallpapers_with(
        &["eDP-1".into()],
        &[saved("eDP-1", "/walls/saved.mp4", "mpvpaper")],
        &io,
    );

    assert_eq!(observed[0].status, RuntimeObservationStatus::Unknown);
    assert!(observed[0]
        .reason
        .as_deref()
        .is_some_and(|reason| reason.contains("Multiple mpvpaper")));
}

#[test]
fn all_displays_expectation_is_expanded_and_named_rows_override_it() {
    let io = FakeIo {
        awww: Ok(r#"{
          "awww-daemon": [{
            "name": "eDP-1",
            "width": 1920,
            "height": 1080,
            "scale": 1,
            "displaying": {"image": "/walls/shared.jpg"}
          }]
        }"#
        .into()),
        processes: Ok(vec![ProcessCommandLine {
            pid: 9001,
            argv: vec![
                "mpvpaper".into(),
                "--fork".into(),
                "HDMI-A-1".into(),
                "--".into(),
                "/walls/monitor.mp4".into(),
            ],
        }]),
    };

    let observed = observe_runtime_wallpapers_with(
        &["eDP-1".into(), "HDMI-A-1".into()],
        &[
            saved_all("/walls/shared.jpg", "awww"),
            saved("HDMI-A-1", "/walls/monitor.mp4", "mpvpaper"),
            saved("DP-9", "/walls/disconnected.jpg", "awww"),
        ],
        &io,
    );

    assert_eq!(
        observed
            .iter()
            .map(|entry| (entry.output.as_str(), entry.wallpaper_path.as_deref()))
            .collect::<Vec<_>>(),
        vec![
            ("eDP-1", Some("/walls/shared.jpg")),
            ("HDMI-A-1", Some("/walls/monitor.mp4")),
        ]
    );
    assert!(observed
        .iter()
        .all(|entry| entry.status == RuntimeObservationStatus::Confirmed));
}

#[test]
fn unsupported_saved_backend_and_disconnected_rows_stay_unknown_or_absent() {
    let io = FakeIo {
        awww: Ok(r#"{"awww-daemon": []}"#.into()),
        processes: Ok(vec![]),
    };

    let observed = observe_runtime_wallpapers_with(
        &["eDP-1".into()],
        &[
            saved("eDP-1", "/walls/unsupported", "future-renderer"),
            saved("DP-9", "/walls/disconnected.jpg", "awww"),
        ],
        &io,
    );

    assert_eq!(observed.len(), 1);
    assert_eq!(observed[0].output, "eDP-1");
    assert_eq!(observed[0].status, RuntimeObservationStatus::Unknown);
}
