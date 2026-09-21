//! Cheap identity checks around prepare and immediately before destructive Stop.
//! Project walks are bounded and follow dependencies inside the project tree,
//! including symlinks; cycles and excessive trees fail closed.
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};
use wc_core::error::WcError;

const MAX_ENTRIES: usize = 16_384;
const MAX_DEPTH: usize = 32;
const WALK_BUDGET: Duration = Duration::from_secs(2);

#[derive(Debug, PartialEq, Eq)]
struct Identity {
    length: u64,
    modified: SystemTime,
    directory: bool,
    #[cfg(unix)]
    inode: (u64, u64, i64, i64),
}

impl Identity {
    fn read(path: &Path) -> Result<Self, WcError> {
        let metadata = fs::metadata(path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                WcError::NotRegularFile(path.into())
            } else {
                WcError::Io(error)
            }
        })?;
        if !metadata.is_dir() && !metadata.is_file() {
            return Err(WcError::NotRegularFile(path.into()));
        }
        Ok(Self {
            length: metadata.len(),
            modified: metadata.modified()?,
            directory: metadata.is_dir(),
            #[cfg(unix)]
            inode: {
                use std::os::unix::fs::MetadataExt;
                (
                    metadata.dev(),
                    metadata.ino(),
                    metadata.ctime(),
                    metadata.ctime_nsec(),
                )
            },
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct MediaStamp {
    root: PathBuf,
    project: bool,
    entries: BTreeMap<PathBuf, Identity>,
}

impl MediaStamp {
    pub(crate) fn file(path: &str) -> Result<Self, WcError> {
        Self::capture(Path::new(path), false)
    }

    pub(crate) fn project(path: &str) -> Result<Self, WcError> {
        let path = Path::new(path);
        let root = if path.is_dir() {
            path
        } else {
            path.parent()
                .ok_or_else(|| WcError::Other("invalid project path".into()))?
        };
        Self::capture(root, true)
    }

    fn capture(root: &Path, project: bool) -> Result<Self, WcError> {
        let mut entries = BTreeMap::new();
        let deadline = Instant::now() + WALK_BUDGET;
        let mut pending = vec![(root.to_path_buf(), 0, Vec::<PathBuf>::new())];
        while let Some((path, depth, mut ancestors)) = pending.pop() {
            if Instant::now() >= deadline || depth > MAX_DEPTH || entries.len() >= MAX_ENTRIES {
                return Err(WcError::Other(
                    "media preflight snapshot exceeds entry/depth/time budget".into(),
                ));
            }
            let identity = Identity::read(&path)?;
            if !project && (identity.directory || identity.length == 0) {
                return Err(WcError::NotRegularFile(path));
            }
            if identity.directory {
                let canonical = path.canonicalize()?;
                if ancestors.contains(&canonical) {
                    return Err(WcError::Other(
                        "cyclic project dependency during media preflight".into(),
                    ));
                }
                ancestors.push(canonical);
                for child in fs::read_dir(&path)? {
                    if entries.len() + pending.len() >= MAX_ENTRIES || Instant::now() >= deadline {
                        return Err(WcError::Other(
                            "media preflight snapshot exceeds entry/time budget".into(),
                        ));
                    }
                    pending.push((child?.path(), depth + 1, ancestors.clone()));
                }
            }
            entries.insert(path, identity);
        }
        Ok(Self {
            root: root.to_path_buf(),
            project,
            entries,
        })
    }

    pub(crate) fn verify(&self) -> Result<(), WcError> {
        if Self::capture(&self.root, self.project)? != *self {
            return Err(WcError::Other(format!(
                "media changed after preflight: {}; retry apply",
                self.root.display()
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn videos_have_no_image_size_limit_and_deleted_files_fail() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("large.mp4");
        fs::File::create(&path)
            .unwrap()
            .set_len(256 * 1024 * 1024)
            .unwrap();
        let stamp = MediaStamp::file(path.to_str().unwrap()).unwrap();
        stamp.verify().unwrap();
        fs::remove_file(path).unwrap();
        assert!(stamp.verify().is_err());
    }
    #[test]
    fn nested_project_changes_and_additions_fail() {
        let tmp = tempfile::tempdir().unwrap();
        let folder = tmp.path().join("materials");
        fs::create_dir(&folder).unwrap();
        let asset = folder.join("texture.tex");
        fs::write(&asset, "old").unwrap();
        let stamp = MediaStamp::project(tmp.path().to_str().unwrap()).unwrap();
        fs::write(&asset, "new contents").unwrap();
        assert!(stamp.verify().is_err());
        let stamp = MediaStamp::project(tmp.path().to_str().unwrap()).unwrap();
        fs::write(folder.join("new.tex"), "asset").unwrap();
        assert!(stamp.verify().is_err());
    }

    #[test]
    fn excessively_deep_projects_fail_closed() {
        let tmp = tempfile::tempdir().unwrap();
        let mut path = tmp.path().to_path_buf();
        for _ in 0..MAX_DEPTH + 1 {
            path.push("nested");
            fs::create_dir(&path).unwrap();
        }
        assert!(MediaStamp::project(tmp.path().to_str().unwrap()).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn project_symlink_dependencies_are_checked_and_cycles_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("project");
        fs::create_dir(&project).unwrap();
        let asset = tmp.path().join("shared.tex");
        fs::write(&asset, "original").unwrap();
        std::os::unix::fs::symlink(&asset, project.join("linked.tex")).unwrap();
        let stamp = MediaStamp::project(project.to_str().unwrap()).unwrap();
        fs::write(asset, "replacement").unwrap();
        assert!(stamp.verify().is_err());
        std::os::unix::fs::symlink(&project, project.join("cycle")).unwrap();
        assert!(MediaStamp::project(project.to_str().unwrap()).is_err());
    }
}
