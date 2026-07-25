//! Shared source-list maintenance helpers for CLI and GUI adapters.

use std::path::{Path, PathBuf};

#[cfg(test)]
use wc_config::ConfigDirExt;
use wc_core::error::WcError;
use wc_scan::{ScanControl, SourceScanEvent};
use wc_storage::{SourceRecord, StorageApi};

use crate::library_rescan::{run_library_rescan, LibraryRescanError, LibraryRescanReport};

/// Paths that fail the shared "valid source" check (`is_dir`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ValidateSourcesReport {
    pub sources: Vec<String>,
    pub missing: Vec<String>,
}

/// Paths successfully removed because they were not directories.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RemoveMissingSourcesReport {
    pub removed: Vec<String>,
}

/// Steam workshop discovery + optional full library rescan after adds.
#[derive(Debug)]
pub struct SteamWorkshopScanReport {
    pub roots: Vec<PathBuf>,
    pub added_paths: Vec<String>,
    pub rescan: Option<LibraryRescanReport>,
}

/// Errors from Steam workshop scan when indexing is requested.
#[derive(Debug)]
pub enum SteamWorkshopScanError {
    Storage(WcError),
    Rescan(LibraryRescanError),
}

impl std::fmt::Display for SteamWorkshopScanError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Storage(error) => write!(formatter, "{error}"),
            Self::Rescan(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for SteamWorkshopScanError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Storage(error) => Some(error),
            Self::Rescan(error) => Some(error),
        }
    }
}

/// List configured sources and classify each with `Path::is_dir` (stricter than `exists`).
pub fn validate_sources(storage: &StorageApi) -> Result<ValidateSourcesReport, WcError> {
    let sources = crate::source_management::list_sources(storage)
        .map_err(|error| WcError::Other(error.to_string()))?
        .into_iter()
        .map(|source| source.path)
        .collect::<Vec<_>>();
    let missing = sources
        .iter()
        .filter(|path| !Path::new(path).is_dir())
        .cloned()
        .collect();
    Ok(ValidateSourcesReport { sources, missing })
}

/// Remove every configured source path that is not a directory.
pub fn remove_missing_sources(storage: &StorageApi) -> Result<RemoveMissingSourcesReport, WcError> {
    let sources = crate::source_management::list_sources(storage)
        .map_err(|error| WcError::Other(error.to_string()))?;
    let mut removed = Vec::new();
    for source in sources {
        if Path::new(&source.path).is_dir() {
            continue;
        }
        crate::source_management::remove_source_by_id(storage, source.id)
            .map_err(|error| WcError::Other(error.to_string()))?;
        removed.push(source.path);
    }
    Ok(RemoveMissingSourcesReport { removed })
}

/// Discover Wallpaper Engine workshop roots under `home`, add new sources, optionally rescan.
///
/// `index_after_add`: GUI passes `true` (full rescan + TSV); CLI passes `false` (add only).
pub fn scan_steam_workshop<F>(
    storage: &StorageApi,
    home: &Path,
    index_after_add: bool,
    mut callback: F,
) -> Result<SteamWorkshopScanReport, SteamWorkshopScanError>
where
    F: FnMut(&SourceRecord, &SourceScanEvent) -> ScanControl,
{
    let roots = wc_scan::discover_steam_workshop_roots(home);
    let mut added_paths = Vec::new();
    for root in &roots {
        let canonical = root.to_string_lossy().to_string();
        let (_, created) = crate::source_management::save_source_intent(storage, &canonical)
            .map_err(|error| SteamWorkshopScanError::Storage(WcError::Other(error.to_string())))?;
        if created {
            added_paths.push(canonical);
        }
    }

    if !index_after_add || roots.is_empty() {
        drop(callback);
        return Ok(SteamWorkshopScanReport {
            roots,
            added_paths,
            rescan: None,
        });
    }

    let rescan = run_library_rescan(storage, |source, event| callback(source, event))
        .map_err(SteamWorkshopScanError::Rescan)?;
    Ok(SteamWorkshopScanReport {
        roots,
        added_paths,
        rescan: Some(rescan),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wc_core::config::ConfigDir;

    fn storage() -> (tempfile::TempDir, StorageApi) {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("config"),
        };
        cd.init().unwrap();
        wc_config::write_config_value(&cd.path, "storage_backend", "sqlite").unwrap();
        (tmp, StorageApi::try_new(cd).unwrap())
    }

    #[test]
    fn validate_sources_marks_non_directories_missing() {
        let (tmp, storage) = storage();
        let good = tmp.path().join("walls");
        std::fs::create_dir(&good).unwrap();
        let file = tmp.path().join("not-a-dir");
        std::fs::write(&file, b"x").unwrap();
        storage.sources_add(&good.to_string_lossy()).unwrap();
        storage.sources_add(&file.to_string_lossy()).unwrap();

        let report = validate_sources(&storage).unwrap();
        assert_eq!(report.sources.len(), 2);
        assert_eq!(report.missing, vec![file.to_string_lossy().to_string()]);
    }

    #[test]
    fn remove_missing_sources_keeps_directories() {
        let (tmp, storage) = storage();
        let good = tmp.path().join("walls");
        std::fs::create_dir(&good).unwrap();
        let missing = tmp.path().join("gone");
        storage.sources_add(&good.to_string_lossy()).unwrap();
        storage.sources_add(&missing.to_string_lossy()).unwrap();

        let report = remove_missing_sources(&storage).unwrap();
        assert_eq!(report.removed, vec![missing.to_string_lossy().to_string()]);
        assert_eq!(
            storage.sources_list().unwrap(),
            vec![good.to_string_lossy().to_string()]
        );
    }

    #[test]
    fn steam_workshop_without_index_does_not_rescan() {
        let (_tmp, storage) = storage();
        let home = tempfile::tempdir().unwrap();
        let report =
            scan_steam_workshop(&storage, home.path(), false, |_, _| ScanControl::Continue)
                .unwrap();
        assert!(report.roots.is_empty());
        assert!(report.rescan.is_none());
    }
}
