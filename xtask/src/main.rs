//! xtask — repository verification runner.

use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VerifySuite {
    Rust,
    Frontend,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VerifyArgs {
    suite: VerifySuite,
    dry_run: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReleasePackageLinuxArgs {
    version: String,
    appimage: PathBuf,
    cli: PathBuf,
    out_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReleasePrepareAppImageArgs {
    appdir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReleaseAssetNames {
    appimage: String,
    cli_archive: String,
    checksums: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum XtaskCommand {
    Help,
    Verify(VerifyArgs),
    ReleasePackageLinux(ReleasePackageLinuxArgs),
    ReleasePrepareAppImage(ReleasePrepareAppImageArgs),
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
    match parse_xtask_command(&raw_args)? {
        XtaskCommand::Help => {
            println!("{}", usage());
            Ok(())
        }
        XtaskCommand::Verify(args) => {
            let repo_root = repo_root();
            for step in steps_for(args.suite) {
                run_step(&repo_root, step, args.dry_run)?;
            }
            Ok(())
        }
        XtaskCommand::ReleasePackageLinux(args) => package_linux_release(&repo_root(), &args),
        XtaskCommand::ReleasePrepareAppImage(args) => {
            let root = repo_root();
            let appdir = resolve_from(&root, &args.appdir);
            prepare_appimage_appdir(&appdir)
        }
    }
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

fn parse_xtask_command(args: &[String]) -> Result<XtaskCommand, String> {
    match args.first().map(String::as_str) {
        Some("--help" | "-h") => Ok(XtaskCommand::Help),
        Some("release") => parse_release_args(args),
        _ => parse_verify_args(args).map(XtaskCommand::Verify),
    }
}

fn parse_release_args(args: &[String]) -> Result<XtaskCommand, String> {
    if args.get(1).map(String::as_str) == Some("prepare-appimage") {
        if args.get(2).map(String::as_str) != Some("--appdir") || args.len() != 4 {
            return Err(format!(
                "invalid prepare-appimage arguments

{}",
                usage()
            ));
        }
        return Ok(XtaskCommand::ReleasePrepareAppImage(
            ReleasePrepareAppImageArgs {
                appdir: PathBuf::from(&args[3]),
            },
        ));
    }
    if args.get(1).map(String::as_str) != Some("package-linux") {
        return Err(format!(
            "unknown release command

{}",
            usage()
        ));
    }

    let mut version = None;
    let mut appimage = None;
    let mut cli = None;
    let mut out_dir = None;
    let mut index = 2;
    while index < args.len() {
        let flag = args[index].as_str();
        let value = args.get(index + 1).ok_or_else(|| {
            format!(
                "missing value for {flag}

{}",
                usage()
            )
        })?;
        match flag {
            "--version" => version = Some(value.clone()),
            "--appimage" => appimage = Some(PathBuf::from(value)),
            "--cli" => cli = Some(PathBuf::from(value)),
            "--out" => out_dir = Some(PathBuf::from(value)),
            other => {
                return Err(format!(
                    "unknown release option: {other}

{}",
                    usage()
                ))
            }
        }
        index += 2;
    }

    let args = ReleasePackageLinuxArgs {
        version: version.ok_or_else(|| "missing --version".to_string())?,
        appimage: appimage.ok_or_else(|| "missing --appimage".to_string())?,
        cli: cli.ok_or_else(|| "missing --cli".to_string())?,
        out_dir: out_dir.ok_or_else(|| "missing --out".to_string())?,
    };
    release_asset_names(&args.version)?;
    Ok(XtaskCommand::ReleasePackageLinux(args))
}

fn release_asset_names(version: &str) -> Result<ReleaseAssetNames, String> {
    let valid = !version.is_empty()
        && version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
        && version
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_digit())
        && version
            .bytes()
            .last()
            .is_some_and(|byte| byte.is_ascii_alphanumeric());
    if !valid {
        return Err(format!("invalid release version: {version}"));
    }

    Ok(ReleaseAssetNames {
        appimage: format!("wallpaper-console_{version}_x86_64.AppImage"),
        cli_archive: format!("wallpaper-console-cli_{version}_x86_64.tar.zst"),
        checksums: "SHA256SUMS",
    })
}

const APPIMAGE_APP_RUN: &str = r#"#!/bin/sh
set -eu
APPDIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
export APPDIR

host_library_path=
host_webkit_available=
for directory in /usr/lib/x86_64-linux-gnu /lib/x86_64-linux-gnu /usr/lib64 /usr/lib /lib64 /lib; do
  if [ -d "$directory" ]; then
    host_library_path="${host_library_path:+$host_library_path:}$directory"
    if [ -e "$directory/libwebkit2gtk-4.1.so.0" ]; then
      host_webkit_available=1
    fi
  fi
done
if [ -n "$host_library_path" ] && [ -n "$host_webkit_available" ]; then
  export LD_LIBRARY_PATH="$host_library_path${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
fi

if [ "${WCR_WEBKIT_DISABLE_DMABUF_RENDERER:-0}" = "1" ] && [ -z "${WEBKIT_DISABLE_DMABUF_RENDERER+x}" ]; then
  export WEBKIT_DISABLE_DMABUF_RENDERER=1
fi
exec "$APPDIR/usr/bin/wallpaper-console-tauri" "$@"
"#;

fn prepare_appimage_appdir(appdir: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    if !appdir.is_dir() {
        return Err(format!("AppDir is missing: {}", appdir.display()));
    }
    let gui = appdir.join("usr/bin/wallpaper-console-tauri");
    if !gui.is_file() {
        return Err(format!("AppDir GUI binary is missing: {}", gui.display()));
    }

    let hooks = appdir.join("apprun-hooks");
    if hooks.exists() {
        std::fs::remove_dir_all(&hooks)
            .map_err(|error| format!("failed to remove {}: {error}", hooks.display()))?;
    }
    for relative in ["AppRun", "AppRun.wrapped"] {
        let path = appdir.join(relative);
        if std::fs::symlink_metadata(&path).is_ok() {
            std::fs::remove_file(&path)
                .map_err(|error| format!("failed to remove {}: {error}", path.display()))?;
        }
    }

    let app_run = appdir.join("AppRun");
    std::fs::write(&app_run, APPIMAGE_APP_RUN)
        .map_err(|error| format!("failed to write {}: {error}", app_run.display()))?;
    let mut permissions = std::fs::metadata(&app_run)
        .map_err(|error| format!("failed to stat {}: {error}", app_run.display()))?
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&app_run, permissions)
        .map_err(|error| format!("failed to make {} executable: {error}", app_run.display()))?;

    println!(
        "prepared host-first AppImage AppDir at {}",
        appdir.display()
    );
    Ok(())
}

fn package_linux_release(repo_root: &Path, args: &ReleasePackageLinuxArgs) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    let project_version = project_version(repo_root)?;
    if args.version != project_version {
        return Err(format!(
            "release version {} does not match workspace version {project_version}",
            args.version
        ));
    }
    let names = release_asset_names(&args.version)?;
    let appimage_source = resolve_from(repo_root, &args.appimage);
    let cli_source = resolve_from(repo_root, &args.cli);
    let out_dir = resolve_from(repo_root, &args.out_dir);
    for (label, path) in [("AppImage", &appimage_source), ("CLI binary", &cli_source)] {
        if !path.is_file() {
            return Err(format!("{label} is missing: {}", path.display()));
        }
    }
    for rel in ["LICENSE", "README.md"] {
        if !repo_root.join(rel).is_file() {
            return Err(format!("release payload is missing {rel}"));
        }
    }

    std::fs::create_dir_all(&out_dir)
        .map_err(|error| format!("failed to create {}: {error}", out_dir.display()))?;
    let appimage_destination = out_dir.join(&names.appimage);
    std::fs::copy(&appimage_source, &appimage_destination).map_err(|error| {
        format!(
            "failed to copy {} to {}: {error}",
            appimage_source.display(),
            appimage_destination.display()
        )
    })?;
    let mut appimage_permissions = std::fs::metadata(&appimage_destination)
        .map_err(|error| format!("failed to stat AppImage: {error}"))?
        .permissions();
    appimage_permissions.set_mode(0o755);
    std::fs::set_permissions(&appimage_destination, appimage_permissions)
        .map_err(|error| format!("failed to make AppImage executable: {error}"))?;

    let bundle_name = format!("wallpaper-console-cli_{}_x86_64", args.version);
    let stage_root = out_dir.join(format!(".{bundle_name}.stage"));
    if stage_root.exists() {
        std::fs::remove_dir_all(&stage_root)
            .map_err(|error| format!("failed to reset release stage: {error}"))?;
    }
    let bundle_dir = stage_root.join(&bundle_name);
    std::fs::create_dir_all(&bundle_dir)
        .map_err(|error| format!("failed to create CLI release stage: {error}"))?;
    let staged_cli = bundle_dir.join("wallpaper-console-rust");
    std::fs::copy(&cli_source, &staged_cli)
        .map_err(|error| format!("failed to stage CLI binary: {error}"))?;
    let mut cli_permissions = std::fs::metadata(&staged_cli)
        .map_err(|error| format!("failed to stat staged CLI: {error}"))?
        .permissions();
    cli_permissions.set_mode(0o755);
    std::fs::set_permissions(&staged_cli, cli_permissions)
        .map_err(|error| format!("failed to make staged CLI executable: {error}"))?;
    for rel in ["LICENSE", "README.md"] {
        std::fs::copy(repo_root.join(rel), bundle_dir.join(rel))
            .map_err(|error| format!("failed to stage {rel}: {error}"))?;
    }

    let archive = out_dir.join(&names.cli_archive);
    let tar_status = Command::new("tar")
        .args([
            "--zstd",
            "--sort=name",
            "--mtime=@0",
            "--owner=0",
            "--group=0",
            "--numeric-owner",
            "-cf",
        ])
        .arg(&archive)
        .arg("-C")
        .arg(&stage_root)
        .arg(&bundle_name)
        .status()
        .map_err(|error| format!("failed to start tar: {error}"))?;
    if !tar_status.success() {
        return Err(format!("tar failed with status {tar_status}"));
    }
    std::fs::remove_dir_all(&stage_root)
        .map_err(|error| format!("failed to remove release stage: {error}"))?;

    let checksums = Command::new("sha256sum")
        .args([names.appimage.as_str(), names.cli_archive.as_str()])
        .current_dir(&out_dir)
        .output()
        .map_err(|error| format!("failed to start sha256sum: {error}"))?;
    if !checksums.status.success() {
        return Err(format!(
            "sha256sum failed: {}",
            String::from_utf8_lossy(&checksums.stderr)
        ));
    }
    std::fs::write(out_dir.join(names.checksums), checksums.stdout)
        .map_err(|error| format!("failed to write SHA256SUMS: {error}"))?;
    let verify_status = Command::new("sha256sum")
        .args(["-c", names.checksums])
        .current_dir(&out_dir)
        .status()
        .map_err(|error| format!("failed to verify SHA256SUMS: {error}"))?;
    if !verify_status.success() {
        return Err("generated SHA256SUMS did not verify".to_string());
    }

    println!("release assets written to {}", out_dir.display());
    Ok(())
}

fn project_version(repo_root: &Path) -> Result<String, String> {
    let manifest = std::fs::read_to_string(repo_root.join("Cargo.toml"))
        .map_err(|error| format!("failed to read workspace Cargo.toml: {error}"))?;
    let section = manifest
        .split("[workspace.package]")
        .nth(1)
        .ok_or_else(|| "Cargo.toml is missing [workspace.package]".to_string())?;
    for line in section.lines().skip(1) {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            break;
        }
        if let Some(value) = trimmed.strip_prefix("version = ") {
            return value
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
                .map(str::to_string)
                .ok_or_else(|| "workspace version must be a quoted string".to_string());
        }
    }
    Err("Cargo.toml workspace version is missing".to_string())
}

#[cfg(test)]
fn json_version(text: &str) -> Option<String> {
    let after_key = text.split_once("\"version\"")?.1;
    let after_colon = after_key.split_once(':')?.1.trim_start();
    let quoted = after_colon.strip_prefix('"')?;
    Some(quoted.split_once('"')?.0.to_string())
}

fn resolve_from(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

fn parse_verify_args(args: &[String]) -> Result<VerifyArgs, String> {
    if args.is_empty() {
        return Err(usage());
    }
    if args[0] != "verify" {
        return Err(format!("unknown xtask command: {}\n\n{}", args[0], usage()));
    }

    let suite = match args.get(1).map(String::as_str) {
        Some("rust") => VerifySuite::Rust,
        Some("frontend") => VerifySuite::Frontend,
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
  cargo run -p xtask -- verify all [--dry-run]
  cargo run -p xtask -- release package-linux --version <version> --appimage <path> --cli <path> --out <dir>
  cargo run -p xtask -- release prepare-appimage --appdir <path>"
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
    fn top_level_help_flags_are_successful_commands() {
        for flag in ["--help", "-h"] {
            assert_eq!(
                parse_xtask_command(&args(&[flag])).unwrap(),
                XtaskCommand::Help
            );
        }
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
    fn parses_linux_release_package_command() {
        assert_eq!(
            parse_xtask_command(&args(&[
                "release",
                "package-linux",
                "--version",
                "0.1.0-rc.1",
                "--appimage",
                "target/app.AppImage",
                "--cli",
                "target/wallpaper-console-rust",
                "--out",
                "dist",
            ]))
            .unwrap(),
            XtaskCommand::ReleasePackageLinux(ReleasePackageLinuxArgs {
                version: "0.1.0-rc.1".to_string(),
                appimage: PathBuf::from("target/app.AppImage"),
                cli: PathBuf::from("target/wallpaper-console-rust"),
                out_dir: PathBuf::from("dist"),
            })
        );
    }

    #[test]
    fn parses_prepare_appimage_command() {
        assert_eq!(
            parse_xtask_command(&args(&[
                "release",
                "prepare-appimage",
                "--appdir",
                "target/AppDir",
            ]))
            .unwrap(),
            XtaskCommand::ReleasePrepareAppImage(ReleasePrepareAppImageArgs {
                appdir: PathBuf::from("target/AppDir"),
            })
        );
    }

    #[test]
    fn linux_release_asset_names_are_stable() {
        assert_eq!(
            release_asset_names("0.1.0-rc.1").unwrap(),
            ReleaseAssetNames {
                appimage: "wallpaper-console_0.1.0-rc.1_x86_64.AppImage".to_string(),
                cli_archive: "wallpaper-console-cli_0.1.0-rc.1_x86_64.tar.zst".to_string(),
                checksums: "SHA256SUMS",
            }
        );
        assert!(release_asset_names("../bad").is_err());
    }

    #[test]
    fn packages_linux_release_assets_and_checksums() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wallpaper-console-release-test-{}-{unique}",
            std::process::id()
        ));
        let repo = root.join("repo");
        let input = root.join("input");
        let out = root.join("out");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::create_dir_all(&input).unwrap();
        std::fs::write(
            repo.join("Cargo.toml"),
            "[workspace]

[workspace.package]
version = \"0.1.0-rc.1\"
",
        )
        .unwrap();
        std::fs::write(
            repo.join("LICENSE"),
            "MIT test license
",
        )
        .unwrap();
        std::fs::write(
            repo.join("README.md"),
            "# Test release
",
        )
        .unwrap();
        let appimage = input.join("source.AppImage");
        let cli = input.join("wallpaper-console-rust");
        std::fs::write(
            &appimage,
            "fake appimage
",
        )
        .unwrap();
        std::fs::write(
            &cli,
            "fake cli
",
        )
        .unwrap();

        package_linux_release(
            &repo,
            &ReleasePackageLinuxArgs {
                version: "0.1.0-rc.1".to_string(),
                appimage,
                cli,
                out_dir: out.clone(),
            },
        )
        .unwrap();

        let names = release_asset_names("0.1.0-rc.1").unwrap();
        assert_eq!(
            std::fs::read_to_string(out.join(&names.appimage)).unwrap(),
            "fake appimage
"
        );
        assert!(out.join(&names.cli_archive).is_file());
        let checksum_status = Command::new("sha256sum")
            .args(["-c", names.checksums])
            .current_dir(&out)
            .status()
            .unwrap();
        assert!(checksum_status.success());

        let archive = Command::new("tar")
            .args(["--zstd", "-tf"])
            .arg(out.join(&names.cli_archive))
            .output()
            .unwrap();
        assert!(archive.status.success());
        let entries = String::from_utf8(archive.stdout).unwrap();
        let prefix = "wallpaper-console-cli_0.1.0-rc.1_x86_64";
        for expected in [
            format!("{prefix}/wallpaper-console-rust"),
            format!("{prefix}/LICENSE"),
            format!("{prefix}/README.md"),
        ] {
            assert!(entries.lines().any(|entry| entry == expected));
        }

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn prepares_host_first_appdir_with_bundled_fallback() {
        use std::os::unix::fs::PermissionsExt;

        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wallpaper-console-appdir-test-{}-{unique}",
            std::process::id()
        ));
        let appdir = root.join("AppDir");
        let gui = appdir.join("usr/bin/wallpaper-console-tauri");
        std::fs::create_dir_all(gui.parent().unwrap()).unwrap();
        std::fs::create_dir_all(appdir.join("usr/lib")).unwrap();
        std::fs::create_dir_all(appdir.join("usr/share/wallpaper-console")).unwrap();
        std::fs::create_dir_all(appdir.join("apprun-hooks")).unwrap();
        std::fs::write(&gui, b"synthetic gui binary").unwrap();
        std::fs::write(appdir.join("usr/lib/libwebkit2gtk-4.1.so.0"), b"bundled").unwrap();
        std::fs::write(appdir.join("usr/share/wallpaper-console/keep"), b"resource").unwrap();
        std::fs::write(appdir.join("AppRun.wrapped"), b"bundled launcher").unwrap();
        std::fs::write(
            appdir.join("apprun-hooks/linuxdeploy-plugin-gtk.sh"),
            b"hook",
        )
        .unwrap();

        prepare_appimage_appdir(&appdir).unwrap();

        let app_run = appdir.join("AppRun");
        let launcher = std::fs::read_to_string(&app_run).unwrap();
        assert!(launcher.starts_with("#!/bin/sh\n"));
        assert!(launcher.contains("WCR_WEBKIT_DISABLE_DMABUF_RENDERER"));
        assert!(launcher.contains("WEBKIT_DISABLE_DMABUF_RENDERER=1"));
        assert!(launcher.contains(r#"exec "$APPDIR/usr/bin/wallpaper-console-tauri" "$@""#));
        assert!(!launcher.contains("AppRun.wrapped"));
        assert!(!launcher.contains("GDK_BACKEND"));
        assert!(launcher.contains("/usr/lib"));
        assert!(launcher.contains("LD_LIBRARY_PATH"));
        assert!(launcher.contains("host_webkit_available"));
        assert!(launcher.contains("libwebkit2gtk-4.1.so.0"));
        assert_ne!(
            std::fs::metadata(&app_run).unwrap().permissions().mode() & 0o111,
            0
        );
        assert!(appdir.join("usr/lib/libwebkit2gtk-4.1.so.0").is_file());
        assert!(!appdir.join("AppRun.wrapped").exists());
        assert!(!appdir.join("apprun-hooks").exists());
        assert!(appdir.join("usr/share/wallpaper-console/keep").is_file());

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_local_only_and_unknown_suites() {
        for suite in ["drift", "perf", "docs"] {
            let err = parse_verify_args(&args(&["verify", suite])).unwrap_err();
            assert!(
                err.contains("unknown verify suite"),
                "suite={suite} err={err}"
            );
        }
    }

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
            "releases/download/appimage-toolchain-v1",
            "linuxdeploy-plugin-appimage.AppImage",
            "appimage-runtime-x86_64",
            "linuxdeploy-plugin-appimage-verified.AppImage",
            "APPIMAGE_PINNED_RUNTIME",
            "immutable mirror provenance",
            "--runtime-file",
            "candidate-second.AppImage",
            "cmp \"${candidate}\" \"${candidate_second}\"",
            "git cat-file -t",
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
            "(cd dist && sha256sum -c SHA256SUMS)",
            "uses: actions/checkout@11bd71901bbe5b1630ceea73d27597364c9af683",
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
