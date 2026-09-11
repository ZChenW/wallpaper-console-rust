//! CLI grammar and help text; execution belongs to the command modules.

use std::path::PathBuf;

use crate::appimage::{ReleasePrepareAppImageArgs, ReleaseVerifyAppImageRuntimeArgs};
use crate::package::{validate_release_version, ReleasePackageLinuxArgs};
use crate::remote_tag::ReleaseVerifyRemoteTagArgs;
use crate::verify::{VerifyArgs, VerifySuite};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum XtaskCommand {
    Help,
    Verify(VerifyArgs),
    ReleasePackageLinux(ReleasePackageLinuxArgs),
    ReleasePrepareAppImage(ReleasePrepareAppImageArgs),
    ReleaseVerifyRemoteTag(ReleaseVerifyRemoteTagArgs),
    ReleaseVerifyAppImageRuntime(ReleaseVerifyAppImageRuntimeArgs),
}

pub(super) fn parse_xtask_command(args: &[String]) -> Result<XtaskCommand, String> {
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
    if args.get(1).map(String::as_str) == Some("verify-remote-tag") {
        let mut tag = None;
        let mut expected_commit = None;
        let mut expected_tag_object = None;
        let mut index = 2;
        while index < args.len() {
            let flag = args[index].as_str();
            let value = args
                .get(index + 1)
                .ok_or_else(|| format!("missing value for {flag}"))?;
            match flag {
                "--tag" => tag = Some(value.clone()),
                "--expected-commit" => expected_commit = Some(value.clone()),
                "--expected-tag-object" => expected_tag_object = Some(value.clone()),
                other => return Err(format!("unknown verify-remote-tag option: {other}")),
            }
            index += 2;
        }
        return Ok(XtaskCommand::ReleaseVerifyRemoteTag(
            ReleaseVerifyRemoteTagArgs {
                tag: tag.ok_or_else(|| "missing --tag".to_string())?,
                expected_commit: expected_commit
                    .ok_or_else(|| "missing --expected-commit".to_string())?,
                expected_tag_object,
            },
        ));
    }
    if args.get(1).map(String::as_str) == Some("verify-appimage-runtime") {
        let mut runtime = None;
        let mut appimage = None;
        let mut runtime_size = None;
        let mut expected_sha256 = None;
        let mut index = 2;
        while index < args.len() {
            let flag = args[index].as_str();
            let value = args
                .get(index + 1)
                .ok_or_else(|| format!("missing value for {flag}"))?;
            match flag {
                "--runtime" => runtime = Some(PathBuf::from(value)),
                "--appimage" => appimage = Some(PathBuf::from(value)),
                "--runtime-size" => {
                    runtime_size = Some(
                        value
                            .parse::<usize>()
                            .map_err(|_| "invalid --runtime-size".to_string())?,
                    )
                }
                "--sha256" => expected_sha256 = Some(value.clone()),
                other => return Err(format!("unknown verify-appimage-runtime option: {other}")),
            }
            index += 2;
        }
        return Ok(XtaskCommand::ReleaseVerifyAppImageRuntime(
            ReleaseVerifyAppImageRuntimeArgs {
                runtime: runtime.ok_or_else(|| "missing --runtime".to_string())?,
                appimage: appimage.ok_or_else(|| "missing --appimage".to_string())?,
                runtime_size: runtime_size
                    .filter(|size| *size > 0)
                    .ok_or_else(|| "missing or zero --runtime-size".to_string())?,
                expected_sha256: expected_sha256.ok_or_else(|| "missing --sha256".to_string())?,
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
    validate_release_version(&args.version)?;
    Ok(XtaskCommand::ReleasePackageLinux(args))
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

pub(super) fn usage() -> String {
    "Usage:
  cargo run -p xtask -- verify rust [--dry-run]
  cargo run -p xtask -- verify frontend [--dry-run]
  cargo run -p xtask -- verify all [--dry-run]
  cargo run -p xtask -- release package-linux --version <version> --appimage <path> --cli <path> --out <dir>
  cargo run -p xtask -- release prepare-appimage --appdir <path>
  cargo run -p xtask -- release verify-remote-tag --tag <tag> --expected-commit <sha> [--expected-tag-object <sha>]
  cargo run -p xtask -- release verify-appimage-runtime --runtime <path> --appimage <path> --runtime-size <bytes> --sha256 <digest>"
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn rejects_local_only_and_unknown_suites() {
        for suite in ["drift", "perf", "docs"] {
            let err = parse_verify_args(&args(&["verify", suite])).unwrap_err();
            assert!(
                err.contains("unknown verify suite"),
                "suite={suite} err={err}"
            );
        }
    }

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }
}
