# Backend Apply State Machine Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make wallpaper apply/stop behavior deterministic across image, video, and Wallpaper Engine scene backends so cross-backend switching avoids stale-state writes, black flashes where preventable, and flashback to an older wallpaper.

**Architecture:** Keep the existing frontend `ApplyRequest` and Tauri `apply_action` API. Move backend lifecycle decisions into one pure Rust state-machine module in `wc-backend`; keep backend process launching in existing backend modules. `apply_wallpaper()` becomes the single place that writes current state, last backend, and history after the target backend is confirmed successful.

**Tech Stack:** Rust workspace (`wc-backend`, `wc-app`, Tauri commands), React/TypeScript frontend, Playwright smoke tests, existing mock backend binaries in unit tests.

---

## Non-Negotiable Scope Rules

- Do **not** re-enable Wallpaper Engine Web as a live wallpaper backend. `WeWeb` remains unsupported/browse-only.
- Do **not** rewrite the frontend action model. Keep `ApplyRequestDTO`, `api.applyAction`, `buildApplyRequest`, and `useLibraryEntryActions`.
- Do **not** add new desktop GUI features, Settings pages, UI themes, or library filtering behavior in this plan.
- Do **not** remove `awww`, `mpvpaper`, or `linux-wallpaperengine` support.
- Do **not** change public CLI behavior except where existing `wc-app`/`wc-backend` unified behavior naturally fixes state writes.
- Do **not** vendor or link external wallpaper projects.
- Do **not** claim visual smoothness is fully verified unless a human actually tests on Niri/Wayland. Automated tests prove state-machine behavior, not perceptual smoothness.

## Current Code Facts

- Frontend latest-intent queue lives in `apps/tauri-gui/frontend/src/App.tsx` as `handleApplyAction()`.
- Frontend action request builder lives in `apps/tauri-gui/frontend/src/domain/applyRequests.ts`.
- Tauri apply commands live in `apps/tauri-gui/src-tauri/src/commands/wallpaper.rs`.
- `APPLY_SEQUENCE` and `APPLY_LOCK` already serialize Tauri apply requests process-locally.
- `wc-app` owns request resolution:
  - `crates/wc-app/src/apply_execution.rs`
  - `crates/wc-app/src/lib.rs::execute_apply_request`
  - `crates/wc-app/src/lib.rs::resolve_apply_request_target`
- `wc-backend` owns real process execution:
  - `crates/wc-backend/src/lib.rs::apply_wallpaper`
  - `crates/wc-backend/src/lib.rs::pre_stop_plan_for_target`
  - `crates/wc-backend/src/lib.rs::post_stop_plan_for_target`
  - `crates/wc-backend/src/lib.rs::post_apply_settle_ms`
  - `crates/wc-backend/src/linux_wallpaperengine.rs::apply`
- Current architectural problem: `Backend::LinuxWallpaperEngine` returns early from `apply_wallpaper()` and writes `current`, `last_backend`, and `history` inside `linux_wallpaperengine::apply()`. Other backends write state in `apply_wallpaper()`. This makes cross-backend behavior harder to reason about and test.

## Target Behavior Matrix

| Previous backend | Target backend | Before target starts | After target confirmed | State write |
|---|---|---|---|---|
| `awww` | `awww` | stop nothing | stop `mpvpaper` only as cleanup | new image path |
| `mpvpaper` | `awww` | stop nothing | settle 180ms, stop `mpvpaper` | new image/gif path |
| `linux-wallpaperengine` | `awww` | stop nothing | settle 180ms, stop LWE | new image/gif path |
| `awww` | `mpvpaper` | stop nothing | settle 150ms, stop `awww` | new video path |
| `mpvpaper` | `mpvpaper` | stop old `mpvpaper` before new one | stop nothing | new video path |
| `linux-wallpaperengine` | `mpvpaper` | stop nothing | settle 150ms, stop LWE | new video path |
| `awww` | `linux-wallpaperengine` | stop nothing | settle 250ms, stop `awww` | WE scene project path |
| `mpvpaper` | `linux-wallpaperengine` | stop nothing | settle 250ms, stop `mpvpaper` | WE scene project path |
| `linux-wallpaperengine` | `linux-wallpaperengine` | stop non-LWE cleanup only | old LWE killed by LWE handoff after new survives | new WE scene project path |
| unknown/legacy | any supported target | stop nothing before target | stop nothing after target | target state only if target succeeds |

Important: “state write” means all three operations happen together after success:

```rust
s.current_write(state_path)?;
s.last_backend_write(backend.as_str())?;
s.history_add(state_path, backend.as_str())?;
```

---

## File Structure

### Create

- `crates/wc-backend/src/lifecycle.rs`
  - Pure state-machine types and functions:
    - `RunningBackend`
    - `StopPlan`
    - `ApplyLifecyclePlan`
    - `plan_apply_lifecycle()`
    - `pre_stop_plan()`
    - `post_success_stop_plan()`
    - `post_success_settle_ms()`

### Modify

- `crates/wc-backend/src/lib.rs`
  - Export `pub mod lifecycle`.
  - Remove private duplicated lifecycle functions from this file.
  - Use lifecycle module from `apply_wallpaper()`.
  - Keep actual process functions (`stop_awww`, `stop_mpvpaper`, `ensure_awww_daemon`, `build_awww_img_command`) here.
  - Make `apply_wallpaper()` the only state/history write boundary for all backends.

- `crates/wc-backend/src/linux_wallpaperengine.rs`
  - Keep LWE process startup, immediate-crash detection, old LWE PID handoff, stale LWE cleanup, diagnostics, and PID write.
  - Remove `current_write`, `last_backend_write`, and `history_add` from `linux_wallpaperengine::apply()`.
  - Remove non-LWE stopping from `linux_wallpaperengine::apply()`; central `apply_wallpaper()` handles cross-backend post-stop after LWE survives.

- `crates/wc-app/src/lib.rs`
  - No API expansion expected. Re-run tests to ensure `execute_apply_request()` still gets correct `state_path`.
  - Add tests only if backend state write behavior exposes a gap at the app layer.

- `apps/tauri-gui/src-tauri/src/commands/wallpaper.rs`
  - Keep `APPLY_LOCK` and stale request guard.
  - Add tests for stale-before-execution if missing.
  - Do not change DTO names.

- `apps/tauri-gui/frontend/src/App.tsx`
  - Keep latest-intent queue.
  - Only add a tiny unit/smoke test if stale-success UI behavior regresses.

- `apps/tauri-gui/frontend/e2e/smoke.spec.ts`
  - Add only apply-semantics smoke coverage that can run against mockBridge.

---

## Task 1: Extract Backend Lifecycle State Machine

**Files:**
- Create: `crates/wc-backend/src/lifecycle.rs`
- Modify: `crates/wc-backend/src/lib.rs`

- [ ] **Step 1: Create lifecycle module with pure types and functions**

Create `crates/wc-backend/src/lifecycle.rs` with this content:

```rust
use wc_core::types::Backend;

use crate::LWE_BACKEND_NAME;

pub const AWWW_CROSS_BACKEND_SETTLE_MS: u64 = 180;
pub const MPVPAPER_CROSS_BACKEND_SETTLE_MS: u64 = 150;
pub const LWE_CROSS_BACKEND_SETTLE_MS: u64 = 250;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunningBackend {
    None,
    Awww,
    Mpvpaper,
    LinuxWallpaperEngine,
    Unsupported,
    Unknown,
}

impl RunningBackend {
    pub fn from_last_backend(raw: &str) -> Self {
        match raw.trim() {
            "" => RunningBackend::None,
            "awww" | "swww" => RunningBackend::Awww,
            "mpvpaper" => RunningBackend::Mpvpaper,
            LWE_BACKEND_NAME => RunningBackend::LinuxWallpaperEngine,
            "unsupported" => RunningBackend::Unsupported,
            _ => RunningBackend::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopPlan {
    All,
    AwwwOnly,
    LweOnly,
    MpvpaperOnly,
    NonLwe,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApplyLifecyclePlan {
    pub previous: RunningBackend,
    pub target: Backend,
    pub pre_stop: StopPlan,
    pub post_success_settle_ms: u64,
    pub post_success_stop: StopPlan,
}

pub fn plan_apply_lifecycle(previous_raw: &str, target: Backend) -> ApplyLifecyclePlan {
    let previous = RunningBackend::from_last_backend(previous_raw);
    ApplyLifecyclePlan {
        previous,
        target,
        pre_stop: pre_stop_plan(previous, target),
        post_success_settle_ms: post_success_settle_ms(previous, target),
        post_success_stop: post_success_stop_plan(previous, target),
    }
}

pub fn pre_stop_plan(previous: RunningBackend, target: Backend) -> StopPlan {
    match target {
        Backend::Awww => StopPlan::None,
        Backend::Mpvpaper => match previous {
            RunningBackend::Mpvpaper => StopPlan::MpvpaperOnly,
            RunningBackend::None => StopPlan::All,
            _ => StopPlan::None,
        },
        Backend::LinuxWallpaperEngine => match previous {
            RunningBackend::LinuxWallpaperEngine => StopPlan::NonLwe,
            _ => StopPlan::None,
        },
        Backend::Unsupported => StopPlan::All,
    }
}

pub fn post_success_stop_plan(previous: RunningBackend, target: Backend) -> StopPlan {
    match target {
        Backend::Awww => match previous {
            RunningBackend::Awww => StopPlan::MpvpaperOnly,
            RunningBackend::Mpvpaper => StopPlan::MpvpaperOnly,
            RunningBackend::LinuxWallpaperEngine => StopPlan::LweOnly,
            RunningBackend::None | RunningBackend::Unsupported | RunningBackend::Unknown => StopPlan::None,
        },
        Backend::Mpvpaper => match previous {
            RunningBackend::Awww => StopPlan::AwwwOnly,
            RunningBackend::LinuxWallpaperEngine => StopPlan::LweOnly,
            RunningBackend::Mpvpaper | RunningBackend::None | RunningBackend::Unsupported | RunningBackend::Unknown => StopPlan::None,
        },
        Backend::LinuxWallpaperEngine => match previous {
            RunningBackend::Awww => StopPlan::AwwwOnly,
            RunningBackend::Mpvpaper => StopPlan::MpvpaperOnly,
            RunningBackend::LinuxWallpaperEngine | RunningBackend::None | RunningBackend::Unsupported | RunningBackend::Unknown => StopPlan::None,
        },
        Backend::Unsupported => StopPlan::None,
    }
}

pub fn post_success_settle_ms(previous: RunningBackend, target: Backend) -> u64 {
    match (previous, target) {
        (RunningBackend::None, _) => 0,
        (RunningBackend::Awww, Backend::Awww) => 0,
        (RunningBackend::Mpvpaper, Backend::Mpvpaper) => 0,
        (RunningBackend::LinuxWallpaperEngine, Backend::LinuxWallpaperEngine) => 0,
        (_, Backend::Awww) => AWWW_CROSS_BACKEND_SETTLE_MS,
        (_, Backend::Mpvpaper) => MPVPAPER_CROSS_BACKEND_SETTLE_MS,
        (RunningBackend::Awww | RunningBackend::Mpvpaper, Backend::LinuxWallpaperEngine) => LWE_CROSS_BACKEND_SETTLE_MS,
        (_, Backend::LinuxWallpaperEngine) => 0,
        (_, Backend::Unsupported) => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_backend_parses_legacy_swww_as_awww() {
        assert_eq!(RunningBackend::from_last_backend("swww"), RunningBackend::Awww);
    }

    #[test]
    fn image_after_video_keeps_old_video_until_new_image_succeeds_then_stops_video() {
        let plan = plan_apply_lifecycle("mpvpaper", Backend::Awww);
        assert_eq!(plan.pre_stop, StopPlan::None);
        assert_eq!(plan.post_success_settle_ms, AWWW_CROSS_BACKEND_SETTLE_MS);
        assert_eq!(plan.post_success_stop, StopPlan::MpvpaperOnly);
    }

    #[test]
    fn video_after_image_keeps_old_image_until_new_video_succeeds_then_stops_image() {
        let plan = plan_apply_lifecycle("awww", Backend::Mpvpaper);
        assert_eq!(plan.pre_stop, StopPlan::None);
        assert_eq!(plan.post_success_settle_ms, MPVPAPER_CROSS_BACKEND_SETTLE_MS);
        assert_eq!(plan.post_success_stop, StopPlan::AwwwOnly);
    }

    #[test]
    fn scene_after_image_keeps_old_image_until_scene_survives_then_stops_image() {
        let plan = plan_apply_lifecycle("awww", Backend::LinuxWallpaperEngine);
        assert_eq!(plan.pre_stop, StopPlan::None);
        assert_eq!(plan.post_success_settle_ms, LWE_CROSS_BACKEND_SETTLE_MS);
        assert_eq!(plan.post_success_stop, StopPlan::AwwwOnly);
    }

    #[test]
    fn scene_after_video_keeps_old_video_until_scene_survives_then_stops_video() {
        let plan = plan_apply_lifecycle("mpvpaper", Backend::LinuxWallpaperEngine);
        assert_eq!(plan.pre_stop, StopPlan::None);
        assert_eq!(plan.post_success_settle_ms, LWE_CROSS_BACKEND_SETTLE_MS);
        assert_eq!(plan.post_success_stop, StopPlan::MpvpaperOnly);
    }

    #[test]
    fn scene_after_scene_uses_lwe_handoff_and_does_not_stop_all() {
        let plan = plan_apply_lifecycle(LWE_BACKEND_NAME, Backend::LinuxWallpaperEngine);
        assert_eq!(plan.pre_stop, StopPlan::NonLwe);
        assert_eq!(plan.post_success_settle_ms, 0);
        assert_eq!(plan.post_success_stop, StopPlan::None);
    }

    #[test]
    fn image_after_scene_stops_lwe_only_after_new_image_succeeds() {
        let plan = plan_apply_lifecycle(LWE_BACKEND_NAME, Backend::Awww);
        assert_eq!(plan.pre_stop, StopPlan::None);
        assert_eq!(plan.post_success_settle_ms, AWWW_CROSS_BACKEND_SETTLE_MS);
        assert_eq!(plan.post_success_stop, StopPlan::LweOnly);
    }

    #[test]
    fn unknown_previous_never_triggers_post_success_stop_all() {
        assert_eq!(
            plan_apply_lifecycle("unknown-backend", Backend::Awww).post_success_stop,
            StopPlan::None
        );
        assert_eq!(
            plan_apply_lifecycle("unknown-backend", Backend::Mpvpaper).post_success_stop,
            StopPlan::None
        );
        assert_eq!(
            plan_apply_lifecycle("unknown-backend", Backend::LinuxWallpaperEngine).post_success_stop,
            StopPlan::None
        );
    }
}
```

- [ ] **Step 2: Export lifecycle module**

In `crates/wc-backend/src/lib.rs`, add this near the existing module declarations:

```rust
pub mod lifecycle;
```

- [ ] **Step 3: Run lifecycle tests**

Run:

```bash
cargo test -p wc-backend lifecycle -- --nocapture
```

Expected: all lifecycle tests pass.

---

## Task 2: Route `apply_wallpaper()` Through the Lifecycle Plan

**Files:**
- Modify: `crates/wc-backend/src/lib.rs`

- [ ] **Step 1: Replace local lifecycle types**

In `crates/wc-backend/src/lib.rs`, remove the local private items:

```rust
enum StopPlan { ... }
fn pre_stop_plan_for_target(...)
fn post_stop_plan_for_target(...)
const DEFAULT_AWWW_CROSS_BACKEND_SETTLE_MS: u64 = 180;
const DEFAULT_MPVPAPER_CROSS_BACKEND_SETTLE_MS: u64 = 150;
fn post_apply_settle_ms(...)
```

Add imports:

```rust
use lifecycle::{ApplyLifecyclePlan, StopPlan};
```

- [ ] **Step 2: Update `execute_stop_plan()`**

Change `execute_stop_plan()` to accept `lifecycle::StopPlan` and support `NonLwe`:

```rust
fn execute_stop_plan(s: &StorageApi, plan: StopPlan) -> Result<(), WcError> {
    match plan {
        StopPlan::All => stop_all_backends(Some(s))?,
        StopPlan::AwwwOnly => stop_awww(),
        StopPlan::LweOnly => linux_wallpaperengine::stop(Some(s)),
        StopPlan::MpvpaperOnly => stop_mpvpaper(),
        StopPlan::NonLwe => stop_non_lwe_backends(s),
        StopPlan::None => {}
    }
    Ok(())
}
```

- [ ] **Step 3: Add a small state write helper**

In `crates/wc-backend/src/lib.rs`, near `apply_wallpaper()`, add:

```rust
fn write_success_state(
    s: &StorageApi,
    state_path: &str,
    backend: Backend,
) -> Result<(), WcError> {
    s.current_write(state_path)?;
    s.last_backend_write(backend.as_str())?;
    s.history_add(state_path, backend.as_str())?;
    Ok(())
}
```

- [ ] **Step 4: Rewrite `apply_wallpaper()` to use the plan**

Replace the body of `apply_wallpaper()` with this structure. Keep existing `ensure_awww_daemon()`, command building, and error messages:

```rust
pub fn apply_wallpaper(s: &StorageApi, path: &str, backend: Backend) -> Result<(), WcError> {
    let p = std::path::Path::new(path);
    if backend == Backend::Unsupported {
        return Err(WcError::UnsupportedFileType(path.to_string()));
    }
    if backend != Backend::LinuxWallpaperEngine && !p.is_file() {
        return Err(WcError::NotRegularFile(p.to_path_buf()));
    }

    let previous_backend_raw = s.last_backend_read()?.unwrap_or_default();
    let lifecycle = lifecycle::plan_apply_lifecycle(&previous_backend_raw, backend);
    execute_stop_plan(s, lifecycle.pre_stop)?;

    match backend {
        Backend::Awww => {
            ensure_awww_daemon()?;
            let resize_raw = s.config_get("awww_resize", "crop");
            let resize = normalize_awww_resize(&resize_raw);
            let transition_type = s.config_get("awww_transition_type", "fade");
            let duration = s.config_get("awww_transition_duration", "1");
            let fps = s.config_get("wallpaper_transition_fps", "60");
            let mut cmd = build_awww_img_command(path, resize, &transition_type, &duration, &fps);
            cmd.arg("--filter").arg("Lanczos3");
            let output = cmd
                .output()
                .map_err(|e| WcError::Other(format!("awww failed: {}", e)))?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let detail = if !stderr.is_empty() {
                    stderr
                } else if !stdout.is_empty() {
                    stdout
                } else {
                    "no renderer output".into()
                };
                return Err(WcError::Other(format!(
                    "awww apply failed with status {}: {}",
                    output.status, detail
                )));
            }
        }
        Backend::Mpvpaper => {
            let opts_raw = s.config_get("mpvpaper_options", "--loop-file=inf --panscan=1.0");
            let opts = normalize_mpvpaper_options(&opts_raw);
            let output = s.config_get("mpvpaper_output", "*");
            let status = Command::new("setsid")
                .args(["-f", "mpvpaper", "--fork", "-o", opts, &output, "--", path])
                .status()
                .map_err(|e| WcError::Other(format!("mpvpaper failed: {}", e)))?;
            if !status.success() {
                return Err(WcError::Other("mpvpaper failed to apply wallpaper".into()));
            }
        }
        Backend::LinuxWallpaperEngine => {
            let project = linux_wallpaperengine::project_from_path(path)?;
            linux_wallpaperengine::apply(s, project)?;
        }
        Backend::Unsupported => unreachable!(),
    }

    if lifecycle.post_success_settle_ms > 0 {
        std::thread::sleep(Duration::from_millis(lifecycle.post_success_settle_ms));
    }

    execute_stop_plan(s, lifecycle.post_success_stop)?;
    write_success_state(s, path, backend)?;
    Ok(())
}
```

Important: This makes `path` the state path. `wc-app::resolve_apply_request_target()` already passes:

- regular image/gif/video actual media path
- preview GIF actual preview path
- WE scene project directory

- [ ] **Step 5: Update existing unit tests that referenced removed private functions**

In `crates/wc-backend/src/lib.rs` tests, replace assertions against `pre_stop_plan_for_target`, `post_stop_plan_for_target`, and `post_apply_settle_ms` with assertions against `lifecycle::plan_apply_lifecycle()`.

Example replacement:

```rust
let plan = lifecycle::plan_apply_lifecycle(LWE_BACKEND_NAME, Backend::Awww);
assert_eq!(plan.pre_stop, StopPlan::None);
assert_eq!(plan.post_success_stop, StopPlan::LweOnly);
```

- [ ] **Step 6: Run backend tests**

Run:

```bash
cargo test -p wc-backend --lib
```

Expected: pass. If failures are only from old tests referencing removed functions, update tests to lifecycle module instead of restoring old functions.

---

## Task 3: Move LWE State Writes to the Unified Boundary

**Files:**
- Modify: `crates/wc-backend/src/linux_wallpaperengine.rs`
- Modify: `crates/wc-backend/src/lib.rs`

- [ ] **Step 1: Remove state/history writes from LWE apply**

In `crates/wc-backend/src/linux_wallpaperengine.rs::apply`, remove these lines near the end:

```rust
s.current_write(&project.project_path)?;
s.last_backend_write(crate::LWE_BACKEND_NAME)?;
s.history_add(&project.project_path, crate::LWE_BACKEND_NAME)?;
```

Keep:

```rust
s.config_set(PID_CONFIG_KEY, &child.id().to_string())?;
Ok(())
```

- [ ] **Step 2: Remove non-LWE stop from LWE apply**

In `linux_wallpaperengine::apply`, remove this block:

```rust
if last_backend != crate::LWE_BACKEND_NAME {
    std::thread::sleep(std::time::Duration::from_millis(250));
    crate::stop_non_lwe_backends(s);
}
```

Keep this scene-to-scene cleanup:

```rust
if last_backend == crate::LWE_BACKEND_NAME {
    crate::stop_non_lwe_backends(s);
}
```

Keep old LWE PID handoff and stale LWE cleanup after the new renderer survives.

- [ ] **Step 3: Update LWE tests to reflect unified state write boundary**

In `crates/wc-backend/src/linux_wallpaperengine.rs`, change `cross_backend_switch_cleans_non_lwe_after_success` because `linux_wallpaperengine::apply()` no longer writes `last_backend`.

Replace the assertion:

```rust
let backend = s.last_backend_read().unwrap().unwrap_or_default();
assert_eq!(backend, crate::LWE_BACKEND_NAME);
```

with:

```rust
let backend = s.last_backend_read().unwrap().unwrap_or_default();
assert_eq!(
    backend,
    "mpvpaper",
    "linux_wallpaperengine::apply only starts the renderer; unified apply_wallpaper writes backend state"
);
```

- [ ] **Step 4: Add a unified LWE state write test in `crates/wc-backend/src/lib.rs`**

Add this test in the existing `#[cfg(test)] mod tests` in `crates/wc-backend/src/lib.rs`:

```rust
#[cfg(unix)]
#[test]
fn apply_wallpaper_lwe_writes_state_after_renderer_survives() {
    use std::os::unix::fs::PermissionsExt;

    let (tmp, s) = temp_storage();
    s.last_backend_write("awww").unwrap();

    let bin = tmp.path().join("test-lwe-state-write");
    std::fs::write(&bin, "#!/bin/sh\nsleep 5\n").unwrap();
    let mut perms = std::fs::metadata(&bin).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&bin, perms).unwrap();
    s.config_set("linux_wallpaperengine_path", &bin.to_string_lossy()).unwrap();

    let scene = tmp.path().join("steamapps/workshop/content/431960/123456");
    std::fs::create_dir_all(&scene).unwrap();
    std::fs::write(scene.join("scene.pkg"), b"scene").unwrap();
    std::fs::write(
        scene.join("project.json"),
        r#"{"type":"scene","file":"scene.pkg","workshopid":"123456"}"#,
    )
    .unwrap();

    apply_wallpaper(&s, &scene.to_string_lossy(), Backend::LinuxWallpaperEngine).unwrap();

    assert_eq!(
        s.current_read().unwrap().as_deref(),
        Some(scene.to_string_lossy().as_ref())
    );
    assert_eq!(
        s.last_backend_read().unwrap().as_deref(),
        Some(LWE_BACKEND_NAME)
    );
    assert_eq!(
        s.history_list().unwrap().last().map(|h| h.path.as_str()),
        Some(scene.to_string_lossy().as_ref())
    );

    let pid = s.config_get("linux_wallpaperengine_pid", "");
    if let Ok(pid) = pid.parse::<i32>() {
        let _ = Command::new("kill")
            .args(["-TERM", &format!("-{}", pid)])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}
```

- [ ] **Step 5: Add LWE failure preserves old state test**

Add this test in `crates/wc-backend/src/lib.rs`:

```rust
#[cfg(unix)]
#[test]
fn apply_wallpaper_lwe_failure_preserves_old_state() {
    use std::os::unix::fs::PermissionsExt;

    let (tmp, s) = temp_storage();
    let old = tmp.path().join("old.jpg");
    std::fs::write(&old, b"old").unwrap();
    s.current_write(&old.to_string_lossy()).unwrap();
    s.last_backend_write("awww").unwrap();
    s.history_add(&old.to_string_lossy(), "awww").unwrap();
    let history_before = s.history_list().unwrap().len();

    let bin = tmp.path().join("test-lwe-fail-state");
    std::fs::write(&bin, "#!/bin/sh\nexit 1\n").unwrap();
    let mut perms = std::fs::metadata(&bin).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&bin, perms).unwrap();
    s.config_set("linux_wallpaperengine_path", &bin.to_string_lossy()).unwrap();

    let scene = tmp.path().join("steamapps/workshop/content/431960/987654");
    std::fs::create_dir_all(&scene).unwrap();
    std::fs::write(scene.join("scene.pkg"), b"scene").unwrap();
    std::fs::write(
        scene.join("project.json"),
        r#"{"type":"scene","file":"scene.pkg","workshopid":"987654"}"#,
    )
    .unwrap();

    let err = apply_wallpaper(&s, &scene.to_string_lossy(), Backend::LinuxWallpaperEngine).unwrap_err();
    assert!(err.to_string().contains("linux-wallpaperengine") || err.to_string().contains("exited"));
    assert_eq!(s.current_read().unwrap().as_deref(), Some(old.to_string_lossy().as_ref()));
    assert_eq!(s.last_backend_read().unwrap().as_deref(), Some("awww"));
    assert_eq!(s.history_list().unwrap().len(), history_before);
}
```

- [ ] **Step 6: Run LWE/backend tests**

Run:

```bash
cargo test -p wc-backend --lib -- --test-threads=1
```

Expected: pass. Use `--test-threads=1` for LWE process tests because they spawn and kill real mock processes.

---

## Task 4: Strengthen App/Tauri Stale Request Guarantees

**Files:**
- Modify: `apps/tauri-gui/src-tauri/src/commands/wallpaper.rs`
- Optional modify: `apps/tauri-gui/frontend/src/App.tsx`

- [ ] **Step 1: Add an explicit stale-before-execution helper**

In `apps/tauri-gui/src-tauri/src/commands/wallpaper.rs`, add:

```rust
fn is_stale_apply(seq: u64) -> bool {
    seq != APPLY_SEQUENCE.load(std::sync::atomic::Ordering::SeqCst)
}
```

Change this block in `execute_and_format_result()`:

```rust
let latest_before = APPLY_SEQUENCE.load(std::sync::atomic::Ordering::SeqCst);
if seq != latest_before {
    return stale_apply_result();
}
```

to:

```rust
if is_stale_apply(seq) {
    return stale_apply_result();
}
```

- [ ] **Step 2: Add tests for stale helper**

In `wallpaper.rs` tests, add:

```rust
#[test]
fn stale_apply_helper_detects_superseded_sequence() {
    APPLY_SEQUENCE.store(10, std::sync::atomic::Ordering::SeqCst);
    assert!(is_stale_apply(9));
    assert!(!is_stale_apply(10));
}
```

- [ ] **Step 3: Do not change frontend queue unless tests fail**

Keep `apps/tauri-gui/frontend/src/App.tsx::handleApplyAction()` as-is unless one of these concrete failures appears:

- newer pending request is dropped
- stale result displays success toast
- `applying` remains true after an exception

If a failure appears, fix only the failing branch and add a unit test if existing frontend test infrastructure can cover it.

- [ ] **Step 4: Run Tauri command tests**

Run:

```bash
cargo test -p wallpaper-console-tauri wallpaper::tests -- --nocapture
```

Expected: pass.

---

## Task 5: Add Apply Pipeline Regression Tests at `wc-app`

**Files:**
- Modify: `crates/wc-app/src/apply_execution.rs`
- Modify only if needed: `crates/wc-app/src/lib.rs`

- [ ] **Step 1: Add test for preview state path**

In `crates/wc-app/src/apply_execution.rs` tests, add:

```rust
#[test]
fn apply_preview_target_state_path_is_preview_file() {
    let (tmp, service) = temp_service();
    let project = scene_project_with_preview(tmp.path());
    let request = ApplyRequest {
        kind: ApplyRequestKind::ApplyPreview,
        path: project.to_string_lossy().to_string(),
        request_id: Some("preview-state".into()),
    };

    let target = service.resolve_apply_request_target(&request).unwrap();
    assert!(target.preview);
    assert!(target.resolved_path.ends_with("preview.gif"));
    assert_eq!(target.state_path, target.resolved_path);
    assert_eq!(target.backend, Backend::Awww);
}
```

- [ ] **Step 2: Add test for WE scene state path**

Add:

```rust
#[test]
fn apply_scene_target_state_path_is_project_dir() {
    let (tmp, service) = temp_service();
    let project = scene_project_with_preview(tmp.path());
    let request = ApplyRequest {
        kind: ApplyRequestKind::Apply,
        path: project.to_string_lossy().to_string(),
        request_id: Some("scene-state".into()),
    };

    let target = service.resolve_apply_request_target(&request).unwrap();
    assert!(!target.preview);
    assert_eq!(target.resolved_path, project.to_string_lossy());
    assert_eq!(target.state_path, project.to_string_lossy());
    assert_eq!(target.backend, Backend::LinuxWallpaperEngine);
}
```

- [ ] **Step 3: Run app tests**

Run:

```bash
cargo test -p wc-app
```

Expected: pass.

---

## Task 6: Add Mock Smoke Coverage for Explicit Apply Semantics

**Files:**
- Modify: `apps/tauri-gui/frontend/src/api/mockBridge.ts`
- Modify: `apps/tauri-gui/frontend/e2e/smoke.spec.ts`

- [ ] **Step 1: Ensure mock `applyAction()` records request kind**

In `mockBridge.ts`, if there is not already a visible mock state for the last apply action, add module-level state:

```ts
let lastApplyRequest: ApplyRequestDTO | null = null;
```

In mock `applyAction`, set:

```ts
lastApplyRequest = request;
```

Do not expose this through production bridge. Only use it through mock UI state if already present. If no simple mock UI path exists, skip adding debug UI and only use visible toast assertions.

- [ ] **Step 2: Add smoke for WE Web no-op apply guard**

In `smoke.spec.ts`, keep or add:

```ts
test('WE Web double click shows cannot apply warning', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('combobox').first().selectOption('we_web');
  const card = page.locator('.wallpaper-card').filter({ hasText: 'WE Web' }).first();
  await card.dblclick();
  await expect(page.locator('.toast')).toContainText('Cannot apply');
});
```

- [ ] **Step 3: Add smoke for preview action success**

Keep or add:

```ts
test('Apply preview GIF completes through explicit preview action', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('combobox').first().selectOption('we_scene');
  const card = page.locator('.wallpaper-card').filter({ hasText: 'WE Scene' }).first();
  await card.click({ button: 'right' });
  await page.getByText('Apply preview GIF').click();
  await expect(page.locator('.toast')).toContainText(/Applied|Preview/);
});
```

- [ ] **Step 4: Run smoke**

Run:

```bash
cd apps/tauri-gui/frontend
npm run smoke
```

Expected: pass.

---

## Task 7: Documentation Update

**Files:**
- Modify: `docs/CURRENT_STATUS.md`
- Modify: `docs/DEVELOPMENT.md`
- Modify: `docs/TAURI_MANUAL_SMOKE_CHECKLIST.md`

- [ ] **Step 1: Update current status**

In `docs/CURRENT_STATUS.md`, add one short row/bullet:

```markdown
- Backend apply lifecycle is centralized in `wc-backend::lifecycle`; `apply_wallpaper()` is the single state/history write boundary after successful backend confirmation.
```

- [ ] **Step 2: Update development docs**

In `docs/DEVELOPMENT.md`, add:

```markdown
### Backend apply lifecycle

`wc-backend::lifecycle` owns the pure transition plan for wallpaper switching. Backend modules start/stop real processes, but `wc-backend::apply_wallpaper()` is the only successful apply path that writes current wallpaper, last backend, and history.

When changing apply behavior:

1. Add or update lifecycle tests first.
2. Keep Wallpaper Engine Web unsupported.
3. Do not stop the previous visible backend until the new backend is confirmed unless the same backend requires a pre-stop.
4. Failed applies must preserve current wallpaper state and must not add history.
```

- [ ] **Step 3: Update manual smoke checklist**

In `docs/TAURI_MANUAL_SMOKE_CHECKLIST.md`, add:

```markdown
### Backend switching

- Image → image: should transition normally.
- Image → video: should not briefly show an older unrelated image.
- Video → image: should not flash the previous image before the requested image.
- Video → scene: brief wait is acceptable; long black screen is not.
- Scene → image/video: old scene should disappear only after target backend has started.
- Failed scene apply: current state should remain the previous wallpaper.
```

- [ ] **Step 4: Run doc grep**

Run:

```bash
rg -n "Backend apply lifecycle|wc-backend::lifecycle|Wallpaper Engine Web unsupported|Backend switching" docs
```

Expected: new docs appear. Do not edit archived Wails docs for this task.

---

## Task 8: Full Verification Matrix

Run these commands from repository root unless command says otherwise:

- [ ] **Rust format**

```bash
cargo fmt --all -- --check
```

Expected: clean. If not clean, run `cargo fmt --all`, then rerun the check.

- [ ] **Backend focused tests**

```bash
cargo test -p wc-backend --lib -- --test-threads=1
```

Expected: pass.

- [ ] **App focused tests**

```bash
cargo test -p wc-app
```

Expected: pass.

- [ ] **Workspace tests**

```bash
cargo test --workspace
```

Expected: pass.

- [ ] **Clippy**

```bash
cargo clippy --workspace -- -D warnings
```

Expected: clean.

- [ ] **Frontend typecheck**

```bash
cd apps/tauri-gui/frontend
npm run typecheck
```

Expected: pass.

- [ ] **Frontend unit tests**

```bash
cd apps/tauri-gui/frontend
npm run test:unit
```

Expected: pass.

- [ ] **Frontend build**

```bash
cd apps/tauri-gui/frontend
npm run build
```

Expected: pass.

- [ ] **Smoke tests**

```bash
cd apps/tauri-gui/frontend
npm run smoke
```

Expected: pass.

---

## Task 9: Five-Round Review Checklist for DS/opencode

After implementation and verification, perform exactly these five review rounds. Do not ask the user whether to continue between rounds.

### Round 1: State write boundary

- [ ] Confirm `linux_wallpaperengine::apply()` no longer calls:
  - `current_write`
  - `last_backend_write`
  - `history_add`
- [ ] Confirm `apply_wallpaper()` writes state once after successful backend execution.
- [ ] Confirm failed Awww/Mpvpaper/LWE applies preserve previous `current`, `last_backend`, and history length.
- [ ] Run:

```bash
rg -n "current_write|last_backend_write|history_add" crates/wc-backend/src
```

Expected: state writes should be in `lib.rs::write_success_state`; LWE module should not write current/history.

### Round 2: Lifecycle transition correctness

- [ ] Confirm all transition rules in “Target Behavior Matrix” have tests.
- [ ] Confirm unknown previous backend never maps to `StopPlan::All` after a successful target launch.
- [ ] Confirm `swww` is still treated as legacy `awww`.
- [ ] Run:

```bash
cargo test -p wc-backend lifecycle -- --nocapture
```

### Round 3: LWE handoff safety

- [ ] Confirm scene-to-scene starts the new LWE process first.
- [ ] Confirm old LWE PID is killed only after the new process survives the startup poll.
- [ ] Confirm LWE startup failure preserves old PID.
- [ ] Confirm cross-backend `awww/mpvpaper -> LWE` does not stop old backend before LWE survives.
- [ ] Run:

```bash
cargo test -p wc-backend linux_wallpaperengine -- --test-threads=1 --nocapture
```

### Round 4: Tauri/frontend latest-intent safety

- [ ] Confirm `APPLY_LOCK` still wraps execution.
- [ ] Confirm stale requests return `stale_apply_request` before `execute_apply_request()`.
- [ ] Confirm frontend `handleApplyAction()` still processes only the latest pending request.
- [ ] Confirm stale failures do not show success toast.
- [ ] Run:

```bash
cargo test -p wallpaper-console-tauri wallpaper::tests -- --nocapture
cd apps/tauri-gui/frontend && npm run smoke
```

### Round 5: Regression and scope control

- [ ] Confirm WE Web remains unsupported:

```bash
rg -n "WeWeb|we_web|Web wallpaper" crates apps/tauri-gui/frontend/src | head -80
```

Expected: references should describe unsupported/browse-only behavior, not live backend support.

- [ ] Confirm no Settings UI redesign happened in this plan.
- [ ] Confirm no `web renderer`, `Chromium backend`, or `Open experimental Chromium preview` path was reintroduced.
- [ ] Run the full verification matrix from Task 8.

---

## Expected Final Report Format for DS/opencode

Use this exact structure:

```markdown
## Summary
- One paragraph describing backend lifecycle hardening.

## Files Changed
- `path`: exact change.

## Lifecycle Behavior
- List each transition class: image->video, video->image, scene->scene, scene->image/video, failed apply.

## Tests Added/Updated
- Rust tests by file and test name.
- Frontend smoke tests by name.

## Verification Matrix
| Command | Result |
|---|---|
| cargo fmt --all -- --check | pass/fail |
| cargo test -p wc-backend --lib -- --test-threads=1 | pass/fail |
| cargo test --workspace | pass/fail |
| cargo clippy --workspace -- -D warnings | pass/fail |
| npm run typecheck | pass/fail |
| npm run test:unit | pass/fail |
| npm run build | pass/fail |
| npm run smoke | pass/fail |

## Five Review Rounds
- Round 1: findings, fixes, verification.
- Round 2: findings, fixes, verification.
- Round 3: findings, fixes, verification.
- Round 4: findings, fixes, verification.
- Round 5: findings, fixes, verification.

## Manual GUI Verification
- State whether real Niri/Wayland GUI switching was tested.
- If not tested, say it was not tested and list the exact manual checks.

## Remaining Risks
- Only real remaining risks. Do not list tasks from this plan as “future work”.
```

---

## Plan Self-Review

- Spec coverage: covers backend lifecycle extraction, unified state write boundary, LWE handoff, stale request guard, tests, docs, and five review rounds.
- Placeholder scan: no `TBD`, no open-ended “add tests” without test names/code, no unspecified files.
- Type consistency: uses existing `Backend`, `StorageApi`, `ApplyRequest`, `ApplyExecutionTarget`, `APPLY_SEQUENCE`, and current file paths.
- Scope control: explicitly excludes WE Web live backend, Settings redesign, CLI behavior changes, and unrelated UI work.
