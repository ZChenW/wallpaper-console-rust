use std::process::Command;

pub(crate) fn stop_mpvpaper() {
    let user = crate::whoami();
    let _ = Command::new("pkill")
        .args(["-u", &user, "-f", r"(^|/)mpvpaper\b"])
        .status();
}

pub(crate) fn normalize_mpvpaper_options(raw: &str) -> &str {
    let trimmed = raw.trim();
    if trimmed == "no-audio --loop-file=inf" || trimmed == "--loop-file=inf" {
        "--loop-file=inf --panscan=1.0"
    } else {
        trimmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_mpvpaper_options_migrates_legacy_silent_default() {
        assert_eq!(
            normalize_mpvpaper_options("no-audio --loop-file=inf"),
            "--loop-file=inf --panscan=1.0"
        );
        assert_eq!(
            normalize_mpvpaper_options("  no-audio --loop-file=inf  "),
            "--loop-file=inf --panscan=1.0"
        );
        assert_eq!(
            normalize_mpvpaper_options("no-audio --loop-file=inf --panscan=1"),
            "no-audio --loop-file=inf --panscan=1"
        );
    }

    #[test]
    fn normalize_mpvpaper_options_migrates_plain_loop_default_to_crop_fill() {
        assert_eq!(
            normalize_mpvpaper_options("--loop-file=inf"),
            "--loop-file=inf --panscan=1.0"
        );
        assert_eq!(
            normalize_mpvpaper_options("  --loop-file=inf  "),
            "--loop-file=inf --panscan=1.0"
        );
    }

    #[test]
    fn normalize_mpvpaper_options_preserves_custom_args() {
        assert_eq!(
            normalize_mpvpaper_options("--loop-file=inf --volume=60"),
            "--loop-file=inf --volume=60"
        );
        assert_eq!(
            normalize_mpvpaper_options("--loop-file=inf --volume=80 --mute=no"),
            "--loop-file=inf --volume=80 --mute=no"
        );
        assert_eq!(normalize_mpvpaper_options(""), "");
    }
}
