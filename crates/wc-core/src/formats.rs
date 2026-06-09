use crate::types::{Backend, FileType};

/// Extension → (FileType, Backend) mapping.
/// Returns None for unsupported extensions.
pub fn classify_extension(ext: &str) -> Option<(FileType, Backend)> {
    match ext.to_lowercase().as_str() {
        // Images → awww
        "png" | "jpg" | "jpeg" | "webp" | "bmp" => Some((FileType::Image, Backend::Awww)),
        // GIF → configurable backend (default awww)
        "gif" => Some((FileType::Gif, Backend::Awww)),
        // Videos → mpvpaper
        "mp4" | "webm" | "mkv" | "mov" => Some((FileType::Video, Backend::Mpvpaper)),
        _ => None,
    }
}

/// Filenames that should never be treated as wallpaper candidates.
pub fn is_preview_filename(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower == "preview.jpg"
        || lower == "preview.png"
        || lower == "preview.gif"
        || lower == "preview.webp"
        || lower == "thumbnail.png"
        || lower == "thumbnail.jpg"
        || lower == "thumb.jpg"
        || lower == "thumb.png"
}

/// Get the file extension (lowercase, without dot).
pub fn get_extension(path: &str) -> Option<String> {
    std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
}

/// Check if an extension is supported.
pub fn is_supported_extension(ext: &str) -> bool {
    classify_extension(ext).is_some()
}

/// Config-driven backend routing for GIFs (default: awww).
pub fn gif_backend(config_gif_backend: Option<&str>) -> Backend {
    match config_gif_backend.unwrap_or("awww") {
        "mpvpaper" => Backend::Mpvpaper,
        _ => Backend::Awww,
    }
}

/// The default backend for a given file type.
pub fn default_backend_for(ft: FileType) -> Backend {
    match ft {
        FileType::Image => Backend::Awww,
        FileType::Gif => Backend::Awww,
        FileType::Video => Backend::Mpvpaper,
    }
}

/// Return the file type as a display string ("image" / "gif" / "video" / "?").
pub fn file_type_for_ext_str(ext: &str) -> &'static str {
    match ext.to_lowercase().as_str() {
        "png" | "jpg" | "jpeg" | "webp" | "bmp" => "image",
        "gif" => "gif",
        "mp4" | "webm" | "mkv" | "mov" => "video",
        _ => "?",
    }
}

/// Preview/metadata display modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewMode {
    Compact,
    Visual,
    Full,
}

impl std::str::FromStr for PreviewMode {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "visual" => PreviewMode::Visual,
            "full" => PreviewMode::Full,
            _ => PreviewMode::Compact,
        })
    }
}
