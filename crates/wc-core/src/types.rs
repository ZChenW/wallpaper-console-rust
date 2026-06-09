use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};

/// Classification of a wallpaper file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileType {
    Image,
    Gif,
    Video,
}

impl FileType {
    pub fn as_str(&self) -> &'static str {
        match self {
            FileType::Image => "image",
            FileType::Gif => "gif",
            FileType::Video => "video",
        }
    }
}

/// Backend used to display wallpapers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    Awww,
    Mpvpaper,
}

impl Backend {
    pub fn as_str(&self) -> &'static str {
        match self {
            Backend::Awww => "awww",
            Backend::Mpvpaper => "mpvpaper",
        }
    }
}

/// Storage backend mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StorageBackend {
    File,
    Hybrid,
    Sqlite,
}

impl std::str::FromStr for StorageBackend {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "file" => Ok(StorageBackend::File),
            "hybrid" => Ok(StorageBackend::Hybrid),
            "sqlite" => Ok(StorageBackend::Sqlite),
            _ => Err(format!("invalid storage_backend: {}", s)),
        }
    }
}

impl StorageBackend {
    pub fn as_str(&self) -> &'static str {
        match self {
            StorageBackend::File => "file",
            StorageBackend::Hybrid => "hybrid",
            StorageBackend::Sqlite => "sqlite",
        }
    }
}

/// A single library entry (matches library.tsv row shape).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WallpaperEntry {
    pub path: Utf8PathBuf,
    pub file_type: FileType,
    pub ext: String,
    pub backend: Backend,
    pub size: u64,
    pub mtime: u64,
    pub resolution: String,
}

impl WallpaperEntry {
    pub fn filename(&self) -> &str {
        self.path.file_name().unwrap_or("")
    }

    pub fn dirname(&self) -> &str {
        self.path.parent().and_then(|p| p.file_name()).unwrap_or("")
    }
}

/// Aggregate library counts.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LibraryCounts {
    pub total: usize,
    pub images: usize,
    pub gifs: usize,
    pub videos: usize,
}
