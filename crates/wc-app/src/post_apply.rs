//! Post-apply theme hook: resolve a still image, then run an external command.
//!
//! Failures are logged and never fail the wallpaper apply itself.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use wc_core::types::{Backend, FileType};
use wc_storage::StorageApi;

/// Context for one successful apply that may trigger the post-apply hook.
#[derive(Debug, Clone)]
pub struct PostApplyContext {
    pub wallpaper_path: String,
    pub backend: Backend,
    pub file_type: FileType,
    pub outputs: String,
}

/// Run the configured post-apply hook. Never returns an error to callers;
/// wallpaper apply has already succeeded.
pub fn run_post_apply_hook(storage: &StorageApi, ctx: &PostApplyContext) {
    if let Err(err) = run_post_apply_hook_inner(storage, ctx, None) {
        log::warn!("post-apply hook skipped or failed: {err}");
    }
}

/// Test seam: optional still-path override bypasses ffmpeg (and video cache).
#[cfg(test)]
pub(crate) fn run_post_apply_hook_with_still_override(
    storage: &StorageApi,
    ctx: &PostApplyContext,
    still_override: Option<PathBuf>,
) -> Result<(), String> {
    run_post_apply_hook_inner(storage, ctx, still_override)
}

fn run_post_apply_hook_inner(
    storage: &StorageApi,
    ctx: &PostApplyContext,
    still_override: Option<PathBuf>,
) -> Result<(), String> {
    let enabled = storage.config_get("post_apply_enabled", "off");
    if enabled != "on" {
        return Ok(());
    }

    let command = storage.config_get("post_apply_command", "");
    let command = command.trim();
    if command.is_empty() {
        return Ok(());
    }

    if matches!(ctx.file_type, FileType::WeWeb | FileType::WeApplication) {
        log::info!(
            "post-apply: skipping unsupported Wallpaper Engine type ({})",
            ctx.file_type.as_str()
        );
        return Ok(());
    }

    let still = match still_override {
        Some(path) => path,
        None => resolve_still_path(storage, &ctx.wallpaper_path, ctx.file_type)?,
    };

    let wallpaper = ctx.wallpaper_path.as_str();
    let still_s = still.to_string_lossy();
    let backend = ctx.backend.as_str();
    let outputs = ctx.outputs.as_str();

    let expanded = expand_post_apply_command(command, wallpaper, &still_s, backend, outputs);
    let timeout_secs: u64 = storage
        .config_get("post_apply_timeout_secs", "30")
        .parse()
        .ok()
        .filter(|v| (1..=600).contains(v))
        .unwrap_or(30);

    log::info!("post-apply: running `{expanded}` (timeout {timeout_secs}s)");
    run_command_with_timeout(
        &expanded,
        &[
            ("WCR_WALLPAPER", wallpaper),
            ("WCR_STILL", still_s.as_ref()),
            ("WCR_BACKEND", backend),
            ("WCR_OUTPUTS", outputs),
        ],
        Duration::from_secs(timeout_secs),
    )
}

/// Expand `$wallpaper` / `$path` / `$still` / `$backend` / `$outputs` in the
/// command template. Longer names are replaced before shorter ones so
/// `$wallpaper` is not partially eaten by `$path`.
pub fn expand_post_apply_command(
    template: &str,
    wallpaper: &str,
    still: &str,
    backend: &str,
    outputs: &str,
) -> String {
    let mut out = template.to_string();
    // Replace longer placeholders first.
    for (needle, value) in [
        ("$wallpaper", wallpaper),
        ("$backend", backend),
        ("$outputs", outputs),
        ("$still", still),
        ("$path", wallpaper),
    ] {
        out = out.replace(needle, value);
    }
    out
}

fn resolve_still_path(
    storage: &StorageApi,
    wallpaper_path: &str,
    file_type: FileType,
) -> Result<PathBuf, String> {
    match file_type {
        FileType::Image | FileType::Gif => {
            let path = PathBuf::from(wallpaper_path);
            if !path.is_file() {
                return Err(format!("wallpaper path is not a file: {wallpaper_path}"));
            }
            Ok(path)
        }
        FileType::Video => extract_video_still(storage, wallpaper_path),
        FileType::WeScene => resolve_we_scene_preview(wallpaper_path),
        FileType::WeWeb | FileType::WeApplication => {
            Err("still extraction not supported for this file type".into())
        }
    }
}

fn resolve_we_scene_preview(project_path: &str) -> Result<PathBuf, String> {
    let project_dir = Path::new(project_path);
    let info = wc_scan::read_we_project_info(project_dir).ok_or_else(|| {
        format!(
            "Wallpaper Engine scene metadata could not be read: {}",
            project_dir.display()
        )
    })?;

    if info.entry_type != FileType::WeScene {
        return Err(format!(
            "Wallpaper Engine project is not a scene: {}",
            project_dir.display()
        ));
    }

    let preview = info.preview_path.ok_or_else(|| {
        format!(
            "Wallpaper Engine scene has no safe, readable preview image: {}",
            project_dir.display()
        )
    })?;
    let entry = wc_scan::make_entry(&preview).ok_or_else(|| {
        format!(
            "Wallpaper Engine scene preview is not a supported image: {}",
            preview
        )
    })?;
    if !matches!(entry.file_type, FileType::Image | FileType::Gif) {
        return Err(format!(
            "Wallpaper Engine scene preview is not an image or GIF: {}",
            preview
        ));
    }

    Ok(PathBuf::from(preview))
}

fn path_cache_key(path: &str) -> String {
    // Stable non-crypto fingerprint for cache filenames.
    let mut h: u128 = 0xcbf2_9ce4_8422_2325;
    for b in path.as_bytes() {
        h ^= u128::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:032x}")
}

fn extract_video_still(storage: &StorageApi, video_path: &str) -> Result<PathBuf, String> {
    let video = Path::new(video_path);
    if !video.is_file() {
        return Err(format!("video path is not a file: {video_path}"));
    }

    let cache_dir = storage.cd.theme_stills_cache_dir();
    std::fs::create_dir_all(&cache_dir)
        .map_err(|e| format!("failed to create theme-stills cache: {e}"))?;

    let dest = cache_dir.join(format!("{}.jpg", path_cache_key(video_path)));
    if dest.is_file() {
        return Ok(dest);
    }

    let tmp = cache_dir.join(format!(
        "{}.tmp.{}.jpg",
        path_cache_key(video_path),
        std::process::id()
    ));

    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-ss",
            "1",
            "-i",
            video_path,
            "-frames:v",
            "1",
            "-q:v",
            "2",
        ])
        .arg(&tmp)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| format!("failed to spawn ffmpeg: {e}"))?;

    if !status.success() {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!(
            "ffmpeg failed to extract a frame from {video_path} (exit {status})"
        ));
    }

    if let Err(e) = std::fs::rename(&tmp, &dest) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("failed to finalize theme still: {e}"));
    }

    Ok(dest)
}

fn run_command_with_timeout(
    command: &str,
    env: &[(&str, &str)],
    timeout: Duration,
) -> Result<(), String> {
    // Launch under setsid so we can kill the whole process group on timeout.
    let child = Command::new("setsid")
        .arg("sh")
        .arg("-c")
        .arg(command)
        .envs(env.iter().copied())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to spawn post-apply command: {e}"))?;

    let pid = child.id();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let output = child.wait_with_output();
        let _ = tx.send(output);
    });

    match rx.recv_timeout(timeout) {
        Ok(Ok(output)) => {
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stdout = String::from_utf8_lossy(&output.stdout);
                let mut msg = format!("post-apply command exited with {}", output.status);
                if !stderr.trim().is_empty() {
                    msg.push_str(": ");
                    msg.push_str(stderr.trim());
                } else if !stdout.trim().is_empty() {
                    msg.push_str(": ");
                    msg.push_str(stdout.trim());
                }
                return Err(msg);
            }
            Ok(())
        }
        Ok(Err(e)) => Err(format!("post-apply command wait failed: {e}")),
        Err(_) => {
            // Kill the process group (negative PID) then wait for the waiter thread.
            // SAFETY: pid came from our setsid child; negative kills the group.
            unsafe {
                let _ = libc::kill(-(pid as i32), libc::SIGKILL);
            }
            // Drain the waiter so we do not leave an orphaned JoinHandle forever.
            let _ = rx.recv_timeout(Duration::from_secs(2));
            Err(format!(
                "post-apply command timed out after {}s and was killed",
                timeout.as_secs()
            ))
        }
    }
}

/// Write a small helper used only by integration-style unit tests.
#[cfg(test)]
#[allow(dead_code)]
fn write_executable_script(path: &Path, body: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut file = std::fs::File::create(path)?;
    file.write_all(body.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = file.metadata()?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wc_config::ConfigDirExt;
    use wc_core::config::ConfigDir;

    fn temp_storage() -> (tempfile::TempDir, StorageApi) {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().to_path_buf(),
        };
        cd.init().unwrap();
        let storage = StorageApi::new(cd);
        (tmp, storage)
    }

    #[test]
    fn expand_replaces_all_placeholders() {
        let expanded = expand_post_apply_command(
            r#"echo $wallpaper $path $still $backend $outputs"#,
            "/walls/a.png",
            "/cache/a.jpg",
            "awww",
            "eDP-1",
        );
        assert_eq!(
            expanded,
            r#"echo /walls/a.png /walls/a.png /cache/a.jpg awww eDP-1"#
        );
    }

    #[test]
    fn expand_wallpaper_before_path_substring() {
        // `$path` must not corrupt `$wallpaper` when both appear.
        let expanded = expand_post_apply_command(
            "matugen image $wallpaper",
            "/w/p.png",
            "/s.jpg",
            "awww",
            "*",
        );
        assert_eq!(expanded, "matugen image /w/p.png");
    }

    #[test]
    fn disabled_hook_is_noop() {
        let (_tmp, storage) = temp_storage();
        storage.config_set("post_apply_enabled", "off").unwrap();
        storage.config_set("post_apply_command", "false").unwrap();
        let ctx = PostApplyContext {
            wallpaper_path: "/nope.png".into(),
            backend: Backend::Awww,
            file_type: FileType::Image,
            outputs: "*".into(),
        };
        run_post_apply_hook_with_still_override(&storage, &ctx, None).unwrap();
    }

    #[test]
    fn empty_command_is_noop_when_enabled() {
        let (_tmp, storage) = temp_storage();
        storage.config_set("post_apply_enabled", "on").unwrap();
        storage.config_set("post_apply_command", "   ").unwrap();
        let ctx = PostApplyContext {
            wallpaper_path: "/nope.png".into(),
            backend: Backend::Awww,
            file_type: FileType::Image,
            outputs: "*".into(),
        };
        run_post_apply_hook_with_still_override(&storage, &ctx, None).unwrap();
    }

    #[test]
    fn we_scene_runs_hook_with_project_preview() {
        let (tmp, storage) = temp_storage();
        let marker = tmp.path().join("marker.txt");
        let script = tmp.path().join("hook.sh");
        write_executable_script(
            &script,
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"$WCR_STILL\" > '{}'\n",
                marker.display()
            ),
        )
        .unwrap();

        let project = tmp.path().join("scene");
        std::fs::create_dir(&project).unwrap();
        let preview = project.join("preview.png");
        std::fs::write(
            &preview,
            [
                0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
                0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x04, 0x00, 0x00,
                0x00, 0xb5, 0x1c, 0x0c, 0x02, 0x00, 0x00, 0x00, 0x0b, 0x49, 0x44, 0x41, 0x54, 0x78,
                0xda, 0x63, 0x64, 0xf8, 0x0f, 0x00, 0x01, 0x05, 0x01, 0x01, 0x27, 0x18, 0xe3, 0x66,
                0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
            ],
        )
        .unwrap();
        std::fs::write(
            project.join("project.json"),
            r#"{"type":"scene","file":"scene.json","preview":"preview.png"}"#,
        )
        .unwrap();

        storage.config_set("post_apply_enabled", "on").unwrap();
        storage
            .config_set("post_apply_command", &format!("\"{}\"", script.display()))
            .unwrap();
        let ctx = PostApplyContext {
            wallpaper_path: project.to_string_lossy().into_owned(),
            backend: Backend::LinuxWallpaperEngine,
            file_type: FileType::WeScene,
            outputs: "*".into(),
        };
        run_post_apply_hook_with_still_override(&storage, &ctx, None).unwrap();

        assert_eq!(
            std::fs::read_to_string(marker).unwrap().trim(),
            preview.canonicalize().unwrap().to_string_lossy()
        );
    }

    #[test]
    fn we_scene_rejects_preview_outside_project() {
        let (tmp, storage) = temp_storage();
        let project = tmp.path().join("scene");
        std::fs::create_dir(&project).unwrap();
        std::fs::write(tmp.path().join("outside.png"), b"outside").unwrap();
        std::fs::write(
            project.join("project.json"),
            r#"{"type":"scene","file":"scene.json","preview":"../outside.png"}"#,
        )
        .unwrap();

        storage.config_set("post_apply_enabled", "on").unwrap();
        storage.config_set("post_apply_command", "false").unwrap();
        let ctx = PostApplyContext {
            wallpaper_path: project.to_string_lossy().into_owned(),
            backend: Backend::LinuxWallpaperEngine,
            file_type: FileType::WeScene,
            outputs: "*".into(),
        };

        let err = run_post_apply_hook_with_still_override(&storage, &ctx, None).unwrap_err();
        assert!(
            err.contains("no safe, readable preview image"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn we_web_and_application_remain_skipped() {
        let (_tmp, storage) = temp_storage();
        storage.config_set("post_apply_enabled", "on").unwrap();
        storage.config_set("post_apply_command", "false").unwrap();

        for file_type in [FileType::WeWeb, FileType::WeApplication] {
            let ctx = PostApplyContext {
                wallpaper_path: "/missing-project".into(),
                backend: Backend::Unsupported,
                file_type,
                outputs: "*".into(),
            };
            run_post_apply_hook_with_still_override(&storage, &ctx, None).unwrap();
        }
    }

    #[test]
    fn hook_runs_command_with_env_and_still() {
        let (tmp, storage) = temp_storage();
        let marker = tmp.path().join("marker.txt");
        let script = tmp.path().join("hook.sh");
        write_executable_script(
            &script,
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"$WCR_WALLPAPER\" \"$WCR_STILL\" \"$WCR_BACKEND\" \"$WCR_OUTPUTS\" > '{}'\n",
                marker.display()
            ),
        )
        .unwrap();

        let still = tmp.path().join("still.jpg");
        std::fs::write(&still, b"fake").unwrap();
        let wall = tmp.path().join("wall.png");
        std::fs::write(&wall, b"fake").unwrap();

        storage.config_set("post_apply_enabled", "on").unwrap();
        storage
            .config_set("post_apply_command", &format!("\"{}\"", script.display()))
            .unwrap();

        let ctx = PostApplyContext {
            wallpaper_path: wall.to_string_lossy().into_owned(),
            backend: Backend::Awww,
            file_type: FileType::Image,
            outputs: "eDP-1,HDMI-A-1".into(),
        };
        run_post_apply_hook_with_still_override(&storage, &ctx, Some(still.clone())).unwrap();

        let body = std::fs::read_to_string(&marker).unwrap();
        let lines: Vec<_> = body.lines().collect();
        assert_eq!(lines[0], wall.to_string_lossy());
        assert_eq!(lines[1], still.to_string_lossy());
        assert_eq!(lines[2], "awww");
        assert_eq!(lines[3], "eDP-1,HDMI-A-1");
    }

    #[test]
    fn failing_command_does_not_panic_and_returns_err_to_inner() {
        let (tmp, storage) = temp_storage();
        let still = tmp.path().join("still.jpg");
        std::fs::write(&still, b"fake").unwrap();

        storage.config_set("post_apply_enabled", "on").unwrap();
        storage.config_set("post_apply_command", "false").unwrap();

        let ctx = PostApplyContext {
            wallpaper_path: still.to_string_lossy().into_owned(),
            backend: Backend::Awww,
            file_type: FileType::Image,
            outputs: "*".into(),
        };
        let err = run_post_apply_hook_with_still_override(&storage, &ctx, Some(still)).unwrap_err();
        assert!(
            err.contains("exited"),
            "expected exit failure message, got: {err}"
        );
    }

    #[test]
    fn path_cache_key_is_stable() {
        assert_eq!(
            path_cache_key("/home/u/a.mp4"),
            path_cache_key("/home/u/a.mp4")
        );
        assert_ne!(
            path_cache_key("/home/u/a.mp4"),
            path_cache_key("/home/u/b.mp4")
        );
    }

    #[test]
    fn timeout_kills_long_running_command() {
        let (tmp, storage) = temp_storage();
        let still = tmp.path().join("still.jpg");
        std::fs::write(&still, b"fake").unwrap();

        storage.config_set("post_apply_enabled", "on").unwrap();
        storage
            .config_set("post_apply_command", "sleep 30")
            .unwrap();
        storage.config_set("post_apply_timeout_secs", "1").unwrap();

        let ctx = PostApplyContext {
            wallpaper_path: still.to_string_lossy().into_owned(),
            backend: Backend::Awww,
            file_type: FileType::Image,
            outputs: "*".into(),
        };
        let err = run_post_apply_hook_with_still_override(&storage, &ctx, Some(still)).unwrap_err();
        assert!(
            err.contains("timed out"),
            "expected timeout message, got: {err}"
        );
    }

    #[test]
    fn public_run_swallows_command_failure() {
        let (tmp, storage) = temp_storage();
        let still = tmp.path().join("still.jpg");
        std::fs::write(&still, b"fake").unwrap();
        storage.config_set("post_apply_enabled", "on").unwrap();
        storage.config_set("post_apply_command", "false").unwrap();
        let ctx = PostApplyContext {
            wallpaper_path: still.to_string_lossy().into_owned(),
            backend: Backend::Awww,
            file_type: FileType::Image,
            outputs: "*".into(),
        };
        // Must not panic even when the command fails.
        run_post_apply_hook(&storage, &ctx);
    }
}
