use crate::types::{Backend, FileType};

/// Extension → (FileType, Backend) mapping.
/// Returns None for unsupported extensions.
pub fn classify_extension(ext: &str) -> Option<(FileType, Backend)> {
    match ext.to_lowercase().as_str() {
        "png" | "apng" | "jpg" | "jpeg" | "webp" | "bmp" => Some((FileType::Image, Backend::Awww)),
        // GIF → configurable backend (default awww)
        "gif" => Some((FileType::Gif, Backend::Awww)),
        // Videos → mpvpaper
        "mp4" | "webm" | "mkv" | "mov" | "avi" | "flv" => {
            Some((FileType::Video, Backend::Mpvpaper))
        }
        _ => None,
    }
}

/// Cheap bounded container inspection, used during indexing, never decoding on
/// Library scroll. Apply still validates actual codec data in a killable worker.
pub fn classify_media_path(path: &std::path::Path) -> Option<(FileType, Backend)> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    let candidate = classify_extension(&ext)?;
    if matches!(ext.as_str(), "png" | "apng" | "webp") && is_animated_image(path).unwrap_or(false) {
        Some((FileType::Gif, Backend::Mpvpaper))
    } else {
        Some(candidate)
    }
}

fn is_animated_image(path: &std::path::Path) -> std::io::Result<bool> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path)?;
    let length = file.metadata()?.len();
    let mut header = [0u8; 12];
    file.read_exact(&mut header)?;
    if &header[..4] == b"RIFF" && &header[8..] == b"WEBP" {
        let mut chunk = [0u8; 8];
        for _ in 0..256 {
            if file.read_exact(&mut chunk).is_err() {
                return Ok(false);
            }
            let size = u32::from_le_bytes(chunk[4..].try_into().unwrap()) as u64;
            if file.stream_position()?.saturating_add(size) > length {
                return Ok(false);
            }
            if &chunk[..4] == b"VP8X" && size >= 10 {
                let mut flags = [0u8; 1];
                file.read_exact(&mut flags)?;
                return Ok(flags[0] & 2 != 0);
            }
            if &chunk[..4] == b"ANIM" {
                return Ok(true);
            }
            file.seek(SeekFrom::Current((size + (size & 1)) as i64))?;
        }
    } else if &header[..8] == b"\x89PNG\r\n\x1a\n" {
        file.seek(SeekFrom::Start(8))?;
        let mut chunk = [0u8; 8];
        for _ in 0..256 {
            if file.read_exact(&mut chunk).is_err() {
                return Ok(false);
            }
            let size = u32::from_be_bytes(chunk[..4].try_into().unwrap()) as u64;
            if file.stream_position()?.saturating_add(size + 4) > length {
                return Ok(false);
            }
            if &chunk[4..] == b"acTL" && size == 8 {
                return Ok(true);
            }
            if matches!(&chunk[4..], b"IDAT" | b"IEND") {
                return Ok(false);
            }
            file.seek(SeekFrom::Current((size + 4) as i64))?;
        }
    }
    Ok(false)
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

/// The default backend for a given file type.
pub fn default_backend_for(ft: FileType) -> Backend {
    match ft {
        FileType::Image => Backend::Awww,
        FileType::Gif => Backend::Awww,
        FileType::Video => Backend::Mpvpaper,
        FileType::WeScene => Backend::LinuxWallpaperEngine,
        FileType::WeWeb => Backend::Unsupported,
        FileType::WeApplication => Backend::Unsupported,
    }
}

/// Return the file type as a display string ("image" / "gif" / "video" / "?").
pub fn file_type_for_ext_str(ext: &str) -> &'static str {
    classify_extension(ext)
        .map(|(kind, _)| kind.as_str())
        .unwrap_or("?")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Backend;

    #[test]
    fn default_backend_image_is_awww() {
        assert_eq!(default_backend_for(FileType::Image), Backend::Awww);
    }

    #[test]
    fn default_backend_video_is_mpvpaper() {
        assert_eq!(default_backend_for(FileType::Video), Backend::Mpvpaper);
    }

    #[test]
    fn default_backend_we_scene_is_lwe() {
        assert_eq!(
            default_backend_for(FileType::WeScene),
            Backend::LinuxWallpaperEngine
        );
    }

    #[test]
    fn default_backend_we_web_is_unsupported() {
        assert_eq!(default_backend_for(FileType::WeWeb), Backend::Unsupported);
    }

    #[test]
    fn default_backend_we_application_is_unsupported() {
        assert_eq!(
            default_backend_for(FileType::WeApplication),
            Backend::Unsupported
        );
    }
}
