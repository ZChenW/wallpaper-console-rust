use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum WcError {
    #[error("HOME is not set; cannot resolve config directory")]
    HomeNotSet,

    #[error("config directory not found: {0}")]
    ConfigDirNotFound(PathBuf),

    #[error("unsupported file type: {0}")]
    UnsupportedFileType(String),

    #[error("backend not found: {0}")]
    BackendNotFound(String),

    #[error("not a regular file: {0}")]
    NotRegularFile(PathBuf),

    #[error("no previous wallpaper to restore")]
    NoPreviousWallpaper,

    #[error("wallpaper no longer exists: {0}")]
    WallpaperMissing(PathBuf),

    #[error("SQLite error: {0}")]
    Sqlite(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(String),
}

impl From<String> for WcError {
    fn from(s: String) -> Self {
        WcError::Other(s)
    }
}
