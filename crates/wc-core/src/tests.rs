//! Unit tests for wc-core.

#[cfg(test)]
mod tests {
    use super::super::*;
    use std::str::FromStr;

    #[test]
    fn test_classify_png() {
        let result = formats::classify_extension("png");
        assert!(result.is_some());
        let (ftype, backend) = result.unwrap();
        assert_eq!(ftype, types::FileType::Image);
        assert_eq!(backend, types::Backend::Awww);
    }

    #[test]
    fn test_classify_gif() {
        let result = formats::classify_extension("gif");
        assert!(result.is_some());
        let (ftype, _) = result.unwrap();
        assert_eq!(ftype, types::FileType::Gif);
    }

    #[test]
    fn test_classify_video() {
        let result = formats::classify_extension("mp4");
        assert!(result.is_some());
        let (ftype, backend) = result.unwrap();
        assert_eq!(ftype, types::FileType::Video);
        assert_eq!(backend, types::Backend::Mpvpaper);
    }

    #[test]
    fn test_unsupported_extension() {
        assert!(formats::classify_extension("json").is_none());
        assert!(formats::classify_extension("txt").is_none());
    }

    #[test]
    fn test_preview_filename_detection() {
        assert!(formats::is_preview_filename("preview.jpg"));
        assert!(formats::is_preview_filename("thumbnail.png"));
        assert!(!formats::is_preview_filename("wallpaper.jpg"));
    }

    #[test]
    fn test_config_parse_basic() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config");
        std::fs::write(&config_path, "key1=val1\nkey2=val with spaces\n# comment\n").unwrap();
        let map = config::parse_config_file(&config_path).unwrap();
        assert_eq!(map.get("key1").unwrap(), "val1");
        assert_eq!(map.get("key2").unwrap(), "val with spaces");
        assert!(!map.contains_key("# comment"));
    }

    #[test]
    fn test_storage_backend_from_str() {
        assert_eq!(
            types::StorageBackend::from_str("sqlite"),
            Ok(types::StorageBackend::Sqlite)
        );
        assert_eq!(
            types::StorageBackend::from_str("file"),
            Ok(types::StorageBackend::Sqlite)
        );
        assert_eq!(
            types::StorageBackend::from_str("hybrid"),
            Ok(types::StorageBackend::Sqlite)
        );
        assert!(types::StorageBackend::from_str("bad").is_err());
    }
}
