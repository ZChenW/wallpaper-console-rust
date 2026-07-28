//! Shared source-list maintenance helpers for CLI and GUI adapters.

use std::path::{Path, PathBuf};

#[cfg(test)]
use wc_config::ConfigDirExt;
use wc_core::error::WcError;
use wc_scan::{ScanControl, SourceScanEvent};
use wc_storage::{SourceKind, SourceRecord, StorageApi};

use crate::library_refresh_round::{
    run_library_refresh_round, LibraryRefreshError, LibraryRefreshRoundReport, RefreshIntent,
    RefreshSelection,
};

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

/// Steam workshop discovery + optional targeted refresh after adds.
#[derive(Debug)]
pub struct SteamWorkshopScanReport {
    pub roots: Vec<PathBuf>,
    pub added_paths: Vec<String>,
    pub refresh: Option<LibraryRefreshRoundReport>,
}

/// Errors from Steam workshop scan when indexing is requested.
#[derive(Debug)]
pub enum SteamWorkshopScanError {
    Storage(WcError),
    Refresh(LibraryRefreshError),
}

impl std::fmt::Display for SteamWorkshopScanError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Storage(error) => write!(formatter, "{error}"),
            Self::Refresh(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for SteamWorkshopScanError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Storage(error) => Some(error),
            Self::Refresh(error) => Some(error),
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

/// Discover Wallpaper Engine workshop roots under `home`, add new sources, optionally refresh.
///
/// `index_after_add`: GUI passes `true` (refresh all configured WE sources + TSV);
/// CLI passes `false` (add only).
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

    if !index_after_add {
        drop(callback);
        return Ok(SteamWorkshopScanReport {
            roots,
            added_paths,
            refresh: None,
        });
    }

    let source_ids = storage
        .source_records()
        .map_err(SteamWorkshopScanError::Storage)?
        .into_iter()
        .filter(|source| source.kind == SourceKind::WallpaperEngineWorkshop)
        .map(|source| source.id)
        .collect::<Vec<_>>();
    if source_ids.is_empty() {
        drop(callback);
        return Ok(SteamWorkshopScanReport {
            roots,
            added_paths,
            refresh: None,
        });
    }

    let refresh = run_library_refresh_round(
        storage,
        RefreshSelection::Sources(source_ids),
        RefreshIntent::Manual,
        |source, event| callback(source, event),
    )
    .map_err(SteamWorkshopScanError::Refresh)?;
    Ok(SteamWorkshopScanReport {
        roots,
        added_paths,
        refresh: Some(refresh),
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
        assert!(report.refresh.is_none());
    }

    #[test]
    fn steam_workshop_index_refreshes_only_wallpaper_engine_sources() {
        let (tmp, storage) = storage();
        let ordinary = tmp.path().join("ordinary");
        std::fs::create_dir_all(&ordinary).unwrap();
        let indexed_ordinary = ordinary.join("indexed.jpg");
        let unindexed_ordinary = ordinary.join("not-yet-indexed.jpg");
        std::fs::write(&indexed_ordinary, b"indexed").unwrap();
        let ordinary_source = storage.source_create(&ordinary.to_string_lossy()).unwrap();
        crate::source_management::refresh_source(&storage, ordinary_source.id, |_, _| {
            ScanControl::Continue
        })
        .unwrap();
        std::fs::write(&unindexed_ordinary, b"new").unwrap();

        let home = tmp.path().join("home");
        let project = home.join(".local/share/Steam/steamapps/workshop/content/431960/123456");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("project.json"),
            r#"{"type":"scene","title":"Workshop scene"}"#,
        )
        .unwrap();

        let report =
            scan_steam_workshop(&storage, &home, true, |_, _| ScanControl::Continue).unwrap();
        assert!(report.refresh.is_some());
        let (_, entries) = wc_storage::sqlite::source_backed_library_snapshot(&storage.cd).unwrap();
        let paths = entries
            .into_iter()
            .map(|entry| entry.path.to_string())
            .collect::<std::collections::HashSet<_>>();

        assert!(paths.contains(&indexed_ordinary.to_string_lossy().into_owned()));
        assert!(paths.contains(
            &std::fs::canonicalize(project)
                .unwrap()
                .to_string_lossy()
                .into_owned()
        ));
        assert!(
            !paths.contains(&unindexed_ordinary.to_string_lossy().into_owned()),
            "Wallpaper Engine scan must not refresh unrelated directory sources"
        );
    }

    #[test]
    fn steam_workshop_index_refreshes_saved_source_that_is_now_offline() {
        let (tmp, storage) = storage();
        let home = tmp.path().join("home");
        let workshop = home.join(".local/share/Steam/steamapps/workshop/content/431960");
        let project = workshop.join("123456");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("project.json"),
            r#"{"type":"scene","title":"Workshop scene"}"#,
        )
        .unwrap();

        let (source, _) =
            crate::source_management::save_source_intent(&storage, &workshop.to_string_lossy())
                .unwrap();
        crate::source_management::refresh_source(&storage, source.id, |_, _| ScanControl::Continue)
            .unwrap();
        std::fs::remove_dir_all(&workshop).unwrap();

        let report =
            scan_steam_workshop(&storage, &home, true, |_, _| ScanControl::Continue).unwrap();

        assert!(report.roots.is_empty());
        let refresh = report.refresh.expect("saved WE source should be refreshed");
        assert_eq!(refresh.refresh.offline_sources, 1);
        let (_, entries) = wc_storage::sqlite::source_backed_library_snapshot(&storage.cd).unwrap();
        assert_eq!(entries.len(), 1, "the last complete snapshot must survive");
    }
}
