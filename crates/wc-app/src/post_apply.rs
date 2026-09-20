//! Post-apply theme hook: publish per-output theme state, resolve stills, run
//! an optional external command.
//!
//! Failures are logged and never fail the wallpaper apply itself.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;
use wc_core::types::{Backend, FileType};
use wc_storage::sqlite::DisplayStateTarget;
use wc_storage::StorageApi;

use crate::theme_source::ThemeSourcePolicy;

/// One connected output's wallpaper + still for theme generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputThemeEntry {
    pub output: String,
    pub wallpaper: String,
    pub backend: String,
    pub file_type: String,
    /// Filled during publish; callers may leave this `None`.
    pub still: Option<String>,
}

/// Context for one successful apply/restore that may publish theme state and
/// trigger the post-apply hook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostApplyContext {
    /// Theme-source wallpaper (compat / WCR_WALLPAPER).
    pub wallpaper_path: String,
    pub backend: Backend,
    pub file_type: FileType,
    /// Changed outputs joined (compat WCR_OUTPUTS / WCR_OUTPUT).
    pub outputs: String,
    pub changed_outputs: Vec<String>,
    pub theme_source_output: Option<String>,
    pub per_output: Vec<OutputThemeEntry>,
}

/// The just-applied wallpaper, when publishing after Apply.
pub struct AppliedThemeSource<'a> {
    pub wallpaper: &'a str,
    pub backend: Backend,
    pub file_type: FileType,
}

/// Everything the theme publisher needs to build a [`PostApplyContext`].
/// Per-output derivation, theme-source policy and fallback selection stay
/// inside this module; Apply/Restore callers only supply raw facts.
pub struct ThemePublishRequest<'a> {
    /// Intended display rows: target, wallpaper path, backend string.
    pub intended: &'a [(DisplayStateTarget, String, String)],
    pub known_outputs: &'a [String],
    pub changed_outputs: &'a [String],
    /// `Some` after Apply (the just-applied target); `None` after Restore,
    /// where the fallback is derived from the first connected `intended` row.
    pub applied: Option<AppliedThemeSource<'a>>,
}

/// Build the publish context from intended display rows.
pub fn build_theme_context(
    storage: &StorageApi,
    request: &ThemePublishRequest<'_>,
) -> PostApplyContext {
    let per_output = per_output_theme_entries(request.intended, request.known_outputs);
    let policy_raw = storage.config_get("post_apply_theme_source", "last_applied");
    let policy = ThemeSourcePolicy::parse(&policy_raw);
    let focused = if matches!(policy, ThemeSourcePolicy::Focused) {
        crate::theme_source::probe_focused_output()
    } else {
        None
    };
    let theme_source_output = crate::theme_source::select_theme_source(
        &policy,
        request.changed_outputs,
        request.known_outputs,
        focused.as_deref(),
    );

    let (fallback_wallpaper, fallback_backend, fallback_file_type) = match &request.applied {
        Some(applied) => (
            applied.wallpaper.to_string(),
            applied.backend,
            applied.file_type,
        ),
        None => restore_fallback_theme_source(request.intended, request.known_outputs),
    };

    let (wallpaper_path, backend, file_type) = theme_source_output
        .as_ref()
        .and_then(|name| {
            per_output
                .iter()
                .find(|entry| &entry.output == name)
                .map(|entry| {
                    (
                        entry.wallpaper.clone(),
                        parse_state_backend(&entry.backend).unwrap_or(fallback_backend),
                        file_type_from_str(&entry.file_type),
                    )
                })
        })
        .unwrap_or((fallback_wallpaper, fallback_backend, fallback_file_type));

    PostApplyContext {
        wallpaper_path,
        backend,
        file_type,
        outputs: request.changed_outputs.join(","),
        changed_outputs: request.changed_outputs.to_vec(),
        theme_source_output,
        per_output,
    }
}

/// Restore has no just-applied target; fall back to the first connected
/// intended row so the theme source still points at a real wallpaper.
fn restore_fallback_theme_source(
    intended: &[(DisplayStateTarget, String, String)],
    known_outputs: &[String],
) -> (String, Backend, FileType) {
    let (wallpaper, backend_str) = intended
        .iter()
        .find_map(|(target, path, backend)| match target {
            DisplayStateTarget::AllDisplays => Some((path.clone(), backend.clone())),
            DisplayStateTarget::Output(name) if known_outputs.iter().any(|o| o == name) => {
                Some((path.clone(), backend.clone()))
            }
            _ => None,
        })
        .unwrap_or_else(|| (String::new(), Backend::Awww.as_str().to_string()));
    let backend = parse_state_backend(&backend_str).unwrap_or(Backend::Awww);
    let file_type = file_type_from_str(&detect_file_type_string(&wallpaper));
    (wallpaper, backend, file_type)
}

/// Parse a persisted display-state backend string; unknown values are not
/// renderable and yield `None` so callers can fall back.
pub(crate) fn parse_state_backend(raw: &str) -> Option<Backend> {
    Some(match raw {
        "awww" => Backend::Awww,
        "mpvpaper" => Backend::Mpvpaper,
        "swaybg" => Backend::Swaybg,
        "feh" => Backend::Feh,
        "linux-wallpaperengine" => Backend::LinuxWallpaperEngine,
        _ => return None,
    })
}

fn per_output_theme_entries(
    intended: &[(DisplayStateTarget, String, String)],
    known_outputs: &[String],
) -> Vec<OutputThemeEntry> {
    use std::collections::HashMap;

    let mut map: HashMap<String, (String, String)> = HashMap::new();

    if let Some((_, path, backend)) = intended
        .iter()
        .find(|(target, _, _)| matches!(target, DisplayStateTarget::AllDisplays))
    {
        for output in known_outputs {
            map.insert(output.clone(), (path.clone(), backend.clone()));
        }
    }

    for (target, path, backend) in intended {
        let DisplayStateTarget::Output(name) = target else {
            continue;
        };
        if !known_outputs.iter().any(|known| known == name) {
            continue;
        }
        map.insert(name.clone(), (path.clone(), backend.clone()));
    }

    known_outputs
        .iter()
        .filter_map(|output| {
            let (wallpaper, backend) = map.get(output)?;
            Some(OutputThemeEntry {
                output: output.clone(),
                wallpaper: wallpaper.clone(),
                backend: backend.clone(),
                file_type: detect_file_type_string(wallpaper),
                still: None,
            })
        })
        .collect()
}

/// Versioned result of the optional action, never the wallpaper result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostApplyReport {
    pub version: u32,
    pub status: PostApplyStatus,
    pub detail: String,
    pub reason: String,
    pub finished_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PostApplyStatus {
    Disabled,
    Skipped,
    Succeeded,
    Failed,
    TimedOut,
}

#[derive(Serialize, Deserialize)]
struct SavedAction {
    context: PostApplyContext,
    report: PostApplyReport,
}

pub fn last_report(storage: &StorageApi) -> Result<Option<PostApplyReport>, String> {
    Ok(read_saved_action(storage)?.map(|saved| saved.report))
}

fn read_saved_action(storage: &StorageApi) -> Result<Option<SavedAction>, String> {
    match std::fs::read(storage.cd.path.join("post-apply-state.json")) {
        Ok(bytes) => {
            let saved: SavedAction = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            if saved.report.version != 1 {
                return Err("Unsupported post-apply state version".into());
            }
            Ok(Some(saved))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Repeat only the action for the last published wallpaper context. The same
/// cross-process lock as Apply/Restore prevents retrying an obsolete context
/// while another wallpaper is being applied. No renderer is touched.
pub fn retry_last_action(storage: &StorageApi) -> Result<PostApplyReport, String> {
    let _guard = crate::output_recovery::RendererMutationGuard::acquire(storage)
        .map_err(|e| e.to_string())?;
    let saved = read_saved_action(storage)?
        .ok_or("Apply a wallpaper before running the post-apply action.")?;
    Ok(publish_theme_and_run_hook_for_reason(
        storage,
        &saved.context,
        "retry",
    ))
}

pub fn publish_theme_and_run_hook(storage: &StorageApi, ctx: &PostApplyContext) -> PostApplyReport {
    publish_theme_and_run_hook_for_reason(storage, ctx, "apply")
}

pub fn publish_theme_and_run_hook_for_reason(
    storage: &StorageApi,
    ctx: &PostApplyContext,
    reason: &str,
) -> PostApplyReport {
    let result = publish_theme_and_run_hook_inner(storage, ctx, None, reason);
    let (status, detail) = match result {
        Ok(outcome) => outcome,
        Err(error) => {
            log::warn!("post-apply action failed: {error}");
            let status = if error.starts_with("post-apply command timed out") {
                PostApplyStatus::TimedOut
            } else {
                PostApplyStatus::Failed
            };
            (status, error)
        }
    };
    let mut report = PostApplyReport {
        version: 1,
        status,
        detail,
        reason: reason.into(),
        finished_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    };
    let save = || -> Result<(), String> {
        let saved = SavedAction {
            context: ctx.clone(),
            report: report.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&saved).map_err(|e| e.to_string())?;
        let path = storage.cd.path.join("post-apply-state.json");
        let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
        std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
    };
    if let Err(error) = save() {
        log::warn!("Could not save post-apply result: {error}");
        report
            .detail
            .push_str(&format!(" Result could not be saved: {error}"));
    }
    report
}

/// Backward-compatible alias for [`publish_theme_and_run_hook`].
pub fn run_post_apply_hook(storage: &StorageApi, ctx: &PostApplyContext) {
    publish_theme_and_run_hook(storage, ctx);
}

/// Test seam: optional still-path override bypasses ffmpeg (and video cache)
/// for the theme-source wallpaper only. Per-output stills still resolve unless
/// overridden entries are pre-filled.
#[cfg(test)]
pub(crate) fn run_post_apply_hook_with_still_override(
    storage: &StorageApi,
    ctx: &PostApplyContext,
    still_override: Option<PathBuf>,
) -> Result<(), String> {
    publish_theme_and_run_hook_inner(storage, ctx, still_override, "test").map(|_| ())
}

fn publish_theme_and_run_hook_inner(
    storage: &StorageApi,
    ctx: &PostApplyContext,
    still_override: Option<PathBuf>,
    reason: &str,
) -> Result<(PostApplyStatus, String), String> {
    let policy_raw = storage.config_get("post_apply_theme_source", "last_applied");
    let policy = ThemeSourcePolicy::parse(&policy_raw);

    let mut resolved_entries = ctx.per_output.clone();
    for entry in &mut resolved_entries {
        if entry.still.is_some() {
            continue;
        }
        let file_type = file_type_from_str(&entry.file_type);
        match resolve_still_path(storage, &entry.wallpaper, file_type) {
            Ok(path) => entry.still = Some(path.to_string_lossy().into_owned()),
            Err(err) => {
                log::warn!(
                    "post-apply: still resolve failed for {}: {err}",
                    entry.output
                );
                entry.still = None;
            }
        }
    }

    let manifest_path = if !resolved_entries.is_empty() {
        let path = write_theme_state_manifest(
            storage,
            &ctx.changed_outputs,
            ctx.theme_source_output.as_deref(),
            &policy,
            &resolved_entries,
        )?;
        Some(path)
    } else {
        None
    };

    let enabled = storage.config_get("post_apply_enabled", "off");
    if enabled != "on" {
        return Ok((
            PostApplyStatus::Disabled,
            "Post-apply action is disabled.".into(),
        ));
    }

    let command = storage.config_get("post_apply_command", "");
    let command = command.trim();
    if command.is_empty() {
        return Ok((
            PostApplyStatus::Skipped,
            "No post-apply command is configured.".into(),
        ));
    }

    if matches!(ctx.file_type, FileType::WeWeb | FileType::WeApplication) {
        log::info!(
            "post-apply: skipping unsupported Wallpaper Engine type ({})",
            ctx.file_type.as_str()
        );
        return Ok((
            PostApplyStatus::Skipped,
            "This wallpaper type has no supported still image.".into(),
        ));
    }

    wc_core::config::validate_config_entry("post_apply_command", command)
        .map_err(|e| e.to_string())?;
    let theme_source_still =
        resolve_theme_source_still(storage, ctx, &resolved_entries, still_override.as_deref())?;

    let wallpaper = ctx.wallpaper_path.as_str();
    let still_s = theme_source_still.to_string_lossy();
    let backend = ctx.backend.as_str();
    let outputs = ctx.outputs.as_str();
    let manifest_s = manifest_path
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let theme_source = ctx.theme_source_output.as_deref().unwrap_or("");

    let timeout_secs: u64 = storage
        .config_get("post_apply_timeout_secs", "30")
        .parse()
        .ok()
        .filter(|v| (1..=600).contains(v))
        .unwrap_or(30);

    log::info!("post-apply: running action (timeout {timeout_secs}s)");

    let env: Vec<(&str, String)> = vec![
        ("WCR_HOOK_VERSION", "1".into()),
        ("WCR_REASON", reason.into()),
        ("WCR_FILE_TYPE", ctx.file_type.as_str().into()),
        ("WCR_THEME_MANIFEST", manifest_s.clone()),
        ("WCR_THEME_SOURCE_OUTPUT", theme_source.into()),
        // Compatibility aliases use shell expansion, never source interpolation.
        ("wallpaper", wallpaper.into()),
        ("path", wallpaper.into()),
        ("still", still_s.to_string()),
        ("backend", backend.into()),
        ("outputs", outputs.into()),
        ("manifest", manifest_s.clone()),
        ("theme_source", theme_source.into()),
        ("WCR_WALLPAPER", wallpaper.to_string()),
        ("WCR_STILL", still_s.into_owned()),
        ("WCR_BACKEND", backend.to_string()),
        ("WCR_OUTPUTS", outputs.to_string()),
        ("WCR_OUTPUT", outputs.to_string()),
    ];
    let env_refs: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    run_command_with_timeout(command, &env_refs, Duration::from_secs(timeout_secs))?;
    Ok((
        PostApplyStatus::Succeeded,
        "Post-apply command completed. Desktop refresh is managed by your command.".into(),
    ))
}

fn resolve_theme_source_still(
    storage: &StorageApi,
    ctx: &PostApplyContext,
    resolved_entries: &[OutputThemeEntry],
    still_override: Option<&Path>,
) -> Result<PathBuf, String> {
    if let Some(path) = still_override {
        return Ok(path.to_path_buf());
    }

    if let Some(name) = ctx.theme_source_output.as_deref() {
        if let Some(entry) = resolved_entries.iter().find(|e| e.output == name) {
            if let Some(still) = entry.still.as_ref() {
                return Ok(PathBuf::from(still));
            }
            return Err(format!(
                "theme-source still missing for output {name}; skipping hook"
            ));
        }
    }

    // Legacy / empty per_output: resolve from theme-source wallpaper fields.
    resolve_still_path(storage, &ctx.wallpaper_path, ctx.file_type)
}

fn write_theme_state_manifest(
    storage: &StorageApi,
    changed_outputs: &[String],
    theme_source_output: Option<&str>,
    policy: &ThemeSourcePolicy,
    entries: &[OutputThemeEntry],
) -> Result<PathBuf, String> {
    let path = storage.cd.theme_state_path();
    let mut outputs = serde_json::Map::new();
    for entry in entries {
        outputs.insert(
            entry.output.clone(),
            json!({
                "wallpaper": entry.wallpaper,
                "still": entry.still,
                "backend": entry.backend,
                "file_type": entry.file_type,
            }),
        );
    }

    let doc = json!({
        "version": 1,
        "changed_outputs": changed_outputs,
        "theme_source_output": theme_source_output,
        "theme_source_policy": policy.as_config_str(),
        "outputs": outputs,
    });

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("failed to create config dir for theme-state: {e}"))?;

    let tmp = path.with_extension(format!("tmp.{}.json", std::process::id()));
    let body = serde_json::to_vec_pretty(&doc)
        .map_err(|e| format!("failed to serialize theme-state: {e}"))?;
    std::fs::write(&tmp, body).map_err(|e| format!("failed to write theme-state tmp: {e}"))?;
    if let Err(e) = std::fs::rename(&tmp, &path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("failed to finalize theme-state: {e}"));
    }
    Ok(path)
}

pub(crate) fn file_type_from_str(raw: &str) -> FileType {
    match raw {
        "image" => FileType::Image,
        "gif" => FileType::Gif,
        "video" => FileType::Video,
        "we_scene" => FileType::WeScene,
        "we_web" => FileType::WeWeb,
        "unsupported" | "we_application" => FileType::WeApplication,
        _ => FileType::Image,
    }
}

pub(crate) fn detect_file_type_string(wallpaper_path: &str) -> String {
    wc_scan::make_entry(wallpaper_path)
        .map(|entry| entry.file_type.as_str().to_string())
        .unwrap_or_else(|| "image".to_string())
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

fn read_diagnostic(mut pipe: impl std::io::Read) -> Vec<u8> {
    use std::io::Read;
    let mut retained = Vec::new();
    let _ = pipe.by_ref().take(16 * 1024).read_to_end(&mut retained);
    let discarded = std::io::copy(&mut pipe, &mut std::io::sink()).unwrap_or(0);
    if discarded > 0 {
        retained.extend_from_slice(b"\n[output truncated]");
    }
    retained
}

fn run_command_with_timeout(
    command: &str,
    env: &[(&str, &str)],
    timeout: Duration,
) -> Result<(), String> {
    // Launch under setsid so we can kill the whole process group on timeout.
    let mut child = Command::new("setsid")
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
    // Drain both pipes even after the retained diagnostic limit, so noisy tools
    // cannot exhaust memory or block on a full pipe.
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let out = thread::spawn(move || read_diagnostic(stdout));
        let err = thread::spawn(move || read_diagnostic(stderr));
        let status = child.wait();
        let stdout = out.join().unwrap_or_default();
        let stderr = err.join().unwrap_or_default();
        let _ = tx.send(status.map(|status| std::process::Output {
            status,
            stdout,
            stderr,
        }));
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

    fn basic_ctx(
        wallpaper: String,
        backend: Backend,
        file_type: FileType,
        outputs: &str,
    ) -> PostApplyContext {
        PostApplyContext {
            wallpaper_path: wallpaper,
            backend,
            file_type,
            outputs: outputs.into(),
            changed_outputs: outputs
                .split(',')
                .filter(|s| !s.is_empty() && *s != "*")
                .map(str::to_string)
                .collect(),
            theme_source_output: None,
            per_output: vec![],
        }
    }

    fn output_row(output: &str, path: &str, backend: &str) -> (DisplayStateTarget, String, String) {
        (
            DisplayStateTarget::Output(output.to_string()),
            path.to_string(),
            backend.to_string(),
        )
    }

    #[test]
    fn build_theme_context_derives_per_output_and_last_applied_source() {
        let (tmp, storage) = temp_storage();
        let image = tmp.path().join("a.png");
        let video = tmp.path().join("b.mp4");
        std::fs::write(&image, b"png").unwrap();
        std::fs::write(&video, b"mp4").unwrap();
        let image = image.to_string_lossy().into_owned();
        let video = video.to_string_lossy().into_owned();
        let known = vec!["eDP-1".to_string(), "DP-8".to_string()];
        let intended = vec![
            output_row("eDP-1", &image, "awww"),
            output_row("DP-8", &video, "mpvpaper"),
        ];
        let changed = vec!["DP-8".to_string()];
        let ctx = build_theme_context(
            &storage,
            &ThemePublishRequest {
                intended: &intended,
                known_outputs: &known,
                changed_outputs: &changed,
                applied: Some(AppliedThemeSource {
                    wallpaper: &video,
                    backend: Backend::Mpvpaper,
                    file_type: FileType::Video,
                }),
            },
        );
        assert_eq!(ctx.per_output.len(), 2);
        assert_eq!(ctx.theme_source_output.as_deref(), Some("DP-8"));
        assert_eq!(ctx.wallpaper_path, video);
        assert_eq!(ctx.backend, Backend::Mpvpaper);
        assert_eq!(ctx.file_type, FileType::Video);
        assert_eq!(ctx.outputs, "DP-8");
    }

    #[test]
    fn build_theme_context_restore_excludes_disconnected_outputs() {
        let (_tmp, storage) = temp_storage();
        let known = vec!["eDP-1".to_string()];
        let intended = vec![
            output_row("DP-8", "/walls/off.png", "awww"),
            output_row("eDP-1", "/walls/on.png", "awww"),
        ];
        let ctx = build_theme_context(
            &storage,
            &ThemePublishRequest {
                intended: &intended,
                known_outputs: &known,
                changed_outputs: &known,
                applied: None,
            },
        );
        assert_eq!(ctx.per_output.len(), 1);
        assert_eq!(ctx.per_output[0].output, "eDP-1");
        assert_eq!(ctx.wallpaper_path, "/walls/on.png");
        assert_eq!(ctx.backend, Backend::Awww);
    }

    #[test]
    fn build_theme_context_without_known_outputs_uses_applied_fallback() {
        let (_tmp, storage) = temp_storage();
        let intended = vec![output_row("eDP-1", "/walls/a.png", "awww")];
        let ctx = build_theme_context(
            &storage,
            &ThemePublishRequest {
                intended: &intended,
                known_outputs: &[],
                changed_outputs: &[],
                applied: Some(AppliedThemeSource {
                    wallpaper: "/walls/new.mp4",
                    backend: Backend::Mpvpaper,
                    file_type: FileType::Video,
                }),
            },
        );
        assert!(ctx.per_output.is_empty());
        assert_eq!(ctx.theme_source_output, None);
        assert_eq!(ctx.wallpaper_path, "/walls/new.mp4");
        assert_eq!(ctx.backend, Backend::Mpvpaper);
    }

    #[test]
    fn build_theme_context_restore_without_known_outputs_uses_first_intended_row() {
        let (_tmp, storage) = temp_storage();
        let intended = vec![(
            DisplayStateTarget::AllDisplays,
            "/walls/all.png".to_string(),
            "awww".to_string(),
        )];
        let ctx = build_theme_context(
            &storage,
            &ThemePublishRequest {
                intended: &intended,
                known_outputs: &[],
                changed_outputs: &[],
                applied: None,
            },
        );
        assert_eq!(ctx.wallpaper_path, "/walls/all.png");
        assert_eq!(ctx.backend, Backend::Awww);
        assert_eq!(ctx.file_type, FileType::Image);
    }

    #[test]
    fn parse_state_backend_roundtrips_renderable_backends() {
        for backend in [
            Backend::Awww,
            Backend::Mpvpaper,
            Backend::Swaybg,
            Backend::Feh,
            Backend::LinuxWallpaperEngine,
        ] {
            assert_eq!(parse_state_backend(backend.as_str()), Some(backend));
        }
        assert_eq!(parse_state_backend("unsupported"), None);
        assert_eq!(parse_state_backend(""), None);
    }

    #[test]
    fn versioned_environment_preserves_special_paths_and_retry_uses_new_command() {
        let (tmp, storage) = temp_storage();
        let path = tmp
            .path()
            .join("中文 space \" ' $(touch INJECTED) `false` $still.png");
        std::fs::write(&path, b"image").unwrap();
        storage.config_set("post_apply_enabled", "on").unwrap();
        let marker = tmp.path().join("capture");
        storage.config_set("post_apply_command", &format!(
            "printf '%s\\n' \"$WCR_HOOK_VERSION\" \"$WCR_REASON\" \"$WCR_STILL\" \"$still\" \"$WCR_THEME_MANIFEST\" > '{}'", marker.display()
        )).unwrap();
        let ctx = basic_ctx(
            path.to_string_lossy().into_owned(),
            Backend::Awww,
            FileType::Image,
            "*",
        );
        let report = publish_theme_and_run_hook(&storage, &ctx);
        assert_eq!(report.status, PostApplyStatus::Succeeded, "{report:?}");
        let actual = std::fs::read_to_string(&marker).unwrap();
        assert_eq!(
            actual,
            format!("1\napply\n{}\n{}\n\n", path.display(), path.display())
        );
        storage
            .config_set("post_apply_command", "printf 'test failure' >&2; exit 7")
            .unwrap();
        let failed = retry_last_action(&storage).unwrap();
        assert_eq!(failed.status, PostApplyStatus::Failed);
        assert_eq!(failed.reason, "retry");
        assert!(failed.detail.contains("test failure"));
        assert_eq!(last_report(&storage).unwrap(), Some(failed));
        storage.config_set("post_apply_command", "true").unwrap();
        assert_eq!(
            retry_last_action(&storage).unwrap().status,
            PostApplyStatus::Succeeded
        );
        assert!(storage.current_read().unwrap().is_none());
        assert!(storage.display_state_list().unwrap().is_empty());
    }

    #[test]
    fn result_distinguishes_disabled_skipped_failed_and_timeout() {
        let (tmp, storage) = temp_storage();
        let path = tmp.path().join("image.png");
        std::fs::write(&path, b"image").unwrap();
        let ctx = basic_ctx(
            path.to_string_lossy().into_owned(),
            Backend::Awww,
            FileType::Image,
            "*",
        );
        assert!(retry_last_action(&storage).is_err());
        assert_eq!(
            publish_theme_and_run_hook(&storage, &ctx).status,
            PostApplyStatus::Disabled
        );
        storage.config_set("post_apply_enabled", "on").unwrap();
        storage.config_set("post_apply_command", "").unwrap();
        assert_eq!(
            publish_theme_and_run_hook(&storage, &ctx).status,
            PostApplyStatus::Skipped
        );
        storage
            .config_set("post_apply_command", "sleep 20")
            .unwrap();
        storage.config_set("post_apply_timeout_secs", "1").unwrap();
        assert_eq!(
            publish_theme_and_run_hook(&storage, &ctx).status,
            PostApplyStatus::TimedOut
        );
        storage.config_set("post_apply_command", "true").unwrap();
        std::fs::remove_file(path).unwrap();
        assert_eq!(
            publish_theme_and_run_hook(&storage, &ctx).status,
            PostApplyStatus::Failed
        );
    }

    #[test]
    fn disabled_hook_is_noop() {
        let (_tmp, storage) = temp_storage();
        storage.config_set("post_apply_enabled", "off").unwrap();
        storage.config_set("post_apply_command", "false").unwrap();
        let ctx = basic_ctx("/nope.png".into(), Backend::Awww, FileType::Image, "*");
        run_post_apply_hook_with_still_override(&storage, &ctx, None).unwrap();
    }

    #[test]
    fn disabled_hook_still_writes_manifest_when_per_output_present() {
        let (tmp, storage) = temp_storage();
        storage.config_set("post_apply_enabled", "off").unwrap();
        storage.config_set("post_apply_command", "false").unwrap();

        let wall_a = tmp.path().join("a.png");
        let wall_b = tmp.path().join("b.png");
        std::fs::write(&wall_a, b"a").unwrap();
        std::fs::write(&wall_b, b"b").unwrap();

        let ctx = PostApplyContext {
            wallpaper_path: wall_a.to_string_lossy().into_owned(),
            backend: Backend::Awww,
            file_type: FileType::Image,
            outputs: "DP-8".into(),
            changed_outputs: vec!["DP-8".into()],
            theme_source_output: Some("DP-8".into()),
            per_output: vec![
                OutputThemeEntry {
                    output: "DP-8".into(),
                    wallpaper: wall_a.to_string_lossy().into_owned(),
                    backend: "awww".into(),
                    file_type: "image".into(),
                    still: None,
                },
                OutputThemeEntry {
                    output: "eDP-1".into(),
                    wallpaper: wall_b.to_string_lossy().into_owned(),
                    backend: "awww".into(),
                    file_type: "image".into(),
                    still: None,
                },
            ],
        };
        run_post_apply_hook_with_still_override(&storage, &ctx, None).unwrap();

        let manifest = storage.cd.theme_state_path();
        assert!(manifest.is_file());
        let doc: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(manifest).unwrap()).unwrap();
        assert_eq!(doc["version"], 1);
        assert_eq!(doc["theme_source_output"], "DP-8");
        assert!(doc["outputs"]["DP-8"]["still"].as_str().is_some());
        assert!(doc["outputs"]["eDP-1"]["still"].as_str().is_some());
    }

    #[test]
    fn still_failure_on_one_output_still_writes_manifest() {
        let (tmp, storage) = temp_storage();
        storage.config_set("post_apply_enabled", "off").unwrap();

        let wall_ok = tmp.path().join("ok.png");
        std::fs::write(&wall_ok, b"ok").unwrap();

        let ctx = PostApplyContext {
            wallpaper_path: wall_ok.to_string_lossy().into_owned(),
            backend: Backend::Awww,
            file_type: FileType::Image,
            outputs: "DP-8".into(),
            changed_outputs: vec!["DP-8".into()],
            theme_source_output: Some("DP-8".into()),
            per_output: vec![
                OutputThemeEntry {
                    output: "DP-8".into(),
                    wallpaper: wall_ok.to_string_lossy().into_owned(),
                    backend: "awww".into(),
                    file_type: "image".into(),
                    still: None,
                },
                OutputThemeEntry {
                    output: "eDP-1".into(),
                    wallpaper: tmp
                        .path()
                        .join("missing.png")
                        .to_string_lossy()
                        .into_owned(),
                    backend: "awww".into(),
                    file_type: "image".into(),
                    still: None,
                },
            ],
        };
        run_post_apply_hook_with_still_override(&storage, &ctx, None).unwrap();

        let doc: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(storage.cd.theme_state_path()).unwrap())
                .unwrap();
        assert!(doc["outputs"]["DP-8"]["still"].as_str().is_some());
        assert!(doc["outputs"]["eDP-1"]["still"].is_null());
    }

    #[test]
    fn empty_command_is_noop_when_enabled() {
        let (_tmp, storage) = temp_storage();
        storage.config_set("post_apply_enabled", "on").unwrap();
        storage.config_set("post_apply_command", "   ").unwrap();
        let ctx = basic_ctx("/nope.png".into(), Backend::Awww, FileType::Image, "*");
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
        let ctx = basic_ctx(
            project.to_string_lossy().into_owned(),
            Backend::LinuxWallpaperEngine,
            FileType::WeScene,
            "*",
        );
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
        let ctx = basic_ctx(
            project.to_string_lossy().into_owned(),
            Backend::LinuxWallpaperEngine,
            FileType::WeScene,
            "*",
        );

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
            let ctx = basic_ctx(
                "/missing-project".into(),
                Backend::Unsupported,
                file_type,
                "*",
            );
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
                "#!/bin/sh\nprintf '%s\\n' \"$WCR_WALLPAPER\" \"$WCR_STILL\" \"$WCR_BACKEND\" \"$WCR_OUTPUTS\" \"$WCR_THEME_MANIFEST\" \"$WCR_THEME_SOURCE_OUTPUT\" > '{}'\n",
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

        let wall_s = wall.to_string_lossy().into_owned();
        let ctx = PostApplyContext {
            wallpaper_path: wall_s.clone(),
            backend: Backend::Awww,
            file_type: FileType::Image,
            outputs: "eDP-1,HDMI-A-1".into(),
            changed_outputs: vec!["eDP-1".into(), "HDMI-A-1".into()],
            theme_source_output: Some("HDMI-A-1".into()),
            per_output: vec![
                OutputThemeEntry {
                    output: "eDP-1".into(),
                    wallpaper: wall_s.clone(),
                    backend: "awww".into(),
                    file_type: "image".into(),
                    still: None,
                },
                OutputThemeEntry {
                    output: "HDMI-A-1".into(),
                    wallpaper: wall_s.clone(),
                    backend: "awww".into(),
                    file_type: "image".into(),
                    still: None,
                },
            ],
        };
        run_post_apply_hook_with_still_override(&storage, &ctx, Some(still.clone())).unwrap();

        let body = std::fs::read_to_string(&marker).unwrap();
        let lines: Vec<_> = body.lines().collect();
        assert_eq!(lines[0], wall.to_string_lossy());
        assert_eq!(lines[1], still.to_string_lossy());
        assert_eq!(lines[2], "awww");
        assert_eq!(lines[3], "eDP-1,HDMI-A-1");
        assert_eq!(lines[4], storage.cd.theme_state_path().to_string_lossy());
        assert_eq!(lines[5], "HDMI-A-1");
    }

    #[test]
    fn last_applied_preserves_wcr_still_from_theme_source_entry() {
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

        let wall_a = tmp.path().join("a.png");
        let wall_b = tmp.path().join("b.png");
        std::fs::write(&wall_a, b"a").unwrap();
        std::fs::write(&wall_b, b"b").unwrap();

        storage.config_set("post_apply_enabled", "on").unwrap();
        storage
            .config_set("post_apply_command", &format!("\"{}\"", script.display()))
            .unwrap();

        let ctx = PostApplyContext {
            wallpaper_path: wall_b.to_string_lossy().into_owned(),
            backend: Backend::Awww,
            file_type: FileType::Image,
            outputs: "DP-8".into(),
            changed_outputs: vec!["eDP-1".into(), "DP-8".into()],
            theme_source_output: Some("DP-8".into()),
            per_output: vec![
                OutputThemeEntry {
                    output: "eDP-1".into(),
                    wallpaper: wall_a.to_string_lossy().into_owned(),
                    backend: "awww".into(),
                    file_type: "image".into(),
                    still: None,
                },
                OutputThemeEntry {
                    output: "DP-8".into(),
                    wallpaper: wall_b.to_string_lossy().into_owned(),
                    backend: "awww".into(),
                    file_type: "image".into(),
                    still: None,
                },
            ],
        };
        run_post_apply_hook_with_still_override(&storage, &ctx, None).unwrap();
        assert_eq!(
            std::fs::read_to_string(marker).unwrap().trim(),
            wall_b.to_string_lossy()
        );
    }

    #[test]
    fn failing_command_does_not_panic_and_returns_err_to_inner() {
        let (tmp, storage) = temp_storage();
        let still = tmp.path().join("still.jpg");
        std::fs::write(&still, b"fake").unwrap();

        storage.config_set("post_apply_enabled", "on").unwrap();
        storage.config_set("post_apply_command", "false").unwrap();

        let ctx = basic_ctx(
            still.to_string_lossy().into_owned(),
            Backend::Awww,
            FileType::Image,
            "*",
        );
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

        let ctx = basic_ctx(
            still.to_string_lossy().into_owned(),
            Backend::Awww,
            FileType::Image,
            "*",
        );
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
        let ctx = basic_ctx(
            still.to_string_lossy().into_owned(),
            Backend::Awww,
            FileType::Image,
            "*",
        );
        // Must not panic even when the command fails.
        run_post_apply_hook(&storage, &ctx);
    }
}
