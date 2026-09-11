//! xtask — repository verification and release commands.

mod appimage;
mod cli;
mod package;
mod remote_tag;
mod verify;

use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cli::{parse_xtask_command, usage, XtaskCommand};

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
        XtaskCommand::Verify(args) => verify::verify(&repo_root(), args),
        XtaskCommand::ReleasePackageLinux(args) => {
            package::package_linux_release(&repo_root(), &args)
        }
        XtaskCommand::ReleasePrepareAppImage(args) => {
            let root = repo_root();
            let appdir = resolve_from(&root, &args.appdir);
            appimage::prepare_appimage_appdir(&appdir)
        }
        XtaskCommand::ReleaseVerifyRemoteTag(args) => {
            let current = env::current_dir()
                .map_err(|error| format!("failed to resolve current directory: {error}"))?;
            let object = remote_tag::verify_remote_release_tag(&current, &args)?;
            println!("{object}");
            Ok(())
        }
        XtaskCommand::ReleaseVerifyAppImageRuntime(args) => {
            appimage::verify_appimage_runtime(&repo_root(), &args)
        }
    }
}

fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
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

fn resolve_from(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask must live under the repository root")
        .to_path_buf()
}
