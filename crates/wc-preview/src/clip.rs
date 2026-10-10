//! Small silent MP4 previews for webviews that can only play blob media URLs.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use crate::generation::{GenerationCoordinator, ThumbnailDeadline};
use crate::{DeadlineCommandError, DeadlineCommandOutput, ThumbnailFailure};

pub const MAX_PREVIEW_CLIP_BYTES: u64 = 12 * 1024 * 1024;
// The clip is always written as complete BT.709, square pixels. A source that names its colour
// matrix but not its primaries or transfer (common in Wallpaper Engine uploads) passed that gap on,
// and WebKitGTK on the NVIDIA driver then drew every frame flat green; the same clip with the tags
// filled in plays correctly (both verified on that desktop). HDR sources are tagged BT.709 too and
// look washed out rather than green.
const CLIP_FILTER: &str = "scale=-2:'trunc(min(720,ih)/2)*2':out_color_matrix=bt709:out_range=tv,\
setparams=color_primaries=bt709:color_trc=bt709:colorspace=bt709:range=tv,setsar=1";
const CLIP_X264_COLOUR: &str = "colorprim=bt709:transfer=bt709:colormatrix=bt709";
// Playing a clip costs about as much as presenting its frames: measured in the app on the real desktop,
// 720p at 30 fps took 14% of a core, at 24 fps 11.5%, at 15 fps 7.8% (and 540p at 30 fps 11.3%).
// 24 is the lowest rate that still reads as video.
const CLIP_FPS: &str = "24";
// Bumped from v1 when the colour tags were added, so clips made without them are not reused.
const CLIP_KEY_PREFIX: &str = "v2-clip";

/// Same canonical-path/mtime-seconds/size identity as GUI thumbnails, in its own namespace.
pub fn preview_clip_cache_key(path: &str, mtime: u64, size: u64) -> String {
    let real = fs::canonicalize(path)
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string());
    let hash = crate::stable_hash_hex(&format!("{CLIP_KEY_PREFIX}-{real}:{mtime}:{size}"));
    format!("{CLIP_KEY_PREFIX}-{hash}.mp4")
}

/// Return a bounded cached MP4 (path, cache hit). Failures never persist markers.
pub fn preview_clip_for(cache_dir: &Path, path: &str) -> Result<(PathBuf, bool), ThumbnailFailure> {
    preview_clip_with(cache_dir, path, |command, deadline| {
        deadline.command_output(command)
    })
}

fn preview_clip_with(
    cache_dir: &Path,
    path: &str,
    generate: impl FnOnce(
        &mut Command,
        ThumbnailDeadline,
    ) -> Result<DeadlineCommandOutput, DeadlineCommandError>,
) -> Result<(PathBuf, bool), ThumbnailFailure> {
    if !matches!(
        wc_core::formats::classify_media_path(Path::new(path)),
        Some((wc_core::types::FileType::Video, _))
    ) {
        return Err(ThumbnailFailure::Unsupported);
    }
    let canonical = fs::canonicalize(path).map_err(|_| ThumbnailFailure::MissingFile)?;
    let meta = fs::metadata(&canonical).map_err(|_| ThumbnailFailure::MissingFile)?;
    if !meta.is_file() {
        return Err(ThumbnailFailure::Unsupported);
    }
    let path = canonical.to_string_lossy();
    let mtime = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let key = preview_clip_cache_key(&path, mtime, meta.len());
    fs::create_dir_all(cache_dir).map_err(|_| ThumbnailFailure::CacheWriteFailed)?;
    let _ = crate::maybe_cleanup_stale_tmp_thumbnails(cache_dir, crate::current_epoch_secs());
    let dst = cache_dir.join(&key);
    if validate_clip(&dst).is_ok() {
        return Ok((dst, true));
    }

    static COORDINATOR: OnceLock<GenerationCoordinator> = OnceLock::new();
    let flight_key = fs::canonicalize(cache_dir)
        .unwrap_or_else(|_| cache_dir.to_path_buf())
        .join(&key);
    COORDINATOR
        .get_or_init(GenerationCoordinator::default)
        .run_clip(flight_key, |deadline| {
            if validate_clip(&dst).is_ok() {
                return Ok((dst, true));
            }
            if dst.exists() {
                fs::remove_file(&dst).map_err(|_| ThumbnailFailure::CacheWriteFailed)?;
            }
            let tmp = crate::reserve_unique_preview_temp(cache_dir, &key, "mp4")?;
            let result = (|| {
                let mut command = clip_command(&path, &tmp);
                match generate(&mut command, deadline) {
                    Ok(output) if output.success => {}
                    Ok(output) => {
                        eprintln!(
                            "ffmpeg preview clip failed: {}",
                            String::from_utf8_lossy(&output.stderr).trim()
                        );
                        return Err(ThumbnailFailure::ProbeFailed);
                    }
                    Err(DeadlineCommandError::TimedOut) => return Err(ThumbnailFailure::TimedOut),
                    Err(error) => {
                        eprintln!("ffmpeg preview clip failed: {error:?}");
                        return Err(ThumbnailFailure::ProbeFailed);
                    }
                }
                validate_clip(&tmp)?;
                match fs::rename(&tmp, &dst) {
                    Ok(()) => Ok((dst.clone(), false)),
                    Err(_) if validate_clip(&dst).is_ok() => Ok((dst.clone(), true)),
                    Err(_) => Err(ThumbnailFailure::CacheWriteFailed),
                }
            })();
            // Also removes partial/invalid output on helper failure or timeout.
            let _ = fs::remove_file(&tmp);
            result
        })
}

fn clip_command(src: &str, dst: &Path) -> Command {
    let mut command = Command::new("ffmpeg");
    command
        .args(["-hide_banner", "-loglevel", "error", "-nostats", "-y", "-i"])
        .arg(src)
        .args([
            "-t",
            "15",
            "-map",
            "0:v:0",
            "-an",
            "-sn",
            "-dn",
            "-vf",
            CLIP_FILTER,
            "-r",
            CLIP_FPS,
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-x264-params",
            CLIP_X264_COLOUR,
            "-preset",
            "veryfast",
            "-crf",
            "27",
            "-movflags",
            "+faststart",
            "-f",
            "mp4",
        ])
        .arg(dst);
    command
}

fn validate_clip(path: &Path) -> Result<(), ThumbnailFailure> {
    let meta = fs::metadata(path).map_err(|_| ThumbnailFailure::ProbeFailed)?;
    if meta.len() == 0 {
        return Err(ThumbnailFailure::EmptyClip);
    }
    if meta.len() > MAX_PREVIEW_CLIP_BYTES {
        return Err(ThumbnailFailure::ClipTooLarge);
    }
    // ffmpeg's MP4 muxer writes ftyp first. Check it without spawning a helper
    // or decoding the cached video on every hit.
    let mut header = [0; 8];
    fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut header))
        .map_err(|_| ThumbnailFailure::ProbeFailed)?;
    if &header[4..] != b"ftyp" {
        return Err(ThumbnailFailure::ProbeFailed);
    }
    Ok(())
}

#[cfg(test)]
#[path = "clip_tests.rs"]
mod tests;
