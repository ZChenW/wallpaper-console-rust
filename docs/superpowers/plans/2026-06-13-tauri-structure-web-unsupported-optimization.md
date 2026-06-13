# Tauri Structure, WE Web Unsupported, and Optimization Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `code-review-expert` criteria for each review cycle. This plan is executed inline in this session.

**Goal:** make the Tauri-only project easier to maintain, remove the failed WE Web backend direction, and reduce Settings/startup/wallpaper-switch complexity without breaking image/video/scene behavior.

**Architecture:** keep WE scene support on `linux-wallpaperengine`; keep WE Web project indexing for browsing/preview metadata, but route WE Web to `Backend::Unsupported` and remove Web renderer/Chromium backend code. Split Settings metadata from UI and keep frontend/backend contracts explicit.

**Tech Stack:** Rust workspace, Tauri 2, React/TypeScript/Vite, SQLite storage, Playwright smoke tests.

**Execution status:** Completed in this working tree on 2026-06-13.

**Verified commands:**
- `cargo fmt --all -- --check`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `cargo build --workspace`
- `npm run typecheck`
- `npm run test:unit`
- `npm run build`
- `npm run smoke`
- `./scripts/test_tauri_before_commands.sh`
- `./scripts/test_install_build_only.sh`

**Final decision:** WE Web remains indexed as `we_web` for browsing and preview metadata, but all runnable WE Web renderer/browser backends were removed from active code. WE Scene remains on optional `linux-wallpaperengine`.

---

## Phase 0 Baseline

- Current workspace includes core crates (`wc-core`, `wc-storage`, `wc-scan`, `wc-backend`, `wc-app`, `wc-cli`, `wc-preview`), Tauri GUI, docs, scripts, and a temporary `wc-web-renderer` crate from prior WE Web experiments.
- Critical paths:
  - Scan/index: `crates/wc-scan/src/lib.rs`
  - Type/backend mapping: `crates/wc-core/src/types.rs`, `crates/wc-core/src/formats.rs`
  - Apply/restore/process lifecycle: `crates/wc-app/src/lib.rs`, `crates/wc-backend/src/lib.rs`, `crates/wc-backend/src/linux_wallpaperengine.rs`
  - GUI DTOs/commands: `apps/tauri-gui/src-tauri/src/commands/common.rs`, `commands/mod.rs`
  - UI: `App.tsx`, `LibraryView.tsx`, `WallpaperGrid.tsx`, `SettingsView.tsx`
- WE Web coupling points:
  - `Backend::ChromiumWeb`, `Backend::WebKitLayerShell`
  - `wc-backend::web_wallpaper`, `wc-backend::web_renderer`
  - `wc-web-renderer` workspace crate and install/build scripts
  - Tauri commands `web_wallpaper_status`, `web_renderer_status`, `open_web_preview`
  - Settings Web renderer/Chromium sections and config defaults
  - Library context actions for Web apply/preview browser
  - Docs/smoke tests describing Web renderer behavior
- Settings complexity source: config arrays, runtime status loading, DB actions, thumbnail actions, WE debug, Web renderer state, and normal config rows all live in one component.
- Startup bottlenecks: Settings loads many optional backend statuses on mount; Tauri before-commands build Web renderer before app startup; frontend imports Settings eagerly.
- Wallpaper switching bottlenecks: scene handoff and process cleanup are the hot path; Web backend cleanup adds unnecessary work once Web is unsupported.
- Dead/legacy candidates: `wc-web-renderer`, `web_renderer.rs`, `web_wallpaper.rs`, Web configs, Web commands, Chromium preview docs/tests.
- High-risk areas: deleting WE Web apply paths must not delete WE Web indexing or WE scene support; removing backend enum variants requires updating SQLite/CLI parsing for historical rows.

---

## Phase 1 Plan: Structural Refactor

### Problems
- `SettingsView.tsx` owns schema, normalization, status loading, action rendering, and row rendering.
- Frontend WE/Web behavior is scattered between `LibraryView`, `WallpaperGrid`, and Settings.
- Backend command module is large; broad movement is risky while Web removal is pending.

### Target
- Extract Settings config metadata and normalization into `frontend/src/settings/configSchema.ts`.
- Keep Settings UI behavior stable but make future deletion of Web config sections a schema change, not a component rewrite.
- Keep runtime behavior unchanged during Phase 1.

### Changes
- Create `apps/tauri-gui/frontend/src/settings/configSchema.ts`.
- Move `ConfigGroup`, backend/WE/library config arrays, `normalizeConfigValue`, `cleanupDays`, and `clampIntString` there.
- Update `SettingsView.tsx` imports and remove duplicated local definitions.

### Validation
- `npm run typecheck`
- `npm run test:unit`
- `npm run build`

### Phase 1 Plan Review
- Review 1 architecture: split reduces SettingsView reasons to change and does not alter runtime contracts.
- Review 2 maintainability: schema file is reusable and avoids a premature full component rewrite.
- Review 3 risk: no backend/API changes; only import movement and TS compile guard.

---

## Phase 2 Plan: WE Web Unsupported Cleanup

### Current WE Web Code
- `FileType::WeWeb` exists and should stay for indexing/filtering.
- Current backend variants include Web runnable backends (`ChromiumWeb`, `WebKitLayerShell`) that must be removed.
- Current runnable modules: `web_wallpaper.rs`, `web_renderer.rs`, crate `wc-web-renderer`.
- Current UI actions expose Web apply/preview.

### WE Web vs WE Scene
- WE scene: `project.json type=scene`, project-level entry, `linux-wallpaperengine` optional backend.
- WE web: `project.json type=web`, project-level entry for browsing, unsupported for apply in this app.

### Changes
- Remove `Backend::ChromiumWeb` and `Backend::WebKitLayerShell`.
- Make `project.json type=web` produce `FileType::WeWeb`, `Backend::Unsupported`, `unsupported_reason`.
- Remove Web renderer/browser config defaults and Settings sections.
- Remove Tauri Web status/open preview commands and bridge APIs.
- Remove runnable backend modules and workspace member `wc-web-renderer`.
- Keep WE Web library filter, badge, preview thumbnail, project folder, copy workshop ID, and Apply preview GIF.
- Update CLI/SQLite backend parsing for old `chromium-web`/`webkit-layer-shell` rows to `Unsupported`.
- Update docs, install scripts, smoke tests, and before-command script.

### Validation
- `cargo fmt --all -- --check`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- Frontend typecheck/unit/build/smoke
- `scripts/test_tauri_before_commands.sh`
- `scripts/test_install_build_only.sh`

### Phase 2 Plan Review
- Review 1 WE Web cleanup: search all naming variants and remove runnable backend/config/entrypoints.
- Review 2 scene preservation: assert `WeScene -> LinuxWallpaperEngine` remains in scan/core/app/backend.
- Review 3 unsupported risk: WE Web still indexed as one project card and preview fallback remains.

---

## Phase 3 Plan: Remaining Optimization

### Settings
- Keep common controls visible: normal image/gif/video backend, WE scene status, library DB, thumbnails.
- Move low-frequency backend tuning into `<details>` advanced sections.
- Remove Web sections entirely.

### Startup
- Remove Web renderer build from Tauri before commands.
- Remove Web status calls from Settings mount.
- Lazy-load Settings route in `App.tsx` to keep first Library paint lighter.

### Wallpaper Switching
- Remove Web stop/apply paths from `stop_all_backends`.
- Keep scene handoff behavior and process-control tests.
- Preserve apply concurrency guard.

### Validation
- Same full matrix as Phase 2, plus smoke assertions for unsupported WE Web UI.

### Phase 3 Plan Review
- Review 1 UX: default Settings page is shorter and Web no longer promises support.
- Review 2 performance: startup no longer builds/loads Web renderer path; Settings import is lazy.
- Review 3 regression: advanced settings still preserve Linux Wallpaper Engine controls and DB maintenance.

---

## Code Review Cycle Template

Each phase and final review runs the same ten `code-review-expert` scopes:

1. Architecture boundaries
2. Module responsibility
3. Import/dependency graph
4. Type safety
5. Runtime behavior
6. Build/bundling
7. Regression risk
8. Dead code and removal completeness
9. Naming consistency
10. Maintainability, tests, and docs

Findings are fixed immediately before advancing.
