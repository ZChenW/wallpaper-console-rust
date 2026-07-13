//! Shared application policy for refreshing every configured wallpaper source.
//!
//! Callers observe progress and may cancel, but only this module decides which
//! scan outcomes are authoritative enough to publish to the library.

use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;

use wc_core::error::WcError;
use wc_core::types::WallpaperEntry;
use wc_scan::{
    ScanControl, ScanFailure, ScanSourceKind, ScanStats, SourceScanEvent, SourceScanOutcome,
    SourceScanRequest,
};
use wc_storage::{SourceAvailability, SourceKind, SourceRecord, StorageApi};

/// Aggregate metadata work performed while refreshing configured sources.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RefreshMetadataStats {
    pub entries_visited: usize,
    pub candidates_found: usize,
    pub entries_indexed: usize,
    pub metadata_reused: usize,
}

impl RefreshMetadataStats {
    fn record(&mut self, stats: ScanStats) {
        self.entries_visited += stats.entries_visited;
        self.candidates_found += stats.candidates_found;
        self.entries_indexed += stats.entries_indexed;
        self.metadata_reused += stats.metadata_reused;
    }
}

/// Non-authoritative terminal state recorded for one configured source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceRefreshIssueKind {
    Offline,
    Incomplete,
}

/// Details needed to explain why one source kept its previous library data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRefreshIssue {
    pub source_id: i64,
    pub source_path: String,
    pub display_name: String,
    pub kind: SourceRefreshIssueKind,
    pub failure: ScanFailure,
}

/// Changes and scan outcomes accumulated across all configured sources.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LibraryRefreshReport {
    pub indexed: usize,
    pub wallpapers_added: usize,
    pub wallpapers_removed: usize,
    pub memberships_added: usize,
    pub memberships_removed: usize,
    pub favorites_removed: usize,
    pub removed_we_workshop_ids: Vec<String>,
    pub complete_sources: usize,
    pub offline_sources: usize,
    pub incomplete_sources: usize,
    pub issues: Vec<SourceRefreshIssue>,
    pub metadata: RefreshMetadataStats,
}

/// Refresh termination that preserves any complete sources committed earlier.
#[derive(Debug)]
pub enum LibraryRefreshError {
    Cancelled {
        current_source: Box<SourceRecord>,
        stats: ScanStats,
        report: Box<LibraryRefreshReport>,
    },
    Storage {
        current_source: Option<Box<SourceRecord>>,
        report: Box<LibraryRefreshReport>,
        error: Box<WcError>,
    },
}

impl fmt::Display for LibraryRefreshError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled { current_source, .. } => write!(
                formatter,
                "library refresh cancelled while scanning {}",
                current_source.display_name
            ),
            Self::Storage {
                current_source,
                error,
                ..
            } => match current_source {
                Some(source) => write!(
                    formatter,
                    "library refresh storage failure for {}: {error}",
                    source.display_name
                ),
                None => write!(formatter, "library refresh storage failure: {error}"),
            },
        }
    }
}

impl std::error::Error for LibraryRefreshError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Cancelled { .. } => None,
            Self::Storage { error, .. } => Some(error.as_ref()),
        }
    }
}

fn source_request(source: &SourceRecord) -> SourceScanRequest {
    SourceScanRequest {
        path: PathBuf::from(&source.path),
        kind: match source.kind {
            SourceKind::Directory => ScanSourceKind::Directory,
            SourceKind::WallpaperEngineWorkshop => ScanSourceKind::WallpaperEngineWorkshop,
        },
        recursive: source.recursive,
    }
}

fn storage_error(
    error: WcError,
    current_source: Option<&SourceRecord>,
    report: &LibraryRefreshReport,
) -> LibraryRefreshError {
    LibraryRefreshError::Storage {
        current_source: current_source.cloned().map(Box::new),
        report: Box::new(report.clone()),
        error: Box::new(error),
    }
}

fn merge_reconcile_report(
    aggregate: &mut LibraryRefreshReport,
    source: wc_storage::sqlite::SourceReconcileReport,
) {
    aggregate.indexed += source.indexed;
    aggregate.wallpapers_added += source.wallpapers_added;
    aggregate.wallpapers_removed += source.wallpapers_removed;
    aggregate.memberships_added += source.memberships_added;
    aggregate.memberships_removed += source.memberships_removed;
    aggregate.favorites_removed += source.favorites_removed;
    aggregate
        .removed_we_workshop_ids
        .extend(source.removed_we_workshop_ids);
    aggregate.removed_we_workshop_ids.sort();
    aggregate.removed_we_workshop_ids.dedup();
}

fn extend_metadata_cache(cache: &mut HashMap<String, WallpaperEntry>, entries: &[WallpaperEntry]) {
    for entry in entries {
        // Complete source scans only publish entries after canonicalizing their
        // paths, so the entry path is already the key expected by the scanner.
        cache.insert(entry.path.to_string(), entry.clone());
    }
}

fn refresh_selected_sources<F>(
    storage: &StorageApi,
    sources: Vec<SourceRecord>,
    mut callback: F,
) -> Result<LibraryRefreshReport, LibraryRefreshError>
where
    F: FnMut(&SourceRecord, &SourceScanEvent) -> ScanControl,
{
    let mut report = LibraryRefreshReport::default();
    let mut metadata_cache = wc_storage::sqlite::prior_metadata_cache_from_sqlite(&storage.cd);

    for source in sources {
        let request = source_request(&source);
        let outcome = wc_scan::scan_source_cached(&request, &metadata_cache, |event| {
            callback(&source, event)
        });
        match outcome {
            SourceScanOutcome::Complete(snapshot) => {
                report.complete_sources += 1;
                report.metadata.record(snapshot.stats());
                let reconcile = wc_storage::sqlite::reconcile_complete_source(
                    &storage.cd,
                    source.id,
                    &snapshot,
                )
                .map_err(|error| storage_error(error, Some(&source), &report))?;
                merge_reconcile_report(&mut report, reconcile);
                extend_metadata_cache(&mut metadata_cache, snapshot.entries());
            }
            SourceScanOutcome::Offline(failure) => {
                report.offline_sources += 1;
                report.metadata.record(failure.stats);
                report.issues.push(SourceRefreshIssue {
                    source_id: source.id,
                    source_path: source.path.clone(),
                    display_name: source.display_name.clone(),
                    kind: SourceRefreshIssueKind::Offline,
                    failure,
                });
                storage
                    .source_set_availability(source.id, SourceAvailability::Offline)
                    .map_err(|error| storage_error(error, Some(&source), &report))?;
            }
            SourceScanOutcome::Incomplete(failure) => {
                report.incomplete_sources += 1;
                report.metadata.record(failure.stats);
                report.issues.push(SourceRefreshIssue {
                    source_id: source.id,
                    source_path: source.path.clone(),
                    display_name: source.display_name.clone(),
                    kind: SourceRefreshIssueKind::Incomplete,
                    failure,
                });
                storage
                    .source_set_availability(source.id, SourceAvailability::Unknown)
                    .map_err(|error| storage_error(error, Some(&source), &report))?;
            }
            SourceScanOutcome::Cancelled(stats) => {
                report.metadata.record(stats);
                return Err(LibraryRefreshError::Cancelled {
                    current_source: Box::new(source),
                    stats,
                    report: Box::new(report),
                });
            }
        }
    }

    report.removed_we_workshop_ids.sort();
    report.removed_we_workshop_ids.dedup();
    Ok(report)
}

/// Refresh all named sources using one global prior-metadata cache.
///
/// Complete scans are reconciled immediately, so a later cancellation does
/// not roll back valid results from earlier sources. Offline and incomplete
/// scans only update availability and retain their previous snapshots.
pub fn refresh_library_sources<F>(
    storage: &StorageApi,
    callback: F,
) -> Result<LibraryRefreshReport, LibraryRefreshError>
where
    F: FnMut(&SourceRecord, &SourceScanEvent) -> ScanControl,
{
    let empty_report = LibraryRefreshReport::default();
    let sources = storage
        .source_records()
        .map_err(|error| storage_error(error, None, &empty_report))?;
    refresh_selected_sources(storage, sources, callback)
}

/// Refresh exactly one configured source selected by its stable database ID.
///
/// Unknown IDs are errors. Complete scans only reconcile the selected source;
/// offline, incomplete, and cancelled scans preserve its previous snapshot.
pub fn refresh_library_source<F>(
    storage: &StorageApi,
    source_id: i64,
    callback: F,
) -> Result<LibraryRefreshReport, LibraryRefreshError>
where
    F: FnMut(&SourceRecord, &SourceScanEvent) -> ScanControl,
{
    let empty_report = LibraryRefreshReport::default();
    let source = storage
        .source_records()
        .map_err(|error| storage_error(error, None, &empty_report))?
        .into_iter()
        .find(|source| source.id == source_id)
        .ok_or_else(|| {
            storage_error(
                WcError::Other(format!("source id {source_id} not found")),
                None,
                &empty_report,
            )
        })?;
    refresh_selected_sources(storage, vec![source], callback)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use wc_core::config::ConfigDir;
    use wc_scan::{ScanControl, SourceScanEvent};
    use wc_storage::{SourceAvailability, StorageApi};

    use super::{refresh_library_source, refresh_library_sources, LibraryRefreshError};

    fn storage() -> (tempfile::TempDir, StorageApi) {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        (tmp, StorageApi::new(cd))
    }

    fn add_source(storage: &StorageApi, path: &Path) -> wc_storage::SourceRecord {
        storage.source_create(&path.to_string_lossy()).unwrap()
    }

    fn library_counts(storage: &StorageApi) -> (i64, i64) {
        let conn = wc_storage::sqlite::open_runtime_connection(&storage.cd).unwrap();
        conn.query_row(
            "SELECT
                 (SELECT COUNT(*) FROM wallpapers),
                 (SELECT COUNT(*) FROM wallpaper_sources)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap()
    }

    fn library_paths(storage: &StorageApi) -> Vec<String> {
        let conn = wc_storage::sqlite::open_runtime_connection(&storage.cd).unwrap();
        let mut statement = conn
            .prepare("SELECT path FROM wallpapers ORDER BY path")
            .unwrap();
        statement
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    }

    fn refresh(storage: &StorageApi) -> super::LibraryRefreshReport {
        refresh_library_sources(storage, |_, _| ScanControl::Continue).unwrap()
    }

    #[test]
    fn complete_scan_publishes_membership_and_aggregates_metadata_stats() {
        let (tmp, storage) = storage();
        let walls = tmp.path().join("walls");
        fs::create_dir_all(&walls).unwrap();
        fs::write(walls.join("one.jpg"), b"image fixture").unwrap();
        let source = add_source(&storage, &walls);

        let first = refresh(&storage);

        assert_eq!(first.indexed, 1);
        assert_eq!(first.wallpapers_added, 1);
        assert_eq!(first.memberships_added, 1);
        assert_eq!(first.complete_sources, 1);
        assert_eq!(first.offline_sources, 0);
        assert_eq!(first.incomplete_sources, 0);
        assert_eq!(first.metadata.entries_indexed, 1);
        assert_eq!(first.metadata.metadata_reused, 0);
        assert_eq!(library_counts(&storage), (1, 1));
        assert_eq!(
            storage
                .source_records()
                .unwrap()
                .into_iter()
                .find(|record| record.id == source.id)
                .unwrap()
                .availability,
            SourceAvailability::Available
        );

        let second = refresh(&storage);
        assert_eq!(second.metadata.entries_indexed, 1);
        assert_eq!(second.metadata.metadata_reused, 1);
    }

    #[test]
    fn offline_source_preserves_published_snapshot_and_marks_offline() {
        let (tmp, storage) = storage();
        let walls = tmp.path().join("offline-walls");
        fs::create_dir_all(&walls).unwrap();
        let wallpaper = walls.join("keep.jpg");
        fs::write(&wallpaper, b"image fixture").unwrap();
        let source = add_source(&storage, &walls);
        refresh(&storage);
        fs::remove_dir_all(&walls).unwrap();

        let report = refresh(&storage);

        assert_eq!(report.offline_sources, 1);
        assert_eq!(report.issues.len(), 1);
        assert_eq!(report.issues[0].source_id, source.id);
        assert_eq!(library_counts(&storage), (1, 1));
        assert_eq!(library_paths(&storage), vec![wallpaper.to_string_lossy()]);
        assert_eq!(
            storage.source_records().unwrap()[0].availability,
            SourceAvailability::Offline
        );
    }

    #[test]
    fn incomplete_source_preserves_published_snapshot_and_marks_unknown() {
        let (tmp, storage) = storage();
        let walls = tmp.path().join("incomplete-walls");
        fs::create_dir_all(&walls).unwrap();
        let wallpaper = walls.join("keep.jpg");
        fs::write(&wallpaper, b"image fixture").unwrap();
        add_source(&storage, &walls);
        refresh(&storage);
        fs::remove_dir_all(&walls).unwrap();
        fs::write(&walls, b"not a directory").unwrap();

        let report = refresh(&storage);

        assert_eq!(report.incomplete_sources, 1);
        assert_eq!(report.issues.len(), 1);
        assert_eq!(library_counts(&storage), (1, 1));
        assert_eq!(library_paths(&storage), vec![wallpaper.to_string_lossy()]);
        assert_eq!(
            storage.source_records().unwrap()[0].availability,
            SourceAvailability::Unknown
        );
    }

    #[test]
    fn cancellation_preserves_current_snapshot_and_availability() {
        let (tmp, storage) = storage();
        let walls = tmp.path().join("cancel-walls");
        fs::create_dir_all(&walls).unwrap();
        let old = walls.join("old.jpg");
        fs::write(&old, b"old image").unwrap();
        let source = add_source(&storage, &walls);
        refresh(&storage);
        storage
            .source_set_availability(source.id, SourceAvailability::Offline)
            .unwrap();
        fs::remove_file(&old).unwrap();
        fs::write(walls.join("new.jpg"), b"new image").unwrap();

        let result = refresh_library_sources(&storage, |_, event| {
            if matches!(event, SourceScanEvent::CandidateFound { .. }) {
                ScanControl::Cancel
            } else {
                ScanControl::Continue
            }
        });

        let LibraryRefreshError::Cancelled {
            current_source,
            report,
            ..
        } = result.unwrap_err()
        else {
            panic!("expected typed cancellation");
        };
        assert_eq!(current_source.id, source.id);
        assert_eq!(report.complete_sources, 0);
        assert_eq!(library_counts(&storage), (1, 1));
        assert_eq!(library_paths(&storage), vec![old.to_string_lossy()]);
        assert_eq!(
            storage.source_records().unwrap()[0].availability,
            SourceAvailability::Offline
        );
    }

    #[test]
    fn cancellation_keeps_complete_results_committed_for_earlier_sources() {
        let (tmp, storage) = storage();
        let first_root = tmp.path().join("a-complete");
        let cancelled_root = tmp.path().join("z-cancelled");
        fs::create_dir_all(&first_root).unwrap();
        fs::create_dir_all(&cancelled_root).unwrap();
        let first_wallpaper = first_root.join("first.jpg");
        fs::write(&first_wallpaper, b"first image").unwrap();
        fs::write(cancelled_root.join("second.jpg"), b"second image").unwrap();
        let first = add_source(&storage, &first_root);
        let cancelled = add_source(&storage, &cancelled_root);

        let result = refresh_library_sources(&storage, |source, event| {
            if source.id == cancelled.id && matches!(event, SourceScanEvent::SourceStarted { .. }) {
                ScanControl::Cancel
            } else {
                ScanControl::Continue
            }
        });

        let LibraryRefreshError::Cancelled {
            current_source,
            report,
            ..
        } = result.unwrap_err()
        else {
            panic!("expected typed cancellation");
        };
        assert_eq!(current_source.id, cancelled.id);
        assert_eq!(report.complete_sources, 1);
        assert_eq!(report.wallpapers_added, 1);
        assert_eq!(report.memberships_added, 1);
        assert_eq!(library_counts(&storage), (1, 1));
        assert_eq!(
            library_paths(&storage),
            vec![first_wallpaper.to_string_lossy()]
        );
        let records = storage.source_records().unwrap();
        assert_eq!(
            records
                .iter()
                .find(|source| source.id == first.id)
                .unwrap()
                .availability,
            SourceAvailability::Available
        );
        assert_eq!(
            records
                .iter()
                .find(|source| source.id == cancelled.id)
                .unwrap()
                .availability,
            SourceAvailability::Unknown
        );
    }

    #[test]
    fn overlapping_sources_publish_one_row_with_two_memberships() {
        let (tmp, storage) = storage();
        let parent = tmp.path().join("walls");
        let child = parent.join("nested");
        fs::create_dir_all(&child).unwrap();
        fs::write(child.join("shared.jpg"), b"shared image").unwrap();
        add_source(&storage, &parent);
        add_source(&storage, &child);

        let report = refresh(&storage);

        assert_eq!(report.complete_sources, 2);
        assert_eq!(report.wallpapers_added, 1);
        assert_eq!(report.memberships_added, 2);
        assert_eq!(report.indexed, 2);
        assert_eq!(library_counts(&storage), (1, 2));
    }

    #[test]
    fn unavailable_sources_do_not_block_a_later_complete_source() {
        let (tmp, storage) = storage();
        let offline_root = tmp.path().join("a-offline");
        let incomplete_root = tmp.path().join("b-incomplete");
        let complete_root = tmp.path().join("c-complete");
        for root in [&offline_root, &incomplete_root, &complete_root] {
            fs::create_dir_all(root).unwrap();
            add_source(&storage, root);
        }
        let offline_wallpaper = offline_root.join("offline.jpg");
        let incomplete_wallpaper = incomplete_root.join("incomplete.jpg");
        fs::write(&offline_wallpaper, b"offline image").unwrap();
        fs::write(&incomplete_wallpaper, b"incomplete image").unwrap();
        refresh(&storage);

        fs::remove_dir_all(&offline_root).unwrap();
        fs::remove_dir_all(&incomplete_root).unwrap();
        fs::write(&incomplete_root, b"not a directory").unwrap();
        let complete_wallpaper = complete_root.join("complete.jpg");
        fs::write(&complete_wallpaper, b"complete image").unwrap();

        let report = refresh(&storage);

        assert_eq!(report.offline_sources, 1);
        assert_eq!(report.incomplete_sources, 1);
        assert_eq!(report.complete_sources, 1);
        assert_eq!(report.memberships_added, 1);
        assert_eq!(library_counts(&storage), (3, 3));
        assert_eq!(
            library_paths(&storage),
            vec![
                offline_wallpaper.to_string_lossy(),
                incomplete_wallpaper.to_string_lossy(),
                complete_wallpaper.to_string_lossy(),
            ]
        );
    }

    #[test]
    fn targeted_refresh_updates_only_the_requested_source() {
        let (tmp, storage) = storage();
        let first_root = tmp.path().join("first");
        let second_root = tmp.path().join("second");
        fs::create_dir_all(&first_root).unwrap();
        fs::create_dir_all(&second_root).unwrap();
        let first_old = first_root.join("old.jpg");
        let second_old = second_root.join("old.jpg");
        fs::write(&first_old, b"first old").unwrap();
        fs::write(&second_old, b"second old").unwrap();
        let first = add_source(&storage, &first_root);
        add_source(&storage, &second_root);
        refresh(&storage);

        let first_new = first_root.join("new.jpg");
        let second_new = second_root.join("new.jpg");
        fs::remove_file(&first_old).unwrap();
        fs::remove_file(&second_old).unwrap();
        fs::write(&first_new, b"first new").unwrap();
        fs::write(&second_new, b"second new").unwrap();

        let report = refresh_library_source(&storage, first.id, |_, _| ScanControl::Continue)
            .expect("targeted refresh should succeed");

        assert_eq!(report.complete_sources, 1);
        assert_eq!(report.wallpapers_added, 1);
        assert_eq!(report.wallpapers_removed, 1);
        assert_eq!(
            library_paths(&storage),
            vec![first_new.to_string_lossy(), second_old.to_string_lossy()]
        );
    }

    #[test]
    fn targeted_refresh_preserves_overlapping_source_membership() {
        let (tmp, storage) = storage();
        let parent = tmp.path().join("walls");
        let child = parent.join("nested");
        fs::create_dir_all(&child).unwrap();
        let shared = child.join("shared.jpg");
        fs::write(&shared, b"shared").unwrap();
        let parent_source = add_source(&storage, &parent);
        add_source(&storage, &child);
        refresh(&storage);
        fs::remove_file(&shared).unwrap();

        let report =
            refresh_library_source(&storage, parent_source.id, |_, _| ScanControl::Continue)
                .expect("targeted overlap refresh should succeed");

        assert_eq!(report.memberships_removed, 1);
        assert_eq!(report.wallpapers_removed, 0);
        assert_eq!(library_counts(&storage), (1, 1));
    }

    #[test]
    fn targeted_offline_and_cancelled_refreshes_preserve_the_snapshot() {
        let (tmp, storage) = storage();
        let offline_root = tmp.path().join("offline");
        let cancelled_root = tmp.path().join("cancelled");
        fs::create_dir_all(&offline_root).unwrap();
        fs::create_dir_all(&cancelled_root).unwrap();
        let offline_old = offline_root.join("old.jpg");
        let cancelled_old = cancelled_root.join("old.jpg");
        fs::write(&offline_old, b"offline old").unwrap();
        fs::write(&cancelled_old, b"cancel old").unwrap();
        let offline = add_source(&storage, &offline_root);
        let cancelled = add_source(&storage, &cancelled_root);
        refresh(&storage);

        fs::remove_dir_all(&offline_root).unwrap();
        let offline_report =
            refresh_library_source(&storage, offline.id, |_, _| ScanControl::Continue)
                .expect("offline is a successful snapshot-preserving outcome");
        assert_eq!(offline_report.offline_sources, 1);

        fs::remove_file(&cancelled_old).unwrap();
        fs::write(cancelled_root.join("new.jpg"), b"cancel new").unwrap();
        let cancelled_result = refresh_library_source(&storage, cancelled.id, |_, event| {
            if matches!(event, SourceScanEvent::CandidateFound { .. }) {
                ScanControl::Cancel
            } else {
                ScanControl::Continue
            }
        });
        assert!(matches!(
            cancelled_result,
            Err(LibraryRefreshError::Cancelled { .. })
        ));
        assert_eq!(
            library_paths(&storage),
            vec![
                cancelled_old.to_string_lossy(),
                offline_old.to_string_lossy()
            ]
        );
    }

    #[test]
    fn targeted_refresh_rejects_an_unknown_stable_id() {
        let (_tmp, storage) = storage();

        let error = refresh_library_source(&storage, 4242, |_, _| ScanControl::Continue)
            .expect_err("unknown source IDs must not silently no-op");

        assert!(error.to_string().contains("source id 4242 not found"));
    }
}
