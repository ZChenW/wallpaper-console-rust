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
mod tests {
    use super::*;
    use filetime::{set_file_mtime, FileTime};

    const MP4_HEADER: &[u8] = b"\0\0\0\x18ftypisom\0\0\0\0isommp42";

    fn mock_clip_with(
        cache: &Path,
        path: &str,
        generate: impl FnMut(
            &mut Command,
            ThumbnailDeadline,
        ) -> Result<DeadlineCommandOutput, DeadlineCommandError>,
    ) -> Result<(PathBuf, bool), ThumbnailFailure> {
        preview_clip_with(cache, path, |_, _, _, _, _| ClipInputs::default(), generate)
    }

    fn successful_output() -> DeadlineCommandOutput {
        DeadlineCommandOutput {
            success: true,
            stdout: Vec::new(),
            stderr: Vec::new(),
        }
    }

    fn output_path(command: &Command) -> PathBuf {
        command.get_args().last().unwrap().into()
    }

    fn cached_path(cache: &Path, source: &Path) -> PathBuf {
        let meta = fs::metadata(source).unwrap();
        let mtime = meta
            .modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        cache.join(preview_clip_cache_key(
            source.to_str().unwrap(),
            mtime,
            meta.len(),
        ))
    }

    #[test]
    fn clip_ffmpeg_arguments_cap_duration_and_make_silent_compatible_video() {
        let command = clip_command(
            "/wallpapers/a movie.mp4",
            Path::new("/cache/tmp.mp4"),
            &ClipInputs::default(),
        );
        assert_eq!(command.get_program(), "ffmpeg");
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_str().unwrap())
            .collect();
        assert_eq!(
            args,
            [
                "-hide_banner",
                "-loglevel",
                "error",
                "-nostats",
                "-y",
                "-i",
                "/wallpapers/a movie.mp4",
                "-t",
                "15",
                "-map",
                "0:v:0",
                "-an",
                "-sn",
                "-dn",
                "-vf",
                "scale=-2:'trunc(min(576,ih)/2)*2':out_color_matrix=bt709:out_range=tv,setparams=color_primaries=bt709:color_trc=bt709:colorspace=bt709:range=tv,setsar=1",
                "-r",
                "24",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-x264-params",
                "colorprim=bt709:transfer=bt709:colormatrix=bt709",
                "-preset",
                "veryfast",
                "-crf",
                "27",
                "-movflags",
                "+faststart",
                "-f",
                "mp4",
                "/cache/tmp.mp4",
            ]
        );
        assert!(!args.contains(&"-ss"), "clips start at the beginning");
    }

    #[test]
    fn clip_cache_hit_never_runs_a_helper() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source.MP4");
        fs::write(&source, b"source").unwrap();
        let cache = tmp.path().join("cache");
        let (clip, hit) = mock_clip_with(&cache, source.to_str().unwrap(), |command, _| {
            let output = output_path(command);
            assert!(output
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .ends_with(".tmp.mp4"));
            fs::write(output, MP4_HEADER).unwrap();
            Ok(successful_output())
        })
        .unwrap();
        assert!(!hit);
        assert_eq!(clip, cached_path(&cache, &source));
        let second = mock_clip_with(&cache, source.to_str().unwrap(), |_, _| {
            panic!("cache hit spawned ffmpeg")
        })
        .unwrap();
        assert_eq!(second, (clip, true));
        assert_eq!(crate::thumbnail_cache_info(&cache).entries, 1);
    }

    #[test]
    fn changed_source_mtime_or_size_generates_a_new_clip() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source.mp4");
        fs::write(&source, b"source").unwrap();
        set_file_mtime(&source, FileTime::from_unix_time(1_700_000_000, 0)).unwrap();
        let cache = tmp.path().join("cache");
        let generate = || {
            mock_clip_with(&cache, source.to_str().unwrap(), |command, _| {
                fs::write(output_path(command), MP4_HEADER).unwrap();
                Ok(successful_output())
            })
            .unwrap()
        };
        let (first, _) = generate();
        set_file_mtime(&source, FileTime::from_unix_time(1_700_000_001, 0)).unwrap();
        let (second, hit) = generate();
        assert!(!hit);
        assert_ne!(first, second);
        fs::write(&source, b"larger source").unwrap();
        set_file_mtime(&source, FileTime::from_unix_time(1_700_000_001, 0)).unwrap();
        let (third, hit) = generate();
        assert!(!hit);
        assert_ne!(second, third);
        assert_eq!(crate::thumbnail_cache_info(&cache).entries, 3);
    }

    #[cfg(unix)]
    #[test]
    fn clip_keys_use_canonical_paths_and_their_own_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source.mp4");
        let alias = tmp.path().join("alias.mp4");
        fs::write(&source, b"source").unwrap();
        std::os::unix::fs::symlink(&source, &alias).unwrap();
        let key = preview_clip_cache_key(source.to_str().unwrap(), 1, 2);
        assert_eq!(key, preview_clip_cache_key(alias.to_str().unwrap(), 1, 2));
        assert!(key.starts_with("v4-clip-") && key.ends_with(".mp4"));
        assert_ne!(
            key,
            crate::gui_thumb_cache_key_v3(source.to_str().unwrap(), 1, 2)
        );
    }

    #[test]
    fn empty_and_oversize_outputs_are_rejected_and_removed_without_failure_markers() {
        for (length, failure) in [
            (0, ThumbnailFailure::EmptyClip),
            (MAX_PREVIEW_CLIP_BYTES + 1, ThumbnailFailure::ClipTooLarge),
        ] {
            let tmp = tempfile::tempdir().unwrap();
            let source = tmp.path().join("source.mp4");
            fs::write(&source, b"source").unwrap();
            let cache = tmp.path().join("cache");
            let result = mock_clip_with(&cache, source.to_str().unwrap(), |command, _| {
                fs::OpenOptions::new()
                    .write(true)
                    .open(output_path(command))
                    .unwrap()
                    .set_len(length)
                    .unwrap();
                Ok(successful_output())
            });
            assert_eq!(result, Err(failure));
            assert_eq!(fs::read_dir(&cache).unwrap().count(), 0);
            assert_eq!(crate::thumbnail_cache_info(&cache).failure_entries, 0);
        }
    }

    #[test]
    fn invalid_cached_clips_are_removed_before_regeneration() {
        for length in [0, MAX_PREVIEW_CLIP_BYTES + 1, 8] {
            let tmp = tempfile::tempdir().unwrap();
            let source = tmp.path().join("source.mp4");
            fs::write(&source, b"source").unwrap();
            let cache = tmp.path().join("cache");
            fs::create_dir(&cache).unwrap();
            let cached = cached_path(&cache, &source);
            fs::File::create(&cached).unwrap().set_len(length).unwrap();
            let result = mock_clip_with(&cache, source.to_str().unwrap(), |_, _| {
                assert!(!cached.exists());
                Err(DeadlineCommandError::Spawn("missing ffmpeg".into()))
            });
            assert_eq!(result, Err(ThumbnailFailure::ProbeFailed));
            assert_eq!(fs::read_dir(&cache).unwrap().count(), 0);
        }
    }

    #[test]
    fn helper_failure_and_timeout_remove_partial_output_and_allow_retry() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source.mp4");
        fs::write(&source, b"source").unwrap();
        let cache = tmp.path().join("cache");
        for timed_out in [false, true] {
            let result = mock_clip_with(&cache, source.to_str().unwrap(), |command, _| {
                fs::write(output_path(command), MP4_HEADER).unwrap();
                if timed_out {
                    Err(DeadlineCommandError::TimedOut)
                } else {
                    Ok(DeadlineCommandOutput {
                        success: false,
                        ..successful_output()
                    })
                }
            });
            assert_eq!(
                result,
                Err(if timed_out {
                    ThumbnailFailure::TimedOut
                } else {
                    ThumbnailFailure::ProbeFailed
                })
            );
            assert_eq!(fs::read_dir(&cache).unwrap().count(), 0);
        }
        assert!(
            mock_clip_with(&cache, source.to_str().unwrap(), |command, _| {
                fs::write(output_path(command), MP4_HEADER).unwrap();
                Ok(successful_output())
            })
            .is_ok()
        );
    }

    #[test]
    fn non_video_inputs_are_unsupported_even_when_cached() {
        let tmp = tempfile::tempdir().unwrap();
        for name in [
            "image.png",
            "image.apng",
            "image.webp",
            "animation.gif",
            "project.json",
            "unknown.txt",
        ] {
            let source = tmp.path().join(name);
            fs::write(&source, b"source").unwrap();
            let cached = cached_path(tmp.path(), &source);
            fs::write(cached, MP4_HEADER).unwrap();
            assert_eq!(
                mock_clip_with(tmp.path(), source.to_str().unwrap(), |_, _| {
                    panic!("non-video spawned ffmpeg")
                }),
                Err(ThumbnailFailure::Unsupported)
            );
        }
    }

    #[test]
    fn cache_status_clear_and_cleanup_old_include_clips_and_stale_clip_temps() {
        let tmp = tempfile::tempdir().unwrap();
        let old_clip = tmp.path().join("v1-clip-old.mp4");
        let new_clip = tmp.path().join("v1-clip-new.mp4");
        let thumb = tmp.path().join("thumbnail.webp");
        let stale_temp = tmp.path().join(".v4-clip-key.mp4.1.1.tmp.mp4");
        for file in [&old_clip, &new_clip, &thumb, &stale_temp] {
            fs::write(file, MP4_HEADER).unwrap();
        }
        let old_time = FileTime::from_unix_time(1_600_000_000, 0);
        set_file_mtime(&old_clip, old_time).unwrap();
        set_file_mtime(&stale_temp, old_time).unwrap();
        let info = crate::thumbnail_cache_info(tmp.path());
        assert_eq!(info.entries, 4);
        assert_eq!(info.total_bytes, (MP4_HEADER.len() * 4) as u64);
        assert_eq!(crate::cleanup_stale_tmp_thumbnails(tmp.path(), 3600), 1);
        assert_eq!(crate::thumbnail_cache_cleanup_old(tmp.path(), 30), 1);
        assert!(!old_clip.exists());
        assert!(new_clip.exists());
        assert_eq!(crate::thumbnail_cache_cleanup_all(tmp.path()), 2);
        assert_eq!(crate::thumbnail_cache_info(tmp.path()).entries, 0);
    }

    fn intro_inputs(tone_map: bool) -> ClipInputs {
        let (width, height) = clip_output_size(1920, 1080, (1, 1)).unwrap();
        ClipInputs {
            intro: Some(ClipIntro {
                still: "/cache/large preview.webp".into(),
                width,
                height,
            }),
            tone_map,
        }
    }

    fn args(command: &Command) -> Vec<String> {
        command
            .get_args()
            .map(|arg| arg.to_str().unwrap().to_owned())
            .collect()
    }

    fn filter(command: &Command) -> String {
        args(command)
            .windows(2)
            .find(|pair| pair[0] == "-vf" || pair[0] == "-filter_complex")
            .unwrap()[1]
            .clone()
    }

    #[test]
    fn intro_arguments_use_the_large_still_and_identical_video_geometry_and_timing() {
        let command = clip_command(
            "/wallpapers/a movie.mp4",
            Path::new("/cache/tmp.mp4"),
            &intro_inputs(false),
        );
        assert_eq!(args(&command), [
            "-hide_banner", "-loglevel", "error", "-nostats", "-y",
            "-loop", "1", "-framerate", "24", "-t", "1.3", "-i", "/cache/large preview.webp",
            "-t", "15", "-i", "/wallpapers/a movie.mp4",
            "-filter_complex",
            "[0:v:0]scale=1024:576:out_color_matrix=bt709:out_range=pc,format=yuv444p,colorspace=iall=bt709:itrc=srgb:irange=pc:all=bt709:range=tv:format=yuv420p,fps=24,settb=1/24,setpts=PTS-STARTPTS,setsar=1[still];[1:v:0]scale=1024:576:out_color_matrix=bt709:out_range=tv,format=yuv420p,fps=24,settb=1/24,setpts=PTS-STARTPTS,setsar=1[video];[still][video]xfade=transition=fade:duration=0.5:offset=0.8,format=yuv420p,setparams=color_primaries=bt709:color_trc=bt709:colorspace=bt709:range=tv,setsar=1[out]",
            "-map", "[out]", "-t", "15.8", "-an", "-sn", "-dn",
            "-r", "24", "-c:v", "libx264", "-pix_fmt", "yuv420p",
            "-x264-params", "colorprim=bt709:transfer=bt709:colormatrix=bt709",
            "-preset", "veryfast", "-crf", "27", "-movflags", "+faststart", "-f", "mp4", "/cache/tmp.mp4",
        ]);
    }

    #[test]
    fn clip_size_uses_even_dimensions_display_aspect_and_capped_source_height() {
        for (width, height, sar, expected) in [
            (1920, 1080, (1, 1), (1024, 576)),
            (1080, 1920, (1, 1), (324, 576)),
            (321, 241, (1, 1), (320, 240)),
            (1919, 1079, (1, 1), (1024, 576)),
            (640, 360, (1, 1), (640, 360)),
            (720, 576, (16, 15), (768, 576)),
        ] {
            assert_eq!(clip_output_size(width, height, sar), Some(expected));
        }
        for (width, height, sar) in [(0, 1080, (1, 1)), (10, 1, (1, 1)), (10, 10, (1, 0))] {
            assert_eq!(clip_output_size(width, height, sar), None);
        }
    }

    #[test]
    fn transfer_probe_only_tone_maps_pq_and_hlg_and_tolerates_missing_tags() {
        for (tag, hdr) in [
            ("", false),
            ("color_space=bt709", false),
            ("color_transfer=bt709", false),
            ("color_transfer=smpte2084", true),
            ("color_transfer=arib-std-b67", true),
        ] {
            let probe = parse_video_probe(&format!(
                "width=1920\nheight=1080\nsample_aspect_ratio=N/A\n{tag}\n"
            ));
            assert_eq!(probe.size, Some((1024, 576)));
            assert_eq!(probe.hdr, hdr);
        }
        assert_eq!(
            parse_video_probe("width=720\nheight=576\nsample_aspect_ratio=16:15\n").size,
            Some((768, 576))
        );
    }

    #[test]
    fn missing_failed_timed_out_or_undecodable_still_uses_the_original_command() {
        let tmp = tempfile::tempdir().unwrap();
        let broken = tmp.path().join("broken.webp");
        fs::write(&broken, b"undecodable").unwrap();
        for result in [
            Err(ThumbnailFailure::ProbeFailed),
            Err(ThumbnailFailure::TimedOut),
            Ok(tmp.path().join("missing.webp")),
            Ok(broken),
        ] {
            let intro = clip_intro_with(Some((1024, 576)), || result);
            assert!(intro.is_none());
            let command = clip_command(
                "source.mp4",
                Path::new("clip.mp4"),
                &ClipInputs {
                    intro,
                    tone_map: false,
                },
            );
            let arguments = args(&command);
            assert_eq!(arguments.iter().filter(|arg| *arg == "-i").count(), 1);
            assert!(!arguments
                .iter()
                .any(|arg| arg == "-filter_complex" || arg == "-loop"));
            assert!(arguments.windows(2).any(|pair| pair == ["-t", "15"]));
            assert_eq!(filter(&command), format!("{CLIP_SCALE},{CLIP_TAGS}"));
        }
    }

    #[test]
    fn hdr_arguments_tone_map_the_video_before_scaling_with_or_without_an_intro() {
        for inputs in [
            intro_inputs(true),
            ClipInputs {
                intro: None,
                tone_map: true,
            },
        ] {
            let command = clip_command("hdr.mp4", Path::new("clip.mp4"), &inputs);
            let graph = filter(&command);
            assert_eq!(graph.matches(HDR_TONEMAP).count(), 1);
            assert!(graph.contains(&format!("{HDR_TONEMAP},scale=")));
            if inputs.intro.is_some() {
                assert!(graph.contains(&format!("[1:v:0]{HDR_TONEMAP}")));
            }
        }
    }

    #[test]
    fn hdr_filter_failure_retries_once_without_tone_mapping_and_keeps_the_intro() {
        for intro in [false, true] {
            let tmp = tempfile::tempdir().unwrap();
            let source = tmp.path().join("hdr.mp4");
            fs::write(&source, b"source").unwrap();
            let mut attempts = 0;
            let mut first_deadline = None;
            let result = preview_clip_with(
                tmp.path(),
                source.to_str().unwrap(),
                |_, _, _, _, _| {
                    if intro {
                        intro_inputs(true)
                    } else {
                        ClipInputs {
                            intro: None,
                            tone_map: true,
                        }
                    }
                },
                |command, deadline| {
                    attempts += 1;
                    let graph = filter(command);
                    assert_eq!(graph.contains("xfade="), intro);
                    if attempts == 1 {
                        assert!(graph.contains(HDR_TONEMAP));
                        first_deadline = Some(deadline);
                        fs::write(output_path(command), b"partial").unwrap();
                        Ok(DeadlineCommandOutput {
                            success: false,
                            stderr: b"No such filter: zscale".to_vec(),
                            ..successful_output()
                        })
                    } else {
                        assert!(!graph.contains("zscale") && !graph.contains("tonemap="));
                        assert_eq!(deadline, first_deadline.unwrap());
                        fs::write(output_path(command), MP4_HEADER).unwrap();
                        Ok(successful_output())
                    }
                },
            );
            assert!(result.is_ok());
            assert_eq!(attempts, 2);
        }
    }

    #[test]
    fn hdr_timeout_does_not_retry_and_failed_hdr_fallback_stops_after_two_attempts() {
        for timeout in [false, true] {
            let tmp = tempfile::tempdir().unwrap();
            let source = tmp.path().join("hdr.mp4");
            fs::write(&source, b"source").unwrap();
            let mut attempts = 0;
            let result = preview_clip_with(
                tmp.path(),
                source.to_str().unwrap(),
                |_, _, _, _, _| ClipInputs {
                    intro: None,
                    tone_map: true,
                },
                |_, _| {
                    attempts += 1;
                    if timeout {
                        Err(DeadlineCommandError::TimedOut)
                    } else {
                        Ok(DeadlineCommandOutput {
                            success: false,
                            ..successful_output()
                        })
                    }
                },
            );
            assert_eq!(attempts, if timeout { 1 } else { 2 });
            assert_eq!(
                result,
                Err(if timeout {
                    ThumbnailFailure::TimedOut
                } else {
                    ThumbnailFailure::ProbeFailed
                })
            );
            assert_eq!(fs::read_dir(tmp.path()).unwrap().count(), 1);
        }
    }

    #[test]
    fn rejected_intro_retries_the_original_clip_without_the_still() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source.mp4");
        fs::write(&source, b"source").unwrap();
        let mut attempts = 0;
        preview_clip_with(
            tmp.path(),
            source.to_str().unwrap(),
            |_, _, _, _, _| intro_inputs(false),
            |command, _| {
                attempts += 1;
                if attempts == 1 {
                    assert!(filter(command).contains("xfade="));
                    Ok(DeadlineCommandOutput {
                        success: false,
                        ..successful_output()
                    })
                } else {
                    assert_eq!(filter(command), format!("{CLIP_SCALE},{CLIP_TAGS}"));
                    fs::write(output_path(command), MP4_HEADER).unwrap();
                    Ok(successful_output())
                }
            },
        )
        .unwrap();
        assert_eq!(attempts, 2);
    }

    #[test]
    #[ignore = "requires local ffmpeg/libx264 and ffprobe"]
    fn real_ffmpeg_clips_preserve_short_duration_cap_long_duration_and_never_upscale() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = tmp.path().join("cache");
        for (width, height, seconds, expected_width, expected_height, expected_duration) in [
            (1600, 900, 1, 1024, 576, 1.8),
            (960, 1080, 1, 512, 576, 1.8),
            (321, 241, 1, 320, 240, 1.8),
            (320, 240, 16, 320, 240, 15.8),
        ] {
            let source = tmp.path().join(format!("{width}x{height}.mp4"));
            let mut fixture = Command::new("ffmpeg");
            fixture
                .args([
                    "-hide_banner",
                    "-loglevel",
                    "error",
                    "-y",
                    "-filter_threads",
                    "1",
                    "-f",
                    "lavfi",
                    "-i",
                ])
                .arg(format!("testsrc=size={width}x{height}:rate=2"))
                .args([
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=frequency=440:sample_rate=8000",
                    "-t",
                ])
                .arg(seconds.to_string())
                .args([
                    "-c:v",
                    "libx264",
                    "-pix_fmt",
                    "yuv444p",
                    "-preset",
                    "ultrafast",
                    "-threads",
                    "1",
                    "-c:a",
                    "aac",
                ])
                .arg(&source);
            assert!(crate::command_succeeded(
                &mut fixture,
                std::time::Duration::from_secs(10),
                "clip fixture"
            ));
            let (clip, hit) = preview_clip_for(&cache, source.to_str().unwrap()).unwrap();
            assert!(!hit);
            let mut probe = Command::new("ffprobe");
            probe
                .args([
                    "-v",
                    "error",
                    "-show_entries",
                    "stream=codec_type,codec_name,width,height,pix_fmt,r_frame_rate,sample_aspect_ratio,color_range,color_space,color_transfer,color_primaries:format=duration",
                    "-of",
                    "default",
                ])
                .arg(&clip);
            let output =
                crate::run_command_with_deadline(&mut probe, std::time::Duration::from_secs(3))
                    .unwrap();
            assert!(output.success);
            let metadata = String::from_utf8(output.stdout).unwrap();
            assert!(metadata.contains("codec_name=h264\n"));
            assert!(metadata.contains("codec_type=video\n"));
            assert!(!metadata.contains("codec_type=audio"));
            assert!(
                metadata.contains(&format!("width={expected_width}\n")),
                "{metadata}"
            );
            assert!(
                metadata.contains(&format!("height={expected_height}\n")),
                "{metadata}"
            );
            assert!(metadata.contains("pix_fmt=yuv420p\n"));
            assert!(metadata.contains("r_frame_rate=24/1\n"));
            // Incomplete colour tags are what drew green on the NVIDIA desktop: all five must be there.
            for tag in [
                "color_range=tv\n",
                "color_space=bt709\n",
                "color_transfer=bt709\n",
                "color_primaries=bt709\n",
                "sample_aspect_ratio=1:1\n",
            ] {
                assert!(metadata.contains(tag), "{tag} missing in {metadata}");
            }
            let duration: f64 = metadata
                .lines()
                .find_map(|line| line.strip_prefix("duration="))
                .unwrap()
                .parse()
                .unwrap();
            assert!((duration - expected_duration).abs() < 0.04, "{metadata}");
            let meta = fs::metadata(&source).unwrap();
            let mtime = meta
                .modified()
                .unwrap()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let (still, hit) = crate::generate_gui_thumbnail_sized(
                &cache,
                source.to_str().unwrap(),
                mtime,
                meta.len(),
                ThumbnailSize::Large,
            )
            .unwrap();
            assert!(hit, "clip must generate/reuse the same Large cache entry");
            let reference = tmp.path().join("reference.png");
            decode_frame(&still, &reference, None, &format!("scale={expected_width}:{expected_height}:out_color_matrix=bt709:out_range=pc,format=rgb24"));
            let first = tmp.path().join("first.png");
            // Compare in sRGB, the colour space of the picture the views draw.
            // Undo the video's BT.709 transfer before comparing RGB sample values.
            let to_srgb = "colorspace=iall=bt709:all=bt709:trc=srgb:range=pc:format=yuv444p,scale=iw:ih:out_color_matrix=bt709:out_range=pc,format=rgb24";
            decode_frame(&clip, &first, None, to_srgb);
            let difference = mean_absolute_difference(&first, &reference);
            assert!(
                difference < 5.0,
                "first frame/still sRGB MAD = {difference}"
            );
            if seconds > 1 {
                let later = tmp.path().join("later.png");
                // Past the 0.8 s still and the 0.5 s fade.
                decode_frame(&clip, &later, Some("2.0"), to_srgb);
                let difference = mean_absolute_difference(&later, &reference);
                assert!(
                    difference > 5.0,
                    "video after 2 s still resembles intro: MAD = {difference}"
                );
            }
            let bytes = fs::read(&clip).unwrap();
            let moov = bytes.windows(4).position(|bytes| bytes == b"moov").unwrap();
            let mdat = bytes.windows(4).position(|bytes| bytes == b"mdat").unwrap();
            assert!(moov < mdat, "faststart metadata must precede video data");
            assert_eq!(
                preview_clip_for(&cache, source.to_str().unwrap()).unwrap(),
                (clip, true)
            );
        }
    }

    fn decode_frame(source: &Path, output: &Path, seek: Option<&str>, filter: &str) {
        let mut command = Command::new("ffmpeg");
        command.args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-filter_threads",
            "1",
        ]);
        if let Some(seek) = seek {
            command.args(["-ss", seek]);
        }
        command
            .arg("-i")
            .arg(source)
            .args(["-frames:v", "1", "-vf", filter, "-threads", "1"])
            .arg(output);
        assert!(crate::command_succeeded(
            &mut command,
            std::time::Duration::from_secs(5),
            "decode clip frame"
        ));
    }

    fn mean_absolute_difference(a: &Path, b: &Path) -> f64 {
        let a = image::open(a).unwrap().to_rgb8();
        let b = image::open(b).unwrap().to_rgb8();
        assert_eq!(a.dimensions(), b.dimensions());
        a.as_raw()
            .iter()
            .zip(b.as_raw())
            .map(|(a, b)| f64::from(a.abs_diff(*b)))
            .sum::<f64>()
            / a.as_raw().len() as f64
    }

    #[test]
    #[ignore = "requires local ffmpeg/libx264, zscale and ffprobe; tries libx265 first"]
    fn real_ffmpeg_pq_source_is_tone_mapped_and_tagged_bt709() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("hdr.mp4");
        let mut made_fixture = false;
        for codec in ["libx265", "libx264"] {
            let mut fixture = Command::new("ffmpeg");
            fixture.args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-filter_threads",
                "1",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=192x108:rate=24",
                "-t",
                "2",
                "-c:v",
                codec,
                "-pix_fmt",
                if codec == "libx265" {
                    "yuv420p10le"
                } else {
                    "yuv420p"
                },
                "-preset",
                "ultrafast",
                "-threads",
                "1",
                "-color_trc",
                "smpte2084",
                "-colorspace",
                "bt2020nc",
                "-color_primaries",
                "bt2020",
                "-color_range",
                "tv",
            ]);
            if codec == "libx265" {
                fixture.args(["-x265-params", "pools=1:frame-threads=1:log-level=error:colorprim=bt2020:transfer=smpte2084:colormatrix=bt2020nc"]);
            }
            if codec == "libx264" {
                fixture.args([
                    "-x264-params",
                    "colorprim=bt2020:transfer=smpte2084:colormatrix=bt2020nc",
                ]);
            }
            fixture.arg(&source);
            if crate::command_succeeded(
                &mut fixture,
                std::time::Duration::from_secs(10),
                "HDR fixture",
            ) {
                made_fixture = true;
                break;
            }
        }
        assert!(made_fixture);
        let mut source_probe = Command::new("ffprobe");
        source_probe
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=color_transfer",
                "-of",
                "default=noprint_wrappers=1",
            ])
            .arg(&source);
        let output =
            crate::run_command_with_deadline(&mut source_probe, std::time::Duration::from_secs(3))
                .unwrap();
        assert!(output.success);
        assert!(
            parse_video_probe(&String::from_utf8_lossy(&output.stdout)).hdr,
            "fixture must actually be PQ-tagged"
        );
        let cache = tmp.path().join("cache");
        let mut attempts = 0;
        let (clip, _) = preview_clip_with(
            &cache,
            source.to_str().unwrap(),
            prepare_clip_inputs,
            |command, deadline| {
                attempts += 1;
                assert!(
                    filter(command).contains(HDR_TONEMAP),
                    "real fixture must succeed with tone mapping"
                );
                deadline.command_output(command)
            },
        )
        .unwrap();
        assert_eq!(attempts, 1, "fixture must not silently use HDR fallback");
        let mut probe = Command::new("ffprobe");
        probe
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=color_range,color_space,color_transfer,color_primaries,sample_aspect_ratio",
                "-of",
                "default",
            ])
            .arg(&clip);
        let output =
            crate::run_command_with_deadline(&mut probe, std::time::Duration::from_secs(3))
                .unwrap();
        assert!(output.success);
        let metadata = String::from_utf8(output.stdout).unwrap();
        for tag in [
            "color_range=tv\n",
            "color_space=bt709\n",
            "color_transfer=bt709\n",
            "color_primaries=bt709\n",
            "sample_aspect_ratio=1:1\n",
        ] {
            assert!(metadata.contains(tag), "{tag} missing in {metadata}");
        }
    }

    #[test]
    #[ignore = "requires local ffmpeg/libx264 and ffprobe"]
    fn real_ffmpeg_undecodable_large_still_falls_back_to_a_15_second_clip() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source.mp4");
        let mut fixture = Command::new("ffmpeg");
        fixture
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=96x80:rate=24",
                "-t",
                "16",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-threads",
                "1",
            ])
            .arg(&source);
        assert!(crate::command_succeeded(
            &mut fixture,
            std::time::Duration::from_secs(10),
            "no-intro fixture"
        ));
        let meta = fs::metadata(&source).unwrap();
        let mtime = meta
            .modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let cache = tmp.path().join("cache");
        fs::create_dir(&cache).unwrap();
        let still = cache.join(crate::gui_thumb_cache_key(
            source.to_str().unwrap(),
            mtime,
            meta.len(),
            ThumbnailSize::Large,
        ));
        fs::write(still, b"invalid WebP").unwrap();
        let (clip, _) = preview_clip_with(
            &cache,
            source.to_str().unwrap(),
            prepare_clip_inputs,
            |command, deadline| {
                assert!(!filter(command).contains("xfade="));
                deadline.command_output(command)
            },
        )
        .unwrap();
        let mut probe = Command::new("ffprobe");
        probe
            .args([
                "-v",
                "error",
                "-show_entries",
                "format=duration",
                "-of",
                "csv=p=0",
            ])
            .arg(clip);
        let output =
            crate::run_command_with_deadline(&mut probe, std::time::Duration::from_secs(3))
                .unwrap();
        assert!(output.success);
        let duration: f64 = String::from_utf8(output.stdout)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert!((duration - 15.0).abs() < 0.04);
    }
}
