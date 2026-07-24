use std::process::Command;

#[test]
fn top_level_help_flags_exit_successfully() {
    for flag in ["--help", "-h"] {
        let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .arg(flag)
            .output()
            .expect("xtask help must be runnable");

        assert!(
            output.status.success(),
            "xtask {flag} exited with {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("Usage:"),
            "xtask {flag} must print usage to stdout"
        );
    }
}
