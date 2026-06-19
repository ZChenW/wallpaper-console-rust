# Readiness, Compatibility & Apply Stage Events — Design

Date: 2026-06-19
Status: Ready for implementation plan
Supersedes / extends: `doc/performance-hardening-plan.md` (Phase 3 apply work)
Execution log: appended to `doc/construct.md` (append-only, per house convention)

## Purpose

Four independent fixes, executed as four separate commits/phases, each independently verifiable:

- P1 (backend): awww-daemon readiness — stop trusting `pgrep` alone.
- P1 (backend+frontend): WE scene / web capability boundaries — make renderer compatibility a first-class concept and stop presenting failed/low-compat scenes as plain failures.
- P2 (frontend): path-level thumbnail store — stop one thumbnail completion re-rendering the whole grid.
- P2 (backend+frontend): structured apply stage events — replace the static "Starting renderer" stall with real progress, covering both success and failure paths.

Phases 1 and 4 share the `apply_wallpaper_with_runtime` pipeline; phase 4 builds on phase 1's readiness work.

## Scope guardrails

- No public command names, config keys, install script, or niri bindings change.
- No backend settle-time defaults change (evidence-first, per the perf-hardening plan).
- wc-backend never depends on Tauri; apply-stage emission goes through an injected reporter.
- Error kinds live in wc-core (no wc-core → wc-backend dependency inversion).

---

## Phase 1 — awww daemon readiness (P1, backend)

### Problem

`ensure_awww_daemon_running()` (`crates/wc-backend/src/runtime.rs:58`) trusts `pgrep` alone. A daemon process can exist while its IPC socket isn't ready, causing `awww img` to race the socket and fail/flicker.

Empirically verified facts:
- awww-daemon creates `${XDG_RUNTIME_DIR}/${WAYLAND_DISPLAY}-awww-daemon.sock` (observed: `/run/user/1000/wayland-1-awww-daemon.sock`) and removes it on exit.
- `awww --help` exposes `awww query` — a non-destructive readiness probe that prints output info and requires a live, responding daemon.
- `awww-daemon --help` documents the socket convention and the `WAYLAND_DISPLAY` → `wayland-0` default.

### Design

`awww_socket_path()` (in `crates/wc-backend/src/runtime.rs`):
- Reads `XDG_RUNTIME_DIR` — **required**. Missing → `Err("XDG_RUNTIME_DIR is not set; cannot locate awww-daemon socket")`. No `/run/user/<uid>` fallback.
- Reads `WAYLAND_DISPLAY`, defaulting to `wayland-0` (matches awww's own default).
- Returns `${XDG_RUNTIME_DIR}/${WAYLAND_DISPLAY}-awww-daemon.sock`.

New `BackendRuntime` trait method:
```rust
enum AwwwReadiness {
    Ready,
    SocketMissing,
    SocketPresentQueryFailed { stderr: String },
}
fn awww_socket_ready(&mut self) -> AwwwReadiness;
```
- Uses `&mut self` so `FakeRuntime` can simulate "ready only on the Nth poll".
- `SystemBackendRuntime::awww_socket_ready`: if the socket path is missing → `SocketMissing`; if present, run `awww query` (Stdio::null on stdout/stderr captured) → success `Ready`, failure `SocketPresentQueryFailed { stderr }`.

Rework `ensure_awww_daemon_running(&mut self)`:
1. `awww_socket_ready() == Ready` → `Ok` immediately (fast path: socket + query healthy; no spawn, no pgrep).
2. Otherwise, if `!is_awww_daemon_running(user)` → spawn `setsid -f awww-daemon --no-cache` once.
3. Poll loop ~2.0s (40 × 50ms): each iteration calls `awww_socket_ready()`; `Ready` → `Ok`; otherwise sleep and continue. Track the last readiness and accumulate query stderr.
4. Timeout error, distinguished:
   - Process running but socket not ready → `Err("awww-daemon is running but socket is not ready for WAYLAND_DISPLAY=<…> (expected <path>); last query stderr: <…>")`.
   - Process absent → existing "awww-daemon failed to start" message.

`is_awww_daemon_running()` stays as a secondary signal for the timeout message only.

### Tests (wc-backend)
- Process exists, socket missing and `awww query` keeps failing → `Err`, not `Ok`.
- Socket appears + `awww query` succeeds after N polls → `Ok`.
- `Ready` fast path → `Ok` with spawn count 0.
- Socket missing + no process → spawn invoked.
- `FakeRuntime` overrides `awww_socket_ready` to simulate the poll progression.

---

## Phase 2 — WE scene / web capability boundaries (P1, backend + frontend)

### Problem
- `we_web` can mislead users into thinking live-apply is possible.
- `we_scene` cards give no renderer-compatibility context; scenes like 3479521040 that apply successfully but look wrong are indistinguishable from a normal success or generic failure.
- `we_compat::record_failure` is **never called in production** today (only `lookup_failure`/`clear_failure` are wired). A failed scene apply reappears as `Available` next time, never as `RetryableFailure`. This phase must wire it.
- `AppError::from_wc_error` (`crates/wc-app/src/lib.rs:218`) flattens every `WcError` to `code: "command_failed"`. The Tauri `error_dto` then re-classifies by string match — fragile. This phase must replace string-matching with a structured error source.

### Design

#### Structured error kind in wc-core (NOT in wc-backend)

In `crates/wc-core/src/error.rs`:
```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendErrorKind {
    RendererLimitation,
    TargetConfig,
    WorkshopDirectory,
    Generic,
}
```
Add a new `WcError` variant:
```rust
#[error("linux-wallpaperengine error ({kind:?}): {detail}")]
LinuxWallpaperEngine { kind: BackendErrorKind, detail: String },
```
The `kind` type lives in wc-core. wc-backend depends on wc-core (existing direction), so it can construct this variant; the dependency never inverts.

#### Classified error from wc-backend

`crates/wc-backend/src/linux_wallpaperengine.rs`:
- `map_renderer_error(status, stderr)` returns `WcError::LinuxWallpaperEngine { kind, detail }` instead of `WcError::Other(String)`.
- Classification (stderr lowercased):
  - contains `"projection must have a width"` → `RendererLimitation`
  - contains `"cannot find workshop directory"` → `WorkshopDirectory`
  - contains `"failed to create window"` / `"no suitable output"` / `"no display"` → `TargetConfig`
  - otherwise → `Generic`
- `detail` carries the status + stderr tail (as today, for diagnostics).

#### AppError code mapping in wc-app

`crates/wc-app/src/lib.rs::AppError::from_wc_error`:
- Add a branch for `WcError::LinuxWallpaperEngine { kind, detail }` mapping to `AppError.code`:
  - `RendererLimitation` → `"renderer_limitation"`
  - `TargetConfig` → `"target_config_error"`
  - `WorkshopDirectory` → `"workshop_directory_missing"`
  - `Generic` → `"linux_wallpaperengine_failed"`
- `detail` flows into `AppError.detail`. `message`/`suggestion` reused from today's `error_dto` text where applicable.
- The existing `WcError::Other` → `"command_failed"` fallback stays for non-LWE errors.
- The Tauri-layer `error_dto` string matching (`commands/common.rs:222`) becomes a **fallback** for messages arriving as `WcError::Other` (e.g. older paths), not the primary classifier.

#### record_failure wired in wc_app on the resolved-target failure path

`wc_app::execute_apply_request_with_options()` (new; `execute_apply_request` delegates with no-op options for existing callers/tests):
1. `let target = self.resolve_apply_request_target(&request)?;` — path/preview/Web-unsupported failures stop here and do **not** touch `we_compat`.
2. Call `wc_backend::apply_wallpaper...`.
3. If `apply` returns an `Err` whose `WcError` is `LinuxWallpaperEngine { kind, .. }` **and** `target.file_type == WeScene`:
   - `we_compat::record_failure(&target.state_path, backend_status, error_kind, error_message, error_detail)`
   - `backend_status = "renderer_limitation"` when `kind == RendererLimitation`; otherwise `"failed"`.
   - `error_kind` = the AppError code string (so `lookup_failure` can drive plan selection).
4. On success: if `target.file_type == WeScene`, `we_compat::clear_failure(&result.state_path)` (today this only happens in the Tauri layer; move/duplicate into wc_app so wc_app owns compat state). The Tauri layer keeps its existing `clear_failure` as a thin extra safety, or it is removed once wc_app owns it — implementation decision, but only one path should own recording/clearing. Spec recommends wc_app owns it; Tauri layer's `clear_failure` call is removed to avoid double writes.

Recording happens in wc_app (not the Tauri layer) because wc_app already knows `target.state_path`, `file_type`, and the classified error; the Tauri layer only formats the DTO and emits events.

Preview failures, path-resolution failures, and WE Web unsupported failures never reach `we_compat` (they fail at step 1 or are not LWE-classified).

#### Scene compatibility disclaimer

`crates/wc-app/src/apply_plan.rs` → `ApplyPlan`:
```rust
pub enum CompatibilityKind {
    NativeScene { disclaimer: String },
}
pub compatibility: Option<CompatibilityKind>,
```
- All `we_scene` plans (Available and RetryableFailure) set `CompatibilityKind::NativeScene { disclaimer: "Rendered by linux-wallpaperengine — may differ from Wallpaper Engine".into() }`.
- `we_web` / `we_application` plans: `None`.

DTO (`apps/tauri-gui/src-tauri/src/commands/common.rs::WallpaperDto`):
- Add `renderer_compatibility: Option<String>` mapped from `plan.compatibility` (the disclaimer string, or `None`).
- For `we_web`, strengthen `apply_reason` to make "cannot live-apply" explicit (the DTO already carries `applyReason`).

#### Failure reclassification in apply_plan

`plan_for_entry(entry, backend_failed)`:
- When `backend_failed` and the cached `error_kind == "renderer_limitation"`, produce a "Renderer limitation" plan:
  - Reason/label = "Renderer limitation" (not generic "failed").
  - Actions: keep `RetryBackendApply`, **Apply preview GIF** (if preview), Open folder, Copy Workshop ID.
  - `compatibility` = `NativeScene { disclaimer }` (same as available scenes).
- Generic LWE failure cached with non-renderer-limitation `error_kind` keeps the existing `RetryableFailure` plan text.
- `we_compat::WeCompatEntry` already stores `error_kind`; `dto_from_entry` already reads the cached entry, so `apply_plan` receives the kind via the existing `cached_failure` plumbing (the `backend_failed: bool` arg may need to widen to `backend_failed: Option<&WeCompatEntry>` or a small struct so `plan_for_entry` can see `error_kind`; implementation chooses the minimal signature change).

#### WE Web — "Apply preview only"

`we_web` stays non-live-applicable. When the project has a `preview_path`, add an `ApplyPreview` action with label **"Apply preview only"** and `reason: "Only the preview GIF can be applied as a static wallpaper; the Web scene itself is not supported."` This avoids implying the Web scene body is supported. When no preview, no apply action (as today).

#### Frontend card rendering

- `WallpaperCard`: render the renderer-compatibility status line for `we_scene` (the disclaimer). For a `renderer_limitation` cached state, show a "Renderer limitation" badge instead of the generic failure badge. `we_web` cards show a "Web · browse only" badge and surface `applyReason`.
- `wallpaperCardHelpers` get the new badge/line helpers + tests.
- `mockBridge` scene fixture updated (one available scene, one renderer-limitation scene, one generic-failure scene); `we_web` fixture gains the "Apply preview only" action.

### Tests
- `apply_plan` unit tests: `compatibility` field on available/retryable scenes; renderer-limitation plan text and actions; `we_web` "Apply preview only" action + reason.
- `wc-app` tests: `from_wc_error` maps each `BackendErrorKind` to the right `AppError.code`; `execute_apply_request_with_options` records `we_compat::record_failure` only for `WeScene` + LWE-classified failures (preview/Web-unsupported/path-resolution failures do **not** record); success clears.
- `linux_wallpaperengine` tests: `map_renderer_error` returns the right `BackendErrorKind` for each stderr pattern.
- `we_compat` tests: unchanged API; new coverage that `record_failure` accepts the renderer_limitation status.
- Tauri `commands/common.rs`: `dto_from_entry` carries `renderer_compatibility`; fallback `error_dto` still classifies legacy `WcError::Other` messages.
- Frontend: `wallpaperCardHelpers` tests for the new badges/lines; `mockBridge` fixture updates.

---

## Phase 3 — path-level thumbnail store (P2, frontend)

### Problem
Global `thumbs: Record<string,string>` (`useThumbnailQueue`) → one thumbnail completing re-renders the whole grid; `WallpaperGrid` reads the whole map and passes `thumbCache[e.path]` to each card; the `virtualizer.range` effect re-enqueues repeatedly; fixed overscan.

### Design — split into two sub-steps for reviewability

#### Step 1: path-level subscription + delta queue (core perf fix)

`ThumbnailRequestQueue` (`hooks/thumbnailQueueCore.ts`) callback becomes **delta** instead of `{ ...thumbs }`:
- Replace `onUpdate: (state: ThumbState) => void` with:
  - `onThumbnail(path: string, thumbnail: string): void`
  - `onFailure(path: string, reason?: string): void`
- Remove `scheduleEmit` / `emit` / the `emitScheduled` flag / the `ThumbState` `thumbs` field. The queue keeps an internal `Map<string, string>` cache for `get(path)` and `snapshot()`; it no longer holds a "state object" that consumers subscribe to.
- `pump()` calls `onThumbnail(item.path, thumb.thumbnail)` on success (or `onFailure` when `thumb.thumbnail` is empty/failed).

New `ThumbnailStore` wrapping the queue:
- Maintains the cache (`Map<path, thumbnail>`).
- `get(path): string | undefined` — sync read; cache hit shows immediately, no enqueue.
- `subscribe(path, cb): () => void` — per-path listener; on `onThumbnail(path, thumb)`, update cache and notify **only** `subscribe(path)` listeners.
- `enqueueVisible(paths, options)` — throttled/coalesced enqueue into the underlying queue.

`ThumbnailStoreContext` provides the singleton store (not a state object). `WallpaperCard` reads its thumbnail via:
```ts
const thumbnail = useSyncExternalStore(
  () => store.subscribe(path, cb),
  () => store.get(path),
);
```
React 19 / `useSyncExternalStore` available (`react@^19`). Only that card re-renders on its thumbnail completion.

`WallpaperGrid`:
- Stop reading `thumbs`. Stop passing `thumbCache` to cards.
- Replace the `virtualizer.range` effect with a throttled `enqueueVisible` on range change — coalesce within one animation frame; no re-trigger on identical ranges.

#### Step 2: dynamic overscan (same phase, second commit)

- Scroll-velocity tracker on the scroll container: bucket into slow / fast.
- Overscan 2 when slow, 6–8 when fast; re-evaluate the virtualizer `overscan` option only on velocity-bucket change (not on every scroll event).
- Kept as a separate commit so the core store/queue/grid refactor (Step 1) reviews cleanly without velocity state mixed in.

### Tests
- `thumbnailQueueCore` store tests: `onThumbnail` notifies only the matching path's subscribers; `get` returns cached values; `enqueueVisible` throttle coalesces multiple calls in one frame.
- Existing queue tests updated to the delta API.
- Smoke test for grid render remains green.

---

## Phase 4 — structured apply stage events (P2, backend + frontend)

### Problem
Apply shows a static "Starting renderer" with no real progress; the "white stall" has no stage feedback.

### Design

#### No Tauri dependency in wc-backend

`crates/wc-backend` defines (no `AppHandle` anywhere):
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
```
A default no-op reporter exists for existing callers/tests. `apply_wallpaper_with_runtime` takes `&mut dyn ApplyStageReporter` and emits at each stage. (A `Box<dyn FnMut(ApplyStageEvent) + Send>` closure is an acceptable alternative if it simplifies plumbing; the trait is preferred for testability.)

#### ApplyExecutionOptions plumbed through

New `ApplyExecutionOptions { request_id: Option<String>, stage_reporter: Option<Box<dyn ApplyStageReporter + Send>> }`. Flow:
- `apps/tauri-gui/src-tauri/src/commands/wallpaper.rs::execute_and_format_result` builds the reporter (calls `app.emit("wc-apply-stage", { requestId, stage, label, detail })`), then calls:
- `wc_app::execute_apply_request_with_options(request, options)` — new; `execute_apply_request` delegates with no-op options for existing callers/tests.
- `wc_app` calls `wc_backend::apply_wallpaper_with_runtime(..., reporter)`.

Tests capture the reporter calls (a `Vec<ApplyStageEvent>`-collecting reporter), never an `AppHandle`.

#### Stage emission — success AND failure paths

Success paths:
- Awww path: `ResolveTarget → EnsureAwwwDaemon → AwwwSocketReady → CleanupPrevious → RefreshStatus`.
- LWE path: `ResolveTarget → StartLwe → WaitRendererAlive → CleanupPrevious → RefreshStatus`.
- Detail distinguishes renderer: preview GIF → "Awww", WE Scene → "linux-wallpaperengine".

Failure paths (the UI must not stall on `WaitRendererAlive`):
- **Stage events do not carry error terminal state** — final error feedback still comes via `CommandResult` → `deps.makeErrorFeedback`.
- But before a failing apply returns, the stages already reached are emitted, so the UI shows where it stopped:
  - Awww socket timeout → emits `EnsureAwwwDaemon` then fails (no `AwwwSocketReady`).
  - LWE immediate crash → emits `StartLwe → WaitRendererAlive` then fails.
  - LWE projection error → emits `StartLwe → WaitRendererAlive` then fails (classified `RendererLimitation` per Phase 2).
- Implementation: `apply_wallpaper_with_runtime` emits the stage, then runs the step; if the step errors, the error propagates and no later stage is emitted. The last emitted stage reflects where work stopped.

#### Frontend

- Dedicated event name `wc-apply-stage` (distinct from `wc-feedback`). `useFeedbackBridge` listens only to `wc-feedback` — **no conflict by construction**, no ignore logic needed.
- `ApplyQueueController` subscribes to `wc-apply-stage` during `await deps.applyAction(req)`, updates feedback with the live stage label/detail, and **always unsubscribes on completion or error**. Keeps current + latest-pending queue semantics.
- Stage labels replace the static "Starting renderer / Settling…" text with the actual current stage.

### Tests
- Backend: capturing reporter asserts stage order for Awww success, LWE success, Awww socket-timeout failure (stops at `EnsureAwwwDaemon`), LWE crash failure (reaches `WaitRendererAlive`).
- `applyQueueController` test: surfaces received stages in feedback, unsubscribes on both success and error.
- Frontend: detail differs for preview (Awww) vs scene (linux-wallpaperengine).

---

## Cross-cutting

### Verification (every phase)
- `cargo test -p wc-backend`
- `cargo test -p wallpaper-console-tauri`
- `cargo test --workspace`
- `npm run typecheck`
- `npm run test:unit`
- `npm run smoke`
- `cargo run -p xtask -- verify all`
- `cargo clippy -D warnings`
- `git diff --check`

### Manual acceptance (per phase)
- Phase 1: kill any awww-daemon; apply an image; socket appears before apply returns; no `awww img` race. Simulate a stuck daemon (process alive, socket missing) → clear error mentioning `WAYLAND_DISPLAY` and the expected socket path.
- Phase 2: a known renderer-limitation scene shows "Renderer limitation" (not generic failure) with preview/folder/copy actions; available scenes show the compatibility disclaimer; a `we_web` card shows "Web · browse only" and "Apply preview only" (when preview exists).
- Phase 3: full-screen Library fast scroll — cards populate per-path without re-rendering the whole grid; no scroll-to-top on thumbnail completion; dynamic overscan reduces blank-ahead during fast scroll.
- Phase 4: applying a scene shows ResolveTarget → StartLwe → WaitRendererAlive → … live; a failing apply shows the last reached stage, then the error feedback; the UI never stalls on a single stage.

### Documentation
- This spec: `doc/specs/2026-06-19-readiness-compat-stages-design.md`.
- Execution log: appended to `doc/construct.md` (append-only, per house convention). No historical edits.

### Out of scope
- Changing public command names, config keys, install script, niri bindings.
- Lowering backend settle-time defaults without evidence.
- Auto-detecting "scene looks bad on success" (infeasible; handled by disclaimer + failure reclassification instead).
- `/run/user/<uid>` fallback for the awww socket.
