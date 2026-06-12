# SQLite Verify Semantics + Chromium Exit-0 Handoff — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix false "VERIFY FAILED" from flat-file comparison drift and false "exited immediately" from Chromium process handoff.

**Architecture:** Phase 1 splits `verify()` into warnings (config/sources — compatibility copies) and errors (data integrity). Phase 2 adds pgrep-based handoff detection in `apply_preflighted()`.

**Tech Stack:** Rust (rusqlite, std::process), TypeScript (React), no new dependencies.

---

### Task 1: Add `VerifyResult` enum to `wc-storage`

**Files:**
- Modify: `crates/wc-storage/src/sqlite.rs:233-357`

- [ ] **Step 1: Add `VerifyResult` enum and modify `verify()` return type**

```rust
/// Result of database verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyResult {
    /// All categories match.
    Ok,
    /// Data integrity is fine, but flat-file compatibility copies have drifted.
    OkWithWarnings(Vec<String>),
    /// Real data mismatch detected (wallpapers, favorites, history, state).
    Failed(Vec<String>),
}
```

Insert this enum above the `verify()` function (after line 233's doc comment, before the `pub fn verify` line).

- [ ] **Step 2: Split errors into warnings and errors in `verify()`**

Change the `errors` vector to two separate vectors: `warnings` and `errors`. Move `config` and `sources` mismatches into `warnings`. Everything else stays in `errors`.

Replace the final return block (lines 348-357):

```rust
    if !errors.is_empty() {
        Err(WcError::Other(format!(
            "VERIFY FAILED: {} mismatch(es) found: {}",
            errors.len(),
            errors.join(", ")
        )))
    } else if !warnings.is_empty() {
        // warnings contains warning items but return type is still Result
        // — we need to change the signature. Do this in the next step.
        Ok(())
    } else {
        Ok(())
    }
```

Wait — the function signature is `Result<(), WcError>`. We need to change it to return `Result<VerifyResult, WcError>`. Let me do it all at once.

Actually, let me split it cleanly:
- Step 2a: Add the enum
- Step 2b: Change `verify()` signature to `Result<VerifyResult, WcError>` and split `errors` into `warnings` + `errors`
- Step 2c: Update all callers

Let me just write the full modified function for clarity. Replace the entire `verify()` function (lines 235-357) and add the enum above it:

After line 233 (`/// Err with mismatch details if not.`), insert the enum:

```rust
/// Result of database verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyResult {
    /// All categories match.
    Ok,
    /// Data integrity is fine, but flat-file compatibility copies have drifted.
    OkWithWarnings(Vec<String>),
    /// Real data mismatch detected (wallpapers, favorites, history, state).
    Failed(Vec<String>),
}
```

Then replace the entire `pub fn verify(cd: &ConfigDir) -> Result<(), WcError>` function. The old function starts at line 235 with its doc comment and ends at line 357 with `}`. Replace this ENTIRE function with:

```rust
/// Compare flat files vs SQLite. Returns:
/// - `Ok(VerifyResult::Ok)` — all consistent
/// - `Ok(VerifyResult::OkWithWarnings(w))` — config/sources compatibility copies
///   have drifted; actual data is fine
/// - `Ok(VerifyResult::Failed(e))` — real data mismatch (wallpapers, favorites,
///   history, state)
/// - `Err(WcError::Sqlite(...))` — schema corruption or missing DB
pub fn verify(cd: &ConfigDir) -> Result<VerifyResult, WcError> {
    let db_path = cd.db_path();
    if !db_path.exists() {
        return Err(WcError::Other(
            "wallpapers.db not found. Run migrate-to-sqlite first.".into(),
        ));
    }
    let conn = Connection::open(&db_path).map_err(|e| WcError::Sqlite(e.to_string()))?;

    let mut warnings: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    // Config — compatibility copy only; drift is a warning.
    {
        let flat_cfg = wc_core::config::parse_config_file(&cd.config_path())?;
        let mut stmt = conn
            .prepare("SELECT key, value FROM config ORDER BY key")
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        let db_rows: Vec<(String, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| WcError::Sqlite(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        let db_cfg: std::collections::HashMap<String, String> = db_rows.into_iter().collect();
        if flat_cfg != db_cfg {
            warnings.push("config".into());
        }
    }

    // Sources — compatibility copy only; drift is a warning.
    {
        let mut flat_src: Vec<String> = flat::sources_list(cd)?;
        flat_src.sort();
        let mut stmt = conn
            .prepare("SELECT path FROM sources ORDER BY path")
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        let db_src: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| WcError::Sqlite(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        if flat_src != db_src {
            warnings.push("sources".into());
        }
    }

    // Favorites — data integrity; mismatch is an error.
    {
        let mut flat_fav: Vec<String> = flat::favorites_list(cd)?;
        flat_fav.sort();
        let mut stmt = conn
            .prepare("SELECT path FROM favorites ORDER BY path")
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        let db_fav: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| WcError::Sqlite(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        if flat_fav != db_fav {
            errors.push("favorites".into());
        }
    }

    // History — data integrity; mismatch is an error.
    {
        let mut flat_hist: Vec<String> = flat::history_list(cd)?;
        flat_hist.sort();
        let mut stmt = conn
            .prepare("SELECT path FROM history ORDER BY path")
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        let mut db_hist: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| WcError::Sqlite(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| WcError::Sqlite(e.to_string()))?;
        db_hist.sort();
        if flat_hist != db_hist {
            errors.push("history".into());
        }
    }

    // State: current — data integrity; mismatch is an error.
    {
        let flat_cur = flat::current_read(cd)?.unwrap_or_default();
        let db_cur: String =
            match conn.query_row("SELECT value FROM state WHERE key='current'", [], |row| {
                row.get(0)
            }) {
                Ok(v) => v,
                Err(rusqlite::Error::QueryReturnedNoRows) => String::new(),
                Err(e) => return Err(WcError::Sqlite(e.to_string())),
            };
        if flat_cur != db_cur {
            errors.push("current".into());
        }
    }

    // State: last_backend — data integrity; mismatch is an error.
    {
        let flat_be = flat::last_backend_read(cd)?.unwrap_or_default();
        let db_be: String = match conn.query_row(
            "SELECT value FROM state WHERE key='last_backend'",
            [],
            |row| row.get(0),
        ) {
            Ok(v) => v,
            Err(rusqlite::Error::QueryReturnedNoRows) => String::new(),
            Err(e) => return Err(WcError::Sqlite(e.to_string())),
        };
        if flat_be != db_be {
            errors.push("last_backend".into());
        }
    }

    if !errors.is_empty() {
        Ok(VerifyResult::Failed(errors))
    } else if !warnings.is_empty() {
        Ok(VerifyResult::OkWithWarnings(warnings))
    } else {
        Ok(VerifyResult::Ok)
    }
}
```

- [ ] **Step 3: Run tests to verify compilation**

Run: `cargo test -p wc-storage 2>&1`
Expected: compilation errors (callers need updating first — we'll fix next)

- [ ] **Step 4: Commit**

```bash
git add crates/wc-storage/src/sqlite.rs
git commit -m "feat: split verify into VerifyResult with warnings for config/sources drift"
```

### Task 2: Update CLI `sqlite-verify` command

**Files:**
- Modify: `crates/wc-cli/src/main.rs:790-797`

- [ ] **Step 1: Update the handler to match new return type**

Replace lines 790-797 with:

```rust
        Commands::SqliteVerify => match wc_storage::sqlite::verify(&s.cd) {
            Ok(wc_storage::sqlite::VerifyResult::Ok) => println!("VERIFY OK"),
            Ok(wc_storage::sqlite::VerifyResult::OkWithWarnings(warnings)) => {
                println!("VERIFY OK WITH WARNINGS");
                for w in &warnings {
                    println!("  warning: flat compatibility copy differs: {}", w);
                }
            }
            Ok(wc_storage::sqlite::VerifyResult::Failed(errors)) => {
                eprintln!(
                    "VERIFY FAILED: {} mismatch(es) found: {}",
                    errors.len(),
                    errors.join(", ")
                );
                std::process::exit(1);
            }
            Err(e) => {
                eprintln!("{}", e);
                let msg = e.to_string();
                if msg.contains("not found") {
                    std::process::exit(2);
                } else {
                    std::process::exit(1);
                }
            }
        }
```

- [ ] **Step 2: Build and verify**

Run: `cargo build -p wc-cli 2>&1`
Expected: no errors

- [ ] **Step 3: Commit**

```bash
git add crates/wc-cli/src/main.rs
git commit -m "feat: update sqlite-verify CLI for VerifyResult with warnings"
```

### Task 3: Update Tauri `sqlite_verify` command

**Files:**
- Modify: `apps/tauri-gui/src-tauri/src/commands/settings.rs:52-65`

- [ ] **Step 1: Update the Tauri command**

Replace lines 52-65 with:

```rust
#[tauri::command]
pub async fn sqlite_verify() -> CommandResult {
    let result = tauri::async_runtime::spawn_blocking(|| {
        match storage()
            .and_then(|s| wc_storage::sqlite::verify(&s.cd).map_err(|e| e.to_string()))
        {
            Ok(wc_storage::sqlite::VerifyResult::Ok) => ok("VERIFY OK"),
            Ok(wc_storage::sqlite::VerifyResult::OkWithWarnings(warnings)) => {
                ok(format!(
                    "VERIFY OK WITH WARNINGS\n{}",
                    warnings.join("\n")
                ))
            }
            Ok(wc_storage::sqlite::VerifyResult::Failed(errors)) => {
                fail(format!(
                    "VERIFY FAILED: {} mismatch(es) found: {}",
                    errors.len(),
                    errors.join(", ")
                ))
            }
            Err(err) => fail(err),
        }
    })
    .await;
    match result {
        Ok(r) => r,
        Err(e) => fail(e.to_string()),
    }
}
```

- [ ] **Step 2: Build and verify**

Run: `cargo build -p wallpaper-console-tauri 2>&1`
Expected: no errors

- [ ] **Step 3: Commit**

```bash
git add apps/tauri-gui/src-tauri/src/commands/settings.rs
git commit -m "feat: update sqlite_verify Tauri command for VerifyResult with warnings"
```

### Task 4: Add `warning` state to frontend feedback

**Files:**
- Modify: `apps/tauri-gui/frontend/src/api/feedback.ts:3-7`
- Modify: `apps/tauri-gui/frontend/src/App.tsx` (feedback banner rendering)

- [ ] **Step 1: Add `warning` state to `CommandFeedback` type**

Replace line 3-7 in `feedback.ts`:

```typescript
export type CommandFeedback =
  | { state: 'idle' }
  | { state: 'running'; label: string }
  | { state: 'success'; label: string; detail?: string }
  | { state: 'warning'; label: string; detail: string }
  | { state: 'error'; label: string; detail: string };
```

- [ ] **Step 2: Add `warning` rendering in App.tsx feedback banner**

Find the feedback banner rendering in App.tsx. Look for `state === 'success'` or `state === 'error'` styling. Add a `warning` case with amber/orange styling:

Find the feedback banner JSX (likely uses className based on state). Add:

```tsx
{feedback.state === 'warning' && (
  <div className="feedback-banner warning" /* or inline style */>
    <span>{feedback.label}</span>
    {feedback.detail && <pre>{feedback.detail}</pre>}
  </div>
)}
```

If the banner uses inline styles, add an amber color for `warning`:
- Background: `#fff3cd` (light amber)  
- Border: `#ffc107`
- Text: `#856404`

- [ ] **Step 3: Update SettingsView to use warning feedback for WARNINGS results**

In `SettingsView.tsx`, find the `runDbAction` callback for verify (around line 364-371). After the `api.sqliteVerify()` call, inspect the result:

```typescript
onClick={() => runDbAction('verify', 'Verify', async () => {
    const result = await api.sqliteVerify();
    if (result.stdout && result.stdout.includes('WITH WARNINGS')) {
        onFeedback({ state: 'warning', label: 'Verify complete', detail: result.stdout });
    }
    return result;
})}
```

- [ ] **Step 4: Verify frontend compiles**

Run: `npm run typecheck` in `apps/tauri-gui/frontend`
Expected: no errors

- [ ] **Step 5: Commit**

```bash
git add apps/tauri-gui/frontend/src/api/feedback.ts apps/tauri-gui/frontend/src/App.tsx apps/tauri-gui/frontend/src/views/SettingsView.tsx
git commit -m "feat: add warning feedback state for verify WITH WARNINGS"
```

### Task 5: Add Rust tests for new verify behavior

**Files:**
- Modify: `crates/wc-storage/src/sqlite.rs` — add to existing test module

- [ ] **Step 1: Add verify tests**

Find the test module in sqlite.rs (starts around line 332). Add these tests inside the `#[cfg(test)] mod tests {` block, after the existing tests:

```rust
    #[test]
    fn verify_ok_when_all_match() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        wc_core::config::write_config_value(&cd.path, "storage_backend", "sqlite").unwrap();
        flat::sources_add(&cd, "/walls").unwrap();
        flat::favorites_add(&cd, "/walls/a.jpg").unwrap();
        flat::history_add(&cd, "/walls/b.jpg", 100).unwrap();
        flat::current_write(&cd, "/walls/cur.jpg").unwrap();
        flat::last_backend_write(&cd, "awww").unwrap();

        // Migrate flat → SQLite so both match.
        crate::sqlite::migrate_to_sqlite(&cd).unwrap();

        let result = crate::sqlite::verify(&cd).unwrap();
        assert_eq!(result, crate::sqlite::VerifyResult::Ok);
    }

    #[test]
    fn verify_warning_when_config_drifts() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        wc_core::config::write_config_value(&cd.path, "storage_backend", "sqlite").unwrap();
        crate::sqlite::migrate_to_sqlite(&cd).unwrap();

        // Drift the flat config to differ from SQLite.
        wc_core::config::write_config_value(&cd.path, "test_key", "new_value").unwrap();

        let result = crate::sqlite::verify(&cd).unwrap();
        assert!(
            matches!(result, crate::sqlite::VerifyResult::OkWithWarnings(ref w) if w.contains(&"config".to_string())),
            "expected OkWithWarnings containing 'config', got: {:?}",
            result
        );
    }

    #[test]
    fn verify_warning_when_sources_drift() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        wc_core::config::write_config_value(&cd.path, "storage_backend", "sqlite").unwrap();
        crate::sqlite::migrate_to_sqlite(&cd).unwrap();

        // Drift flat sources (add one that SQLite doesn't have).
        flat::sources_add(&cd, "/extra-source").unwrap();

        let result = crate::sqlite::verify(&cd).unwrap();
        assert!(
            matches!(result, crate::sqlite::VerifyResult::OkWithWarnings(ref w) if w.contains(&"sources".to_string())),
            "expected OkWithWarnings containing 'sources', got: {:?}",
            result
        );
    }

    #[test]
    fn verify_failed_when_favorites_differ() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        wc_core::config::write_config_value(&cd.path, "storage_backend", "sqlite").unwrap();
        crate::sqlite::migrate_to_sqlite(&cd).unwrap();

        // Drift flat favorites.
        flat::favorites_add(&cd, "/extra-fav.jpg").unwrap();

        let result = crate::sqlite::verify(&cd).unwrap();
        assert!(
            matches!(result, crate::sqlite::VerifyResult::Failed(ref e) if e.contains(&"favorites".to_string())),
            "expected Failed containing 'favorites', got: {:?}",
            result
        );
    }

    #[test]
    fn verify_error_when_db_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();

        let result = crate::sqlite::verify(&cd);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p wc-storage 2>&1`
Expected: all tests pass (including new ones)

- [ ] **Step 3: Commit**

```bash
git add crates/wc-storage/src/sqlite.rs
git commit -m "test: verify ok/warning/failed/missing - assure semantic split"
```

### Task 6: Fix Chromium exit-0 handoff in `apply_preflighted`

**Files:**
- Modify: `crates/wc-backend/src/web_wallpaper.rs:287-339`

- [ ] **Step 1: Extract testable `check_browser_handoff` function**

Insert this new function BEFORE `apply_preflighted` (before line 287):

```rust
/// Check whether a browser process is still running with the given profile dir.
/// Returns true if pgrep finds a matching process.
fn check_browser_handoff(profile_dir: &std::path::Path) -> bool {
    let pattern = format!(
        "--user-data-dir={}",
        profile_dir.display()
    );
    match std::process::Command::new("pgrep")
        .args(["-f", "--", &pattern])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
    {
        Ok(status) => status.success(),
        Err(_) => false,
    }
}
```

- [ ] **Step 2: Modify `apply_preflighted` exit-0 handoff logic**

Replace lines 328-336 (the `try_wait` block) with:

```rust
    std::thread::sleep(Duration::from_millis(300));
    if let Ok(Some(status)) = child.try_wait() {
        if status.success() {
            // Browser wrapper exited cleanly (status 0) — may have handed off
            // to an existing browser process. Check if our profile is still in use.
            if check_browser_handoff(&profile_dir) {
                return Ok(());
            }
        }
        // Either non-zero exit or zero exit with no handoff.
        let _ = s.config_set(PID_CONFIG_KEY, "");
        return Err(WcError::Other(format!(
            "Web wallpaper browser exited with status {}. \
             Check that the browser can access the project file and the display server is available.",
            status
        )));
    }
```

- [ ] **Step 3: Build and verify**

Run: `cargo build -p wc-backend 2>&1`
Expected: no errors

- [ ] **Step 4: Commit**

```bash
git add crates/wc-backend/src/web_wallpaper.rs
git commit -m "feat: exit-0 handoff check for Chromium browser process reuse"
```

### Task 7: Add tests for Chromium exit-0 handoff

**Files:**
- Modify: `crates/wc-backend/src/web_wallpaper.rs` — test module

- [ ] **Step 1: Find or create test module location**

Check if web_wallpaper.rs has a `#[cfg(test)] mod tests` block. If not, find the end of file and add one.

Run: `grep -n '#\[cfg(test)\]' crates/wc-backend/src/web_wallpaper.rs`
If none found, add a test module at the end of the file.

- [ ] **Step 2: Add handoff tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handoff_detects_running_browser() {
        // Spawn a sleep process with our profile pattern in its args,
        // then check that handoff detection finds it.
        let dir = tempfile::tempdir().unwrap();
        let profile = dir.path().join("web-wallpaper-profile");
        std::fs::create_dir_all(&profile).unwrap();

        // Spawn a process that pgrep can find.
        let mut child = std::process::Command::new("sleep")
            .arg("5")
            .arg(format!("--user-data-dir={}", profile.display()))
            .spawn()
            .unwrap();

        assert!(check_browser_handoff(&profile));

        child.kill().ok();
        child.wait().ok();
    }

    #[test]
    fn handoff_reports_false_when_no_process() {
        let dir = tempfile::tempdir().unwrap();
        let profile = dir.path().join("nonexistent-profile");
        std::fs::create_dir_all(&profile).unwrap();

        assert!(!check_browser_handoff(&profile));
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p wc-backend web_wallpaper 2>&1`
Expected: both tests pass

- [ ] **Step 4: Commit**

```bash
git add crates/wc-backend/src/web_wallpaper.rs
git commit -m "test: Chromium exit-0 handoff detection tests"
```

### Task 8: Full verification

- [ ] **Step 1: Rust verification**

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo build --workspace
```
Expected: all pass

- [ ] **Step 2: Frontend verification**

```bash
cd apps/tauri-gui/frontend
npm run typecheck
npm run test:unit
npm run build
npm run smoke
```
Expected: all pass

- [ ] **Step 3: Commit final state if any fmt changes**

```bash
git status
# If fmt made changes:
git add -u && git commit -m "chore: fmt after verify and exit-0 fixes"
```
