# Tauri Maturity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** mature the now Tauri-only wallpaper-console-rust project from a functional GUI into a verified, maintainable, observable, and production-ready desktop app.

**Architecture:** keep the existing Rust crates and CLI as stable core APIs. Improve the Tauri shell in layers: first prove install/runtime behavior, then split the oversized Tauri command surface, add observability, harden thumbnails/scanning/SQLite UX, expand tests, and finally archive stale migration documentation. Each phase must preserve CLI behavior and `$XDG_CONFIG_HOME/wallpaper-console` data compatibility.

**Tech Stack:** Rust workspace, Tauri 2, React 19, TypeScript, Vite, SQLite/TSV storage, `@tanstack/react-virtual`, Playwright smoke tests, existing `wc-*` crates.

---

## Current Closeout Status (2026-06-11)

This section is the current status source for the maturity work. The original phase checklists below are kept as implementation history and acceptance detail.

| Area | Status | Verification |
|------|--------|--------------|
| Tauri-only install/runtime path | Completed | `./install.sh --build-only`, temp prefix install/uninstall, package build |
| Tauri command module split | Completed | `cargo test -p wallpaper-console-tauri --lib` and workspace verification |
| GUI performance diagnostics | Completed | Unit tests, typecheck, production build, smoke tests |
| Thumbnail hardening | Completed | `wc-preview` failure-cache tests, queue priority tests, cache status UI |
| Scan progress/cancel | Completed | scan state tests, atomic SQLite replace tests |
| SQLite-first GUI status/fallback | Completed | command/frontend wiring and smoke coverage |
| Test matrix expansion | Completed | Rust, frontend, smoke, install, storage, thumbnail, scan tests |
| Documentation slimming/archive cleanup | Completed | README + DEVELOPMENT are Tauri-current; Wails docs are historical only |
| Performance baseline | Completed | 1k/10k/50k TSV vs SQLite benchmark recorded in `docs/PERFORMANCE_BASELINE.md` |
| CI automation | Workflow added | `.github/workflows/ci.yml` runs Rust fmt/check/clippy/tests and frontend typecheck/unit/build/smoke on push, PR, and manual dispatch; first GitHub-hosted run remains to be observed after push |
| Real desktop GUI visual acceptance | Scripted evidence path available; acceptance still open until run | `scripts/manual_tauri_acceptance.sh` captures profile CSV, compositor window listing when available, and screenshot path; complete `docs/TAURI_MANUAL_SMOKE_CHECKLIST.md` before marking accepted |
| Persistent tab shell | Completed | Library/History/Favorites stay mounted across switches; scroll preserved; `active` prop gates thumbnail enqueue/resize; colCount re-measured on activation; smoke test for tab persistence |
| History/Favorites pagination | Completed | `history_page`/`favorites_page` Rust commands with SQLite/flat fallback (empty-SQLite-table fallback too); "Load more" button; 2 Rust fallback tests (`history_page_sqlite_empty_falls_back_to_flat`, `favorites_page_sqlite_empty_falls_back_to_flat`) |
| Global thumbnail store | Completed | `ThumbnailStoreContext` shared queue (concurrency 4); `forget()` API for invalidation; `invalidate()` clears both React state and queue-internal cache before re-enqueue; dedup + forget tests |
| Export diagnostics | Completed | `export_diagnostics` Rust command writes privacy-safe diagnostic file; Settings button; `path_basename` strips directories; 3 Rust tests; smoke test for button visibility |
| SQLite-only GUI + WE one-click ingest | Completed | Default `storage_backend=sqlite`; `gui_library_source` removed; Scan Wallpaper Engine discovers projects AND indexes wallpapers in one action; Library always reads SQLite (auto-creates schema); Settings simplified (no TSV/SQLite selector, advanced DB folded); `prior_metadata_cache_from_sqlite` replaces TSV cache; CLI parity tests pinned to flat-file mode |
| Wallpaper Engine scene/Web model | Completed | Scene (`we_scene`): optional external `linux-wallpaperengine` with compat cache for incompatibility detection. Web (`we_web`): indexed for browsing/preview metadata only and intentionally routed to `unsupported`; previous Web renderer/Chromium backend experiments are removed from the active code path. |
| Settings/startup cleanup | Completed | Settings schema extracted from the view, low-frequency controls folded under advanced sections, and Settings is lazy-loaded so the Library first paint does not pull the Settings chunk. |
| Frontend apply action model consolidation | Completed | domain/applyActions.ts normalizes backend applyActions DTO; LibraryView builds context menu from normalized actions; WallpaperGrid uses isApplyAvailable(); legacy DTO fallback centralized; 28 unit tests; smoke tests pass |
| Apply execution pipeline | Completed | `apply_action` Tauri command, `ApplyRequest`/`ApplyExecutionResult` Rust types, `buildApplyRequest` frontend, latest-intent frontend queue, stale request guard (AtomicU64), preview resolves from project metadata, state/history write boundaries, 48 smoke tests, Rust: 15 wc-app + 32 wallpaper-console-tauri + 22 wc-backend tests |
| Backend apply lifecycle | Completed | Backend apply lifecycle is centralized in `wc-backend::lifecycle`; `apply_wallpaper()` is the single state/history write boundary after successful backend confirmation. |
| SQL-level GUI pagination | Completed | `library_page_gui`, `favorites_page`, and `history_page` use shared SQLite `COUNT` + `LIMIT/OFFSET` helpers; storage and Tauri tests pass |
| Frontend apply queue extraction | Completed | `useApplyQueue` isolates latest-intent queueing from `App.tsx`; `useFeedbackBridge` extracts wc-feedback listener; 34 unit tests pass |
| Thumbnail stale completion hardening | Completed | Generation tokens and path-version invalidation prevent stale writes after `reset()`/`forget()`/`dispose()`; 2 race tests added; `forget()` exposed through hooks and store |
| Streaming scan progress | Completed | `scan_wallpapers_with_callback` reports `SourceStarted`/`CandidateFound` during directory walk; cancel integration at walking stage; SQLite prior cache reused in metadata phase |
| Legacy Tauri list commands removed | Completed | `library_list`/`favorites_list`/`history_list` removed from Tauri commands and frontend bridge; all views use paged APIs only |
| wc-storage SQLite module split | Completed | 2297-line `sqlite.rs` split into 6 submodules (schema, library_page, backup, source_config_state, metadata_cache, row_map); public API preserved via `pub use` re-exports; 31 tests pass |
| Frontend grid and thumbnail hot-path | Completed | WallpaperGrid: removed O(n) `rows` precompute, O(1) `entryByPath` Map for context menu lookup; thumbnail queue: `snapshot()` includes cached count; 39 unit + 70 smoke tests pass |
| Backend runtime test seam | Completed | `BackendRuntime` trait with `SystemBackendRuntime` (production) and `FakeRuntime` (tests); `apply_wallpaper_with_runtime` accepts injected runtime for stops AND command execution (awww/mpvpaper); `execute_stop_plan_with_runtime` dispatches all `StopPlan` variants; LWE PID cleanup preserved; LWE apply path remains direct (complex scene projector); 87 backend tests pass |

Important verification boundary: automated build/test/smoke/package checks were run locally. Manual GUI acceptance remains open until an interactive desktop session can complete `docs/TAURI_MANUAL_SMOKE_CHECKLIST.md`.
Manual desktop acceptance remains open; automated smoke tests do not prove compositor/runtime behavior on niri.

---

## Global Rules

- Do not reintroduce Wails, Go, Wails bindings, or `apps/wails-gui`.
- Do not change Rust CLI command names or existing output shapes unless a task explicitly says so.
- Do not delete TSV support. SQLite can be preferred, but TSV must keep working.
- Do not apply wallpapers during automated tests unless the test uses a mocked backend.
- Keep each phase independently buildable.
- If a phase cannot be completed, document the exact blocker in the final report and keep prior phases passing.

## Final Verification Matrix

Run this after every major phase unless the phase only edits docs:

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo build --workspace

cd apps/tauri-gui/frontend
npm run test:unit
npm run typecheck
npm run build
npm run smoke

cd ../src-tauri
cargo tauri build --bundles deb,rpm
```

If `npm run smoke` fails with `listen EPERM`, rerun in an environment that permits Playwright's local Vite web server.

---

## Phase 1: Tauri Install And Real Runtime Closure

**Purpose:** prove the Tauri-only app can be built, installed, launched, and packaged from documented commands.

**Files:**
- Modify: `install.sh`
- Modify: `README.md`
- Modify: `docs/TAURI_ARCHITECTURE.md`
- Create: `docs/TAURI_MANUAL_SMOKE_CHECKLIST.md`
- Optional modify: `apps/tauri-gui/src-tauri/tauri.conf.json`

**Tasks:**

- [ ] Run install build-only from repo root:
  ```bash
  ./install.sh --build-only
  ```
  Expected:
  - builds `target/release/wallpaper-console-rust`
  - builds `target/release/wallpaper-console-tauri`
  - does not require Go or Wails

- [ ] Run install into a temporary prefix:
  ```bash
  tmp_prefix="$(mktemp -d)"
  ./install.sh --prefix "$tmp_prefix"
  "$tmp_prefix/bin/wallpaper-console-rust" status
  test -x "$tmp_prefix/bin/wallpaper-console-gui-rust"
  ```
  Expected:
  - CLI runs.
  - GUI binary is installed as `wallpaper-console-gui-rust`.
  - original Bash/Python command names are not touched.

- [ ] Verify package artifacts:
  ```bash
  cd apps/tauri-gui/src-tauri
  cargo tauri build --bundles deb,rpm
  ls -l ../../../target/release/bundle/deb/*.deb
  ls -l ../../../target/release/bundle/rpm/*.rpm
  ```

- [ ] Add `docs/TAURI_MANUAL_SMOKE_CHECKLIST.md` with exact manual checks:
  ```markdown
  # Tauri Manual Smoke Checklist

  Run after `./install.sh` or after installing a `.deb` / `.rpm` bundle.

  - [ ] Launch `wallpaper-console-gui-rust` from a terminal.
  - [ ] Confirm Library renders without a blank view.
  - [ ] Switch Library / Favorites / History / Sources / Settings.
  - [ ] Run Rescan and confirm the status bar reports completion.
  - [ ] Open Sources and run Scan Wallpaper Engine.
  - [ ] Open Settings and run SQLite Verify.
  - [ ] Open Settings and run Thumbnail Cache Status / Clear.
  - [ ] Right-click a wallpaper card and confirm context menu placement.
  - [ ] Apply a known safe image wallpaper.
  - [ ] Run Stop and Restore.
  - [ ] On niri, confirm app-id/window rules still match the Tauri app.
  - [ ] Note WebKitGTK 4.1 rendering or animation issues.
  ```

- [ ] Update README to link the manual checklist near Build & Verify.

**Acceptance:**
- `./install.sh --build-only` works.
- temp-prefix install works.
- README points to real manual GUI checks.
- No command in this phase mentions Wails as a current dependency.

---

## Phase 2: Split Tauri Backend Commands By Responsibility

**Purpose:** make the Tauri backend maintainable by splitting the large `commands.rs` into focused modules without changing frontend API names.

**Files:**
- Modify: `apps/tauri-gui/src-tauri/src/commands.rs`
- Create: `apps/tauri-gui/src-tauri/src/commands/mod.rs`
- Create: `apps/tauri-gui/src-tauri/src/commands/common.rs`
- Create: `apps/tauri-gui/src-tauri/src/commands/wallpaper.rs`
- Create: `apps/tauri-gui/src-tauri/src/commands/library.rs`
- Create: `apps/tauri-gui/src-tauri/src/commands/scan.rs`
- Create: `apps/tauri-gui/src-tauri/src/commands/sources.rs`
- Create: `apps/tauri-gui/src-tauri/src/commands/settings.rs`
- Create: `apps/tauri-gui/src-tauri/src/commands/database.rs`
- Create: `apps/tauri-gui/src-tauri/src/commands/thumbnails.rs`
- Create: `apps/tauri-gui/src-tauri/src/commands/files.rs`
- Modify: `apps/tauri-gui/src-tauri/src/lib.rs`

**Target module boundaries:**

- `common.rs`: DTOs, `ok`, `fail`, `storage`, `format_bytes`, source labels, entry-to-DTO hydration.
- `wallpaper.rs`: `status`, `apply`, `stop`, `restore`, backend selection.
- `library.rs`: `library_count`, `library_list`, `library_page`, SQLite read/filter/page helpers, hydration helpers.
- `scan.rs`: `rescan`, `scan_progress`, `scan_cancel`, scan state guard, shared SQLite indexing path.
- `sources.rs`: `sources_list`, `source_add`, `source_remove`, `validate_sources`, `remove_missing_sources`, `scan_steam_workshop`.
- `settings.rs`: `config_get`, `config_set`, diagnostics export.
- `database.rs`: SQLite migrate/verify/resync/backup/restore/export.
- `thumbnails.rs`: `thumbnail_for`, `thumbnail_cache_status`, `thumbnail_cache_clear`.
- `files.rs`: `open_path`, `reveal_in_file_manager`, `browse_directory`, external command runner.

**Tasks:**

- [ ] First run:
  ```bash
  cargo test -p wallpaper-console-tauri
  ```
  Expected: current Tauri crate tests pass.

- [ ] Move shared structs and helpers into `commands/common.rs`.

- [ ] Move commands in small batches. After each module move, run:
  ```bash
  cargo check -p wallpaper-console-tauri
  ```

- [ ] Re-export all command functions from `commands/mod.rs`:
  ```rust
  pub mod common;
  mod files;
  mod library;
  mod settings;
  mod sources;
  mod thumbnails;
  mod wallpaper;

  pub use files::*;
  pub use library::*;
  pub use settings::*;
  pub use sources::*;
  pub use thumbnails::*;
  pub use wallpaper::*;
  ```

- [ ] Keep `lib.rs` invoke handler command names unchanged:
  ```rust
  .invoke_handler(tauri::generate_handler![
      commands::status,
      commands::apply,
      commands::stop,
      commands::restore,
      commands::config_get,
      commands::config_set,
      commands::sources_list,
      commands::source_add,
      commands::source_remove,
      commands::validate_sources,
      commands::remove_missing_sources,
      commands::scan_steam_workshop,
      commands::favorites_list,
      commands::favorite_add,
      commands::favorite_remove,
      commands::history_list,
      commands::history_clear,
      commands::library_count,
      commands::library_list,
      commands::library_page,
      commands::rescan,
      commands::migrate_to_sqlite,
      commands::sqlite_verify,
      commands::sqlite_resync,
      commands::sqlite_backup,
      commands::sqlite_restore,
      commands::sqlite_export_flat,
      commands::thumbnail_for,
      commands::thumbnail_cache_status,
      commands::thumbnail_cache_clear,
      commands::open_path,
      commands::reveal_in_file_manager,
      commands::browse_directory,
  ])
  ```

- [ ] Preserve existing Tauri tests. Move tests to the module that owns the helper being tested.

**Acceptance:**
- `commands.rs` no longer contains all command implementation logic.
- Frontend bridge command names remain unchanged.
- `cargo test -p wallpaper-console-tauri` passes.
- Full verification matrix passes.

---

## Phase 3: Add GUI Performance Metrics And Developer Diagnostics

**Purpose:** make future performance work measurable instead of relying on subjective “feels slow”.

**Files:**
- Create: `apps/tauri-gui/frontend/src/perf/metrics.ts`
- Create: `apps/tauri-gui/frontend/src/components/PerformanceOverlay.tsx`
- Modify: `apps/tauri-gui/frontend/src/App.tsx`
- Modify: `apps/tauri-gui/frontend/src/hooks/useThumbnailQueue.ts`
- Modify: `apps/tauri-gui/frontend/src/hooks/thumbnailQueueCore.ts`
- Modify: `apps/tauri-gui/frontend/src/views/LibraryView.tsx`
- Modify: `apps/tauri-gui/frontend/src/api/bridge.ts`
- Optional modify: `apps/tauri-gui/src-tauri/src/commands/library.rs` after Phase 2, or `commands.rs` before Phase 2.

**Metrics to collect:**

- `view.switch.ms`
- `library.page.ms`
- `library.page.total`
- `thumbnail.queue.pending`
- `thumbnail.queue.inFlight`
- `thumbnail.cache.hit`
- `thumbnail.cache.miss`
- `rescan.ms`

**Tasks:**

- [ ] Add a tiny in-memory metrics store:
  ```ts
  export type MetricSample = {
    name: string;
    value: number;
    ts: number;
    tags?: Record<string, string | number | boolean>;
  };

  const samples: MetricSample[] = [];
  const MAX_SAMPLES = 300;

  export function recordMetric(name: string, value: number, tags?: MetricSample['tags']) {
    samples.push({ name, value, ts: Date.now(), tags });
    if (samples.length > MAX_SAMPLES) samples.splice(0, samples.length - MAX_SAMPLES);
  }

  export function getMetricsSnapshot() {
    return samples.slice();
  }

  export function measureAsync<T>(name: string, fn: () => Promise<T>, tags?: MetricSample['tags']) {
    const start = performance.now();
    return fn().finally(() => recordMetric(name, performance.now() - start, tags));
  }
  ```

- [ ] Wrap `api.libraryPage`, `api.rescan`, and `api.thumbnailFor` calls with metrics at the call site, not inside the bridge if that would obscure context.

- [ ] Add `PerformanceOverlay` gated by either:
  - `localStorage.setItem('wcPerfOverlay', '1')`, or
  - `?perf=1` query parameter.

- [ ] Overlay should display recent values only:
  - latest library page ms
  - latest rescan ms
  - thumbnail cache hit/miss counts
  - queue pending/in-flight

- [ ] Add unit tests for `metrics.ts` using Node's built-in test runner.

**Acceptance:**
- Overlay is hidden by default.
- Smoke tests still pass.
- Metrics add no backend dependency and no production network calls.
- A developer can enable the overlay without rebuilding.

---

## Phase 4: Thumbnail Pipeline Hardening

**Purpose:** improve thumbnail reliability and long-term cache behavior now that Tauri calls Rust directly.

**Files:**
- Modify: `crates/wc-preview/src/lib.rs`
- Modify: `apps/tauri-gui/src-tauri/src/commands/thumbnails.rs` or current `commands.rs`
- Modify: `apps/tauri-gui/frontend/src/hooks/thumbnailQueueCore.ts`
- Modify: `apps/tauri-gui/frontend/src/hooks/thumbnailQueueCore.test.ts`
- Modify: `apps/tauri-gui/frontend/src/components/WallpaperGrid.tsx`
- Modify: `apps/tauri-gui/frontend/src/views/SettingsView.tsx`

**Tasks:**

- [ ] Add typed thumbnail failure reasons in Rust without breaking existing DTO shape:
  - `unsupported`
  - `timeout`
  - `probe_failed`
  - `cache_write_failed`
  - `missing_file`

- [ ] Preserve frontend fallback to type icons when thumbnail generation fails.

- [ ] Add cache metadata helpers:
  - count entries
  - total bytes
  - oldest entry
  - newest entry
  - failed generation count if stored

- [ ] Add cache cleanup command option:
  - clear all
  - clear entries older than N days
  - optional max size target, implemented conservatively

- [ ] Add tests in `wc-preview` for deterministic cache key stability and non-existent file behavior.

- [ ] Add frontend queue tests:
  - failed thumbnail does not retry forever
  - stale result does not overwrite newer request
  - visible paths are prioritized

**Acceptance:**
- Existing thumbnails still load.
- Failed thumbnails show stable fallback icons.
- Thumbnail cache status is more informative in Settings.
- No repeated infinite retry loop on bad files.

---

## Phase 5: Scan Task UX, Progress, And Cancellation

**Purpose:** turn long scans from opaque “wait for result” operations into visible, cancellable work.

**Files:**
- Modify: `crates/wc-scan/src/lib.rs`
- Modify: `apps/tauri-gui/src-tauri/src/commands/library.rs` or current `commands.rs`
- Modify: `apps/tauri-gui/frontend/src/api/bridge.ts`
- Modify: `apps/tauri-gui/frontend/src/components/Toolbar.tsx`
- Modify: `apps/tauri-gui/frontend/src/views/SourcesView.tsx`
- Modify: `apps/tauri-gui/frontend/src/components/StatusBar.tsx`
- Optional create: `apps/tauri-gui/frontend/src/hooks/useScanTask.ts`

**Tasks:**

- [ ] Start with a conservative model: one scan at a time. If a scan is running, new scan requests return a clear error.

- [ ] Add a Tauri state object for scan status:
  ```rust
  #[derive(Clone, Serialize)]
  pub struct ScanProgressDto {
      pub running: bool,
      pub scanned: usize,
      pub total_hint: Option<usize>,
      pub reused_metadata: usize,
      pub probed_metadata: usize,
      pub inserted_sqlite: usize,
      pub current_path: Option<String>,
      pub cancel_requested: bool,
      pub error: Option<String>,
  }
  ```

- [ ] Add commands:
  - `scan_progress() -> ScanProgressDto`
  - `scan_cancel() -> CommandResult`

- [ ] Update `rescan()` to update progress periodically while scanning. If full progress requires intrusive `wc-scan` changes, first implement stage-level progress:
  - loading sources
  - walking files
  - reading prior metadata
  - probing metadata
  - writing SQLite
  - writing TSV

- [ ] Add frontend polling while scan is running.

- [ ] Add cancel button state in Toolbar or StatusBar.

- [ ] Add tests for progress state transitions at the Rust helper level where possible.

**Acceptance:**
- User sees scan running status.
- User can cancel before or during expensive stages.
- Cancel does not corrupt TSV or SQLite.
- Rescan still returns a useful summary.

---

## Phase 6: SQLite-First GUI Experience

**Purpose:** make SQLite the recommended large-library path while keeping TSV compatibility explicit.

**Files:**
- Modify: `apps/tauri-gui/src-tauri/src/commands/library.rs` or current `commands.rs`
- Modify: `apps/tauri-gui/frontend/src/views/LibraryView.tsx`
- Modify: `apps/tauri-gui/frontend/src/views/SettingsView.tsx`
- Modify: `apps/tauri-gui/frontend/src/components/StatusBar.tsx`
- Modify: `crates/wc-storage/src/sqlite.rs`
- Modify: `docs/TAURI_ARCHITECTURE.md`
- Modify: `README.md`

**Tasks:**

- [x] Define an explicit library source status DTO:
  ```ts
  export interface LibrarySourceStatusDTO {
    configured: string;
    effective: string;
    sqliteReady: boolean;
    sqliteRows: number;
    tsvRows: number;
    stale: boolean;
    message: string;
  }
  ```

- [x] Add Tauri command `library_source_status`.

- [x] In GUI, show effective source in Library header/status bar:
  - SQLite

- [x] Remove `gui_library_source` from active GUI behavior. The GUI is SQLite-only; TSV remains a best-effort legacy export for CLI compatibility and compatibility commands.

- [x] Add Settings actions:
  - “Rebuild Database”
  - “Verify Database”
  - backup/restore/export actions under advanced database controls

- [x] Add tests for source status, SQLite paging, and legacy fallback compatibility commands.

**Acceptance:**
- Users understand which library source they are viewing.
- Empty SQLite no longer looks like data loss.
- SQLite is clearly recommended for large libraries.
- TSV remains selectable and tested.

---

## Phase 7: Expand Automated Test Matrix

**Purpose:** catch regressions in install, storage parity, Tauri command helpers, and GUI smoke behavior.

**Files:**
- Create: `tests/install_build_only.sh` or `scripts/test_install_build_only.sh`
- Modify: `crates/wc-storage/src/tsv.rs`
- Modify: `crates/wc-storage/src/sqlite.rs`
- Modify: `apps/tauri-gui/src-tauri/src/commands/library.rs` or current command tests
- Modify: `apps/tauri-gui/frontend/e2e/smoke.spec.ts`
- Modify: `apps/tauri-gui/frontend/package.json`
- Optional create: `.github/workflows/ci.yml` if this repo uses GitHub Actions.

**Tasks:**

- [ ] Add install build-only test:
  ```bash
  ./install.sh --build-only
  test -x target/release/wallpaper-console-rust
  test -x target/release/wallpaper-console-tauri
  ```

- [ ] Add storage parity tests:
  - same fixture rows produce same first page in TSV and SQLite
  - `newest`, `largest`, `name` ordering match
  - search/filter match
  - empty SQLite fallback behavior if Phase 6 is implemented

- [ ] Add Tauri command tests for helper functions after Phase 2 split:
  - source label
  - library source status
  - thumbnail cache status formatting
  - scan progress state transitions

- [ ] Expand smoke tests:
  - Settings page can open SQLite controls
  - Sources page shows Wallpaper Engine group
  - Performance overlay hidden by default
  - Library source badge visible after Phase 6

- [ ] If CI exists, add jobs:
  - Rust fmt/test/clippy/build
  - Tauri frontend typecheck/build/smoke
  - optional Tauri bundle build on Linux image with WebKitGTK 4.1

**Acceptance:**
- Test matrix catches the project-specific failure modes found during migration.
- Install path is tested.
- Smoke tests remain backend-safe.

---

## Phase 8: Documentation Slimming And Historical Archive Cleanup

**Purpose:** make current docs easy to follow and move old migration plans out of the active path.

**Files:**
- Modify: `README.md`
- Modify: `docs/TAURI_ARCHITECTURE.md`
- Create: `docs/DEVELOPMENT.md`
- Create: `docs/HISTORICAL_WAILS_ARCHIVE.md`
- Modify or delete/archive: `docs/WAILS_ARCHITECTURE.md`
- Modify or archive: `docs/MIGRATION_COMPLETION_AND_PERFORMANCE_PLAN.md`
- Modify or archive: `docs/OPENCODE_TAURI_ONLY_MIGRATION_PLAN.md`
- Modify or archive: `docs/OPENCODE_REMAINING_OPTIMIZATION_PLAN.md`
- Modify: `docs/PERFORMANCE_BASELINE.md`

**Tasks:**

- [ ] Make README user-focused:
  - quick install
  - launch CLI/GUI
  - basic commands
  - build/verify summary
  - link to `docs/DEVELOPMENT.md`

- [ ] Create `docs/DEVELOPMENT.md` with:
  - prerequisites
  - Rust verification
  - Tauri frontend verification
  - Tauri bundle build
  - Playwright smoke notes
  - install test

- [ ] Keep `docs/TAURI_ARCHITECTURE.md` as the current architecture source of truth.

- [ ] Create `docs/HISTORICAL_WAILS_ARCHIVE.md` that summarizes:
  - why Wails existed
  - why it was retired
  - where archived docs live
  - do not execute Wails commands

- [ ] Add a top banner to obsolete plans:
  ```markdown
  > ARCHIVED: This plan has been completed or superseded. Do not execute it as current guidance.
  ```

- [ ] Run doc grep:
  ```bash
  rg -n "Wails|wails|wails3|apps/wails-gui|Go bridge|generated bindings" README.md docs
  ```
  Expected:
  - hits only in files that are explicitly archived or historical.

**Acceptance:**
- A new contributor can use README + DEVELOPMENT without reading migration history.
- Historical Wails docs are clearly separated.
- Current docs contain no Wails build/install instructions.

---

## Recommended Execution Order

1. Phase 1: Tauri install/runtime closure.
2. Phase 2: command module split.
3. Phase 7: test matrix base additions for install and parity.
4. Phase 3: performance metrics.
5. Phase 4: thumbnail hardening.
6. Phase 5: scan progress/cancel.
7. Phase 6: SQLite-first UX.
8. Phase 8: documentation slimming.

Reason: prove the app installs first, then make backend code easier to change, then add tests before deeper UX/behavior work.

## Required Final Report

OpenCode should report:

- Which phases were completed.
- Exact files created/modified/deleted.
- Verification commands and results.
- Manual GUI checks completed or skipped.
- Any remaining Wails references and whether each is historical.
- Any performance numbers gathered from the new metrics overlay.
- Known residual risks.
