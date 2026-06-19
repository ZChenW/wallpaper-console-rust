# Readiness, Compatibility & Apply Stage Events — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix awww-daemon readiness, productize WE scene/web capability boundaries, replace the global thumbnail store with path-level subscriptions, and add structured apply stage events — four phases, four commits.

**Architecture:** Phase 1 adds a socket+query readiness probe to wc-backend. Phase 2 introduces `BackendErrorKind` in wc-core, wires `we_compat::record_failure` in wc_app on the resolved-target failure path, and adds renderer-compatibility disclaimers to the frontend. Phase 3 converts the thumbnail queue to delta callbacks + per-path `useSyncExternalStore` subscriptions, then adds dynamic overscan. Phase 4 defines a no-Tauri `ApplyStageReporter` trait in wc-backend, plumbs `ApplyExecutionOptions` through wc_app, and has the frontend `ApplyQueueController` consume `wc-apply-stage` events.

**Tech Stack:** Rust (workspace crates: wc-core, wc-backend, wc-app, wc-storage, wallpaper-console-tauri), TypeScript/React 19 (Vite, node:test, Playwright). Verify gate: `cargo run -p xtask -- verify all`.

**Spec:** `doc/specs/2026-06-19-readiness-compat-stages-design.md`

---

## Phase 1 — awww daemon readiness (backend)

### Task 1.1: Add `awww_socket_path()` and `AwwwReadiness` + `awww_socket_ready` to BackendRuntime

**Files:**
- Modify: `crates/wc-core/src/error.rs` (no change needed here in P1, but referenced)
- Modify: `crates/wc-backend/src/runtime.rs`

- [ ] **Step 1: Add `AwwwReadiness` enum and `awww_socket_ready` to the trait**

In `crates/wc-backend/src/runtime.rs`, add above `BackendRuntime`:

```rust
pub enum AwwwReadiness {
    Ready,
    SocketMissing,
    SocketPresentQueryFailed { stderr: String },
}
```

Add to the `BackendRuntime` trait:
```rust
fn awww_socket_ready(&mut self) -> AwwwReadiness;
```

- [ ] **Step 2: Add `awww_socket_path()` function**

```rust
pub fn awww_socket_path() -> Result<std::path::PathBuf, WcError> {
    let xdg = std::env::var("XDG_RUNTIME_DIR").map_err(|_| {
        WcError::Other("XDG_RUNTIME_DIR is not set; cannot locate awww-daemon socket".into())
    })?;
    let wayland = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".to_string());
    Ok(std::path::PathBuf::from(xdg).join(format!("{wayland}-awww-daemon.sock"))
}
```

- [ ] **Step 3: Implement `awww_socket_ready` for `SystemBackendRuntime`**

```rust
fn awww_socket_ready(&mut self) -> AwwwReadiness {
    let path = match awww_socket_path() {
        Ok(p) => p,
        Err(_) => return AwwwReadiness::SocketMissing,
    };
    if !path.exists() {
        return AwwwReadiness::SocketMissing;
    }
    let output = Command::new("awww")
        .arg("query")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();
    match output {
        Ok(o) if o.status.success() => AwwwReadiness::Ready,
        Ok(o) => AwwwReadiness::SocketPresentQueryFailed {
            stderr: String::from_utf8_lossy(&o.stderr).trim().to_string(),
        },
        Err(e) => AwwwReadiness::SocketPresentQueryFailed {
            stderr: format!("awww query failed to execute: {}", e),
        },
    }
}
```

- [ ] **Step 4: Rework `ensure_awww_daemon_running`**

```rust
fn ensure_awww_daemon_running(&mut self) -> Result<(), WcError> {
    if matches!(self.awww_socket_ready(), AwwwReadiness::Ready) {
        return Ok(());
    }
    let user = crate::whoami();
    let was_running = crate::is_awww_daemon_running(&user);
    if !was_running {
        let mut cmd = build_awww_daemon_command();
        let status = self.command_status(&mut cmd).map_err(|_| {
            WcError::Other(
                "setsid not available — cannot launch awww-daemon. \
                 setsid is part of util-linux; install it with your package manager."
                    .into(),
            )
        })?;
        if !status.success() {
            return Err(WcError::Other(
                "awww-daemon not found. Install awww (pip install awww or AUR).".into(),
            ));
        }
    }
    let mut last_stderr = String::new();
    for _ in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(50));
        match self.awww_socket_ready() {
            AwwwReadiness::Ready => return Ok(()),
            AwwwReadiness::SocketMissing => {}
            AwwwReadiness::SocketPresentQueryFailed { stderr } => {
                last_stderr = stderr;
            }
        }
    }
    let socket_path = awww_socket_path()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "<unknown>".to_string());
    let wayland = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".to_string());
    if crate::is_awww_daemon_running(&user) {
        Err(WcError::Other(format!(
            "awww-daemon is running but socket is not ready for WAYLAND_DISPLAY={} \
             (expected {}); last query stderr: {}",
            wayland, socket_path, last_stderr
        )))
    } else {
        Err(WcError::Other(
            "awww-daemon failed to start. Check 'awww-daemon' is installed and your \
             compositor supports wlr-layer-shell."
                .into(),
        ))
    }
}
```

- [ ] **Step 5: Update `FakeRuntime` in lib.rs tests**

Add `awww_readiness_sequence` field and implement `awww_socket_ready`:

```rust
// Add to FakeRuntime struct:
awww_readiness_sequence: std::cell::RefCell<Vec<AwwwReadiness>>,

// In impl BackendRuntime for FakeRuntime:
fn awww_socket_ready(&mut self) -> AwwwReadiness {
    let mut seq = self.awww_readiness_sequence.borrow_mut();
    if seq.len() > 1 {
        seq.remove(0)
    } else if !seq.is_empty() {
        seq[0].clone()
    } else {
        AwwwReadiness::Ready
    }
}
```

Make `AwwwReadiness` `Clone` and `Debug`.

- [ ] **Step 6: Write failing tests**

In `lib.rs` test module:

```rust
#[test]
fn ensure_daemon_ok_when_socket_ready_fast_path() {
    let mut rt = FakeRuntime {
        command_status_success: true,
        awww_readiness_sequence: std::cell::RefCell::new(vec![AwwwReadiness::Ready]),
        ..Default::default()
    };
    assert!(rt.ensure_awww_daemon_running().is_ok());
    assert_eq!(rt.command_status_count, 0, "fast path must not spawn daemon");
}

#[test]
fn ensure_daemon_err_when_process_running_socket_never_ready() {
    // Use a FakeRuntime that always reports SocketMissing but we simulate
    // process running by overriding ensure_awww_daemon_running is not possible
    // (it's a trait method). Instead test the logic via a custom runtime.
    // See Task 1.2 for the readiness-only helper test.
}
```

For the socket-missing-but-process-running case, extract the poll loop into a testable helper:

```rust
// In runtime.rs, refactor the poll into:
pub(crate) fn wait_for_awww_socket_ready(
    runtime: &mut dyn BackendRuntime,
    user: &str,
) -> Result<(), WcError> { ... }
```

Then `ensure_awww_daemon_running` calls spawn + `wait_for_awww_socket_ready`.

- [ ] **Step 7: Run tests**

```bash
cargo test -p wc-backend -- awww readiness
```

- [ ] **Step 8: Commit**

```bash
git add crates/wc-backend/src/runtime.rs crates/wc-backend/src/lib.rs
git commit -m "fix(backend): gate awww daemon readiness on socket + awww query probe"
```

---

## Phase 2 — WE scene / web capability boundaries (backend + frontend)

### Task 2.1: Add `BackendErrorKind` + `WcError::LinuxWallpaperEngine` to wc-core

**Files:**
- Modify: `crates/wc-core/src/error.rs`

- [ ] **Step 1: Add `BackendErrorKind` enum and new `WcError` variant**

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendErrorKind {
    RendererLimitation,
    TargetConfig,
    WorkshopDirectory,
    Generic,
}
```

Add to `WcError`:
```rust
#[error("linux-wallpaperengine error ({kind:?}): {detail}")]
LinuxWallpaperEngine { kind: BackendErrorKind, detail: String },
```

- [ ] **Step 2: Run `cargo build -p wc-core`**

- [ ] **Step 3: Commit**

```bash
git add crates/wc-core/src/error.rs
git commit -m "feat(core): add BackendErrorKind + WcError::LinuxWallpaperEngine variant"
```

### Task 2.2: Return structured error from `map_renderer_error`

**Files:**
- Modify: `crates/wc-backend/src/linux_wallpaperengine.rs`

- [ ] **Step 1: Update `map_renderer_error` to return `WcError::LinuxWallpaperEngine`**

Replace the `WcError::Other(...)` returns with `WcError::LinuxWallpaperEngine { kind, detail }` using the classification:
- `"projection must have a width"` → `RendererLimitation`
- `"cannot find workshop directory"` → `WorkshopDirectory`
- `"failed to create window"` / `"no suitable output"` / `"no display"` → `TargetConfig`
- else → `Generic`

- [ ] **Step 2: Update existing tests to match new error shape**

The `map_renderer_error` tests should assert on `WcError::LinuxWallpaperEngine { kind, .. }` variants instead of string matching.

- [ ] **Step 3: Run tests**

```bash
cargo test -p wc-backend linux_wallpaperengine
```

- [ ] **Step 4: Commit**

```bash
git add crates/wc-backend/src/linux_wallpaperengine.rs
git commit -m "feat(backend): classify LWE renderer errors via BackendErrorKind"
```

### Task 2.3: Map `BackendErrorKind` → `AppError.code` in wc-app

**Files:**
- Modify: `crates/wc-app/src/lib.rs`

- [ ] **Step 1: Update `AppError::from_wc_error` to handle `WcError::LinuxWallpaperEngine`**

```rust
pub fn from_wc_error(err: WcError) -> Self {
    match &err {
        WcError::LinuxWallpaperEngine { kind, detail } => {
            let (code, message, suggestion) = match kind {
                wc_core::error::BackendErrorKind::RendererLimitation => (
                    "renderer_limitation",
                    "This Wallpaper Engine scene is not compatible with linux-wallpaperengine.".to_string(),
                    Some("Use the preview GIF or choose another Wallpaper Engine scene.".to_string()),
                ),
                wc_core::error::BackendErrorKind::TargetConfig => (
                    "target_config_error",
                    "linux-wallpaperengine could not find the correct display output.".to_string(),
                    Some("Set target_mode=screen-root and target=<output name> in Settings (e.g. eDP-1).".to_string()),
                ),
                wc_core::error::BackendErrorKind::WorkshopDirectory => (
                    "workshop_directory_missing",
                    "Wallpaper Engine workshop directory not found.".to_string(),
                    Some("Check the workshop content path in your Wallpaper Engine sources.".to_string()),
                ),
                wc_core::error::BackendErrorKind::Generic => (
                    "linux_wallpaperengine_failed",
                    "Wallpaper Engine scene support is not ready.".to_string(),
                    Some("Use the preview GIF or choose another Wallpaper Engine scene.".to_string()),
                ),
            };
            AppError {
                code: code.into(),
                message,
                detail: Some(detail.clone()),
                recoverable: true,
                suggestion,
            }
        }
        _ => {
            let text = err.to_string();
            AppError {
                code: "command_failed".into(),
                message: text,
                detail: None,
                recoverable: true,
                suggestion: None,
            }
        }
    }
}
```

- [ ] **Step 2: Write tests for the mapping**

```rust
#[test]
fn from_wc_error_maps_renderer_limitation() {
    let err = WcError::LinuxWallpaperEngine {
        kind: wc_core::error::BackendErrorKind::RendererLimitation,
        detail: "projection must have a width".into(),
    };
    let app_err = AppError::from_wc_error(err);
    assert_eq!(app_err.code, "renderer_limitation");
    assert!(app_err.recoverable);
}
```

Add similar tests for TargetConfig, WorkshopDirectory, Generic.

- [ ] **Step 3: Run tests** — `cargo test -p wc-app`

- [ ] **Step 4: Commit**

### Task 2.4: Wire `record_failure` in wc_app on resolved-target failure path

**Files:**
- Modify: `crates/wc-app/src/lib.rs`

- [ ] **Step 1: Add `execute_apply_request_with_options`**

Add `ApplyExecutionOptions` struct (with `request_id` and a `stage_reporter` field for Phase 4 — but for now just `request_id`). Actually for Phase 2, the key change is in `execute_apply_request`: after `apply_wallpaper` fails with an LWE-classified error and `target.file_type == WeScene`, call `we_compat::record_failure`. On success with `WeScene`, call `clear_failure`.

```rust
pub fn execute_apply_request(&self, request: ApplyRequest) -> Result<ApplyExecutionResult, AppError> {
    let target = self.resolve_apply_request_target(&request)?;
    let result = wc_backend::apply_wallpaper(
        &self.storage,
        &target.resolved_path,
        target.backend,
        target.fallback_path.as_deref(),
    );
    match result {
        Ok(()) => {
            if target.file_type == FileType::WeScene {
                let _ = wc_storage::we_compat::clear_failure(&target.state_path);
            }
            Ok(ApplyExecutionResult {
                request_id: request.request_id,
                applied_path: target.resolved_path,
                state_path: target.state_path,
                backend: target.backend,
                file_type: target.file_type,
                preview: target.preview,
            })
        }
        Err(wc_core::error::WcError::LinuxWallpaperEngine { kind, detail }) => {
            if target.file_type == FileType::WeScene {
                let backend_status = if kind == wc_core::error::BackendErrorKind::RendererLimitation {
                    "renderer_limitation"
                } else {
                    "failed"
                };
                let app_err = AppError::from_wc_error(
                    wc_core::error::WcError::LinuxWallpaperEngine { kind: kind.clone(), detail: detail.clone() }
                );
                let _ = wc_storage::we_compat::record_failure(
                    &target.state_path,
                    backend_status,
                    &app_err.code,
                    &app_err.message,
                    Some(detail),
                );
            }
            Err(AppError::from_wc_error(
                wc_core::error::WcError::LinuxWallpaperEngine { kind, detail }
            ))
        }
        Err(e) => Err(AppError::from_wc_error(e)),
    }
}
```

- [ ] **Step 2: Remove the Tauri-layer `clear_failure` call** in `wallpaper.rs:175` (wc_app now owns it).

- [ ] **Step 3: Write test — scene failure records we_compat**

Use a mock LWE binary that exits 1 with "Projection must have a width" stderr; verify `we_compat::lookup_failure` returns the recorded entry with `error_kind == "renderer_limitation"`.

- [ ] **Step 4: Write test — preview failure does NOT record we_compat**

- [ ] **Step 5: Write test — we_web unsupported does NOT record we_compat**

- [ ] **Step 6: Run tests** — `cargo test -p wc-app`, `cargo test -p wallpaper-console-tauri`

- [ ] **Step 7: Commit**

### Task 2.5: Add `CompatibilityKind` to `ApplyPlan` + renderer-limitation plan

**Files:**
- Modify: `crates/wc-app/src/apply_plan.rs`

- [ ] **Step 1: Add `CompatibilityKind` enum + `compatibility` field to `ApplyPlan`**

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompatibilityKind {
    NativeScene { disclaimer: String },
}
```

Add `pub compatibility: Option<CompatibilityKind>` to `ApplyPlan`.

- [ ] **Step 2: Set `NativeScene` disclaimer on all we_scene plans**

In `plan_we_scene`, both `backend_failed` and non-failed branches:
```rust
compatibility: Some(CompatibilityKind::NativeScene {
    disclaimer: "Rendered by linux-wallpaperengine — may differ from Wallpaper Engine".into(),
}),
```

Image/video/web/application plans: `compatibility: None`.

- [ ] **Step 3: Add renderer-limitation plan branch**

`plan_for_entry` needs the `error_kind` from the cached failure. Change `backend_failed: bool` to accept the kind. The call site in `common.rs::dto_from_entry` passes `cached_failure.as_ref().map(|f| f.error_kind.as_str())`.

When `error_kind == "renderer_limitation"`, produce a plan with reason "Renderer limitation" and the RetryBackendApply + ApplyPreview + OpenFolder + CopyWorkshopId actions.

- [ ] **Step 4: Add `ApplyPreview` ("Apply preview only") to we_web plans with preview**

In `plan_we_web`, if the project has `preview_path`, add `ApplyAction { kind: ApplyPreview, label: "Apply preview only", enabled: true, reason: Some("Only the preview GIF can be applied as a static wallpaper; the Web scene itself is not supported.".into()) }`.

- [ ] **Step 5: Update all existing apply_plan tests** to account for the new `compatibility` field and the changed `backend_failed` parameter.

- [ ] **Step 6: Run tests** — `cargo test -p wc-app apply_plan`

- [ ] **Step 7: Commit**

### Task 2.6: Add `renderer_compatibility` to WallpaperDto + wire frontend

**Files:**
- Modify: `apps/tauri-gui/src-tauri/src/commands/common.rs` — add `renderer_compatibility: Option<String>` to `WallpaperDto`, map from `plan.compatibility`.
- Modify: `apps/tauri-gui/frontend/src/api/bridge.ts` — add `rendererCompatibility?: string` to `WallpaperDTO`.
- Modify: `apps/tauri-gui/frontend/src/api/mockBridge.ts` — update scene/web fixtures.
- Modify: `apps/tauri-gui/frontend/src/components/wallpaperCardHelpers.ts` — add compatibility line + "Web · browse only" + "Renderer limitation" badge.
- Modify: `apps/tauri-gui/frontend/src/components/wallpaperCardHelpers.test.ts` — new tests.
- Modify: `apps/tauri-gui/frontend/src/components/WallpaperCard.tsx` — render the compatibility line.

- [ ] **Step 1-N: TDD per file, then run `npm run typecheck && npm run test:unit`**

- [ ] **Commit**

### Task 2.7: Update Tauri `error_dto` as fallback

**Files:**
- Modify: `apps/tauri-gui/src-tauri/src/commands/common.rs`

The `error_dto` string matching stays as a fallback for `WcError::Other` messages, but the primary classification now comes through `AppError.code` (from `command_error_from_app_error`). Verify the Tauri command path uses `command_error_from_app_error` (it already does at `wallpaper.rs:190`).

- [ ] **Run full backend tests, commit**

---

## Phase 3 — path-level thumbnail store (frontend)

### Task 3.1: Convert `ThumbnailRequestQueue` to delta callbacks

**Files:**
- Modify: `apps/tauri-gui/frontend/src/hooks/thumbnailQueueCore.ts`
- Modify: `apps/tauri-gui/frontend/src/hooks/thumbnailQueueCore.test.ts`

- [ ] **Step 1: Replace `onUpdate` with `onThumbnail` + `onFailure`**

Queue options:
```ts
interface QueueOptions {
  concurrency: number;
  load: (path: string) => Promise<ThumbnailDTO>;
  onThumbnail: (path: string, thumbnail: string) => void;
  onFailure: (path: string, reason?: string) => void;
}
```

Remove `scheduleEmit`, `emit`, `emitScheduled`, `thumbs: ThumbState`. Keep an internal `Map<string, string>` cache for `get`/`snapshot`. In `pump()`, on success call `onThumbnail(path, thumb)`, on empty/failed call `onFailure(path, reason)`.

- [ ] **Step 2: Update all existing tests to the new API**

- [ ] **Step 3: Run tests** — `npm run test:unit -- src/hooks/thumbnailQueueCore.test.ts`

- [ ] **Step 4: Commit**

### Task 3.2: Add `ThumbnailStore` with per-path subscribe

**Files:**
- Create: `apps/tauri-gui/frontend/src/state/thumbnailStore.ts`
- Create: `apps/tauri-gui/frontend/src/state/thumbnailStore.test.ts`
- Modify: `apps/tauri-gui/frontend/src/state/ThumbnailStoreContext.tsx`
- Modify: `apps/tauri-gui/frontend/src/hooks/useThumbnailQueue.ts`

- [ ] **Step 1: Write `ThumbnailStore` class**

```ts
export class ThumbnailStore {
  private cache = new Map<string, string>();
  private listeners = new Map<string, Set<() => void>>();
  private queue: ThumbnailRequestQueue;
  private enqueueScheduled = false;
  private pendingPaths: string[] = [];

  constructor(concurrency: number, load: (path: string) => Promise<ThumbnailDTO>) {
    this.queue = new ThumbnailRequestQueue({
      concurrency,
      load,
      onThumbnail: (path, thumb) => {
        this.cache.set(path, thumb);
        this.listeners.get(path)?.forEach(cb => cb());
      },
      onFailure: (path) => {
        this.listeners.get(path)?.forEach(cb => cb());
      },
    });
  }

  get(path: string): string | undefined { return this.cache.get(path); }

  subscribe(path: string, cb: () => void): () => void {
    if (!this.listeners.has(path)) this.listeners.set(path, new Set());
    this.listeners.get(path)!.add(cb);
    return () => { this.listeners.get(path)?.delete(cb); };
  }

  enqueueVisible(paths: string[], options?: EnqueueOptions): void {
    this.pendingPaths.push(...paths);
    if (this.enqueueScheduled) return;
    this.enqueueScheduled = true;
    const flush = () => {
      this.enqueueScheduled = false;
      const unique = Array.from(new Set(this.pendingPaths));
      this.pendingPaths = [];
      this.queue.enqueue(unique, options);
    };
    if (typeof requestAnimationFrame === 'function') requestAnimationFrame(flush);
    else Promise.resolve().then(flush);
  }

  forget(paths: string[]): void { this.queue.forget(paths); }
  reset(): void { this.queue.reset(); this.cache.clear(); this.listeners.clear(); }
  snapshot() { return this.queue.snapshot(); }
}
```

- [ ] **Step 2: Write tests** — `onThumbnail` notifies only matching path subscriber; `get` returns cached; `enqueueVisible` coalesces.

- [ ] **Step 3: Update `ThumbnailStoreContext` to provide the store**

- [ ] **Step 4: Run tests, commit**

### Task 3.3: WallpaperCard uses `useSyncExternalStore`

**Files:**
- Modify: `apps/tauri-gui/frontend/src/components/WallpaperCard.tsx`
- Modify: `apps/tauri-gui/frontend/src/components/WallpaperGrid.tsx`

- [ ] **Step 1: Card subscribes to its own thumbnail**

```tsx
import { useSyncExternalStore } from 'react';
import { useThumbnailStore } from '../state/ThumbnailStoreContext';

// In WallpaperCardImpl:
const store = useThumbnailStore();
const thumbnail = useSyncExternalStore(
  (cb) => store.subscribe(entry.path, cb),
  () => store.get(entry.path),
);
```

- [ ] **Step 2: WallpaperGrid stops reading `thumbs`, uses throttled `enqueueVisible`**

Remove `thumbCache` prop. Replace the `virtualizer.range` effect with a throttled `enqueueVisible` call.

- [ ] **Step 3: Run `npm run typecheck && npm run test:unit && npm run smoke`**

- [ ] **Step 4: Commit**

### Task 3.4: Dynamic overscan (second sub-step)

**Files:**
- Modify: `apps/tauri-gui/frontend/src/components/WallpaperGrid.tsx`

- [ ] **Step 1: Add scroll-velocity tracker → overscan bucket**

Track scroll delta per frame; bucket into `slow` (overscan 2) / `fast` (overscan 6). Re-evaluate `overscan` on bucket change.

- [ ] **Step 2: Run tests + smoke, commit**

---

## Phase 4 — structured apply stage events (backend + frontend)

### Task 4.1: Define `ApplyStage`, `ApplyStageEvent`, `ApplyStageReporter` in wc-backend

**Files:**
- Create: `crates/wc-backend/src/apply_stage.rs`
- Modify: `crates/wc-backend/src/lib.rs` (add `pub mod apply_stage;`)
- Modify: `crates/wc-backend/src/runtime.rs` — no, stages go in `apply_wallpaper_with_runtime` in `lib.rs`

- [ ] **Step 1: Define types**

```rust
pub enum ApplyStage {
    ResolveTarget, EnsureAwwwDaemon, AwwwSocketReady,
    StartLwe, WaitRendererAlive, CleanupPrevious, RefreshStatus,
}
pub struct ApplyStageEvent {
    pub stage: ApplyStage,
    pub request_id: Option<String>,
}
pub trait ApplyStageReporter {
    fn emit(&mut self, event: ApplyStageEvent);
}
pub struct NoopReporter;
impl ApplyStageReporter for NoopReporter {
    fn emit(&mut self, _event: ApplyStageEvent) {}
}
```

- [ ] **Step 2: Commit**

### Task 4.2: Emit stages in `apply_wallpaper_with_runtime`

**Files:**
- Modify: `crates/wc-backend/src/lib.rs`

- [ ] **Step 1: Add `reporter: &mut dyn ApplyStageReporter` parameter to `apply_wallpaper_with_runtime`**

Emit stages at the right points:
- Awww path: ResolveTarget → EnsureAwwwDaemon (before `ensure_awww_daemon_running`) → AwwwSocketReady (after daemon ready) → CleanupPrevious (before stop plan) → RefreshStatus (at end)
- LWE path: ResolveTarget → StartLwe (before `linux_wallpaperengine::apply`) → WaitRendererAlive (in the poll loop) → CleanupPrevious → RefreshStatus

- [ ] **Step 2: Update all callers** — `apply_wallpaper` passes `&mut NoopReporter`. Tests pass a capturing reporter.

- [ ] **Step 3: Write tests** — capturing reporter asserts stage order for Awww success, LWE success, Awww failure (stops at EnsureAwwwDaemon), LWE crash (reaches WaitRendererAlive).

- [ ] **Step 4: Run tests, commit**

### Task 4.3: Plumb `ApplyExecutionOptions` through wc_app

**Files:**
- Modify: `crates/wc-app/src/lib.rs`
- Modify: `crates/wc-app/src/apply_execution.rs`

- [ ] **Step 1: Add `ApplyExecutionOptions`**

```rust
pub struct ApplyExecutionOptions {
    pub request_id: Option<String>,
    pub stage_reporter: Option<Box<dyn wc_backend::apply_stage::ApplyStageReporter + Send>>,
}
```

`execute_apply_request` delegates to `execute_apply_request_with_options` with default options.

- [ ] **Step 2: Run tests, commit**

### Task 4.4: Tauri layer emits `wc-apply-stage` events

**Files:**
- Modify: `apps/tauri-gui/src-tauri/src/commands/wallpaper.rs`

- [ ] **Step 1: Build a Tauri reporter in `execute_and_format_result`**

```rust
struct TauriStageReporter {
    app: tauri::AppHandle,
    request_id: Option<String>,
}
impl wc_backend::apply_stage::ApplyStageReporter for TauriStageReporter {
    fn emit(&mut self, event: wc_backend::apply_stage::ApplyStageEvent) {
        let _ = self.app.emit("wc-apply-stage", serde_json::json!({
            "requestId": event.request_id,
            "stage": format!("{:?}", event.stage),
            "label": stage_label(&event.stage),
            "detail": stage_detail(&event.stage),
        }));
    }
}
```

- [ ] **Step 2: Remove the old `emit_apply_feedback("Starting backend", ...)` calls** (replaced by stage events).

- [ ] **Step 3: Run `cargo test -p wallpaper-console-tauri`, commit**

### Task 4.5: Frontend `ApplyQueueController` consumes `wc-apply-stage`

**Files:**
- Modify: `apps/tauri-gui/frontend/src/hooks/applyQueueController.ts`
- Modify: `apps/tauri-gui/frontend/src/hooks/useApplyQueue.test.ts`
- Modify: `apps/tauri-gui/frontend/src/events/appEvents.ts` — add `applyStage: 'wc-apply-stage'`

- [ ] **Step 1: Add stage subscription in `ApplyQueueController.run`**

Subscribe to `wc-apply-stage` Tauri events during `await deps.applyAction(req)`; update feedback with the live stage label/detail; unsubscribe on completion/error.

- [ ] **Step 2: Update tests** — assert stage feedback is surfaced, unsubscribe happens on both success and error.

- [ ] **Step 3: Run `npm run typecheck && npm run test:unit`, commit**

---

## Final verification

- [ ] **Step 1: Full backend gate**
```bash
cargo test -p wc-backend && cargo test -p wallpaper-console-tauri && cargo test --workspace && cargo clippy --workspace -- -D warnings
```

- [ ] **Step 2: Full frontend gate**
```bash
cd apps/tauri-gui/frontend && npm run typecheck && npm run test:unit && npm run smoke
```

- [ ] **Step 3: Full verify**
```bash
cargo run -p xtask -- verify all
```

- [ ] **Step 4: Append to `doc/construct.md`**

- [ ] **Step 5: Final commit**
