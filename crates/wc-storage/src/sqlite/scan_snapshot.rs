use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use wc_core::types::WallpaperEntry;

pub const SCAN_SNAPSHOT_FORMAT_VERSION: u32 = 2;

#[derive(Debug)]
pub enum ScanSnapshotError {
    Io(std::io::Error),
    Sqlite(rusqlite::Error),
    Invalid(String),
    Incomplete,
    UncleanExit,
    WrongSource { expected: i64, actual: i64 },
    UnsupportedVersion { expected: u32, actual: u32 },
}

impl fmt::Display for ScanSnapshotError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "scan snapshot I/O failed: {error}"),
            Self::Sqlite(error) => write!(formatter, "scan snapshot SQLite failed: {error}"),
            Self::Invalid(reason) => write!(formatter, "invalid scan snapshot: {reason}"),
            Self::Incomplete => write!(formatter, "scan snapshot is incomplete"),
            Self::UncleanExit => write!(formatter, "scan worker did not exit cleanly"),
            Self::WrongSource { expected, actual } => {
                write!(
                    formatter,
                    "scan snapshot source mismatch: expected {expected}, got {actual}"
                )
            }
            Self::UnsupportedVersion { expected, actual } => write!(
                formatter,
                "unsupported scan snapshot format: expected {expected}, got {actual}"
            ),
        }
    }
}

impl std::error::Error for ScanSnapshotError {}

impl From<std::io::Error> for ScanSnapshotError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<rusqlite::Error> for ScanSnapshotError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Sqlite(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotPathPresence {
    Present,
    Missing,
    Unknown,
}

impl SnapshotPathPresence {
    fn as_str(self) -> &'static str {
        match self {
            Self::Present => "present",
            Self::Missing => "missing",
            Self::Unknown => "unknown",
        }
    }

    fn parse(value: &str) -> Result<Self, ScanSnapshotError> {
        match value {
            "present" => Ok(Self::Present),
            "missing" => Ok(Self::Missing),
            "unknown" => Ok(Self::Unknown),
            _ => Err(ScanSnapshotError::Invalid(
                "invalid prior-path presence value".to_string(),
            )),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ValidatedScanSnapshot {
    path: PathBuf,
    source_id: i64,
    item_count: usize,
    source_path: Option<PathBuf>,
    source_kind: Option<String>,
    source_recursive: Option<bool>,
}

impl ValidatedScanSnapshot {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn source_id(&self) -> i64 {
        self.source_id
    }

    pub fn item_count(&self) -> usize {
        self.item_count
    }

    pub fn source_config(&self) -> Option<(&Path, &str, bool)> {
        Some((
            self.source_path.as_deref()?,
            self.source_kind.as_deref()?,
            self.source_recursive?,
        ))
    }

    pub fn read_entries(&self) -> Result<Vec<WallpaperEntry>, ScanSnapshotError> {
        let conn = open_read_only(&self.path)?;
        let mut statement = conn.prepare("SELECT payload_json FROM scan_items ORDER BY ordinal")?;
        let entries = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .map(|row| {
                let payload = row?;
                serde_json::from_str(&payload).map_err(|error| {
                    ScanSnapshotError::Invalid(format!("invalid scan item payload: {error}"))
                })
            })
            .collect();
        entries
    }

    pub fn read_prior_presence(
        &self,
    ) -> Result<Vec<(String, SnapshotPathPresence)>, ScanSnapshotError> {
        let conn = open_read_only(&self.path)?;
        let mut statement =
            conn.prepare("SELECT path, presence FROM prior_path_presence ORDER BY path")?;
        let presence = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .map(|row| {
                let (path, presence) = row?;
                Ok((path, SnapshotPathPresence::parse(&presence)?))
            })
            .collect();
        presence
    }
}

pub fn create_incomplete_scan_snapshot(
    path: &Path,
    source_id: i64,
) -> Result<(), ScanSnapshotError> {
    create_private_file(path)?;
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "PRAGMA journal_mode = DELETE;
         PRAGMA synchronous = FULL;
         CREATE TABLE scan_meta (
             key TEXT PRIMARY KEY,
             value TEXT NOT NULL
         ) STRICT;
         CREATE TABLE scan_items (
             ordinal INTEGER PRIMARY KEY,
             path TEXT NOT NULL UNIQUE,
             payload_json TEXT NOT NULL
         ) STRICT;
         CREATE TABLE prior_path_presence (
             path TEXT PRIMARY KEY,
             presence TEXT NOT NULL CHECK (presence IN ('present', 'missing', 'unknown'))
         ) STRICT;",
    )?;
    conn.execute(
        "INSERT INTO scan_meta (key, value) VALUES ('format_version', ?1)",
        params![SCAN_SNAPSHOT_FORMAT_VERSION.to_string()],
    )?;
    conn.execute(
        "INSERT INTO scan_meta (key, value) VALUES ('source_id', ?1)",
        params![source_id.to_string()],
    )?;
    Ok(())
}

pub fn create_incomplete_scan_snapshot_for_source(
    path: &Path,
    source_id: i64,
    source_path: &Path,
    source_kind: &str,
    recursive: bool,
) -> Result<(), ScanSnapshotError> {
    create_incomplete_scan_snapshot(path, source_id)?;
    let mut connection = Connection::open(path)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    for (key, value) in [
        ("source_path", source_path.to_string_lossy().into_owned()),
        ("source_kind", source_kind.to_string()),
        ("source_recursive", i64::from(recursive).to_string()),
    ] {
        transaction.execute(
            "INSERT INTO scan_meta (key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
    }
    transaction.commit()?;
    Ok(())
}

pub fn complete_scan_snapshot(
    path: &Path,
    source_id: i64,
    entries: &[WallpaperEntry],
    prior_presence: &[(String, SnapshotPathPresence)],
) -> Result<(), ScanSnapshotError> {
    let mut conn = Connection::open(path)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let actual_source = read_meta_i64(&tx, "source_id")?;
    if actual_source != source_id {
        return Err(ScanSnapshotError::WrongSource {
            expected: source_id,
            actual: actual_source,
        });
    }
    let actual_version = read_meta_u32(&tx, "format_version")?;
    if actual_version != SCAN_SNAPSHOT_FORMAT_VERSION {
        return Err(ScanSnapshotError::UnsupportedVersion {
            expected: SCAN_SNAPSHOT_FORMAT_VERSION,
            actual: actual_version,
        });
    }
    for (ordinal, entry) in entries.iter().enumerate() {
        let payload = serde_json::to_string(entry)
            .map_err(|error| ScanSnapshotError::Invalid(error.to_string()))?;
        tx.execute(
            "INSERT INTO scan_items (ordinal, path, payload_json) VALUES (?1, ?2, ?3)",
            params![
                i64::try_from(ordinal).unwrap_or(i64::MAX),
                entry.path.as_str(),
                payload
            ],
        )?;
    }
    for (prior_path, presence) in prior_presence {
        tx.execute(
            "INSERT INTO prior_path_presence (path, presence) VALUES (?1, ?2)",
            params![prior_path, presence.as_str()],
        )?;
    }
    tx.execute(
        "INSERT INTO scan_meta (key, value) VALUES ('complete', '1')",
        [],
    )?;
    tx.commit()?;
    Ok(())
}

pub fn validate_scan_snapshot(
    path: &Path,
    expected_source_id: i64,
    clean_exit: bool,
) -> Result<ValidatedScanSnapshot, ScanSnapshotError> {
    if !clean_exit {
        return Err(ScanSnapshotError::UncleanExit);
    }
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() {
        return Err(ScanSnapshotError::Invalid(
            "snapshot is not a regular file".to_string(),
        ));
    }
    let conn = open_read_only(path)?;
    let integrity: String = conn.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
    if integrity != "ok" {
        return Err(ScanSnapshotError::Invalid(
            "snapshot failed SQLite structural validation".to_string(),
        ));
    }
    let actual_version = read_meta_u32(&conn, "format_version")?;
    if actual_version != SCAN_SNAPSHOT_FORMAT_VERSION {
        return Err(ScanSnapshotError::UnsupportedVersion {
            expected: SCAN_SNAPSHOT_FORMAT_VERSION,
            actual: actual_version,
        });
    }
    let actual_source = read_meta_i64(&conn, "source_id")?;
    if actual_source != expected_source_id {
        return Err(ScanSnapshotError::WrongSource {
            expected: expected_source_id,
            actual: actual_source,
        });
    }
    let complete = read_meta(&conn, "complete")?;
    if complete.as_deref() != Some("1") {
        return Err(ScanSnapshotError::Incomplete);
    }
    let count = conn.query_row("SELECT COUNT(*) FROM scan_items", [], |row| {
        row.get::<_, i64>(0)
    })?;
    let validated = ValidatedScanSnapshot {
        path: path.to_path_buf(),
        source_id: actual_source,
        item_count: usize::try_from(count).map_err(|_| {
            ScanSnapshotError::Invalid("negative or oversized item count".to_string())
        })?,
        source_path: read_meta(&conn, "source_path")?.map(PathBuf::from),
        source_kind: read_meta(&conn, "source_kind")?,
        source_recursive: read_meta(&conn, "source_recursive")?
            .map(|value| match value.as_str() {
                "0" => Ok(false),
                "1" => Ok(true),
                _ => Err(ScanSnapshotError::Invalid(
                    "invalid source_recursive marker".to_string(),
                )),
            })
            .transpose()?,
    };
    // Decode every row and validate every presence marker before accepting.
    let entries = validated.read_entries()?;
    if entries.len() != validated.item_count {
        return Err(ScanSnapshotError::Invalid(
            "scan item count changed during validation".to_string(),
        ));
    }
    let _ = validated.read_prior_presence()?;
    Ok(validated)
}

pub fn cleanup_stale_scan_artifacts(directory: &Path) -> Result<usize, ScanSnapshotError> {
    let mut removed = 0usize;
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error.into()),
    };
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("wc-scan-")
            || !(name.ends_with(".sqlite") || name.ends_with(".request.json"))
        {
            continue;
        }
        fs::remove_file(entry.path())?;
        removed += 1;
    }
    Ok(removed)
}

fn read_meta(conn: &Connection, key: &str) -> Result<Option<String>, ScanSnapshotError> {
    conn.query_row(
        "SELECT value FROM scan_meta WHERE key = ?1",
        params![key],
        |row| row.get(0),
    )
    .optional()
    .map_err(Into::into)
}

fn read_meta_i64(conn: &Connection, key: &str) -> Result<i64, ScanSnapshotError> {
    let value = read_meta(conn, key)?
        .ok_or_else(|| ScanSnapshotError::Invalid(format!("missing {key} marker")))?;
    value
        .parse()
        .map_err(|_| ScanSnapshotError::Invalid(format!("invalid {key} marker")))
}

fn read_meta_u32(conn: &Connection, key: &str) -> Result<u32, ScanSnapshotError> {
    let value = read_meta(conn, key)?
        .ok_or_else(|| ScanSnapshotError::Invalid(format!("missing {key} marker")))?;
    value
        .parse()
        .map_err(|_| ScanSnapshotError::Invalid(format!("invalid {key} marker")))
}

fn open_read_only(path: &Path) -> Result<Connection, ScanSnapshotError> {
    Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(Into::into)
}

fn create_private_file(path: &Path) -> Result<(), ScanSnapshotError> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?;
    Ok(())
}

#[cfg(test)]
fn set_scan_snapshot_format_for_test(path: &Path, version: u32) -> Result<(), ScanSnapshotError> {
    let conn = Connection::open(path)?;
    conn.execute(
        "UPDATE scan_meta SET value = ?1 WHERE key = 'format_version'",
        params![version.to_string()],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use camino::Utf8PathBuf;
    use wc_core::types::{Backend, FileType, WallpaperEntry};

    use super::*;

    fn entry(path: &Path) -> WallpaperEntry {
        WallpaperEntry {
            path: Utf8PathBuf::from_path_buf(path.to_path_buf()).unwrap(),
            file_type: FileType::Image,
            ext: "jpg".to_string(),
            backend: Backend::Awww,
            size: 7,
            mtime: 11,
            resolution: "1x1".to_string(),
            project: None,
        }
    }

    #[test]
    fn incomplete_snapshot_is_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("wc-scan-incomplete.sqlite");
        create_incomplete_scan_snapshot(&path, 42).unwrap();

        let error = validate_scan_snapshot(&path, 42, true).unwrap_err();
        assert!(matches!(error, ScanSnapshotError::Incomplete));
    }

    #[test]
    fn completed_snapshot_requires_matching_source_and_version() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("wc-scan-complete.sqlite");
        create_incomplete_scan_snapshot(&path, 42).unwrap();
        complete_scan_snapshot(&path, 42, &[entry(&temp.path().join("a.jpg"))], &[]).unwrap();

        assert!(matches!(
            validate_scan_snapshot(&path, 7, true),
            Err(ScanSnapshotError::WrongSource {
                expected: 7,
                actual: 42
            })
        ));
        set_scan_snapshot_format_for_test(&path, SCAN_SNAPSHOT_FORMAT_VERSION + 1).unwrap();
        assert!(matches!(
            validate_scan_snapshot(&path, 42, true),
            Err(ScanSnapshotError::UnsupportedVersion { .. })
        ));
    }

    #[test]
    fn completed_snapshot_requires_clean_worker_exit() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("wc-scan-complete.sqlite");
        create_incomplete_scan_snapshot(&path, 42).unwrap();
        complete_scan_snapshot(&path, 42, &[entry(&temp.path().join("a.jpg"))], &[]).unwrap();

        assert!(matches!(
            validate_scan_snapshot(&path, 42, false),
            Err(ScanSnapshotError::UncleanExit)
        ));
        let valid = validate_scan_snapshot(&path, 42, true).unwrap();
        assert_eq!(valid.source_id(), 42);
        assert_eq!(valid.item_count(), 1);
    }

    #[test]
    fn stale_worker_artifacts_are_removed_without_touching_other_files() {
        let temp = tempfile::tempdir().unwrap();
        let snapshot = temp.path().join("wc-scan-1.sqlite");
        let request = temp.path().join("wc-scan-1.request.json");
        let unrelated = temp.path().join("keep.sqlite");
        std::fs::write(&snapshot, b"partial").unwrap();
        std::fs::write(&request, b"private").unwrap();
        std::fs::write(&unrelated, b"keep").unwrap();

        assert_eq!(cleanup_stale_scan_artifacts(temp.path()).unwrap(), 2);
        assert!(!snapshot.exists());
        assert!(!request.exists());
        assert!(unrelated.exists());
    }
}
