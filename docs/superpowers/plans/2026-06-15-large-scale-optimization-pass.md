# Large Scale Optimization Pass Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** complete one high-quality optimization pass across architecture, performance, reliability, maintainability, and verification for the Tauri/Rust wallpaper-console project.

**Architecture:** keep Rust crates as the runtime source of truth, keep Tauri commands as adapters, and preserve CLI command names and output shapes. Optimize along existing boundaries: `wc-core` owns config parsing/defaults, `wc-storage` owns SQLite/flat persistence, `wc-scan` owns discovery and metadata reuse, `wc-backend` owns process lifecycle, `wc-app` owns apply decisions, and the React frontend owns view state and feedback presentation.

**Tech Stack:** Rust workspace, rusqlite, Tauri 2, React 19, TypeScript, Vite, Playwright, shell scripts, GitHub Actions.

---

## Phase 1 Project Understanding

### What This Project Is

`wallpaper-console-rust` is a Rust rewrite of a Linux/Wayland wallpaper manager. It ships:

- `wallpaper-console-rust`: Rust CLI with source management, scanning, SQLite/flat storage, favorites/history, fzf browsing, and backend control.
- `wallpaper-console-gui-rust`: Tauri 2 GUI with React, virtualized library grid, SQLite-backed paging/search, thumbnails, settings, source management, scan progress, and apply/stop/restore workflows.

### Main Technical Stack

- Rust workspace crates:
  - `wc-core`: config paths, defaults, core types, error types.
  - `wc-storage`: flat files, SQLite schema, migration, verification, backup/restore, paging, metadata cache.
  - `wc-scan`: source dedupe, Wallpaper Engine parsing, streaming scan, metadata probing/reuse.
  - `wc-backend`: awww/mpvpaper/linux-wallpaperengine process lifecycle and visual handoff.
  - `wc-app`: apply target resolution and user-facing apply errors.
  - `wc-preview`: terminal previews and GUI thumbnail cache.
  - `wc-cli`: command-line interface.
  - `wallpaper-console-tauri`: Tauri command adapter.
- Frontend:
  - React 19 + TypeScript + Vite.
  - `@tanstack/react-virtual` for grid virtualization.
  - Playwright smoke tests using a mock bridge.
- Verification:
  - `.github/workflows/ci.yml`
  - Rust fmt/check/clippy/tests.
  - Frontend typecheck/unit/build/smoke.
  - manual desktop acceptance scripts for real Tauri/niri behavior.

### Current Visible Risks

- `wc-core::config::write_config_value` rewrites config from `HashMap`, producing nondeterministic key order and noisy diffs; config defaults are not represented by a stable registry.
- SQLite migration still escapes values before passing them as bound parameters in several places. This stores apostrophes incorrectly and makes persistence behavior harder to reason about.
- SQLite FTS rebuild is guarded by a timestamp-only `fts_rebuilt_at`; future FTS schema/content changes will not force rebuild.
- CLI `rescan` still accumulates all paths and all entries before writing, while Tauri scan is already streaming.
- SQLite scan staging inserts each row outside a push-level transaction, making large scans slower than necessary.
- CLI `stop` stops processes but does not clear runtime state, while Tauri `stop` does.
- Frontend `LibraryView`, `FavoritesView`, and `HistoryView` duplicate paged loading, request sequencing, append/reset handling, and stale response protection.
- Frontend feedback and invalidation events are emitted through raw `window.dispatchEvent(new CustomEvent(...))` call sites, making event names and payload shapes easy to drift.
- `xtask` is currently a placeholder, so local verification commands can drift from CI and docs.
- Docs are accurate for previous rounds, but they do not yet describe the new single-command verification entrypoint and this pass's risk boundaries.

### Runnable Verification Commands

```bash
cargo fmt --all -- --check
cargo check --workspace
cargo clippy --workspace -- -D warnings
cargo test --workspace

cd apps/tauri-gui/frontend
npm run typecheck
npm run test:unit
npm run build
npm run smoke

git diff --check
bash -n scripts/manual_tauri_acceptance.sh scripts/profile_gui.sh scripts/test_install_build_only.sh
```

---

## Phase 2 Ten Optimization Directions

### 1. Stable Config Registry And Deterministic Writes

**Current problem:** config defaults are built from `HashMap`, and `write_config_value` serializes `HashMap` iteration order. Repeated `config-set` calls can reorder the config file and create noisy diffs.

**Impact range:** `wc-core::config`, `StorageApi::config_set`, CLI/GUI settings writes, tests.

**Why worth doing:** config files are user-visible and frequently edited by GUI/CLI. Stable order reduces surprises and makes backups/diagnostics easier to compare.

**Expected benefit:** deterministic config output, central default key list, easier future validation.

**Complexity:** medium.

**Risk:** changing ordering can affect tests that compare raw config text; no public command shape changes.

**Verification:** add unit tests for stable default order and deterministic rewrites; run `cargo test -p wc-core`.

### 2. Remove Incorrect SQLite Escaping Before Bound Parameters

**Current problem:** migration uses `sqlite_escape()` and then passes escaped strings through `params![]`. Values containing `'` are stored as doubled apostrophes, not the original text. `StorageApi::_sqlite_config_get` also uses formatted SQL.

**Impact range:** `wc-storage::sqlite::schema`, `StorageApi`, migration/export/verify behavior.

**Why worth doing:** this is real data integrity and SQL-boundary cleanup.

**Expected benefit:** apostrophes and special characters round-trip correctly; fewer ad-hoc SQL strings.

**Complexity:** medium.

**Risk:** migrations created by old code may already contain doubled apostrophes; this pass fixes new writes, not historical repair.

**Verification:** tests for config/source/current/history values containing apostrophes; run `cargo test -p wc-storage`.

### 3. Versioned SQLite FTS Rebuild And Verify Coverage

**Current problem:** FTS rebuild runs only once based on `fts_rebuilt_at`, not a schema/content version. If FTS columns or triggers change, stale databases may skip rebuild.

**Impact range:** `wc-storage::sqlite::schema`, `backup::verify`, SQLite search.

**Why worth doing:** FTS search powers GUI/CLI search paging and can silently return stale results.

**Expected benefit:** deterministic FTS rebuild when expected version changes; verify catches FTS count drift.

**Complexity:** medium.

**Risk:** first startup after version bump may rebuild FTS, adding one-time cost proportional to library size.

**Verification:** tests for old meta value triggering rebuild and verify failing/warning on broken FTS; run `cargo test -p wc-storage sqlite`.

### 4. Batch SQLite Scan Staging Writes In Transactions

**Current problem:** `library_replace_session_push()` inserts every staged row with autocommit behavior. The final replace is transactional, but staging batches are not.

**Impact range:** `wc-storage::sqlite::library_session`, Tauri scan indexing, future CLI streaming rescan.

**Why worth doing:** large scans should spend less time on SQLite commit overhead.

**Expected benefit:** faster scans and tighter atomicity around each pushed batch.

**Complexity:** low-medium.

**Risk:** borrow/transaction lifetime mistakes; existing tests cover commit/abort semantics.

**Verification:** add a test that a duplicate/staging error rolls back the current push batch; run `cargo test -p wc-storage library_replace_session`.

### 5. Stream CLI Rescan Instead Of Collecting Full Path And Entry Vectors

**Current problem:** CLI `rescan` uses `scan_wallpapers()` to collect every path, then builds a full `Vec<WallpaperEntry>`, then writes TSV and SQLite. Tauri scan already streams.

**Impact range:** `wc-cli`, `wc-scan`, `wc-storage::sqlite::library_session`.

**Why worth doing:** CLI rescan is a core workflow and should scale like GUI rescan.

**Expected benefit:** lower peak memory on large libraries; one streaming implementation style across CLI/GUI.

**Complexity:** high.

**Risk:** CLI output timing/counts can change; TSV must remain atomically written and compatible.

**Verification:** parity tests for rescan outputs, TSV content, SQLite content, cancel-free streaming behavior; run `cargo test -p wc-cli --test parity_tests`.

### 6. Make CLI Stop Clear Runtime State Like GUI Stop

**Current problem:** Tauri `stop()` clears `current` and `last_backend`; CLI `stop` only calls `stop_all_backends`. After CLI stop, `status` can report a stale current wallpaper.

**Impact range:** `wc-cli`, `wc-storage`, backend stop semantics.

**Why worth doing:** removes CLI/GUI state inconsistency and prevents stale restore/apply hints.

**Expected benefit:** accurate status after stop; less stale backend handoff risk.

**Complexity:** low.

**Risk:** users who expect `stop` to preserve current may notice state clears; history remains intact so restore history is not deleted.

**Verification:** CLI parity test in sqlite mode and file mode; run targeted parity tests.

### 7. Extract Shared Frontend Paged Wallpaper Hook

**Current problem:** `LibraryView`, `FavoritesView`, and `HistoryView` each implement their own `entries`, `total`, `loading`, request sequence, append/load-more, and stale response logic.

**Impact range:** React views, unit tests, smoke tests.

**Why worth doing:** pagination is now the core GUI data path; duplicated async state machines are regression-prone.

**Expected benefit:** one tested stale-response and append implementation; smaller views.

**Complexity:** high.

**Risk:** view-specific invalidation and search/filter resets must remain correct.

**Verification:** unit tests for initial load, append, stale response suppression, reset-on-query-change; smoke tests for Library/Favorites/History.

### 8. Centralize Frontend Event Bus For Feedback And Cache Invalidation

**Current problem:** raw `window.dispatchEvent(new CustomEvent(...))` calls exist in multiple views/hooks/components with stringly event names and payloads.

**Impact range:** frontend views/hooks/components, feedback bridge.

**Why worth doing:** the app relies on events for toasts, cache invalidation, and settings changes; typed helpers reduce payload drift.

**Expected benefit:** consistent event names/payloads, easier tests and future refactors.

**Complexity:** medium.

**Risk:** missing one replacement can leave mixed style; event names must stay compatible.

**Verification:** unit tests for event helpers; smoke tests for feedback toasts and favorites/history invalidation.

### 9. Replace Placeholder `xtask` With Verification Runner

**Current problem:** `xtask` only prints a placeholder. CI and docs list many commands manually.

**Impact range:** `xtask`, CI workflow, docs.

**Why worth doing:** a single local command reduces verification drift and makes future agents less likely to skip a step.

**Expected benefit:** `cargo run -p xtask -- verify rust|frontend|all` can execute the same matrix CI/docs use.

**Complexity:** medium.

**Risk:** frontend smoke may fail in constrained environments; runner must report command and exit code clearly.

**Verification:** unit-light manual command tests: `cargo run -p xtask -- verify rust --dry-run`, `cargo run -p xtask -- verify frontend --dry-run`; use actual commands in final verification.

### 10. Update Operational Docs And Status For The New Boundaries

**Current problem:** docs mention the broad verification matrix but not the new `xtask` entrypoint, deterministic config behavior, streaming CLI rescan, FTS versioning, or CLI stop state semantics.

**Impact range:** README, DEVELOPMENT, CURRENT_STATUS, PERFORMANCE_BASELINE.

**Why worth doing:** this repo has had stale-plan drift before; docs must match runtime truth.

**Expected benefit:** future work starts from accurate commands and current architecture.

**Complexity:** low.

**Risk:** overclaiming manual GUI acceptance; avoid claiming desktop acceptance if not rerun.

**Verification:** doc truthfulness pass, `rg` for stale claims, final code-review-expert checks.

---

## Phase 3 Detailed Plans

### Plan 1: Stable Config Registry And Deterministic Writes

**Files:**
- Modify: `crates/wc-core/src/config.rs`
- Test: `crates/wc-core/src/config.rs`

**Steps:**
- [ ] Add `DEFAULT_CONFIG_PAIRS: &[(&str, &str)]` containing the same defaults in a deliberate stable order.
- [ ] Change `default_config()` to build from `DEFAULT_CONFIG_PAIRS`.
- [ ] Add `default_config_keys()` returning `Vec<&'static str>` in the pair order.
- [ ] Change `init_config_dir()` to append missing defaults in pair order.
- [ ] Change `write_config_value()` to serialize keys in this order: known default keys first, sorted unknown keys after.
- [ ] Add tests:
  - `default_config_keys_are_unique`
  - `write_config_value_is_deterministic`
  - `write_config_value_preserves_unknown_keys_sorted_after_defaults`
- [ ] Run `cargo test -p wc-core config`.

**Compatibility:** config key names and values stay unchanged.

**Rollback:** revert `config.rs`; old behavior resumes.

**Acceptance:** config files are stable across repeated writes with the same logical map.

### Plan 2: SQLite Parameterization Cleanup

**Files:**
- Modify: `crates/wc-storage/src/lib.rs`
- Modify: `crates/wc-storage/src/sqlite/schema.rs`
- Test: `crates/wc-storage/src/lib.rs`
- Test: `crates/wc-storage/src/sqlite/schema.rs`

**Steps:**
- [ ] Add failing tests for SQLite mode `config_get` with `artist's value`.
- [ ] Add failing migration test for source/current/history/config values containing apostrophes.
- [ ] Replace `_sqlite_config_get()` formatted SQL with `SELECT value FROM config WHERE key=?1`.
- [ ] In `migrate_to_sqlite()`, pass raw `key`, `value`, `path`, `cur`, `be`, and `source_runtime_dir` into `params![]`; do not call `sqlite_escape()` before bound params.
- [ ] Remove `sqlite_escape()` if it has no remaining production use.
- [ ] Run `cargo test -p wc-storage sqlite`.

**Compatibility:** fixes future writes; old corrupted values are not auto-repaired in this pass.

**Rollback:** restore escaped inserts and formatted query.

**Acceptance:** apostrophe-containing values round-trip through migration and SQLite reads.

### Plan 3: Versioned SQLite FTS Rebuild And Verify Coverage

**Files:**
- Modify: `crates/wc-storage/src/sqlite/schema.rs`
- Modify: `crates/wc-storage/src/sqlite/backup.rs`
- Test: `crates/wc-storage/src/sqlite/schema.rs`
- Test: `crates/wc-storage/src/sqlite/backup.rs`

**Steps:**
- [ ] Add `const FTS_SCHEMA_VERSION: &str = "2";`.
- [ ] Change `ensure_wallpapers_fts_rebuilt()` to read `db_meta.key='fts_schema_version'` and rebuild when value differs.
- [ ] After rebuild, write `fts_schema_version=2` and `fts_rebuilt_at=datetime('now')`.
- [ ] Add `wallpapers_fts_count()` helper in schema or backup module.
- [ ] Extend `verify()` to compare `COUNT(*) FROM wallpapers` against `COUNT(*) FROM wallpapers_fts`; report failure if they differ.
- [ ] Add tests:
  - old/no `fts_schema_version` triggers rebuild and stores version.
  - verify fails when FTS is manually deleted for an existing wallpaper.
- [ ] Run `cargo test -p wc-storage fts`.

**Compatibility:** existing DBs rebuild FTS once.

**Rollback:** restore timestamp-only guard.

**Acceptance:** version mismatch rebuilds FTS; verify detects count drift.

### Plan 4: Transactional SQLite Scan Staging Batches

**Files:**
- Modify: `crates/wc-storage/src/sqlite/library_session.rs`
- Test: `crates/wc-storage/src/sqlite/library_session.rs`

**Steps:**
- [ ] Add test `library_replace_session_push_rolls_back_failed_batch` with two entries in one push where the second violates uniqueness; assert neither row from that push remains in `wallpapers_stage`.
- [ ] Wrap `library_replace_session_push()` in `unchecked_transaction()`.
- [ ] Prepare/execute inserts inside the transaction and commit only after all entries succeed.
- [ ] Ensure `session.inserted` increments after commit, not before.
- [ ] Run `cargo test -p wc-storage library_replace_session`.

**Compatibility:** public API unchanged.

**Rollback:** remove transaction wrapper.

**Acceptance:** failed batch push leaves stage table unchanged for that push.

### Plan 5: Streaming CLI Rescan

**Files:**
- Modify: `crates/wc-cli/src/main.rs`
- Test: `crates/wc-cli/tests/parity_tests.rs`

**Steps:**
- [ ] Add parity test that creates 300 image files, runs `rescan`, and asserts:
  - command succeeds;
  - `library.tsv` has 300 lines;
  - `library-json --sqlite` reports 300 paths;
  - stdout still contains `entries: 300` and `sqlite: 300`.
- [ ] Add helper `rescan_library(s: &StorageApi) -> anyhow::Result<RescanSummary>` in `wc-cli/src/main.rs`.
- [ ] Implement `RescanSummary` fields: `sources`, `duplicates_skipped`, `walked`, `entries`, `sqlite_count`, `reused`, `probed`, timing durations.
- [ ] Use `wc_scan::visit_wallpapers_with_callback()` to stream candidate paths.
- [ ] Open `library.tsv.tmp` and write each accepted entry line with `BufWriter`.
- [ ] Start `library_replace_session_start()` and push `Vec<WallpaperEntry>` batches of 250.
- [ ] Commit SQLite session only after walk finishes; remove temp TSV and abort session on error.
- [ ] Rename temp TSV only after SQLite commit succeeds.
- [ ] Keep CLI stdout labels compatible with current output.
- [ ] Run `cargo test -p wc-cli --test parity_tests rescan`.

**Compatibility:** command name/output labels stay the same.

**Rollback:** restore old vector-based branch.

**Acceptance:** CLI rescan no longer calls `scan_wallpapers()` in the rescan command and still writes TSV + SQLite atomically.

### Plan 6: CLI Stop Clears Runtime State

**Files:**
- Modify: `crates/wc-cli/src/main.rs`
- Test: `crates/wc-cli/tests/parity_tests.rs`

**Steps:**
- [ ] Add sqlite-mode parity test that writes current/last_backend/history, runs `stop`, and asserts current/last_backend are empty while history remains.
- [ ] Add flat-file parity test for same behavior.
- [ ] Change `Commands::Stop` to call `wc_backend::stop_all_backends(Some(s))?; s.runtime_state_clear()?;`.
- [ ] Keep stdout text `All wallpaper backends stopped.` unchanged.
- [ ] Run targeted tests.

**Compatibility:** CLI command name and stdout unchanged; state is more accurate.

**Rollback:** remove `runtime_state_clear()`.

**Acceptance:** CLI and GUI Stop semantics match.

### Plan 7: Shared Frontend Paged Wallpaper Hook

**Files:**
- Create: `apps/tauri-gui/frontend/src/hooks/usePagedWallpapers.ts`
- Create: `apps/tauri-gui/frontend/src/hooks/usePagedWallpapers.test.ts`
- Modify: `apps/tauri-gui/frontend/src/views/LibraryView.tsx`
- Modify: `apps/tauri-gui/frontend/src/views/FavoritesView.tsx`
- Modify: `apps/tauri-gui/frontend/src/views/HistoryView.tsx`

**Steps:**
- [ ] Add pure `createPagedWallpaperController()` or exported reducer helpers so unit tests can cover stale response and append logic without React test renderer.
- [ ] Implement hook API:
  ```ts
  usePagedWallpapers({
    pageSize,
    deps,
    loadPage,
    metricName,
    resetKeys,
    invalidationEvent,
  })
  ```
- [ ] Hook state: `entries`, `total`, `loading`, `reload`, `loadMore`, `entryByPath`.
- [ ] Preserve request sequence cancellation so stale responses do not overwrite current entries.
- [ ] Convert `LibraryView` to pass filter/sort/debouncedSearch/libraryVersion as `resetKeys`.
- [ ] Convert `FavoritesView` and `HistoryView` to use `invalidationEvent`.
- [ ] Add tests:
  - first response loads entries.
  - append response extends entries.
  - stale earlier response is ignored.
  - reset clears append state and replaces entries.
- [ ] Run `npm run test:unit` and `npm run typecheck`.

**Compatibility:** UI behavior and API calls unchanged.

**Rollback:** restore per-view state machines.

**Acceptance:** duplicated pagination state is removed from three views.

### Plan 8: Typed Frontend Event Bus

**Files:**
- Create: `apps/tauri-gui/frontend/src/events/appEvents.ts`
- Create: `apps/tauri-gui/frontend/src/events/appEvents.test.ts`
- Modify: raw event call sites in frontend `src/`

**Steps:**
- [ ] Define constants:
  - `WC_FEEDBACK_EVENT`
  - `WC_CONFIG_CHANGED_EVENT`
  - `FAVORITES_CACHE_INVALIDATED_EVENT`
  - `HISTORY_CACHE_INVALIDATED_EVENT`
- [ ] Export helpers:
  - `emitFeedback(feedback: CommandFeedback)`
  - `onFeedback(handler)`
  - `emitConfigChanged(key, value)`
  - `emitFavoritesInvalidated()`
  - `onFavoritesInvalidated(handler)`
  - `emitHistoryInvalidated()`
  - `onHistoryInvalidated(handler)`
- [ ] Update `useFeedbackBridge`, `LibraryView`, `FavoritesView`, `HistoryView`, `WallpaperGrid`, `ContextMenu`, `SettingsView`, and `useLibraryEntryActions` to use helpers.
- [ ] Keep actual event names unchanged.
- [ ] Add tests that subscribe, emit, and unsubscribe.
- [ ] Run `npm run test:unit` and `npm run typecheck`.

**Compatibility:** event names stay unchanged.

**Rollback:** replace helpers with direct `window.dispatchEvent`.

**Acceptance:** no production `new CustomEvent('wc-feedback'...)` raw call sites remain.

### Plan 9: Verification `xtask`

**Files:**
- Modify: `xtask/Cargo.toml`
- Modify: `xtask/src/main.rs`
- Modify: `.github/workflows/ci.yml`
- Test: dry-run commands.

**Steps:**
- [ ] Implement CLI:
  ```text
  cargo run -p xtask -- verify rust [--dry-run]
  cargo run -p xtask -- verify frontend [--dry-run]
  cargo run -p xtask -- verify all [--dry-run]
  ```
- [ ] Use `std::process::Command` with explicit working directories.
- [ ] Print each command before running it.
- [ ] Stop on first non-zero exit and return that exit code.
- [ ] Rust group commands:
  - `cargo fmt --all -- --check`
  - `cargo check --workspace`
  - `cargo clippy --workspace -- -D warnings`
  - `cargo test --workspace`
- [ ] Frontend group commands from `apps/tauri-gui/frontend`:
  - `npm run typecheck`
  - `npm run test:unit`
  - `npm run build`
  - `npm run smoke`
- [ ] Update CI to call `cargo run -p xtask -- verify rust` in Rust job after Rust setup, and `cargo run -p xtask -- verify frontend` in frontend job after npm install and Playwright install.
- [ ] Add dry-run verification commands to final matrix.

**Compatibility:** CI checks stay equivalent, just centralized.

**Rollback:** restore explicit CI steps and placeholder xtask.

**Acceptance:** dry-run prints exact expected commands; CI uses xtask without losing coverage.

### Plan 10: Docs And Status Sync

**Files:**
- Modify: `README.md`
- Modify: `docs/DEVELOPMENT.md`
- Modify: `docs/CURRENT_STATUS.md`
- Modify: `docs/PERFORMANCE_BASELINE.md`
- Modify: this plan file with completion notes.

**Steps:**
- [ ] Document deterministic config writes.
- [ ] Document CLI streaming rescan and transactional SQLite staging.
- [ ] Document FTS versioned rebuild/verify behavior.
- [ ] Document CLI Stop state semantics.
- [ ] Document `cargo run -p xtask -- verify all`.
- [ ] Preserve manual GUI acceptance as open unless a real desktop checklist is completed.
- [ ] Add final completion table with each priority's tests.
- [ ] Run `rg -n "cargo build --release -p wallpaper-console-tauri|CI/release automation|Wails measurements are current|manual GUI acceptance.*Completed" README.md docs .github scripts`.

**Compatibility:** docs only.

**Rollback:** revert doc changes.

**Acceptance:** docs match source behavior and do not overclaim desktop acceptance.

---

## Phase 4 Plan Self-Review

### Misunderstanding Check

- The project is Tauri-only for GUI; no plan reintroduces Wails or Go.
- SQLite is the GUI source of truth; TSV remains for CLI compatibility.
- Backend process tests must not apply real wallpapers unless already guarded by fake scripts or runtime seams.
- Manual GUI acceptance is not automated by Playwright; this plan does not claim it.

### Dependency Check

- Plans 4 and 5 depend on `library_session`; implement Plan 4 before Plan 5.
- Plan 7 can run independently of backend changes but benefits from stable frontend unit tests before event-bus rewrites.
- Plan 8 should run after Plan 7 to avoid moving raw events in code that Plan 7 deletes.
- Plan 9 should run after code changes, so final verification command reflects the final matrix.
- Plan 10 must be last.

### Compatibility Check

- CLI command names and stdout labels remain.
- Tauri command names remain.
- Event names remain.
- Config keys/values remain.
- SQLite schema changes are additive/meta-only.

### Overdesign Check

- No new external dependencies are introduced.
- `xtask` uses standard library only.
- Frontend shared hooks replace duplicated state machines but do not introduce routing/state libraries.
- Config registry is a static pair list, not a validation framework.

### Testability Check

- Each production behavior has targeted tests or dry-run command evidence.
- GUI manual rendering remains a known gap; final report must separate automated smoke from real desktop acceptance.

### Revised Implementation Order

1. Config determinism.
2. SQLite parameterization.
3. FTS version/verify.
4. Transactional staging push.
5. CLI streaming rescan.
6. CLI Stop state clear.
7. Shared paged frontend hook.
8. Typed frontend event bus.
9. `xtask` verification runner + CI wiring.
10. Docs/status sync.

### Remaining Accepted Risks

- Historical databases that already stored doubled apostrophes are not automatically repaired.
- FTS rebuild may add a one-time startup cost on first run after this version.
- `npm run smoke` may fail if the environment cannot run Playwright's local server; this is environmental and must be reported if encountered.

---

## Implementation Closeout Notes

Completed in this pass:

1. Deterministic config registry added in `wc-core`.
2. SQLite migration/config reads now rely on bound parameters without manual escaping.
3. SQLite FTS schema versioning and integrity verification added.
4. SQLite replacement session batch pushes are transactional.
5. CLI `rescan` streams scan candidates through metadata probing, TSV output, and SQLite staging batches.
6. CLI `stop` clears runtime state after backend stop while preserving history.
7. Library/Favorites/History paging state extracted into `usePagedWallpapers`.
8. Frontend feedback/config/cache events centralized in `events/appEvents.ts`.
9. `xtask` now runs `verify rust|frontend|all`, and CI calls xtask.
10. README, DEVELOPMENT, CURRENT_STATUS, and PERFORMANCE_BASELINE updated.

Manual desktop GUI acceptance remains separate and must still use `docs/TAURI_MANUAL_SMOKE_CHECKLIST.md`.
