//! Linux release assets, deterministic CLI archive, and verified checksums.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{project_version, resolve_from};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ReleasePackageLinuxArgs {
    pub(super) version: String,
    pub(super) appimage: PathBuf,
    pub(super) cli: PathBuf,
    pub(super) out_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReleaseAssetNames {
    appimage: String,
    cli_archive: String,
    checksums: &'static str,
}

pub(super) fn validate_release_version(version: &str) -> Result<(), String> {
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

    Ok(())
}

fn release_asset_names(version: &str) -> Result<ReleaseAssetNames, String> {
    validate_release_version(version)?;
    Ok(ReleaseAssetNames {
        appimage: format!("wallpaper-console_{version}_x86_64.AppImage"),
        cli_archive: format!("wallpaper-console-cli_{version}_x86_64.tar.zst"),
        checksums: "SHA256SUMS",
    })
}

pub(super) fn package_linux_release(
    repo_root: &Path,
    args: &ReleasePackageLinuxArgs,
) -> Result<(), String> {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
