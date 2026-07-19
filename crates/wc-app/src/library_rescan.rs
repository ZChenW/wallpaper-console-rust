//! Shared library rescan orchestration: file lock, dirty marker, refresh, TSV snapshot.
//!
//! Callers retain their own progress/cancel UI state and observe scan events via callback.

use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, ErrorKind, Write};
use std::path::PathBuf;
use std::time::Duration;

use fs2::FileExt;
#[cfg(test)]
use wc_config::ConfigDirExt;
use wc_core::error::WcError;
use wc_core::types::WallpaperEntry;
use wc_scan::{ScanControl, SourceScanEvent};
use wc_storage::{SourceRecord, StorageApi};

use crate::library_refresh::{refresh_library_sources, LibraryRefreshError, LibraryRefreshReport};

/// Result of a full library rescan, including the legacy TSV snapshot size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryRescanReport {
    pub source_count: usize,
    /// `None` when there were no configured sources (refresh skipped).
    pub refresh: Option<LibraryRefreshReport>,
    pub snapshot_count: usize,
    pub refresh_time: Duration,
    pub snapshot_time: Duration,
}

/// Errors from the shared rescan pipeline.
#[derive(Debug)]
pub enum LibraryRescanError {
    Io(io::Error),
    Storage(WcError),
    Refresh(LibraryRefreshError),
    Snapshot(String),
    ScanBusy {
        source_id: Option<i64>,
        waited: Duration,
    },
}

impl fmt::Display for LibraryRescanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "{error}"),
            Self::Storage(error) => write!(formatter, "{error}"),
            Self::Refresh(error) => write!(formatter, "{error}"),
            Self::Snapshot(message) => write!(formatter, "{message}"),
            Self::ScanBusy { source_id, waited } => match source_id {
                Some(source_id) => write!(
                    formatter,
                    "scan_busy: source {source_id} is already being scanned (waited {waited:?})"
                ),
                None => write!(
                    formatter,
                    "scan_busy: another library scan is active (waited {waited:?})"
                ),
            },
        }
    }
}

impl std::error::Error for LibraryRescanError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Storage(error) => Some(error),
            Self::Refresh(error) => Some(error),
            Self::Snapshot(_) => None,
            Self::ScanBusy { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanLockIntent {
    Background,
    Manual,
}

const MANUAL_SCAN_LOCK_DEADLINE: Duration = Duration::from_secs(2);
const SCAN_LOCK_POLL: Duration = Duration::from_millis(25);

fn open_scan_lock(storage: &StorageApi, name: &str) -> Result<File, LibraryRescanError> {
    std::fs::create_dir_all(&storage.cd.path)?;
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(storage.cd.path.join(name))
        .map_err(LibraryRescanError::Io)
}

fn acquire_scan_lock(
    lock: File,
    source_id: Option<i64>,
    intent: ScanLockIntent,
) -> Result<Option<File>, LibraryRescanError> {
    let started = std::time::Instant::now();
    loop {
        match FileExt::try_lock_exclusive(&lock) {
            Ok(()) => return Ok(Some(lock)),
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                if intent == ScanLockIntent::Background {
                    return Ok(None);
                }
                let waited = started.elapsed();
                if waited >= MANUAL_SCAN_LOCK_DEADLINE {
                    return Err(LibraryRescanError::ScanBusy { source_id, waited });
                }
                std::thread::sleep(
                    SCAN_LOCK_POLL.min(MANUAL_SCAN_LOCK_DEADLINE.saturating_sub(waited)),
                );
            }
            Err(error) => return Err(LibraryRescanError::Io(error)),
        }
    }
}

/// Acquire a source-scoped scan lock. Background work never waits; manual GUI
/// and CLI work waits for at most two seconds and then returns `scan_busy`.
pub fn acquire_source_scan_lock(
    storage: &StorageApi,
    source_id: i64,
    intent: ScanLockIntent,
) -> Result<Option<File>, LibraryRescanError> {
    let lock = open_scan_lock(storage, &format!(".library.source.{source_id}.lock"))?;
    acquire_scan_lock(lock, Some(source_id), intent)
}

impl From<io::Error> for LibraryRescanError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<WcError> for LibraryRescanError {
    fn from(error: WcError) -> Self {
        Self::Storage(error)
    }
}

impl From<LibraryRefreshError> for LibraryRescanError {
    fn from(error: LibraryRefreshError) -> Self {
        Self::Refresh(error)
    }
}

/// Path of the durable stale-snapshot marker written before refresh mutates SQLite.
pub fn library_dirty_marker_path(storage: &StorageApi) -> PathBuf {
    storage.cd.path.join("library.dirty")
}

/// Exclusive file lock serializing CLI and GUI rescans against the same config dir.
pub fn acquire_rescan_lock(storage: &StorageApi) -> Result<File, LibraryRescanError> {
    let lock = open_scan_lock(storage, ".library.rescan.lock")?;
    acquire_scan_lock(lock, None, ScanLockIntent::Manual)?.ok_or({
        LibraryRescanError::ScanBusy {
            source_id: None,
            waited: Duration::ZERO,
        }
    })
}

/// Create `library.dirty` if absent. Existing markers are left untouched.
pub fn establish_library_dirty_marker(storage: &StorageApi) -> io::Result<()> {
    let dirty = library_dirty_marker_path(storage);
    let marker = match OpenOptions::new().write(true).create_new(true).open(&dirty) {
        Ok(marker) => marker,
        Err(error) if error.kind() == ErrorKind::AlreadyExists => return Ok(()),
        Err(error) => return Err(error),
    };
    marker.sync_all()?;
    Ok(())
}

/// Establish the dirty marker, then run `operation` (marker stays until TSV publish).
pub fn with_dirty_library_marker<T, E, F>(storage: &StorageApi, operation: F) -> Result<T, E>
where
    F: FnOnce() -> Result<T, E>,
    E: From<io::Error>,
{
    establish_library_dirty_marker(storage)?;
    operation()
}

/// Reject reads that would treat a dirty-but-missing SQLite as an empty library.
pub fn ensure_dirty_sqlite_is_readable(storage: &StorageApi) -> Result<(), LibraryRescanError> {
    if library_dirty_marker_path(storage).exists() && !storage.cd.db_path().exists() {
        return Err(LibraryRescanError::Snapshot(
            "library snapshot is stale: library.dirty exists but SQLite is unavailable; run rescan"
                .into(),
        ));
    }
    Ok(())
}

fn write_library_tsv_entry<W: Write>(writer: &mut W, entry: &WallpaperEntry) -> io::Result<()> {
    writeln!(
        writer,
        "{}\t{}\t{}\t{}\t{}\t{}\t{}",
        entry.file_type.as_str(),
        entry.ext,
        entry.backend.as_str(),
        entry.size,
        entry.mtime,
        entry.resolution,
        entry.path
    )
}

fn sqlite_library_snapshot(
    storage: &StorageApi,
) -> Result<(u64, Vec<WallpaperEntry>), LibraryRescanError> {
    ensure_dirty_sqlite_is_readable(storage)?;
    wc_storage::sqlite::source_backed_library_snapshot(&storage.cd).map_err(Into::into)
}

/// Export source-backed SQLite library rows to `library.tsv` and clear the dirty marker.
pub fn write_legacy_tsv_snapshot(storage: &StorageApi) -> Result<usize, LibraryRescanError> {
    let (revision, entries) = sqlite_library_snapshot(storage)?;
    let tsv_path = storage.cd.library_tsv_path();
    let tsv_tmp = tsv_path.with_extension("tsv.tmp");
    let write_result = (|| -> Result<(), LibraryRescanError> {
        let tsv_file = File::create(&tsv_tmp)?;
        let mut writer = BufWriter::new(tsv_file);
        for entry in &entries {
            write_library_tsv_entry(&mut writer, entry)?;
        }
        writer.flush()?;
        drop(writer);
        wc_storage::sqlite::with_library_revision_publish_guard(&storage.cd, revision, || {
            std::fs::rename(&tsv_tmp, &tsv_path).map_err(wc_core::error::WcError::Io)?;
            let dirty = library_dirty_marker_path(storage);
            if dirty.exists() {
                std::fs::remove_file(dirty).map_err(wc_core::error::WcError::Io)?;
            }
            Ok(())
        })?;
        Ok(())
    })();
    if write_result.is_err() {
        let _ = std::fs::remove_file(&tsv_tmp);
    }
    write_result?;
    Ok(entries.len())
}

/// Full rescan: file lock → dirty marker → refresh (if sources exist) → TSV snapshot.
///
/// Progress/cancel remain caller-owned; pass the same scan-event callback used by
/// [`crate::library_refresh::refresh_library_sources`].
pub fn run_library_rescan<F>(
    storage: &StorageApi,
    mut callback: F,
) -> Result<LibraryRescanReport, LibraryRescanError>
where
    F: FnMut(&SourceRecord, &SourceScanEvent) -> ScanControl,
{
    let _rescan_guard = acquire_rescan_lock(storage)?;
    let source_count = with_dirty_library_marker(storage, || {
        storage
            .source_records()
            .map(|sources| sources.len())
            .map_err(LibraryRescanError::from)
    })?;

    if source_count == 0 {
        let snapshot_start = std::time::Instant::now();
        let snapshot_count = write_legacy_tsv_snapshot(storage)?;
        return Ok(LibraryRescanReport {
            source_count: 0,
            refresh: None,
            snapshot_count,
            refresh_time: Duration::ZERO,
            snapshot_time: snapshot_start.elapsed(),
        });
    }

    let refresh_start = std::time::Instant::now();
    let refresh = refresh_library_sources(storage, |source, event| callback(source, event))?;
    let refresh_time = refresh_start.elapsed();

    let snapshot_start = std::time::Instant::now();
    let snapshot_count = write_legacy_tsv_snapshot(storage)?;
    Ok(LibraryRescanReport {
        source_count,
        refresh: Some(refresh),
        snapshot_count,
        refresh_time,
        snapshot_time: snapshot_start.elapsed(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wc_core::config::ConfigDir;
    use wc_scan::ScanControl;

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
    fn rescan_with_no_sources_replaces_stale_tsv_and_clears_dirty_marker() {
        let (_tmp, storage) = storage();
        std::fs::write(
            storage.cd.library_tsv_path(),
            "image\tjpg\tawww\t1\t1\t1x1\t/stale/wall.jpg\n",
        )
        .unwrap();
        let dirty = library_dirty_marker_path(&storage);
        std::fs::write(&dirty, b"stale").unwrap();

        let report = run_library_rescan(&storage, |_, _| ScanControl::Continue).unwrap();

        assert_eq!(report.source_count, 0);
        assert!(report.refresh.is_none());
        assert_eq!(report.snapshot_count, 0);
        assert_eq!(
            std::fs::read_to_string(storage.cd.library_tsv_path()).unwrap(),
            ""
        );
        assert!(!dirty.exists());
    }

    #[test]
    fn rescan_lock_serializes_overlapping_callers() {
        let (_tmp, storage) = storage();
        let first_guard = acquire_rescan_lock(&storage).unwrap();
        let config_path = storage.cd.path.clone();
        let (attempting_tx, attempting_rx) = std::sync::mpsc::channel();
        let (acquired_tx, acquired_rx) = std::sync::mpsc::channel();

        let second = std::thread::spawn(move || {
            let storage = StorageApi::try_new(ConfigDir { path: config_path }).unwrap();
            attempting_tx.send(()).unwrap();
            let _guard = acquire_rescan_lock(&storage).unwrap();
            acquired_tx.send(()).unwrap();
        });

        attempting_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        assert!(acquired_rx
            .recv_timeout(std::time::Duration::from_millis(100))
            .is_err());

        drop(first_guard);
        acquired_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        second.join().unwrap();
    }

    #[test]
    fn background_source_lock_skips_immediately_on_contention() {
        let (_tmp, storage) = storage();
        let first = acquire_source_scan_lock(&storage, 7, ScanLockIntent::Manual)
            .unwrap()
            .expect("manual lock should acquire");
        let started = std::time::Instant::now();
        let second = acquire_source_scan_lock(&storage, 7, ScanLockIntent::Background).unwrap();
        assert!(second.is_none());
        assert!(started.elapsed() < Duration::from_millis(100));
        drop(first);
    }

    #[test]
    fn manual_source_lock_returns_scan_busy_after_two_seconds() {
        let (_tmp, storage) = storage();
        let first = acquire_source_scan_lock(&storage, 9, ScanLockIntent::Manual)
            .unwrap()
            .expect("manual lock should acquire");
        let started = std::time::Instant::now();
        let error = acquire_source_scan_lock(&storage, 9, ScanLockIntent::Manual).unwrap_err();
        assert!(matches!(
            error,
            LibraryRescanError::ScanBusy {
                source_id: Some(9),
                ..
            }
        ));
        assert!(started.elapsed() >= Duration::from_secs(2));
        assert!(started.elapsed() < Duration::from_millis(2_300));
        drop(first);
    }

    #[test]
    fn dirty_marker_is_established_before_operation_body() {
        let (_tmp, storage) = storage();
        let dirty = library_dirty_marker_path(&storage);

        let value = with_dirty_library_marker(&storage, || {
            assert!(dirty.exists());
            Ok::<_, LibraryRescanError>(42)
        })
        .unwrap();

        assert_eq!(value, 42);
        assert!(dirty.exists());
    }
}
