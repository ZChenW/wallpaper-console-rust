//! wc-preview — fzf preview helpers, metadata, thumbnail paths.

use wc_core::config::ConfigDir;
use wc_core::types::WallpaperEntry;

/// Get the cached video thumbnail path for an entry (same key as TUI).
pub fn video_thumbnail_path(cd: &ConfigDir, entry: &WallpaperEntry) -> Option<String> {
    let key = video_thumb_cache_key(entry);
    let thumb = cd.preview_cache_dir().join(&key);
    if thumb.exists() {
        Some(thumb.to_string_lossy().to_string())
    } else {
        None
    }
}

/// Get the GUI thumbnail path (400px webp).
pub fn gui_thumbnail_path(cd: &ConfigDir, entry: &WallpaperEntry) -> Option<String> {
    let key = gui_thumb_cache_key(entry);
    let thumb = cd.gui_thumbnail_cache_dir().join(&key);
    if thumb.exists() {
        Some(thumb.to_string_lossy().to_string())
    } else {
        None
    }
}

fn video_thumb_cache_key(entry: &WallpaperEntry) -> String {
    use std::hash::{Hash, Hasher};
    let raw = format!("{}:{}", entry.path, entry.mtime);
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    raw.hash(&mut hasher);
    format!("{:x}.jpg", hasher.finish())
}

fn gui_thumb_cache_key(entry: &WallpaperEntry) -> String {
    use std::hash::{Hash, Hasher};
    let real = std::fs::canonicalize(entry.path.as_str())
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| entry.path.to_string());
    let raw = format!("{}:{}:{}", real, entry.mtime, entry.size);
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    raw.hash(&mut hasher);
    format!("{:x}.webp", hasher.finish())
}
