//! Install-tree verification suites and their execution policy.

use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum VerifySuite {
    Rust,
    Frontend,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct VerifyArgs {
    pub(super) suite: VerifySuite,
    pub(super) dry_run: bool,
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
        args: &["test", "--workspace", "--", "--test-threads=1"],
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
        name: "Frontend build",
        cwd: StepCwd::Frontend,
        program: "npm",
        args: &["run", "build"],
    },
];

pub(super) fn verify(repo_root: &Path, args: VerifyArgs) -> Result<(), String> {
    for step in steps_for(args.suite) {
        run_step(repo_root, step, args.dry_run)?;
    }
    Ok(())
}

fn steps_for(suite: VerifySuite) -> Vec<Step> {
    match suite {
        VerifySuite::Rust => RUST_STEPS.to_vec(),
        VerifySuite::Frontend => FRONTEND_STEPS.to_vec(),
        VerifySuite::All => RUST_STEPS
            .iter()
            .chain(FRONTEND_STEPS.iter())
            .copied()
            .collect(),
    }
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

impl StepCwd {
    fn path(self, repo_root: &Path) -> PathBuf {
        match self {
            StepCwd::RepoRoot => repo_root.to_path_buf(),
            StepCwd::Frontend => repo_root.join("apps/tauri-gui/frontend"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{project_version, repo_root};

    #[test]
    fn rust_workspace_tests_are_serialized_for_process_global_state() {
        let step = RUST_STEPS
            .iter()
            .find(|step| step.name == "Rust tests")
            .expect("Rust test step");
        assert_eq!(
            step.args,
            &["test", "--workspace", "--", "--test-threads=1"]
        );
    }

    #[test]
    fn all_steps_run_release_tree_checks_only() {
        let steps = steps_for(VerifySuite::All);
        let names: Vec<&str> = steps.iter().map(|step| step.name).collect();
        assert_eq!(steps.len(), RUST_STEPS.len() + FRONTEND_STEPS.len());
        assert_eq!(names.first(), Some(&"Rust format"));
        assert_eq!(names.last(), Some(&"Frontend build"));
        assert!(steps.iter().all(|step| !step.args.iter().any(|arg| {
            arg.contains("scripts/") || arg.contains("test:unit") || arg.contains("smoke")
        })));
    }

    #[test]
    fn steps_for_individual_suites_are_scoped() {
        let rust = steps_for(VerifySuite::Rust);
        assert_eq!(rust.len(), RUST_STEPS.len());
        assert_eq!(rust.first().map(|step| step.name), Some("Rust format"));

        let frontend = steps_for(VerifySuite::Frontend);
        assert_eq!(frontend.len(), FRONTEND_STEPS.len());
        assert_eq!(
            frontend.iter().map(|step| step.name).collect::<Vec<_>>(),
            ["Frontend typecheck", "Frontend build"]
        );
    }

    #[test]
    fn frontend_steps_name_existing_package_scripts() {
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
    fn install_minimal_sources_do_not_reference_local_only_modules() {
        let root = repo_root();
        for (source, forbidden) in [
            ("crates/wc-app/src/lib.rs", "mod display_discovery_tests;"),
            ("crates/wc-backend/src/lib.rs", "mod test_support;"),
            ("crates/wc-core/src/lib.rs", "mod tests;"),
        ] {
            let text = std::fs::read_to_string(root.join(source)).unwrap();
            assert!(
                !text.contains(forbidden),
                "install-minimal source {source} references ignored module {forbidden}"
            );
        }
    }

    #[test]
    fn release_versions_are_aligned() {
        let root = repo_root();
        let expected = project_version(&root).unwrap();
        for rel in [
            "apps/tauri-gui/src-tauri/tauri.conf.json",
            "apps/tauri-gui/frontend/package.json",
            "apps/tauri-gui/frontend/package-lock.json",
        ] {
            let text = std::fs::read_to_string(root.join(rel)).unwrap();
            assert_eq!(
                json_version(&text).as_deref(),
                Some(expected.as_str()),
                "{rel}"
            );
        }
    }

    #[test]
    fn frontend_lockfile_uses_the_official_npm_registry() {
        let lockfile =
            std::fs::read_to_string(repo_root().join("apps/tauri-gui/frontend/package-lock.json"))
                .expect("frontend package-lock.json must be readable");
        assert!(
            !lockfile.contains("registry.npmmirror.com"),
            "release lockfile must not pin dependencies to registry.npmmirror.com"
        );
        for line in lockfile
            .lines()
            .filter(|line| line.contains("\"resolved\""))
        {
            assert!(
                line.contains("https://registry.npmjs.org/"),
                "registry dependency must resolve from registry.npmjs.org: {line}"
            );
        }
    }

    #[test]
    fn release_workflow_builds_and_publishes_expected_assets() {
        let workflow = std::fs::read_to_string(repo_root().join(".github/workflows/release.yml"))
            .expect("release workflow must be tracked");
        assert!(
            !workflow.contains("--clobber"),
            "published release assets must not be replaced in place"
        );
        assert!(
            !workflow
                .lines()
                .any(|line| line.trim_start().starts_with("! grep")),
            "negative grep under set -e does not fail the workflow; use an explicit if"
        );
        for forbidden in [
            "releases/download/continuous",
            "/master/",
            "gh release delete",
        ] {
            assert!(
                !workflow.contains(forbidden),
                "release workflow must not download mutable packaging input: {forbidden}"
            );
        }
        for required in [
            "runs-on: ubuntu-22.04",
            "permissions:\n  contents: read",
            "cargo run --locked -p xtask -- verify all",
            "cargo tauri build --bundles appimage",
            "release prepare-appimage",
            "Pin Tauri Linux packaging tools",
            "releases/download/appimage-toolchain-v2",
            "linuxdeploy-plugin-appimage.AppImage",
            "appimage-runtime-x86_64",
            "linuxdeploy-plugin-appimage-verified.AppImage",
            "APPIMAGE_PINNED_RUNTIME",
            "immutable mirror provenance",
            "--runtime-file",
            "candidate-second.AppImage",
            "cmp \"${candidate}\" \"${candidate_second}\"",
            "git cat-file -t",
            "verified_tag_ref",
            "git fetch --force origin",
            "release verify-remote-tag",
            "--expected-tag-object",
            "release verify-appimage-runtime",
            "tag_object=\"$(git rev-parse --verify \"${verified_tag_ref}\")\"",
            "tag_object: ${{ steps.release.outputs.tag_object }}",
            "--expected-tag-object \"${{ steps.release.outputs.tag_object }}\"",
            "published release state does not match the contract",
            ".immutable == true",
            "ref: ${{ needs.build-linux-x86_64.outputs.commit }}",
            "LD_LIBRARY_PATH=/usr/lib/x86_64-linux-gnu ldd",
            "env -u LD_LIBRARY_PATH ldd",
            "20eebde3c18ae2e44279bd624fc72482503aece216d5d77f10932235342f71c1",
            "cb379f9b0733e9ad9f8bd78f8c2fa038aef2478523bb7d4c8e64ff6a1ea3501a",
            "c107b49d84edbffc6ab226ed1007e0626a4f7aa2c3a36b7782bef62351d49e94",
            "e0129b8070e0c7b37151027e46e9fa44fe97ea29e3692705a2c5cff3771d3121",
            "439731bfc9b4620ad11802ad5a3c22707f24f3a49de09461ec937ce6e35dd5cd",
            "SOURCE_DATE_EPOCH",
            "touch -h -d",
            "--appdir",
            "release package-linux",
            "libwebkit2gtk-4.1.so",
            "grep -q 'LD_LIBRARY_PATH'",
            "WCR_APPIMAGE_FORCE_BUNDLED=1",
            "xvfb-run -a target/appimage-smoke/squashfs-root/AppRun",
            "Unable to spawn a new child process",
            "(cd dist && sha256sum -c SHA256SUMS)",
            "uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1",
            "uses: actions/setup-node@820762786026740c76f36085b0efc47a31fe5020",
            "uses: Swatinem/rust-cache@63fed3e2fecf6f7b51dc6f043341b79ef82a9ae7",
            "uses: actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a",
            "uses: actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c",
            "- \"v*\"",
            "prerelease: ${{ steps.release.outputs.prerelease }}",
            "RELEASE_PRERELEASE",
            "release_flags=(",
            "gh release create",
            "--draft",
            "gh release download",
            "--draft=false",
            "--prerelease",
            "RELEASE_NOTES.md",
        ] {
            assert!(
                workflow.contains(required),
                "release workflow is missing: {required}"
            );
        }
    }

    #[test]
    fn release_verification_assets_exist_and_are_tracked() {
        let root = repo_root();
        let assets = [
            "Cargo.toml",
            "Cargo.lock",
            "LICENSE",
            "README.md",
            "RELEASE_NOTES.md",
            ".github/workflows/release.yml",
            "install.sh",
            "xtask/Cargo.toml",
            "xtask/src/main.rs",
            "apps/tauri-gui/frontend/package.json",
            "apps/tauri-gui/frontend/package-lock.json",
            "apps/tauri-gui/src-tauri/tauri.conf.json",
        ];

        for rel in assets {
            assert!(
                root.join(rel).is_file(),
                "release verification asset missing: {rel}"
            );
            let status = Command::new("git")
                .args(["ls-files", "--error-unmatch", "--", rel])
                .current_dir(&root)
                .status()
                .expect("git ls-files must be runnable");
            assert!(
                status.success(),
                "release verification asset is not tracked: {rel}"
            );
        }
    }

    fn json_version(text: &str) -> Option<String> {
        let after_key = text.split_once("\"version\"")?.1;
        let after_colon = after_key.split_once(':')?.1.trim_start();
        let quoted = after_colon.strip_prefix('"')?;
        Some(quoted.split_once('"')?.0.to_string())
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

        block[..block_end]
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
}
