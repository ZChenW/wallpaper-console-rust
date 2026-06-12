# SQLite Verify Semantics + Chromium Exit-0 Handoff Fix

## Scope

Two independent fixes scoped to the current session. No new crates or dependencies.

## Phase 1: SQLite verify semantics

### Problem

`wc_storage::sqlite::verify()` compares SQLite against flat files for ALL categories including `config` and `sources`. In SQLite-first mode, flat files are best-effort compatibility copies — they drift by design. Comparing them produces false "VERIFY FAILED" results.

### Design

**Return type** — introduce `VerifyResult` enum in `wc-storage/src/sqlite.rs`:

```rust
pub enum VerifyResult {
    Ok,
    OkWithWarnings(Vec<String>),
    Failed(Vec<String>),
}
```

**Verification tiers:**

| Tier | Categories | Mismatch result |
|------|-----------|-----------------|
| Warning | config, sources | Collected into `warnings` vector |
| Error | wallpapers, favorites, history, current, last_backend | Collected into `errors` vector |

**Return logic:**
- Errors non-empty → `VerifyResult::Failed(errors)`
- Errors empty, warnings non-empty → `VerifyResult::OkWithWarnings(warnings)`
- Both empty → `VerifyResult::Ok`

**Schema integrity** — if `wallpapers.db` is missing, required tables missing, or any SQLite query fails, return `Err(WcError::Sqlite(...))` as before (this is a hard failure, not a comparison mismatch).

**Tauri command** — `sqlite_verify` in `apps/tauri-gui/src-tauri/src/commands/settings.rs`:
- `VerifyResult::Ok` → `ok("VERIFY OK")`
- `VerifyResult::OkWithWarnings(w)` → `ok(format!("VERIFY OK WITH WARNINGS\n{}", w.join("\n")))`
- `VerifyResult::Failed(e)` → `fail(format!("VERIFY FAILED: {}", e.join(", ")))`
- `Err(e)` → `fail(e.to_string())` (unchanged)

**CLI command** — `sqlite-verify` in `crates/wc-cli/src/main.rs`:
- Same mapping, printed to stdout for Ok/OkWithWarnings, stderr for Failed/Err

**Frontend** — `SettingsView.tsx`:
- `runDbAction` callback inspects the result string. If it contains `"WITH WARNINGS"`, call `onFeedback({ state: 'warning', label: 'Verify complete', detail: result })` to show an amber banner. If it contains `"VERIFY OK"` without warnings, use the existing green success path. If it's an error, use the existing red error path.
- The `FeedbackState` type in `feedback.ts` already supports `state: 'success' | 'error' | 'running'`. Add `'warning'` to the union and use an amber color in the feedback banner component.

### Tests

File: `crates/wc-storage/src/sqlite.rs` test module

1. `verify_ok_when_all_match` — fresh DB with matching flat files → `VerifyResult::Ok`
2. `verify_warning_when_config_drifts` — SQLite config differs from flat → `VerifyResult::OkWithWarnings` containing config
3. `verify_warning_when_sources_drift` — SQLite sources differ → `VerifyResult::OkWithWarnings`
4. `verify_failed_when_wallpapers_differ` — wallpaper count differs → `VerifyResult::Failed`
5. `verify_failed_when_db_missing` — no wallpapers.db → `Err(WcError::Sqlite(...))`
6. `verify_stores_state_correctly_after_sqlite_export_flat` — after `sqlite_export_flat`, verify returns Ok (flat now matches)

## Phase 2: Chromium exit-0 handoff

### Problem

`apply_preflighted()` launches the browser, waits 300ms, checks `try_wait()`. Chromium often spawns a wrapper that exits immediately (status 0) after handing the window to an existing browser process. This causes false "exited immediately" errors even though the web wallpaper is actually running.

### Design

**Modified `apply_preflighted()` in `crates/wc-backend/src/web_wallpaper.rs`:**

After 300ms sleep + `try_wait()`:

```
if exit status != 0:
    → Err("browser exited with status N")

if exit status == 0:
    check if profile is still in use:
        run: pgrep -f -- "--user-data-dir=$PROFILE_DIR"
    if pgrep finds a process:
        → Ok(())  (handoff to existing browser process succeeded)
    else:
        → Err("browser exited and no process using the profile was found")

if still running:
    → Ok(())  (unchanged)
```

**Implementation details:**
- `PROFILE_DIR` is the canonicalized `s.cd.path.join("web-wallpaper-profile")`
- Use `std::process::Command::new("pgrep")` with args `["-f", "--", pattern]`
- `pgrep` exit status 0 means match found, exit status 1 means no match, anything else is a pgrep error
- Treat pgrep exit 1 as "no handoff" (error)
- Treat pgrep errors (exit > 1) the same as no handoff

### Tests

File: `crates/wc-backend/src/web_wallpaper.rs` test module

1. `apply_preflighted_exit_zero_with_profile_match_is_success` — mock that leaves a pgrep-able process → Ok
2. `apply_preflighted_exit_zero_no_profile_match_is_error` — mock that exits 0 but pgrep finds nothing → Err
3. `apply_preflighted_exit_nonzero_is_error` — mock that exits 1 → Err (unchanged behavior)

For unit testing without real Chromium, extract the handoff-check logic into a testable function `fn check_browser_handoff(profile_dir: &str) -> bool` that wraps the pgrep call.

## Affected files

| File | Change |
|------|--------|
| `crates/wc-storage/src/sqlite.rs` | New `VerifyResult` enum, modified `verify()` return type, new warning/error split logic |
| `crates/wc-cli/src/main.rs` | `sqlite-verify` command maps `VerifyResult` variants |
| `apps/tauri-gui/src-tauri/src/commands/settings.rs` | `sqlite_verify` Tauri command maps `VerifyResult` variants |
| `apps/tauri-gui/frontend/src/views/SettingsView.tsx` | Amber warning banner for `VERIFY OK WITH WARNINGS` |
| `crates/wc-backend/src/web_wallpaper.rs` | Modified `apply_preflighted()` exit-0 handoff check |
| `crates/wc-backend/src/lib.rs` | No change needed (caller unchanged) |

## Verification

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo build --workspace
npm run typecheck
npm run test:unit
npm run build
npm run smoke
```
