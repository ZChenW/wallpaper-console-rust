//! Safe type-to-renderer routing shared by apply, preview, planning, and restore.

use crate::types::{Backend, FileType};

/// Normalized renderer preferences.
///
/// Construction clamps every raw value to a renderer that can handle the
/// corresponding media type. Callers therefore cannot accidentally route a
/// video or Wallpaper Engine project to an incompatible backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackendRouting {
    image: Backend,
    gif: Backend,
}

impl BackendRouting {
    pub fn from_raw(image: &str, gif: &str, _video: &str) -> Self {
        Self {
            image: normalize_still_or_animated_image_backend(image),
            gif: normalize_still_or_animated_image_backend(gif),
        }
    }

    pub fn backend_for(self, file_type: FileType) -> Backend {
        match file_type {
            FileType::Image => self.image,
            FileType::Gif => self.gif,
            FileType::Video => Backend::Mpvpaper,
            FileType::WeScene => Backend::LinuxWallpaperEngine,
            FileType::WeWeb | FileType::WeApplication => Backend::Unsupported,
        }
    }
}

impl Default for BackendRouting {
    fn default() -> Self {
        Self::from_raw("awww", "awww", "mpvpaper")
    }
}

fn normalize_still_or_animated_image_backend(raw: &str) -> Backend {
    match raw {
        "mpvpaper" => Backend::Mpvpaper,
        _ => Backend::Awww,
    }
}

#[cfg(test)]
mod tests {
    use super::BackendRouting;
    use crate::types::{Backend, FileType};

    #[test]
    fn default_routing_uses_only_compatible_renderers() {
        let routing = BackendRouting::default();

        assert_eq!(routing.backend_for(FileType::Image), Backend::Awww);
        assert_eq!(routing.backend_for(FileType::Gif), Backend::Awww);
        assert_eq!(routing.backend_for(FileType::Video), Backend::Mpvpaper);
        assert_eq!(
            routing.backend_for(FileType::WeScene),
            Backend::LinuxWallpaperEngine
        );
        assert_eq!(routing.backend_for(FileType::WeWeb), Backend::Unsupported);
        assert_eq!(
            routing.backend_for(FileType::WeApplication),
            Backend::Unsupported
        );
    }

    #[test]
    fn image_and_gif_can_opt_into_mpvpaper() {
        let routing = BackendRouting::from_raw("mpvpaper", "mpvpaper", "mpvpaper");

        assert_eq!(routing.backend_for(FileType::Image), Backend::Mpvpaper);
        assert_eq!(routing.backend_for(FileType::Gif), Backend::Mpvpaper);
    }

    #[test]
    fn invalid_and_legacy_image_backends_fall_back_to_awww() {
        for raw in ["", "unknown", "swww", "linux-wallpaperengine"] {
            let routing = BackendRouting::from_raw(raw, raw, "mpvpaper");
            assert_eq!(routing.backend_for(FileType::Image), Backend::Awww);
            assert_eq!(routing.backend_for(FileType::Gif), Backend::Awww);
        }
    }

    #[test]
    fn video_is_clamped_to_mpvpaper_for_every_config_value() {
        for raw in ["mpvpaper", "awww", "swww", "", "unknown"] {
            let routing = BackendRouting::from_raw("awww", "awww", raw);
            assert_eq!(routing.backend_for(FileType::Video), Backend::Mpvpaper);
        }
    }
}
