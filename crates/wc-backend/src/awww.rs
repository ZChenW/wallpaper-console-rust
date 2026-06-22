use std::process::{Command, Stdio};
use std::time::Duration;

pub(crate) fn stop_awww() {
    let pc = crate::process_control::RealProcessControl::new();
    stop_awww_daemon_with_wait(&pc, Duration::from_millis(50));
    clear_awww_cache();
}

pub(crate) fn stop_awww_daemon_with_wait(
    pc: &dyn crate::process_control::ProcessControl,
    sleep: Duration,
) {
    const AWWW_DAEMON_PATTERN: &str = r"(^|/)awww-daemon\b";
    const TERM_CHECKS: usize = 20;
    const KILL_CHECKS: usize = 5;

    for pid in pc.find_processes(AWWW_DAEMON_PATTERN) {
        pc.term_process(pid);
    }

    for _ in 0..TERM_CHECKS {
        let running = pc.find_processes(AWWW_DAEMON_PATTERN);
        if running.is_empty() {
            return;
        }
        std::thread::sleep(sleep);
    }

    for pid in pc.find_processes(AWWW_DAEMON_PATTERN) {
        pc.kill_process(pid);
    }

    for _ in 0..KILL_CHECKS {
        if pc.find_processes(AWWW_DAEMON_PATTERN).is_empty() {
            return;
        }
        std::thread::sleep(sleep);
    }
}

fn clear_awww_cache() {
    let _ = Command::new("awww")
        .arg("clear-cache")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

pub(crate) fn is_awww_daemon_running(user: &str) -> bool {
    if user.is_empty() {
        return false;
    }
    matches!(
        std::process::Command::new("pgrep")
            .arg("-u")
            .arg(user)
            .arg("-x")
            .arg("awww-daemon")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status(),
        Ok(s) if s.success()
    )
}

pub(crate) fn normalize_awww_resize(raw: &str) -> &'static str {
    match raw {
        "crop" => "crop",
        "fit" => "fit",
        "stretch" => "stretch",
        _ => "crop",
    }
}

pub(crate) fn normalize_awww_transition_type(raw: &str) -> &'static str {
    match raw.trim() {
        "simple" => "simple",
        "fade" => "fade",
        "left" => "left",
        "right" => "right",
        "top" => "top",
        "bottom" => "bottom",
        "wipe" => "wipe",
        "grow" => "grow",
        "center" => "center",
        "outer" => "outer",
        "random" => "random",
        "wave" => "wave",
        "slide" => "left",  // legacy GUI value
        "none" => "simple", // legacy instant value
        _ => "fade",
    }
}

pub(crate) fn build_awww_instant_command(path: &str, resize: &str, fps: &str) -> Command {
    let mut cmd = Command::new("awww");
    cmd.arg("img")
        .arg(path)
        .arg("--resize")
        .arg(resize)
        .arg("--transition-type")
        .arg("simple")
        .arg("--transition-duration")
        .arg("0")
        .arg("--transition-fps")
        .arg(fps)
        .arg("--filter")
        .arg("Lanczos3");
    cmd
}

pub(crate) fn build_awww_img_command(
    path: &str,
    resize: &str,
    transition_type: &str,
    duration: &str,
    fps: &str,
) -> Command {
    let mut cmd = Command::new("awww");
    cmd.arg("img")
        .arg(path)
        .arg("--resize")
        .arg(resize)
        .arg("--transition-type")
        .arg(transition_type)
        .arg("--transition-duration")
        .arg(duration)
        .arg("--transition-fps")
        .arg(fps);
    cmd
}

#[cfg(test)]
fn build_pkill_exact_command(user: &str, process_name: &str) -> Command {
    let mut cmd = Command::new("pkill");
    cmd.args(["-u", user, "-x", process_name]);
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn awww_command_includes_transition_fps() {
        let cmd = build_awww_img_command("/tmp/test.jpg", "crop", "fade", "1", "60");
        let args: Vec<String> = cmd
            .get_args()
            .map(|s| s.to_string_lossy().to_string())
            .collect();
        assert!(args.contains(&"--resize".to_string()));
        assert!(args.contains(&"crop".to_string()));
        assert!(args.contains(&"--transition-type".to_string()));
        assert!(args.contains(&"fade".to_string()));
        assert!(args.contains(&"--transition-fps".to_string()));
        assert!(args.contains(&"60".to_string()));
    }

    #[test]
    fn normalize_awww_resize_known_values() {
        assert_eq!(normalize_awww_resize("crop"), "crop");
        assert_eq!(normalize_awww_resize("fit"), "fit");
        assert_eq!(normalize_awww_resize("stretch"), "stretch");
    }

    #[test]
    fn normalize_awww_resize_unknown_fallback() {
        assert_eq!(normalize_awww_resize("unknown"), "crop");
        assert_eq!(normalize_awww_resize(""), "crop");
        assert_eq!(normalize_awww_resize("center"), "crop");
    }

    #[test]
    fn normalize_awww_transition_type_legacy_slide() {
        assert_eq!(normalize_awww_transition_type("slide"), "left");
    }

    #[test]
    fn normalize_awww_transition_type_legacy_none() {
        assert_eq!(normalize_awww_transition_type("none"), "simple");
    }

    #[test]
    fn normalize_awww_transition_type_known_values() {
        for v in &[
            "simple", "fade", "left", "right", "top", "bottom", "wipe", "grow", "center", "outer",
            "random", "wave",
        ] {
            assert_eq!(normalize_awww_transition_type(v), *v);
        }
    }

    #[test]
    fn normalize_awww_transition_type_unknown_fallback() {
        assert_eq!(normalize_awww_transition_type("invalid"), "fade");
        assert_eq!(normalize_awww_transition_type(""), "fade");
    }

    #[test]
    fn build_awww_img_command_normalizes_slide_to_left() {
        let cmd = build_awww_img_command(
            "/tmp/test.jpg",
            "crop",
            normalize_awww_transition_type("slide"),
            "1",
            "60",
        );
        let args: Vec<String> = cmd
            .get_args()
            .map(|s| s.to_string_lossy().to_string())
            .collect();
        assert!(
            !args.contains(&"slide".to_string()),
            "slide must not appear: {:?}",
            args
        );
        assert!(
            args.contains(&"left".to_string()),
            "slide should normalize to left"
        );
    }

    #[test]
    fn build_awww_instant_never_uses_none() {
        let cmd = build_awww_instant_command("/tmp/test.jpg", "crop", "60");
        let args: Vec<String> = cmd
            .get_args()
            .map(|s| s.to_string_lossy().to_string())
            .collect();
        assert!(
            !args.contains(&"none".to_string()),
            "instant must not use none"
        );
        assert!(
            args.contains(&"simple".to_string()),
            "instant must use simple"
        );
    }

    #[test]
    fn awww_command_clamps_invalid_fps_and_duration() {
        let duration = wc_core::config_normalizer::normalize_awww_transition_duration("-1");
        let fps = wc_core::config_normalizer::normalize_awww_transition_fps("999");
        let cmd = build_awww_img_command("/tmp/test.jpg", "crop", "fade", &duration, &fps);
        let args: Vec<String> = cmd
            .get_args()
            .map(|s| s.to_string_lossy().to_string())
            .collect();

        assert!(args.contains(&"1".to_string()));
        assert!(args.contains(&"240".to_string()));
        assert!(!args.contains(&"-1".to_string()));
        assert!(!args.contains(&"999".to_string()));
    }

    #[test]
    fn awww_resize_unknown_fallback_to_crop() {
        let cmd = build_awww_img_command("/tmp/test.jpg", "crop", "fade", "1", "60");
        let args: Vec<String> = cmd
            .get_args()
            .map(|s| s.to_string_lossy().to_string())
            .collect();
        assert!(args.contains(&"--resize".to_string()));
        assert!(args.contains(&"crop".to_string()));
        assert!(!args.contains(&"unknown".to_string()));
    }

    #[test]
    fn stop_awww_uses_exact_daemon_process_name() {
        let cmd = build_pkill_exact_command("alice", "awww-daemon");
        let args: Vec<String> = cmd
            .get_args()
            .map(|s| s.to_string_lossy().to_string())
            .collect();
        assert_eq!(args, ["-u", "alice", "-x", "awww-daemon"]);
        assert!(!args.contains(&r"(^|/)awww\b".to_string()));
    }

    #[test]
    fn stop_awww_waits_until_daemon_is_gone_before_returning() {
        let pc = SequenceProcessControl::new(vec![vec![42], vec![42], vec![]]);

        stop_awww_daemon_with_wait(&pc, std::time::Duration::ZERO);

        assert_eq!(pc.termed(), vec![42]);
        assert!(pc.killed().is_empty());
        assert_eq!(pc.find_calls(), 3);
    }

    #[test]
    fn stop_awww_kills_daemon_when_term_does_not_exit() {
        let pc = SequenceProcessControl::new(vec![vec![42], vec![42], vec![42], vec![42]]);

        stop_awww_daemon_with_wait(&pc, std::time::Duration::ZERO);

        assert_eq!(pc.termed(), vec![42]);
        assert_eq!(pc.killed(), vec![42]);
    }

    struct SequenceProcessControl {
        results: std::cell::RefCell<Vec<Vec<u32>>>,
        last: std::cell::RefCell<Vec<u32>>,
        find_calls: std::cell::Cell<usize>,
        termed: std::cell::RefCell<Vec<u32>>,
        killed: std::cell::RefCell<Vec<u32>>,
    }

    impl SequenceProcessControl {
        fn new(results: Vec<Vec<u32>>) -> Self {
            Self {
                results: std::cell::RefCell::new(results),
                last: std::cell::RefCell::new(Vec::new()),
                find_calls: std::cell::Cell::new(0),
                termed: std::cell::RefCell::new(Vec::new()),
                killed: std::cell::RefCell::new(Vec::new()),
            }
        }

        fn find_calls(&self) -> usize {
            self.find_calls.get()
        }

        fn termed(&self) -> Vec<u32> {
            self.termed.borrow().clone()
        }

        fn killed(&self) -> Vec<u32> {
            self.killed.borrow().clone()
        }
    }

    impl crate::process_control::ProcessControl for SequenceProcessControl {
        fn find_processes(&self, _pattern: &str) -> Vec<u32> {
            self.find_calls.set(self.find_calls.get() + 1);
            let next = if self.results.borrow().is_empty() {
                self.last.borrow().clone()
            } else {
                self.results.borrow_mut().remove(0)
            };
            *self.last.borrow_mut() = next.clone();
            next
        }

        fn process_group_of(&self, _pid: u32) -> Option<u32> {
            None
        }

        fn term_process(&self, pid: u32) {
            self.termed.borrow_mut().push(pid);
        }

        fn kill_process(&self, pid: u32) {
            self.killed.borrow_mut().push(pid);
        }
    }
}
