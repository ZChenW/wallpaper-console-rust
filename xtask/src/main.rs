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
    Step {
        name: "Library 10k performance gate",
        cwd: StepCwd::RepoRoot,
        program: "cargo",
        args: &[
            "test",
            "-p",
            "wc-storage",
            "--test",
            "library_browser_perf",
            "--",
            "--nocapture",
        ],
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
        // Mock Vite + Playwright Chromium only — not Tauri WebView / native runtime E2E.
        name: "Frontend mock-browser UI smoke",
        cwd: StepCwd::Frontend,
        program: "npm",
        args: &["run", "smoke"],
    },
    Step {
        name: "Frontend Library performance gate",
        cwd: StepCwd::Frontend,
        program: "npm",
        args: &["run", "perf:library"],
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

    for step in steps_for(args.suite) {
        run_step(&repo_root, step, args.dry_run)?;
    }

    Ok(())
}

fn steps_for(suite: VerifySuite) -> Vec<Step> {
    match suite {
        VerifySuite::Rust => RUST_STEPS.to_vec(),
        VerifySuite::Frontend => FRONTEND_STEPS.to_vec(),
        VerifySuite::Drift => DRIFT_STEPS.to_vec(),
        VerifySuite::All => RUST_STEPS
            .iter()
            .chain(FRONTEND_STEPS.iter())
            .chain(DRIFT_STEPS.iter())
            .copied()
            .collect(),
    }
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
    fn all_steps_run_rust_then_frontend_then_drift() {
        let steps = steps_for(VerifySuite::All);
        let names: Vec<&str> = steps.iter().map(|s| s.name).collect();
        assert_eq!(
            steps.len(),
            RUST_STEPS.len() + FRONTEND_STEPS.len() + DRIFT_STEPS.len()
        );
        assert_eq!(names.first(), Some(&"Rust format"));
        assert_eq!(names.last(), Some(&"Runtime/config drift"));
    }

    #[test]
    fn steps_for_rust_returns_only_rust_steps() {
        let steps = steps_for(VerifySuite::Rust);
        assert_eq!(steps.len(), RUST_STEPS.len());
        assert_eq!(steps.first().map(|s| s.name), Some("Rust format"));
    }

    #[test]
    fn steps_for_frontend_returns_only_frontend_steps() {
        let steps = steps_for(VerifySuite::Frontend);
        assert_eq!(steps.len(), FRONTEND_STEPS.len());
        assert_eq!(steps.first().map(|s| s.name), Some("Frontend typecheck"));
    }

    #[test]
    fn steps_for_drift_returns_only_drift_steps() {
        let steps = steps_for(VerifySuite::Drift);
        assert_eq!(steps.len(), DRIFT_STEPS.len());
        assert_eq!(steps.first().map(|s| s.name), Some("Runtime/config drift"));
    }

    #[test]
    fn rejects_unknown_suite() {
        let err = parse_verify_args(&args(&["verify", "docs"])).unwrap_err();
        assert!(err.contains("unknown verify suite"));
    }

    #[test]
    fn frontend_npm_steps_name_existing_package_scripts() {
        let package_json =
            std::fs::read_to_string(repo_root().join("apps/tauri-gui/frontend/package.json"))
                .expect("frontend package.json must be readable");
        let scripts = package_json_script_names(&package_json);

        let missing: Vec<&str> = FRONTEND_STEPS
            .iter()
            .filter(|step| step.program == "npm" && step.args.first() == Some(&"run"))
            .filter_map(|step| step.args.get(1).copied())
            .filter(|script| !scripts.contains(*script))
            .collect();

        assert!(
            missing.is_empty(),
            "FRONTEND_STEPS reference npm scripts missing from package.json: {missing:?}"
        );
    }

    #[test]
    fn verification_assets_exist_and_are_not_gitignored() {
        let root = repo_root();
        let mut missing = Vec::new();
        let mut ignored = Vec::new();

        for rel in REQUIRED_VERIFICATION_ASSETS {
            let path = root.join(rel);
            if !path.exists() {
                missing.push(*rel);
                continue;
            }
            if path_is_gitignored(&root, rel) {
                ignored.push(*rel);
            }
        }

        assert!(
            missing.is_empty() && ignored.is_empty(),
            "verification assets must exist and be trackable for a fresh clone; missing={missing:?} ignored={ignored:?}"
        );
    }

    #[test]
    fn package_lock_includes_playwright_for_smoke() {
        let lockfile =
            std::fs::read_to_string(repo_root().join("apps/tauri-gui/frontend/package-lock.json"))
                .expect("frontend package-lock.json must be readable");
        assert!(
            lockfile.contains("\"node_modules/@playwright/test\"")
                || lockfile.contains("\"@playwright/test\""),
            "package-lock.json must include @playwright/test so npm ci can install smoke deps"
        );
    }

    #[test]
    fn smoke_script_bootstraps_or_preflights_chromium_before_tests() {
        let package_json =
            std::fs::read_to_string(repo_root().join("apps/tauri-gui/frontend/package.json"))
                .expect("frontend package.json must be readable");
        let smoke = package_json_script_body(&package_json, "smoke")
            .expect("package.json must declare a smoke script");

        let bootstraps = smoke.contains("playwright install");
        let preflights = smoke.contains("preflight") || smoke.contains("ensure");
        assert!(
            bootstraps || preflights,
            "smoke must self-bootstrap matching Chromium or run one clear preflight before tests; got: {smoke}"
        );
        assert!(
            smoke.contains("playwright test") || smoke.contains("playwright"),
            "smoke must still run Playwright tests; got: {smoke}"
        );
    }

    #[test]
    fn frontend_smoke_step_is_labeled_mock_browser_ui_smoke() {
        let smoke = FRONTEND_STEPS
            .iter()
            .find(|step| step.args.get(1) == Some(&"smoke"))
            .expect("FRONTEND_STEPS must include the smoke npm script");
        let name = smoke.name.to_ascii_lowercase();
        assert!(
            name.contains("mock-browser") || name.contains("mock browser"),
            "smoke step must be labeled as mock-browser UI smoke so it is not mistaken for Tauri runtime E2E; got: {}",
            smoke.name
        );
        assert!(
            !name.contains("e2e") || name.contains("mock"),
            "smoke step name must not read as generic Tauri E2E; got: {}",
            smoke.name
        );
    }

    #[test]
    fn playwright_webserver_uses_env_and_cwd_relative_config() {
        let config = std::fs::read_to_string(
            repo_root().join("apps/tauri-gui/frontend/e2e/playwright.config.ts"),
        )
        .expect("playwright.config.ts must be readable");

        assert!(
            !config.contains("NO_PROXY=127.0.0.1") && !config.contains("NO_PROXY=localhost"),
            "webServer.command must not use POSIX inline env assignment (NO_PROXY=...)"
        );
        assert!(
            !config.contains("no_proxy=127.0.0.1") && !config.contains("no_proxy=localhost"),
            "webServer.command must not use POSIX inline env assignment (no_proxy=...)"
        );
        assert!(
            !config.contains("${mockConfig}") && !config.contains("${frontendDir}"),
            "webServer.command must not interpolate unquoted absolute paths"
        );
        assert!(
            config.contains("env:") || config.contains("env :"),
            "playwright config must set webServer.env for proxy bypass"
        );
        assert!(
            config.contains("vite.mock.config.ts")
                && (config.contains("cwd:")
                    || config.contains("cwd :")
                    || config.contains("../vite.mock.config.ts")),
            "webServer must use a cwd-relative vite mock config path"
        );
    }

    #[test]
    fn drift_check_rejects_multiline_forbidden_schema_options() {
        let root = repo_root();
        let fixture_dir = std::env::temp_dir().join(format!(
            "wcr-drift-multiline-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&fixture_dir).expect("temp fixture dir");
        let schema_path = fixture_dir.join("configSchema.ts");
        std::fs::write(
            &schema_path,
            r#"
export const ALL_SETTINGS = [
  {
    key: 'awww_transition_type',
    type: 'select',
    options: [
      'fade',
      'slide',
    ],
  },
  {
    key: 'linux_wallpaperengine_target_mode',
    type: 'select',
    options: [
      'auto',
      'window',
    ],
  },
  {
    key: 'storage_backend',
    type: 'select',
    options: [
      'sqlite',
      'file',
      'hybrid',
    ],
  },
];
"#,
        )
        .expect("write multiline forbidden schema fixture");

        let output = Command::new("bash")
            .arg(root.join("scripts/check_runtime_config_drift.sh"))
            .env("DRIFT_CONFIG_SCHEMA", &schema_path)
            .current_dir(&root)
            .output()
            .expect("drift check must be runnable");

        let _ = std::fs::remove_dir_all(&fixture_dir);

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let combined = format!("{stdout}{stderr}");

        assert!(
            !output.status.success(),
            "multiline forbidden options must be rejected; stdout={stdout} stderr={stderr}"
        );
        assert!(
            combined.contains("slide") && combined.contains("DRIFT"),
            "must report slide drift; output={combined}"
        );
        assert!(
            combined.contains("window") && combined.contains("DRIFT"),
            "must report window drift; output={combined}"
        );
        assert!(
            (combined.contains("file")
                || combined.contains("hybrid")
                || combined.contains("storage_backend"))
                && combined.contains("DRIFT"),
            "must report storage_backend file/hybrid drift; output={combined}"
        );
    }

    const REQUIRED_VERIFICATION_ASSETS: &[&str] = &[
        "xtask/Cargo.toml",
        "xtask/src/main.rs",
        "scripts/check_runtime_config_drift.sh",
        "apps/tauri-gui/frontend/package-lock.json",
        "apps/tauri-gui/frontend/vite.mock.config.ts",
        "apps/tauri-gui/frontend/e2e/playwright.config.ts",
        "apps/tauri-gui/frontend/e2e/smoke.spec.ts",
    ];

    fn path_is_gitignored(repo_root: &Path, rel: &str) -> bool {
        let status = Command::new("git")
            .args(["check-ignore", "-q", "--", rel])
            .current_dir(repo_root)
            .status()
            .expect("git check-ignore must be runnable");
        match status.code() {
            Some(0) => true,
            Some(1) => false,
            other => panic!("git check-ignore exited unexpectedly: {other:?}"),
        }
    }

    fn package_json_script_names(package_json: &str) -> std::collections::HashSet<&str> {
        let scripts_key = package_json
            .find("\"scripts\"")
            .expect("package.json must declare scripts");
        let after_key = &package_json[scripts_key..];
        let block_start = after_key
            .find('{')
            .expect("scripts value must be an object");
        let block = &after_key[block_start + 1..];
        let block_end = block.find('}').expect("scripts object must close");
        let scripts_block = &block[..block_end];

        scripts_block
            .lines()
            .filter_map(|line| {
                let trimmed = line.trim();
                if !trimmed.starts_with('"') {
                    return None;
                }
                trimmed.split('"').nth(1)
            })
            .collect()
    }

    fn package_json_script_body<'a>(package_json: &'a str, name: &str) -> Option<&'a str> {
        let key = format!("\"{name}\"");
        let scripts_key = package_json.find("\"scripts\"")?;
        let after_scripts = &package_json[scripts_key..];
        let block_start = after_scripts.find('{')?;
        let block = &after_scripts[block_start + 1..];
        let block_end = block.find('}')?;
        let scripts_block = &block[..block_end];

        for line in scripts_block.lines() {
            let trimmed = line.trim();
            if !trimmed.starts_with(&key) {
                continue;
            }
            let after_key = trimmed.get(key.len()..)?.trim_start();
            let after_colon = after_key.strip_prefix(':')?.trim_start();
            let body = after_colon.strip_prefix('"')?.trim_end();
            return Some(body.trim_end_matches([',', '"']));
        }
        None
    }
}
