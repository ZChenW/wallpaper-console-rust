# Wallpaper Engine Web Backend Implementation Plan

> **ARCHIVED / OBSOLETE (2026-06-12)**
>
> This plan described the original Chromium-based Web wallpaper backend. Real-world Niri/Wayland testing proved that Chromium app windows **cannot** serve as true desktop wallpapers — they are normal tiled windows, not layer-shell background layers.
>
> **Current decision:** WE Web is unsupported for live apply. Chromium preview and native WebKit renderer experiments have been removed from the active project.
>
> **Current implementation:** `we_web` is still indexed as one project-level Library card with preview metadata, but its backend is `unsupported`. Use preview GIF fallback, open the project folder, or copy the Workshop ID. This archived plan remains only as history for the rejected Web-renderer direction.
>
> See `docs/DEVELOPMENT.md` for current Web wallpaper support status.

> **Original execution instructions below are OBSOLETE and must NOT be followed.**

**Goal (original, now obsolete):** make Wallpaper Engine `type=web` projects run as real Web wallpapers through a dedicated Chromium backend instead of remaining preview-only.

**Architecture (original):** keep `we_scene` on `linux-wallpaperengine`; route `we_web` through a new `chromium-web` backend that launches the project's `index.html` in an isolated Chromium app window. The first version targets Arch/niri/Wayland with documented compositor window rules, not automatic niri config mutation.

**Tech Stack:** Rust workspace (`wc-core`, `wc-scan`, `wc-backend`, `wc-storage`, Tauri commands), Tauri 2, React/TypeScript/Vite frontend, Chromium external process, Playwright smoke tests.

---

## Current Problem

`linux-wallpaperengine` does not reliably render Wallpaper Engine Web/HTML/JS/WebGL/Live2D projects. The current app correctly indexes `we_web` projects but treats them as preview-only. The next implementation must add a separate Web backend rather than forcing Web projects through `linux-wallpaperengine`.

Known user target:

- Desktop/session: niri on Wayland.
- Browser backend: `/usr/bin/chromium` exists locally; implementation must still support `auto` and custom browser paths.
- First version: Chromium app/window backend plus user-managed niri window rules.
- Audio default: enabled.
- Mouse interaction: not a first-version goal; avoid stealing focus where possible.

## Global Rules

- Do not route `we_web` through `linux-wallpaperengine`.
- Do not break existing `we_scene`, image, gif, video, favorites, history, or SQLite behavior.
- Do not apply `preview.gif` unless the user explicitly selects `Apply preview GIF`.
- Do not auto-edit niri or compositor config.
- Do not use shell string concatenation for external process commands; use structured `Command` args.
- Do not kill all Chromium processes; stop only the PID/process group started by this backend.

## Target Behavior (Obsolete)

- `we_web` Library card shows `WE Web` and `Chromium Web backend`.
- Context menu for `we_web` shows:
  - `Apply Web wallpaper`
  - `Apply preview GIF`
  - `Open Project Folder`
  - `Copy Workshop ID`
- Applying a Web wallpaper launches Chromium against `project.json.file`, usually `index.html`.
- Current state records the WE project path and backend `chromium-web`.
- If Chromium is missing or the Web project is invalid, GUI shows a specific user-facing error and keeps preview fallback available.
- Settings shows Web backend status and a copyable niri rule template/instructions.

## Implementation Tasks (Obsolete)

### Task 1: Add Core Backend Identity And Config Defaults

**Files:**

- Modify: `crates/wc-core/src/types.rs`
- Modify: `crates/wc-core/src/config.rs`
- Modify: `crates/wc-core/src/formats.rs` only if backend inference needs updating

- [ ] Add `Backend::ChromiumWeb` with serialized value `chromium-web`.
- [ ] Add config defaults:
  - `web_wallpaper_enabled=on`
  - `web_wallpaper_browser=auto`
  - `web_wallpaper_audio=on`
  - `web_wallpaper_extra_args=`
  - `web_wallpaper_window_width=1920`
  - `web_wallpaper_window_height=1080`
- [ ] Ensure `Backend::as_str()` returns `chromium-web`.
- [ ] Update backend parsing wherever strings are converted back into `Backend`.
- [ ] Add Rust tests for backend parse/serialize if the project already tests backend parsing.

### Task 2: Route WE Web Scanning To Chromium Backend

**Files:**

- Modify: `crates/wc-scan/src/lib.rs`
- Modify: `crates/wc-cli/tests/parity_tests.rs`
- Modify: storage tests that assert WE metadata/backend

- [ ] Change `project.json type=web` project entries from `Backend::Unsupported` to `Backend::ChromiumWeb`.
- [ ] Keep project-level indexing: the entry path remains the project directory, not `index.html` or `preview.gif`.
- [ ] Preserve `preview_path`, `workshop_id`, `title`, and `we_file`.
- [ ] Keep assets filtering: `assets/*.png`, `index.html`, and other project internals must not become separate library items.
- [ ] Add/update tests:
  - `type=web` and `type=Web` become `we_web` with backend `chromium-web`.
  - missing/invalid project assets do not pollute Library.
  - scene behavior remains `linux-wallpaperengine`.

### Task 3: Implement `wc-backend::web_wallpaper`

**Files:**

- Create: `crates/wc-backend/src/web_wallpaper.rs`
- Modify: `crates/wc-backend/src/lib.rs`
- Modify: `crates/wc-backend/Cargo.toml` only if a small dependency is truly required

- [ ] Define a focused config struct read from `StorageApi`:
  - enabled
  - browser path
  - audio enabled
  - extra args
  - window width/height
- [ ] Implement browser resolution:
  - custom path if set and executable
  - otherwise auto-detect `chromium`, `google-chrome-stable`, `google-chrome`, `brave`, `brave-browser`, `vivaldi`
- [ ] Implement project resolution:
  - path must be a WE project directory
  - `project.json` must parse
  - `type` must be web, case-insensitive
  - `file` must exist under the project root
  - reject path traversal; `file` must stay inside project root after normalization
- [ ] Implement command spec:
  - browser executable
  - `--app=file:///<project>/<file>`
  - `--user-data-dir=<config>/web-wallpaper-profile`
  - `--no-first-run`
  - `--disable-session-crashed-bubble`
  - `--autoplay-policy=no-user-gesture-required`
  - `--allow-file-access-from-files`
  - window size from config
  - append configured extra args as whitespace-split args only after trimming
- [ ] If audio is disabled, add `--mute-audio`; default is audio enabled.
- [ ] Start Chromium through a process-group-friendly wrapper consistent with existing backend style.
- [ ] Store PID under a dedicated config key such as `web_wallpaper_pid`.
- [ ] Stop only the recorded PID/process group; do not `pkill chromium`.
- [ ] On successful apply, write current project path, last backend `chromium-web`, and history.
- [ ] Add Rust tests:
  - command uses `index.html`, not preview.
  - missing browser returns `web_backend_missing`.
  - missing `file` or missing `index.html` returns `invalid_web_project`.
  - path traversal in `file` is rejected.
  - apply writes current project path, not preview.
  - stop targets stored PID behavior through a harmless mock executable.

### Task 4: Integrate Backend Dispatch And Error Mapping

**Files:**

- Modify: `crates/wc-backend/src/lib.rs`
- Modify: `apps/tauri-gui/src-tauri/src/commands/common.rs`
- Modify: `apps/tauri-gui/src-tauri/src/commands/wallpaper.rs`

- [ ] Dispatch `Backend::ChromiumWeb` to `web_wallpaper::apply`.
- [ ] Include Chromium Web in stop-all behavior by calling `web_wallpaper::stop`.
- [ ] Ensure `restore()` routes `FileType::WeWeb` to `Backend::ChromiumWeb`, not `LinuxWallpaperEngine`.
- [ ] Add structured command errors:
  - `web_backend_missing`
  - `web_backend_disabled`
  - `invalid_web_project`
  - `web_backend_exited`
  - `web_backend_permission_denied`
  - `web_display_rule_needed`
- [ ] Do not write `we_compatibility.json` scene compatibility entries for Web backend errors.
- [ ] Add Tauri command for Web backend status, mirroring the linux-wallpaperengine status pattern.

### Task 5: Update Frontend Library Actions

**Files:**

- Modify: `apps/tauri-gui/frontend/src/views/LibraryView.tsx`
- Modify: `apps/tauri-gui/frontend/src/components/WallpaperGrid.tsx`
- Modify: `apps/tauri-gui/frontend/src/api/bridge.ts`
- Modify: `apps/tauri-gui/frontend/src/api/mockBridge.ts`

- [ ] Change WE Web card text from `preview only` to backend-aware text:
  - ready: `Chromium Web backend`
  - missing: `Web backend missing`
  - failed: `Web backend failed`
- [ ] Add `Apply Web wallpaper` context action for `we_web`.
- [ ] Keep `Apply preview GIF` visible for `we_web`.
- [ ] Keep `Apply with linux-wallpaperengine` visible only for `we_scene`.
- [ ] Keep `Retry backend apply` only for failed scene compatibility unless a separate Web failure cache is intentionally added.
- [ ] Add Web backend status DTO/type in bridge and mock data.
- [ ] Ensure mock `3650880224` shows the new Web apply action.

### Task 6: Add Settings Web Backend Section

**Files:**

- Modify: `apps/tauri-gui/frontend/src/views/SettingsView.tsx`
- Modify: frontend CSS only if needed for readable rule block
- Modify: docs as listed in Task 8

- [ ] Add `Web Wallpaper Backend` settings group:
  - enable on/off
  - browser path
  - audio on/off
  - window width/height
  - extra args
- [ ] Show backend status:
  - ready with resolved browser path
  - missing with install/config suggestion
  - disabled
- [ ] Add niri rule guidance with copyable text. Do not modify niri config automatically.
- [ ] The rule guidance must say the first version uses a compositor-managed Chromium window, not a native layer-shell background.
- [ ] Keep linux-wallpaperengine Settings text scene-specific; remove claims that it supports Web.

### Task 7: Frontend And Smoke Tests

**Files:**

- Modify: frontend unit tests as appropriate
- Modify: `apps/tauri-gui/frontend/e2e/smoke.spec.ts`
- Modify: mock bridge data

- [ ] Add frontend tests:
  - WE Web card shows `Apply Web wallpaper`.
  - WE Web card still shows `Apply preview GIF`.
  - WE Web card does not show `Apply with linux-wallpaperengine`.
  - WE Scene still shows `Apply with linux-wallpaperengine`.
  - Web backend missing status displays a clear prompt.
- [ ] Add smoke tests:
  - mock `3650880224` is a Web card with `Apply Web wallpaper`.
  - Web preview fallback remains visible.
  - Scene failed compatibility UI remains unchanged.
  - Settings displays `Web Wallpaper Backend`.
- [ ] Keep existing 76 smoke tests passing or update counts only when tests are intentionally added.

### Task 8: Documentation Updates

**Files:**

- Modify: `README.md`
- Modify: `docs/DEVELOPMENT.md`
- Modify: `docs/TAURI_MANUAL_SMOKE_CHECKLIST.md`
- Modify: `docs/OPENCODE_TAURI_MATURITY_PLAN.md`
- Modify or add a short runtime note if needed

- [ ] Update the current support matrix:
  - `we_scene`: linux-wallpaperengine, partial compatibility.
  - `we_web`: Chromium Web backend, compositor-rule dependent.
  - image/gif/video: existing backends.
- [ ] Remove or correct any claim that linux-wallpaperengine handles Web wallpapers.
- [ ] Document Chromium dependency and auto-detection order.
- [ ] Document niri rule requirement and that the app does not auto-edit niri config.
- [ ] Update manual smoke checklist for `3650880224`:
  - card appears as WE Web.
  - Apply Web wallpaper launches Chromium backend.
  - current state points to project folder.
  - preview fallback still works.

## Five Review / Fix Loops

After implementation and initial verification, run five real review loops. Each loop must record findings, fixes, and verification in the final report.

### Review Loop 1: Requirement Coverage

- [ ] Confirm `we_web` no longer says preview-only in normal ready state.
- [ ] Confirm `we_web` does not use `linux-wallpaperengine`.
- [ ] Confirm `we_scene` behavior and compatibility cache did not regress.
- [ ] Confirm current state for Web uses project path, not preview.
- [ ] Fix any mismatch immediately.
- [ ] Rerun targeted Rust and frontend tests.

### Review Loop 2: Process Lifecycle

- [ ] Confirm Web apply stops previous wallpaper backend first.
- [ ] Confirm Web stop kills only the recorded Chromium process/process group.
- [ ] Confirm user Chromium/browser sessions are not killed.
- [ ] Confirm repeated apply/stop/apply does not leave obvious orphan processes.
- [ ] Fix lifecycle issues immediately.
- [ ] Rerun backend tests.

### Review Loop 3: Security And Path Safety

- [ ] Confirm `project.json.file` cannot escape the project root.
- [ ] Confirm command args are structured, not shell-concatenated.
- [ ] Confirm extra args parsing cannot execute shell syntax.
- [ ] Confirm file URLs are built from canonical project-local paths.
- [ ] Fix path or command safety issues immediately.
- [ ] Rerun path-safety tests.

### Review Loop 4: GUI UX And Error Clarity

- [ ] Confirm Web missing/disabled/invalid errors are distinct and understandable.
- [ ] Confirm Web cards always keep preview fallback.
- [ ] Confirm Settings explains niri rule requirement without claiming automatic desktop-layer support.
- [ ] Confirm no outdated `linux-wallpaperengine supports web` text remains.
- [ ] Fix UI/doc inconsistencies immediately.
- [ ] Rerun typecheck, unit tests, and smoke tests.

### Review Loop 5: Full Regression And Merge Readiness

- [ ] Run the full verification matrix.
- [ ] Inspect `git diff` for unrelated churn.
- [ ] Confirm docs match code behavior.
- [ ] Confirm no Wails references re-enter active docs.
- [ ] Confirm any unverified manual GUI steps are honestly marked.
- [ ] Fix final issues and rerun affected checks.

## Final Verification Matrix

Run before reporting completion:

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo build --workspace

cd apps/tauri-gui/frontend
npm run typecheck
npm run test:unit
npm run build
npm run smoke
```

If a real desktop session is available, also manually test:

- Apply a WE Web project such as `3650880224`.
- Confirm Chromium starts with the expected project `index.html`.
- Confirm preview fallback still works.
- Confirm stop terminates the Web wallpaper process.
- Confirm niri rule instructions are sufficient; do not mark this complete if the environment blocks visual desktop validation.

## Final Report Requirements

The final report must include:

- Files changed by subsystem.
- New backend behavior and config keys.
- Exact commands run and results.
- Five review loops with findings/fixes/verification.
- Manual GUI acceptance result or explicit environment limitation.
- Remaining risks limited to real non-blocking risks.
