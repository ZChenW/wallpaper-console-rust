//! Small silent MP4 previews for webviews that can only play blob media URLs.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use crate::generation::{GenerationCoordinator, ThumbnailDeadline};
use crate::{DeadlineCommandError, DeadlineCommandOutput, ThumbnailFailure, ThumbnailSize};

pub const MAX_PREVIEW_CLIP_BYTES: u64 = 12 * 1024 * 1024;
// The clip is always written as complete BT.709, square pixels. A source that names its colour
// matrix but not its primaries or transfer (common in Wallpaper Engine uploads) passed that gap on,
// and WebKitGTK on the NVIDIA driver then drew every frame flat green; the same clip with the tags
// filled in plays correctly (both verified on that desktop).
const CLIP_SCALE: &str = "scale=-2:'trunc(min(576,ih)/2)*2':out_color_matrix=bt709:out_range=tv";
const CLIP_TAGS: &str =
    "setparams=color_primaries=bt709:color_trc=bt709:colorspace=bt709:range=tv,setsar=1";
const HDR_TONEMAP: &str = "zscale=t=linear:npl=100,format=gbrpf32le,zscale=p=bt709,\
tonemap=tonemap=hable:desat=0,zscale=t=bt709:m=bt709:r=tv,format=yuv420p";
const CLIP_X264_COLOUR: &str = "colorprim=bt709:transfer=bt709:colormatrix=bt709";
// Playing a clip costs about as much as presenting its frames. Measured in the app on the real desktop
// with the software video sink the app uses: 720p at 24 fps took 16.5% of a core, 576p at 24 fps 13.6%,
// 540p at 20 fps 11.8%. 24 is the lowest rate that still reads as video, and 576 is the height of the
// assembled picture in a 2560x1440 window, so nothing visible is lost there.
// The clip opens on the still for 0.8 s, then fades into the video over 0.5 s. The still part is that
// long because WebKitGTK's player steps its position back by a few tenths of a second once, about half
// a second after it starts (seen on both test machines); inside the still that step cannot be seen,
// inside the fade it would be.
const CLIP_STILL_ALONE: &str = "0.8";
const CLIP_STILL_INPUT: &str = "1.3";
const CLIP_TOTAL: &str = "15.8";
const CLIP_FPS: &str = "24";
// Bumped whenever the encoding changes (colour tags, then size and rate), so older clips are not reused.
const CLIP_KEY_PREFIX: &str = "v4-clip";

#[derive(Default)]
struct ClipInputs {
    intro: Option<ClipIntro>,
    tone_map: bool,
}

struct ClipIntro {
    still: PathBuf,
    width: u32,
    height: u32,
}

struct VideoProbe {
    size: Option<(u32, u32)>,
    hdr: bool,
}

/// Match the source's display aspect, including non-square source pixels. Round
/// width to the nearest even number, as scale=-2 did, and never increase height.
fn clip_output_size(width: u32, height: u32, sar: (u32, u32)) -> Option<(u32, u32)> {
    if width == 0 || height < 2 || sar.0 == 0 || sar.1 == 0 {
        return None;
    }
    let out_height = height.min(576) / 2 * 2;
    let display_aspect =
        f64::from(width) * f64::from(sar.0) / (f64::from(height) * f64::from(sar.1));
    let out_width = (f64::from(out_height) * display_aspect / 2.0).round() * 2.0;
    if !(2.0..=f64::from(u32::MAX - 1)).contains(&out_width) {
        return None;
    }
    Some((out_width as u32, out_height))
}

fn parse_video_probe(text: &str) -> VideoProbe {
    let value = |key: &str| text.lines().find_map(|line| line.strip_prefix(key));
    let sar = value("sample_aspect_ratio=")
        .and_then(|ratio| ratio.split_once(':'))
        .and_then(|(n, d)| Some((n.parse::<u32>().ok()?, d.parse::<u32>().ok()?)))
        .filter(|&(n, d)| n > 0 && d > 0)
        .unwrap_or((1, 1));
    let size = value("width=").and_then(|width| {
        clip_output_size(width.parse().ok()?, value("height=")?.parse().ok()?, sar)
    });
    VideoProbe {
        size,
        hdr: matches!(value("color_transfer="), Some("smpte2084" | "arib-std-b67")),
    }
}

fn prepare_clip_inputs(
    cache_dir: &Path,
    path: &str,
    mtime: u64,
    size: u64,
    deadline: ThumbnailDeadline,
) -> ClipInputs {
    let mut probe = Command::new("ffprobe");
    probe.args([
        "-v",
        "error",
        "-select_streams",
        "v:0",
        "-show_entries",
        "stream=width,height,sample_aspect_ratio,color_transfer",
        "-of",
        "default=noprint_wrappers=1",
        "--",
        path,
    ]);
    let Some(output) =
        crate::command_output(&mut probe, deadline.metadata_timeout(), "ffprobe clip")
    else {
        // Probe failure must not prevent the original single-input clip path.
        return ClipInputs::default();
    };
    let video = parse_video_probe(&String::from_utf8_lossy(&output.stdout));
    let intro = clip_intro_with(video.size, || {
        // Reuse exactly the Large cache identity, frame selection, validation,
        // atomic publication and single-flight lane used by thumbnail_for_sized.
        // Large's 30-second budget (including lane/flight waits here) leaves time
        // for encoding when still generation fails; it never restarts the clip budget.
        let still_deadline = ThumbnailDeadline::new(ThumbnailSize::Large).bounded_by(deadline);
        crate::generate_gui_thumbnail_sized_before(
            cache_dir,
            path,
            mtime,
            size,
            ThumbnailSize::Large,
            Some(still_deadline),
        )
        .map(|(still, _)| still)
    });
    ClipInputs {
        intro,
        tone_map: video.hdr,
    }
}

fn clip_intro_with(
    size: Option<(u32, u32)>,
    thumbnail: impl FnOnce() -> Result<PathBuf, ThumbnailFailure>,
) -> Option<ClipIntro> {
    let (width, height) = size?;
    let still = thumbnail().ok()?;
    crate::validate_generated_thumbnail_within(&still, crate::MAX_LARGE_THUMBNAIL_SIDE).ok()?;
    Some(ClipIntro {
        still,
        width,
        height,
    })
}

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
    preview_clip_with(cache_dir, path, prepare_clip_inputs, |command, deadline| {
        deadline.command_output(command)
    })
}

fn preview_clip_with(
    cache_dir: &Path,
    path: &str,
    prepare: impl FnOnce(&Path, &str, u64, u64, ThumbnailDeadline) -> ClipInputs,
    mut generate: impl FnMut(
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
                let mut inputs = prepare(cache_dir, &path, mtime, meta.len(), deadline);
                loop {
                    let mut command = clip_command(&path, &tmp, &inputs);
                    match generate(&mut command, deadline) {
                        Ok(output) if output.success => break,
                        Ok(output) => {
                            eprintln!(
                                "ffmpeg preview clip failed: {}",
                                String::from_utf8_lossy(&output.stderr).trim()
                            );
                            if inputs.tone_map {
                                // A build without zscale still gets a usable clip.
                                inputs.tone_map = false;
                            } else if inputs.intro.is_some() {
                                // A cached still may disappear or ffmpeg may reject it
                                // even after native decoding. Keep the original fallback.
                                inputs.intro = None;
                            } else {
                                return Err(ThumbnailFailure::ProbeFailed);
                            }
                        }
                        Err(DeadlineCommandError::TimedOut) => {
                            return Err(ThumbnailFailure::TimedOut)
                        }
                        Err(error) => {
                            eprintln!("ffmpeg preview clip failed: {error:?}");
                            return Err(ThumbnailFailure::ProbeFailed);
                        }
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

fn clip_command(src: &str, dst: &Path, inputs: &ClipInputs) -> Command {
    let mut command = Command::new("ffmpeg");
    command.args(["-hide_banner", "-loglevel", "error", "-nostats", "-y"]);
    let tone_map = if inputs.tone_map {
        format!("{HDR_TONEMAP},")
    } else {
        String::new()
    };
    if let Some(intro) = &inputs.intro {
        command
            .args([
                "-loop",
                "1",
                "-framerate",
                CLIP_FPS,
                "-t",
                CLIP_STILL_INPUT,
                "-i",
            ])
            .arg(&intro.still)
            .args(["-t", "15", "-i", src]);
        let (w, h) = (intro.width, intro.height);
        // colorspace does the sRGB -> BT.709 transfer conversion as well as
        // range/matrix conversion. It needs YUV input: first convert the still
        // to full-range BT.709 YUV without changing its sRGB transfer. This uses
        // no zscale so the HDR fallback also works on builds without libzimg.
        let graph = format!(
            "[0:v:0]scale={w}:{h}:out_color_matrix=bt709:out_range=pc,format=yuv444p,\
colorspace=iall=bt709:itrc=srgb:irange=pc:all=bt709:range=tv:format=yuv420p,\
fps={CLIP_FPS},settb=1/{CLIP_FPS},setpts=PTS-STARTPTS,setsar=1[still];\
[1:v:0]{tone_map}scale={w}:{h}:out_color_matrix=bt709:out_range=tv,format=yuv420p,\
fps={CLIP_FPS},settb=1/{CLIP_FPS},setpts=PTS-STARTPTS,setsar=1[video];\
[still][video]xfade=transition=fade:duration=0.5:offset={CLIP_STILL_ALONE},format=yuv420p,{CLIP_TAGS}[out]"
        );
        command.args([
            "-filter_complex",
            &graph,
            "-map",
            "[out]",
            "-t",
            CLIP_TOTAL,
            "-an",
            "-sn",
            "-dn",
        ]);
    } else {
        // Keep the established single-input SDR command when the still is unavailable.
        command
            .args([
                "-i", src, "-t", "15", "-map", "0:v:0", "-an", "-sn", "-dn", "-vf",
            ])
            .arg(format!("{tone_map}{CLIP_SCALE},{CLIP_TAGS}"));
    }
    command
        .args([
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
