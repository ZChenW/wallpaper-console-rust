use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};

/// Classification of a wallpaper file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileType {
    Image,
    Gif,
    Video,
    WeScene,
    WeWeb,
    WeApplication,
}

impl FileType {
    pub fn as_str(&self) -> &'static str {
        match self {
            FileType::Image => "image",
            FileType::Gif => "gif",
            FileType::Video => "video",
            FileType::WeScene => "we_scene",
            FileType::WeWeb => "we_web",
            FileType::WeApplication => "unsupported",
        }
    }
}

/// Backend used to display wallpapers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    Awww,
    Mpvpaper,
    #[serde(rename = "linux-wallpaperengine")]
    LinuxWallpaperEngine,
    Unsupported,
}

impl Backend {
    pub fn as_str(&self) -> &'static str {
        match self {
            Backend::Awww => "awww",
            Backend::Mpvpaper => "mpvpaper",
            Backend::LinuxWallpaperEngine => "linux-wallpaperengine",
            Backend::Unsupported => "unsupported",
        }
    }
}

/// Runtime storage backend. Runtime is SQLite-only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StorageBackend {
    Sqlite,
}

impl std::str::FromStr for StorageBackend {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "sqlite" | "file" | "hybrid" => Ok(StorageBackend::Sqlite),
            _ => Err(format!("invalid storage_backend: {}", s)),
        }
    }
}

impl StorageBackend {
    pub fn as_str(&self) -> &'static str {
        "sqlite"
    }
}

/// Runtime storage is SQLite-only. Legacy values are accepted for migration
/// compatibility but normalize to SQLite before use.
pub fn normalize_storage_backend(_raw: &str) -> StorageBackend {
    StorageBackend::Sqlite
}

/// Wallpaper Engine project-level metadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WallpaperProject {
    pub project_type: String,
    pub preview_path: Option<String>,
    pub workshop_id: Option<String>,
    pub title: Option<String>,
    pub we_file: Option<String>,
    pub backend: Option<String>,
    pub unsupported_reason: Option<String>,
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
    pub project: Option<WallpaperProject>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn awww_as_str() {
        assert_eq!(Backend::Awww.as_str(), "awww");
    }
}
