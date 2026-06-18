//! xtask — repository verification runner.

use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VerifySuite {
    Rust,
    Frontend,
    Drift,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VerifyArgs {
    suite: VerifySuite,
    dry_run: bool,
}

#[derive(Debug, Clone, Copy)]
struct Step {
    name: &'static str,
    cwd: StepCwd,
    program: &'static str,
    args: &'static [&'static str],
}

#[derive(Debug, Clone, Copy)]
enum StepCwd {
    RepoRoot,
    Frontend,
}

const RUST_STEPS: &[Step] = &[
    Step {
        name: "Rust format",
        cwd: StepCwd::RepoRoot,
        program: "cargo",
        args: &["fmt", "--all", "--", "--check"],
    },
    Step {
        name: "Rust check",
        cwd: StepCwd::RepoRoot,
        program: "cargo",
        args: &["check", "--workspace"],
    },
    Step {
        name: "Rust clippy",
        cwd: StepCwd::RepoRoot,
        program: "cargo",
        args: &["clippy", "--workspace", "--", "-D", "warnings"],
    },
    Step {
        name: "Rust tests",
        cwd: StepCwd::RepoRoot,
        program: "cargo",
        args: &["test", "--workspace"],
    },
];

const FRONTEND_STEPS: &[Step] = &[
    Step {
        name: "Frontend typecheck",
        cwd: StepCwd::Frontend,
        program: "npm",
        args: &["run", "typecheck"],
    },
    Step {
        name: "Frontend unit tests",
        cwd: StepCwd::Frontend,
        program: "npm",
        args: &["run", "test:unit"],
    },
    Step {
        name: "Frontend build",
        cwd: StepCwd::Frontend,
        program: "npm",
        args: &["run", "build"],
    },
    Step {
        name: "Frontend smoke tests",
        cwd: StepCwd::Frontend,
        program: "npm",
        args: &["run", "smoke"],
    },
];

const DRIFT_STEPS: &[Step] = &[Step {
    name: "Runtime/config drift",
    cwd: StepCwd::RepoRoot,
    program: "bash",
    args: &["scripts/check_runtime_config_drift.sh"],
}];

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let raw_args: Vec<String> = env::args().skip(1).collect();
    let args = parse_verify_args(&raw_args)?;
    let repo_root = repo_root();

    let steps: Vec<Step> = match args.suite {
        VerifySuite::Rust => RUST_STEPS.to_vec(),
        VerifySuite::Frontend => FRONTEND_STEPS.to_vec(),
        VerifySuite::Drift => DRIFT_STEPS.to_vec(),
        VerifySuite::All => RUST_STEPS
            .iter()
            .chain(FRONTEND_STEPS.iter())
            .chain(DRIFT_STEPS.iter())
            .copied()
            .collect(),
    };

    for step in steps {
        run_step(&repo_root, step, args.dry_run)?;
    }

    Ok(())
}

fn parse_verify_args(args: &[String]) -> Result<VerifyArgs, String> {
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" {
        return Err(usage());
    }
    if args[0] != "verify" {
        return Err(format!("unknown xtask command: {}\n\n{}", args[0], usage()));
    }

    let suite = match args.get(1).map(String::as_str) {
        Some("rust") => VerifySuite::Rust,
        Some("frontend") => VerifySuite::Frontend,
        Some("drift") => VerifySuite::Drift,
        Some("all") => VerifySuite::All,
        Some(other) => return Err(format!("unknown verify suite: {other}\n\n{}", usage())),
        None => return Err(usage()),
    };

    let mut dry_run = false;
    for arg in &args[2..] {
        match arg.as_str() {
            "--dry-run" => dry_run = true,
            other => return Err(format!("unknown verify option: {other}\n\n{}", usage())),
        }
    }

    Ok(VerifyArgs { suite, dry_run })
}

fn run_step(repo_root: &Path, step: Step, dry_run: bool) -> Result<(), String> {
    let cwd = step.cwd.path(repo_root);
    println!("==> {}", step.name);
    println!("    {}", format_command(&cwd, step));
    if dry_run {
        return Ok(());
    }

    let status = Command::new(step.program)
        .args(step.args)
        .current_dir(&cwd)
        .status()
        .map_err(|err| format!("failed to start {}: {err}", step.name))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "{} failed with exit code {}",
            step.name,
            status
                .code()
                .map(|code| code.to_string())
                .unwrap_or_else(|| "signal".to_string())
        ))
    }
}

fn format_command(cwd: &Path, step: Step) -> String {
    let args = step.args.join(" ");
    format!("cd {} && {} {}", cwd.display(), step.program, args)
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask must live under the repository root")
        .to_path_buf()
}

impl StepCwd {
    fn path(self, repo_root: &Path) -> PathBuf {
        match self {
            StepCwd::RepoRoot => repo_root.to_path_buf(),
            StepCwd::Frontend => repo_root.join("apps/tauri-gui/frontend"),
        }
    }
}

fn usage() -> String {
    "Usage:
  cargo run -p xtask -- verify rust [--dry-run]
  cargo run -p xtask -- verify frontend [--dry-run]
  cargo run -p xtask -- verify drift [--dry-run]
  cargo run -p xtask -- verify all [--dry-run]"
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_rust_dry_run() {
        assert_eq!(
            parse_verify_args(&args(&["verify", "rust", "--dry-run"])).unwrap(),
            VerifyArgs {
                suite: VerifySuite::Rust,
                dry_run: true,
            }
        );
    }

    #[test]
    fn parses_frontend() {
        assert_eq!(
            parse_verify_args(&args(&["verify", "frontend"])).unwrap(),
            VerifyArgs {
                suite: VerifySuite::Frontend,
                dry_run: false,
            }
        );
    }

    #[test]
    fn parses_drift() {
        assert_eq!(
            parse_verify_args(&args(&["verify", "drift"])).unwrap(),
            VerifyArgs {
                suite: VerifySuite::Drift,
                dry_run: false,
            }
        );
    }

    #[test]
    fn parses_drift_dry_run() {
        assert_eq!(
            parse_verify_args(&args(&["verify", "drift", "--dry-run"])).unwrap(),
            VerifyArgs {
                suite: VerifySuite::Drift,
                dry_run: true,
            }
        );
    }

    #[test]
    fn parses_all_dry_run() {
        assert_eq!(
            parse_verify_args(&args(&["verify", "all", "--dry-run"])).unwrap(),
            VerifyArgs {
                suite: VerifySuite::All,
                dry_run: true,
            }
        );
    }

    #[test]
    fn rejects_unknown_suite() {
        let err = parse_verify_args(&args(&["verify", "docs"])).unwrap_err();
        assert!(err.contains("unknown verify suite"));
    }
}
