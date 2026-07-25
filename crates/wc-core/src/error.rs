use std::path::PathBuf;
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendErrorKind {
    RendererLimitation,
    TargetConfig,
    WorkshopDirectory,
    Generic,
}

/// Category of lock that timed out during acquisition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockCategory {
    /// Shared or exclusive maintenance lock (prevents DB replacement).
    Maintenance,
    /// Exclusive schema lock (migration / repair / restore / bootstrap).
    Schema,
}

impl std::fmt::Display for LockCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LockCategory::Maintenance => write!(f, "database maintenance"),
            LockCategory::Schema => write!(f, "schema"),
        }
    }
}

/// What kind of lock operation was attempted when the timeout occurred.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockOperation {
    /// A shared lock (ordinary reads / writes).
    Shared,
    /// An exclusive lock (migration / repair / restore / replacement).
    Exclusive,
}

impl std::fmt::Display for LockOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LockOperation::Shared => write!(f, "shared"),
            LockOperation::Exclusive => write!(f, "exclusive"),
        }
    }
}

#[derive(Error, Debug)]
pub enum WcError {
    #[error("HOME is not set; cannot resolve config directory")]
    HomeNotSet,

    #[error("unsupported file type: {0}")]
    UnsupportedFileType(String),

    #[error("backend not found: {0}")]
    BackendNotFound(String),

    #[error("not a regular file: {0}")]
    NotRegularFile(PathBuf),

    #[error("wallpaper no longer exists: {0}")]
    WallpaperMissing(PathBuf),

    #[error("SQLite error: {0}")]
    Sqlite(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("linux-wallpaperengine error ({kind:?}): {detail}")]
    LinuxWallpaperEngine {
        kind: BackendErrorKind,
        detail: String,
    },

    /// A file-lock acquisition timed out.
    ///
    /// The error carries enough structured information for callers to pick a
    /// user-visible message or derive a retry schedule, without including
    /// private filesystem paths.
    #[error(
        "timed out waiting for {category} {operation} lock ({stage}) after {waited:?} (deadline {deadline:?})"
    )]
    LockTimeout {
        category: LockCategory,
        operation: LockOperation,
        /// Which code path requested the lock (e.g. "runtime_open",
        /// "schema_bootstrap", "maintenance_replace").
        stage: &'static str,
        waited: Duration,
        deadline: Duration,
    },

    /// The database schema version (observed) is newer than the maximum version
    /// this build supports (supported). The caller must not attempt to write
    /// compatibility data or downgrade the schema.
    #[error(
        "database schema version {observed} is newer than supported version {supported}. \
         Upgrade wallpaper-console to open this database."
    )]
    SchemaTooNew { supported: i64, observed: i64 },

    /// A revision-bound Library cursor or total request no longer matches
    /// the current read snapshot. No query text or cursor payload is exposed.
    #[error("revision_changed: library revision changed from {expected} to {observed}")]
    RevisionChanged { expected: u64, observed: u64 },

    #[error(
        "config_revision_changed: behavior settings revision changed from {expected} to {observed}"
    )]
    ConfigRevisionChanged { expected: String, observed: String },

    /// An opaque Library cursor could not be validated. The reason is a
    /// static category so malformed tokens are never reflected into logs.
    #[error("invalid_cursor: {reason}")]
    InvalidCursor { reason: &'static str },

    #[error("{0}")]
    Other(String),
}

impl From<String> for WcError {
    fn from(s: String) -> Self {
        WcError::Other(s)
    }
}

impl From<&str> for WcError {
    fn from(s: &str) -> Self {
        WcError::Other(s.to_string())
    }
}
