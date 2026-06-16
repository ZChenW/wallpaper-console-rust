# Development Guide

## Prerequisites

- Rust 1.77+
- Node.js 22+
- `webkit2gtk-4.1` (Tauri 2)
- Optional: `ffmpeg`, `imagemagick`, `ffmpegthumbnailer` (thumbnails)
- Optional: `fzf`, `kitty`/`chafa` (CLI browse preview)
- Optional: `linux-wallpaperengine` for Wallpaper Engine scene wallpapers
  - Arch/AUR: `yay -S linux-wallpaperengine-git`

## Priority Hardening Review Rule

For production-hardening work that touches Tauri commands, scan performance, thumbnail queues, or backend lifecycle:

1. Implement one priority at a time.
2. Run targeted tests for the touched subsystem.
3. Run `code-review-expert` on the current git diff.
4. Fix all P0/P1 findings before continuing.
5. Run the broader verification matrix before final closeout.

## Rust Verification

```bash
cargo run -p xtask -- verify rust

cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo build --workspace
```

## Application Service Layer

Shared application decisions live in `crates/wc-app`:

- apply path inspection and Wallpaper Engine project/media resolution
- backend routing for image/gif/video/WE scene/WE web
- structured user-facing error mapping
- scene compatibility failure recording/clearing

The Tauri `apply` command and CLI `apply`/`inspect` commands should call `wc-app`
instead of duplicating backend selection. This keeps GUI and CLI behavior aligned
while preserving the existing public command names.

## Frontend Apply Action Domain

Frontend action availability is driven by the backend `ApplyPlan::plan_for_entry()` through `WallpaperDTO.applyActions`. All UI action decisions (Apply, Retry, Preview, Open folder, Copy Workshop ID) use `domain/applyActions.ts` to normalize DTO actions into enabled/disabled render decisions:

```ts
import { normalizeApplyActions, isApplyAvailable, hasEnabledAction } from './domain/applyActions';

const actions = normalizeApplyActions(entry);
if (isApplyAvailable(entry)) { /* show Apply button */ }
```

A legacy fallback exists for old DTOs without `applyActions` (centralized in one file). New backends MUST provide `applyActions`. Frontend components should NOT infer action availability from `entry.type` or `entry.backendStatus`.

## Tauri Frontend

```bash
cd apps/tauri-gui/frontend
npm install
npm run test:unit
npm run typecheck
npm run build
npm run smoke          # Playwright smoke tests (requires chromium)
```

From the repository root, the same frontend matrix is available as:

```bash
cargo run -p xtask -- verify frontend
```

## Tauri Bundle

```bash
cd apps/tauri-gui/src-tauri
cargo tauri build --bundles deb,rpm
```

Outputs:
- `target/release/bundle/deb/wallpaper-console-gui-rust_0.1.0_amd64.deb`
- `target/release/bundle/rpm/wallpaper-console-gui-rust-0.1.0-1.x86_64.rpm`

## Install Test

```bash
./install.sh --build-only
./scripts/test_install_build_only.sh
```

For a prefix install/uninstall check without touching the normal user prefix:

```bash
tmp_prefix="$(mktemp -d)"
./install.sh --prefix "$tmp_prefix"
test -x "$tmp_prefix/bin/wallpaper-console-rust"
test -x "$tmp_prefix/bin/wallpaper-console-gui-rust"
test -f "$tmp_prefix/share/applications/wallpaper-console-gui-rust.desktop"
test -f "$tmp_prefix/share/icons/hicolor/128x128/apps/wallpaper-console-gui-rust.png"
./install.sh --prefix "$tmp_prefix" --uninstall
test ! -e "$tmp_prefix/bin/wallpaper-console-rust"
test ! -e "$tmp_prefix/bin/wallpaper-console-gui-rust"
```

## Smoke Test (Playwright)

```bash
cd apps/tauri-gui/frontend
npm run smoke
```

Uses a mock bridge API — does not apply wallpapers or start backends. Screenshots land in `e2e/screenshots/`.

## Performance Overlay

Enable via `localStorage` or query parameter:

```js
localStorage.setItem('wcPerfOverlay', '1')
// or: ?perf=1 in the URL
```

Shows library page ms, thumbnail cache hit/miss, queue depth, and rescan timing.

## Debug Logs

The GUI keeps normal logging quiet by default. To enable additional frontend diagnostic logging:

1. Open Settings.
2. Set **Debug logs** to `on`.
3. Reproduce the issue.
4. Set **Debug logs** back to `off`.

The setting is persisted in `$XDG_CONFIG_HOME/wallpaper-console/config`. Frontend debug messages go to the WebKit developer console. Avoid sharing logs that include private filesystem paths unless that path context is needed for troubleshooting.

## Export Diagnostics

The Settings page includes an "Export diagnostics" button that writes a privacy-safe diagnostic file to `$XDG_CONFIG_HOME/wallpaper-console/diagnostics/`. The file contains app version, OS/arch, config settings (basenames only), library status, source counts, and thumbnail cache info. Full filesystem paths are not included in the file content; only the returned file path contains the full location.

## Wallpaper Engine Scene/Web Backend

Wallpaper Engine `scene` and `web` projects are indexed as project-level library entries. The library path is the project folder, not `preview.gif` and not files under `assets/`. The scanner reads `project.json` case-insensitively, extracts `title`, `type`, `file`, `preview`, and Workshop ID, and stores GUI metadata in SQLite columns:

- `project_type`
- `preview_path`
- `workshop_id`
- `title`
- `we_file`
- `unsupported_reason`

### Scene wallpapers (we_scene)

Scene projects use the optional external `linux-wallpaperengine` command:

```bash
yay -S linux-wallpaperengine-git
```

Not all scenes are compatible; projection-incompatible scenes show a "Scene incompatible" badge and can still use the preview GIF.

### Web wallpapers (we_web)

Web projects are indexed and displayed in the Library with preview GIF thumbnails, but they are not live-apply supported. The previous Chromium/WebKit renderer experiments were removed because they behaved inconsistently on Niri/Wayland and added maintenance cost without reliable wallpaper behavior.

From the Library, Web projects offer:
- **Open folder** — open the Workshop item directory
- **Copy Workshop ID**

Double-clicking or applying a WE Web project returns a structured unsupported error. Use a WE Scene/image/video wallpaper for live apply.

### Residual backend cleanup

If old backend processes survive after stop or apply (e.g. because `setsid` forked and the recorded PID is the parent rather than the actual renderer), you can manually clear all backend processes:

```bash
# In the GUI or CLI, run "Stop" first. If processes are still visible:
pgrep -af 'linux-wallpaperengine'
# If any remain, kill them:
pkill -u "$USER" -f '(^|/)linux-wallpaperengine\b'
```

The `apply` and `stop` commands should normally handle this automatically. The fallback above is only for manual diagnosis when a stale process is suspected.

Both scene and web projects retain the preview GIF fallback. Current wallpaper state records the WE project path, never the preview path unless explicitly applied.

### Apply execution pipeline

The GUI receives `applyActions` from Rust DTOs and sends explicit `ApplyRequestDTO`
objects to Tauri through `apply_action`.

- `apply`: apply the real wallpaper path/project.
- `retry_backend_apply`: clear compatibility failure first, then apply the real project.
- `apply_preview`: apply the project's preview media as a normal image/GIF; the frontend sends
  the project path and the backend resolves the preview path.

The older `apply(path)` command remains for compatibility but should not be used by new GUI actions.

Key execution guarantees:
- Failed backend apply does not write current/history state.
- Unsupported WE Web apply returns a structured error without stopping the current wallpaper.
- Stale apply requests (superseded by a newer request) return `stale_apply_request` error.
- Frontend latest-intent queue prevents overlapping applies; only the most recent intent succeeds.

Rust implementation: `crates/wc-app/src/apply_execution.rs` — `ApplyRequest`, `ApplyRequestKind`, `ApplyExecutionTarget`, `ApplyExecutionResult`.
Tauri command: `apps/tauri-gui/src-tauri/src/commands/wallpaper.rs` — `apply_action`.
Frontend: `apps/tauri-gui/frontend/src/domain/applyRequests.ts` — `buildApplyRequest`.
Tests: `cargo test -p wc-app` (execution+preview), `cargo test -p wc-backend` (state preservation), `cargo test -p wallpaper-console-tauri` (stale guard).
Smoke: `apps/tauri-gui/frontend/e2e/smoke.spec.ts` (preview apply, WE Web double-click guard).

## Tab Persistence

Library, Favorites, and History tabs stay mounted after first visit. Switching between them uses `display: none/flex` CSS toggling instead of unmounting, preserving scroll position, thumbnail state, and loaded data. Thumbnails are shared across all views via a global thumbnail store (concurrency 4). Hidden views skip thumbnail enqueue and reset operations via the `active` prop.

## Pagination

History and Favorites use server-side pagination (120 items per page) via `history_page`/`favorites_page` Rust commands. Both support SQLite and flat-file storage, with automatic fallback to flat-file data when SQLite is not available.

## Performance Baseline

See [PERFORMANCE_BASELINE.md](PERFORMANCE_BASELINE.md) for benchmark methodology, current numbers, and manual GUI profiling notes.

## Continuous Integration

The repository includes `.github/workflows/ci.yml`.

The Rust job runs:

```bash
cargo run -p xtask -- verify rust
```

The frontend job installs npm dependencies and Playwright Chromium, then runs:

```bash
cargo run -p xtask -- verify frontend
```

The workflow does not publish releases. Release publishing remains a manual owner action.

## Manual GUI Smoke

See [TAURI_MANUAL_SMOKE_CHECKLIST.md](TAURI_MANUAL_SMOKE_CHECKLIST.md).

## Architecture

See [TAURI_ARCHITECTURE.md](TAURI_ARCHITECTURE.md).

### Backend apply lifecycle

`wc-backend::lifecycle` owns the pure transition plan for wallpaper switching. Backend modules start/stop real processes, but `wc-backend::apply_wallpaper()` is the only successful apply path that writes current wallpaper, last backend, and history.

When changing apply behavior:

1. Add or update lifecycle tests first.
2. Keep Wallpaper Engine Web unsupported.
3. Do not stop the previous visible backend until the new backend is confirmed unless the same backend requires a pre-stop.
4. Failed applies must preserve current wallpaper state and must not add history.

## Historical Wails

See [archive/WAILS_ARCHITECTURE.md](archive/WAILS_ARCHITECTURE.md).
