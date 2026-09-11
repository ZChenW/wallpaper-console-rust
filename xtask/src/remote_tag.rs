//! Verify an annotated release tag freshly fetched from origin.

use std::path::Path;
use std::process::Command;

use crate::is_lower_hex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ReleaseVerifyRemoteTagArgs {
    pub(super) tag: String,
    pub(super) expected_commit: String,
    pub(super) expected_tag_object: Option<String>,
}

fn git_stdout(repository: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(repository)
        .output()
        .map_err(|error| format!("failed to run git {args:?}: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn is_release_tag(tag: &str) -> bool {
    let Some(version) = tag.strip_prefix('v') else {
        return false;
    };
    let (base, rc) = match version.split_once("-rc.") {
        Some((base, rc)) if !rc.contains("-rc.") => (base, Some(rc)),
        Some(_) => return false,
        None => (version, None),
    };
    let mut components = base.split('.');
    let numeric = |component: &str| {
        !component.is_empty() && component.bytes().all(|byte| byte.is_ascii_digit())
    };
    numeric(components.next().unwrap_or_default())
        && numeric(components.next().unwrap_or_default())
        && numeric(components.next().unwrap_or_default())
        && components.next().is_none()
        && rc.is_none_or(numeric)
}

pub(super) fn verify_remote_release_tag(
    repository: &Path,
    args: &ReleaseVerifyRemoteTagArgs,
) -> Result<String, String> {
    if !is_release_tag(&args.tag) {
        return Err(format!("invalid release tag: {}", args.tag));
    }
    if !is_lower_hex(&args.expected_commit, 40) {
        return Err("expected commit must be a full lowercase SHA-1".to_string());
    }
    if args
        .expected_tag_object
        .as_deref()
        .is_some_and(|object| !is_lower_hex(object, 40))
    {
        return Err("expected tag object must be a full lowercase SHA-1".to_string());
    }

    let tag_ref = format!("refs/tags/{}", args.tag);
    let verified_ref = format!("refs/tags/{}.release-verified", args.tag);
    let _ = Command::new("git")
        .args(["update-ref", "-d", &verified_ref])
        .current_dir(repository)
        .status();
    let fetch_spec = format!("{tag_ref}:{verified_ref}");
    git_stdout(repository, &["fetch", "--force", "origin", &fetch_spec])?;
    if git_stdout(repository, &["cat-file", "-t", &verified_ref])? != "tag" {
        return Err(format!("release tag must be annotated: {}", args.tag));
    }
    let actual_object = git_stdout(repository, &["rev-parse", "--verify", &verified_ref])?;
    let peeled_ref = format!("{verified_ref}^{{commit}}");
    let actual_commit = git_stdout(repository, &["rev-parse", "--verify", &peeled_ref])?;
    if actual_commit != args.expected_commit {
        return Err(format!(
            "tag {} resolves to {actual_commit}, expected {}",
            args.tag, args.expected_commit
        ));
    }
    if let Some(expected_object) = &args.expected_tag_object {
        if &actual_object != expected_object {
            return Err(format!(
                "tag {} object changed from {expected_object} to {actual_object}",
                args.tag
            ));
        }
    }
    Ok(actual_object)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_tag_verifier_rejects_lightweight_wrong_and_moved_tags() {
        fn git(dir: &Path, args: &[&str]) -> String {
            let output = Command::new("git")
                .args(args)
                .current_dir(dir)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        }
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("wcr-tag-test-{}-{unique}", std::process::id()));
        let remote = root.join("remote.git");
        let source = root.join("source");
        std::fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "--bare", remote.to_str().unwrap()]);
        git(&root, &["init", source.to_str().unwrap()]);
        git(&source, &["config", "user.name", "Release Test"]);
        git(
            &source,
            &["config", "user.email", "release@example.invalid"],
        );
        std::fs::write(source.join("payload"), "first\n").unwrap();
        git(&source, &["add", "payload"]);
        git(&source, &["commit", "-m", "first"]);
        let first = git(&source, &["rev-parse", "HEAD"]);
        git(
            &source,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git(
            &source,
            &["tag", "-a", "v9.9.9", "-m", "stable release", &first],
        );
        git(&source, &["push", "origin", "refs/tags/v9.9.9"]);
        verify_remote_release_tag(
            &source,
            &ReleaseVerifyRemoteTagArgs {
                tag: "v9.9.9".into(),
                expected_commit: first.clone(),
                expected_tag_object: None,
            },
        )
        .expect("annotated stable release tag must verify");
        git(
            &source,
            &["tag", "-a", "v9.9.9-rc.9", "-m", "release", &first],
        );
        git(&source, &["push", "origin", "refs/tags/v9.9.9-rc.9"]);
        git(&source, &["tag", "-f", "v9.9.9-rc.9", &first]);
        let base = ReleaseVerifyRemoteTagArgs {
            tag: "v9.9.9-rc.9".into(),
            expected_commit: first.clone(),
            expected_tag_object: None,
        };
        let original = verify_remote_release_tag(&source, &base).unwrap();

        std::fs::write(source.join("payload"), "second\n").unwrap();
        git(&source, &["commit", "-am", "second"]);
        let second = git(&source, &["rev-parse", "HEAD"]);
        assert!(verify_remote_release_tag(
            &source,
            &ReleaseVerifyRemoteTagArgs {
                expected_commit: second.clone(),
                ..base.clone()
            }
        )
        .is_err());
        git(&source, &["tag", "v9.9.9-rc.8", &first]);
        git(&source, &["push", "origin", "refs/tags/v9.9.9-rc.8"]);
        assert!(verify_remote_release_tag(
            &source,
            &ReleaseVerifyRemoteTagArgs {
                tag: "v9.9.9-rc.8".into(),
                expected_commit: first,
                expected_tag_object: None,
            }
        )
        .is_err());
        git(
            &source,
            &["tag", "-f", "-a", "v9.9.9-rc.9", "-m", "moved", &second],
        );
        git(
            &source,
            &["push", "--force", "origin", "refs/tags/v9.9.9-rc.9"],
        );
        assert!(verify_remote_release_tag(
            &source,
            &ReleaseVerifyRemoteTagArgs {
                tag: "v9.9.9-rc.9".into(),
                expected_commit: second,
                expected_tag_object: Some(original),
            }
        )
        .is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
