//! Deep module for persisted wallpaper-source operations.
//!
//! Presentation adapters provide progress/cancellation and user-facing copy.
//! This module owns source lookup, durable intent changes, targeted refresh,
//! removal coordination, and structured partial outcomes.

use std::fmt;

use wc_core::error::WcError;
use wc_scan::{ScanControl, SourceScanEvent};
use wc_storage::{SourceRecord, StorageApi};

use crate::library_refresh_round::{
    run_library_refresh_round, LegacyProjectionStatus, LibraryRefreshError,
    LibraryRefreshRoundReport, RefreshIntent, RefreshSelection,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SavedSourceChange {
    None,
    Added { created: bool },
    Recursive { recursive: bool },
}

impl SavedSourceChange {
    pub fn persisted(self) -> bool {
        !matches!(self, Self::None)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRefreshOutcome {
    pub source: SourceRecord,
    pub saved_change: SavedSourceChange,
    pub round: LibraryRefreshRoundReport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRemoveOutcome {
    pub removed: Option<SourceRecord>,
    pub library_rows: usize,
    pub projection: Option<LegacyProjectionStatus>,
}

#[derive(Debug)]
pub enum SourceManagementError {
    Storage(WcError),
    Refresh {
        source: SourceRecord,
        saved_change: SavedSourceChange,
        error: Box<LibraryRefreshError>,
    },
    Coordination(String),
}

impl SourceManagementError {
    pub fn saved_change(&self) -> SavedSourceChange {
        match self {
            Self::Refresh { saved_change, .. } => *saved_change,
            Self::Storage(_) | Self::Coordination(_) => SavedSourceChange::None,
        }
    }

    pub fn source(&self) -> Option<&SourceRecord> {
        match self {
            Self::Refresh { source, .. } => Some(source),
            Self::Storage(_) | Self::Coordination(_) => None,
        }
    }
}

impl fmt::Display for SourceManagementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Storage(error) => write!(formatter, "{error}"),
            Self::Refresh { error, .. } => write!(formatter, "{error}"),
            Self::Coordination(message) => write!(formatter, "{message}"),
        }
    }
}

impl std::error::Error for SourceManagementError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Storage(error) => Some(error),
            Self::Refresh { error, .. } => Some(error.as_ref()),
            Self::Coordination(_) => None,
        }
    }
}

impl From<WcError> for SourceManagementError {
    fn from(error: WcError) -> Self {
        Self::Storage(error)
    }
}

pub fn list_sources(storage: &StorageApi) -> Result<Vec<SourceRecord>, SourceManagementError> {
    storage.source_records().map_err(Into::into)
}

pub fn source_by_id(
    storage: &StorageApi,
    source_id: i64,
) -> Result<SourceRecord, SourceManagementError> {
    storage.source_get(source_id).map_err(Into::into)
}

pub fn rename_source(
    storage: &StorageApi,
    source_id: i64,
    display_name: &str,
) -> Result<SourceRecord, SourceManagementError> {
    storage
        .source_rename(source_id, display_name)
        .map_err(Into::into)
}

fn refresh_saved_source<F>(
    storage: &StorageApi,
    source: SourceRecord,
    saved_change: SavedSourceChange,
    callback: F,
) -> Result<SourceRefreshOutcome, SourceManagementError>
where
    F: FnMut(&SourceRecord, &SourceScanEvent) -> ScanControl,
{
    let round = run_library_refresh_round(
        storage,
        RefreshSelection::Sources(vec![source.id]),
        RefreshIntent::Manual,
        callback,
    )
    .map_err(|error| SourceManagementError::Refresh {
        source: source.clone(),
        saved_change,
        error: Box::new(error),
    })?;
    let source = storage.source_get(source.id).unwrap_or(source);
    Ok(SourceRefreshOutcome {
        source,
        saved_change,
        round,
    })
}

pub fn add_source<F>(
    storage: &StorageApi,
    path: &str,
    callback: F,
) -> Result<SourceRefreshOutcome, SourceManagementError>
where
    F: FnMut(&SourceRecord, &SourceScanEvent) -> ScanControl,
{
    let (source, created) = save_source_intent(storage, path)?;
    refresh_saved_source(
        storage,
        source,
        SavedSourceChange::Added { created },
        callback,
    )
}

pub(crate) fn save_source_intent(
    storage: &StorageApi,
    path: &str,
) -> Result<(SourceRecord, bool), SourceManagementError> {
    storage.source_create_with_status(path).map_err(Into::into)
}

pub fn set_source_recursive<F>(
    storage: &StorageApi,
    source_id: i64,
    recursive: bool,
    callback: F,
) -> Result<SourceRefreshOutcome, SourceManagementError>
where
    F: FnMut(&SourceRecord, &SourceScanEvent) -> ScanControl,
{
    let source = storage.source_set_recursive(source_id, recursive)?;
    refresh_saved_source(
        storage,
        source,
        SavedSourceChange::Recursive { recursive },
        callback,
    )
}

pub fn refresh_source<F>(
    storage: &StorageApi,
    source_id: i64,
    callback: F,
) -> Result<SourceRefreshOutcome, SourceManagementError>
where
    F: FnMut(&SourceRecord, &SourceScanEvent) -> ScanControl,
{
    let source = source_by_id(storage, source_id)?;
    refresh_saved_source(storage, source, SavedSourceChange::None, callback)
}

pub fn remove_source_by_id(
    storage: &StorageApi,
    source_id: i64,
) -> Result<SourceRemoveOutcome, SourceManagementError> {
    let _source_lock = crate::library_rescan::acquire_source_scan_lock(
        storage,
        source_id,
        crate::library_rescan::ScanLockIntent::Manual,
    )
    .map_err(|error| SourceManagementError::Coordination(error.to_string()))?
    .ok_or_else(|| {
        SourceManagementError::Coordination(format!("source {source_id} is already being scanned"))
    })?;
    crate::library_rescan::establish_library_dirty_marker(storage)
        .map_err(WcError::Io)
        .map_err(SourceManagementError::Storage)?;
    let removed = storage.source_remove_by_id(source_id)?;
    let library_rows = wc_storage::sqlite::source_backed_library_count(&storage.cd)?;
    let projection = match crate::library_rescan::write_legacy_tsv_snapshot(storage) {
        Ok(rows) => LegacyProjectionStatus::Published { rows },
        Err(error) => LegacyProjectionStatus::Degraded {
            message: error.to_string(),
        },
    };
    Ok(SourceRemoveOutcome {
        removed: Some(removed),
        library_rows,
        projection: Some(projection),
    })
}

pub fn remove_source_by_path(
    storage: &StorageApi,
    path: &str,
) -> Result<SourceRemoveOutcome, SourceManagementError> {
    let target = wc_scan::normalize_source_path(path);
    let source = list_sources(storage)?
        .into_iter()
        .find(|source| wc_scan::normalize_source_path(&source.path) == target);
    let Some(source) = source else {
        let library_rows = wc_storage::sqlite::source_backed_library_count(&storage.cd)?;
        return Ok(SourceRemoveOutcome {
            removed: None,
            library_rows,
            projection: None,
        });
    };
    remove_source_by_id(storage, source.id)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use wc_core::config::ConfigDir;

    use super::*;

    fn storage() -> (tempfile::TempDir, StorageApi) {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        (tmp, StorageApi::new(cd))
    }

    #[test]
    fn offline_add_keeps_saved_source_intent() {
        let (tmp, storage) = storage();
        let missing = tmp.path().join("offline");

        let outcome = add_source(&storage, &missing.to_string_lossy(), |_, _| {
            ScanControl::Continue
        })
        .unwrap();

        assert!(matches!(
            outcome.saved_change,
            SavedSourceChange::Added { created: true }
        ));
        assert_eq!(outcome.round.refresh.offline_sources, 1);
        assert_eq!(list_sources(&storage).unwrap(), vec![outcome.source]);
    }

    #[test]
    fn cancelled_recursive_change_stays_saved_and_preserves_snapshot() {
        let (tmp, storage) = storage();
        let root = tmp.path().join("walls");
        fs::create_dir_all(root.join("nested")).unwrap();
        let old = root.join("nested/old.jpg");
        fs::write(&old, b"old").unwrap();
        let initial = add_source(&storage, &root.to_string_lossy(), |_, _| {
            ScanControl::Continue
        })
        .unwrap();

        let error = set_source_recursive(&storage, initial.source.id, false, |_, event| {
            if matches!(event, SourceScanEvent::SourceStarted { .. }) {
                ScanControl::Cancel
            } else {
                ScanControl::Continue
            }
        })
        .unwrap_err();

        assert_eq!(
            error.saved_change(),
            SavedSourceChange::Recursive { recursive: false }
        );
        assert!(!source_by_id(&storage, initial.source.id).unwrap().recursive);
        let paths = wc_storage::sqlite::source_backed_library_snapshot(&storage.cd)
            .unwrap()
            .1;
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].path.to_string(), old.to_string_lossy());
    }

    #[test]
    fn removal_withdraws_membership_but_preserves_shared_metadata() {
        let (tmp, storage) = storage();
        let parent = tmp.path().join("walls");
        let nested = parent.join("nested");
        fs::create_dir_all(&nested).unwrap();
        let wallpaper = nested.join("shared.jpg");
        fs::write(&wallpaper, b"shared").unwrap();
        let parent = add_source(&storage, &parent.to_string_lossy(), |_, _| {
            ScanControl::Continue
        })
        .unwrap();
        let nested = add_source(&storage, &nested.to_string_lossy(), |_, _| {
            ScanControl::Continue
        })
        .unwrap();

        let removed = remove_source_by_id(&storage, parent.source.id).unwrap();

        assert_eq!(list_sources(&storage).unwrap(), vec![nested.source]);
        let connection = wc_storage::sqlite::open_runtime_connection(&storage.cd).unwrap();
        let counts = connection
            .query_row(
                "SELECT
                    (SELECT COUNT(*) FROM wallpapers),
                    (SELECT COUNT(*) FROM wallpaper_sources)",
                [],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )
            .unwrap();
        assert_eq!(counts, (1, 1));
        assert_eq!(
            removed.projection,
            Some(LegacyProjectionStatus::Published { rows: 1 })
        );
        assert_eq!(
            fs::read_to_string(storage.cd.library_tsv_path())
                .unwrap()
                .lines()
                .count(),
            1
        );
    }
}
