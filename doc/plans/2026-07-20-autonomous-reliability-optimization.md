# Autonomous Reliability Optimization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix six demonstrated reliability, UX, diagnostic, and verification failures without changing the accepted visual design.

**Architecture:** Put retry/deduplication and draft parsing behind small pure interfaces, keep React and background-thread code as adapters, and make runtime failures explicit at existing seams. Each task owns a disjoint file cluster and produces an independently reversible `jj` commit.

**Tech Stack:** Rust 2021, Tauri 2, React 19, TypeScript 5.7, node:test, Playwright, Bash, Jujutsu (`jj`).

---

## File map

- `apps/tauri-gui/frontend/src/shell/startupWatchdog.ts`: first-paint predicates, acknowledged ready delivery, timeout resolution.
- `apps/tauri-gui/frontend/src/shell/recurringErrorGate.ts`: continuous-error deduplication with recovery reset.
- `apps/tauri-gui/frontend/src/shell/SinglePageShell.tsx`: React adapter for readiness, timeout, and scan notices.
- `apps/tauri-gui/src-tauri/src/library_scheduler.rs`: watcher registry and Linux inotify worker lifecycle.
- `apps/tauri-gui/frontend/src/components/libraryViewModel.ts`: shared adapter semantics, including display apply reason.
- `apps/tauri-gui/frontend/src/components/LibraryViewport.tsx`: Grid adapter wiring.
- `apps/tauri-gui/frontend/src/components/WallpaperCard.tsx`: non-applicable action feedback.
- `apps/tauri-gui/frontend/src/shell/deferredNumberInput.ts`: parse/reset/commit rules for number drafts.
- `apps/tauri-gui/frontend/src/shell/CompactSettingsPanel.tsx`: uncontrolled number-input adapter.
- `apps/tauri-gui/src-tauri/src/commands/library.rs`: fail-fast interactive Library status.
- `apps/tauri-gui/src-tauri/src/commands/settings.rs`: best-effort, privacy-safe diagnostic statuses.
- `scripts/test_tauri_before_commands.sh`: sourceable before-command probe.
- `scripts/test_tauri_before_commands_unit.sh`: exit-code regression test.
- `apps/tauri-gui/frontend/package.json`, `xtask/src/main.rs`: non-duplicated verification matrix.

### Task 1: Make frontend startup maintenance delivery recoverable

**Files:**
- Modify: `apps/tauri-gui/frontend/src/shell/startupWatchdog.ts`
- Modify: `apps/tauri-gui/frontend/src/shell/startupWatchdog.test.ts`
- Create: `apps/tauri-gui/frontend/src/shell/recurringErrorGate.ts`
- Create: `apps/tauri-gui/frontend/src/shell/recurringErrorGate.test.ts`
- Modify: `apps/tauri-gui/frontend/src/shell/SinglePageShell.tsx`

- [ ] **Step 1: Write failing ready-delivery tests**

Add deterministic injected-timer tests proving that a rejected first send is retried, one request is in flight at a time, acknowledgement stops future sends, deactivation cancels a pending retry, and reactivation resumes delivery. The intended interface is:

```ts
export interface LibraryReadyDelivery {
  readonly acknowledged: boolean;
  activate(): void;
  deactivate(): void;
}

export function createLibraryReadyDelivery(
  send: () => Promise<void>,
  timers?: {
    setTimer(callback: () => void, delayMs: number): unknown;
    clearTimer(handle: unknown): void;
  },
): LibraryReadyDelivery;
```

- [ ] **Step 2: Write failing state-reset tests**

Add `shouldClearLibraryTimeout()` cases for resolved entries, confirmed empty, and terminal load error, while unresolved loading remains false. Add a recurring error gate test for `A -> A -> null -> A`, expecting `true, false, false, true`.

```ts
export interface RecurringErrorGate {
  shouldNotify(error: string | null): boolean;
}
```

- [ ] **Step 3: Run RED tests**

```bash
cd apps/tauri-gui/frontend
node --experimental-strip-types --test src/shell/startupWatchdog.test.ts src/shell/recurringErrorGate.test.ts
```

Expected: FAIL because the delivery controller, timeout predicate, and recurring error gate do not exist.

- [ ] **Step 4: Implement the pure controllers**

Implement a capped retry schedule of `250, 1000, 2000, 5000` milliseconds, reusing the final delay for later failures. Mark `acknowledged` only after the promise resolves. `deactivate()` clears the pending timer but keeps in-flight and acknowledged knowledge so React StrictMode cannot double-send. Implement `shouldClearLibraryTimeout` using resolved Library data/error only, and implement the recurring gate with one private `lastError` reset by `null`.

- [ ] **Step 5: Wire the React adapter**

Replace `createLibraryReadyGate()` usage with one delivery controller stored in a ref. Activate it when `libraryPaintActive` becomes true and return `deactivate` from the effect. Add a resolved-state effect that clears `initialRequestTimedOut`. Replace the raw `lastScanError` comparison with the recurring gate.

- [ ] **Step 6: Run focused GREEN checks**

```bash
cd apps/tauri-gui/frontend
npm run test:unit
npm run typecheck
npx playwright test --config e2e/playwright.config.ts smoke.spec.ts --grep "first painted|first Library request|scan"
```

Expected: all selected checks pass and no visual snapshot changes.

- [ ] **Step 7: Commit**

```bash
jj commit -m "fix(frontend): recover Library startup maintenance"
```

### Task 2: Recreate failed Library source watchers

**Files:**
- Modify: `apps/tauri-gui/src-tauri/src/library_scheduler.rs`

- [ ] **Step 1: Write failing watcher-registry tests**

Add a helper test that inserts two `WatchHandle` values, retires one source, and asserts the selected handle is removed, its stop flag is true, and the unrelated watcher remains live. Add an absent-ID case returning false.

```rust
fn retire_watcher(
    watchers: &mut BTreeMap<i64, WatchHandle>,
    source_id: i64,
) -> bool;
```

- [ ] **Step 2: Run RED**

```bash
cargo test -p wallpaper-console-tauri library_scheduler::tests::failed_watcher -- --nocapture
```

Expected: FAIL because the retirement lifecycle is absent.

- [ ] **Step 3: Implement retirement and worker termination**

On `WatchMessage::Failed(id)`, retire the handle before marking the source dirty and scheduling refresh. Log only `Library watcher failed for source {id}; scheduling recreation`. Return from the worker after initial recursive setup fails, closing the descriptor. When adding watches after an event fails, send `Failed(id)` and break.

- [ ] **Step 4: Run focused checks**

```bash
cargo fmt --all -- --check
cargo test -p wallpaper-console-tauri library_scheduler -- --nocapture
cargo clippy -p wallpaper-console-tauri -- -D warnings
```

- [ ] **Step 5: Commit**

```bash
jj commit -m "fix(gui): recreate failed Library watchers"
```

### Task 3: Align Grid and Flow display applicability

**Files:**
- Modify: `apps/tauri-gui/frontend/src/components/libraryViewModel.ts`
- Modify: `apps/tauri-gui/frontend/src/components/LibraryViewport.tsx`
- Modify: `apps/tauri-gui/frontend/src/components/WallpaperGrid.tsx`
- Modify: `apps/tauri-gui/frontend/src/components/WallpaperCard.tsx`
- Modify: `apps/tauri-gui/frontend/src/components/WallpaperFlow.tsx`
- Modify tests/fixtures that construct `LibraryViewModel`
- Modify: `apps/tauri-gui/frontend/e2e/smoke.spec.ts`

- [ ] **Step 1: Write failing tests**

Extend the model with `readonly displayApplyDisabledReason: string | null`. Add Playwright coverage that persists disconnected output `DP-9`, opens Grid, clicks a compatible card, and proves selection changes while no apply request is sent and feedback says `The selected display is unavailable.` Switch to Flow and assert the same reason.

- [ ] **Step 2: Run RED**

```bash
cd apps/tauri-gui/frontend
node --experimental-strip-types --test src/components/LibraryViewport.test.ts src/components/WallpaperCard.test.ts
npx playwright test --config e2e/playwright.config.ts smoke.spec.ts --grep "disconnected display"
```

Expected: FAIL because Grid ignores display availability and has no display-reason seam.

- [ ] **Step 3: Implement shared semantics**

Set `displayApplyDisabledReason` in `SinglePageShell` only when `displayModel.canApply` is false. Grid receives a stable never-applicable predicate in that state. Add `applyDisabledReason?: string | null` through `WallpaperGrid` to `WallpaperCard`, and prefer it for blocked feedback. Flow consumes the model reason instead of duplicating the string.

- [ ] **Step 4: Run focused checks**

```bash
cd apps/tauri-gui/frontend
npm run test:unit
npm run typecheck
npx playwright test --config e2e/playwright.config.ts smoke.spec.ts --grep "disconnected display|single/double-click|Flow"
```

- [ ] **Step 5: Commit**

```bash
jj commit -m "fix(frontend): align display apply availability"
```

### Task 4: Defer numeric settings commits until editing completes

**Files:**
- Create: `apps/tauri-gui/frontend/src/shell/deferredNumberInput.ts`
- Create: `apps/tauri-gui/frontend/src/shell/deferredNumberInput.test.ts`
- Modify: `apps/tauri-gui/frontend/src/shell/CompactSettingsPanel.tsx`
- Modify: `apps/tauri-gui/frontend/src/shell/CompactSettingsPanel.test.ts`
- Modify: `apps/tauri-gui/frontend/e2e/smoke.spec.ts`

- [ ] **Step 1: Write failing parser tests**

```ts
export function committedNumberDraft(raw: string, confirmed: number): number;
```

Whitespace/empty, `NaN`, and infinite drafts return `confirmed`; finite decimal and integer strings return their number. Range clamping remains owned by the existing settings normalizer.

- [ ] **Step 2: Write failing component/browser tests**

Call `onBlur` instead of `onChange`, proving an empty draft restores the confirmed value and performs no update. Add Enter-commit and Escape-reset cases. In Playwright, clear Transition FPS, type `144`, press Enter, and assert persistence receives `144` with no intermediate clamped write.

- [ ] **Step 3: Run RED**

```bash
cd apps/tauri-gui/frontend
node --experimental-strip-types --test src/shell/deferredNumberInput.test.ts src/shell/CompactSettingsPanel.test.ts
npx playwright test --config e2e/playwright.config.ts smoke.spec.ts --grep "numeric behavior"
```

- [ ] **Step 4: Implement deferred inputs**

Use `defaultValue` plus a key derived from the confirmed value. On blur, parse and write the committed value back to the DOM; update behavior only when it differs. Enter calls `blur()`. Escape prevents default, restores the confirmed string, and does not persist. Apply uniformly to transition duration, transition FPS, scene FPS, and scene volume.

- [ ] **Step 5: Run focused checks**

```bash
cd apps/tauri-gui/frontend
npm run test:unit
npm run typecheck
npx playwright test --config e2e/playwright.config.ts smoke.spec.ts --grep "numeric behavior|compact settings"
```

- [ ] **Step 6: Commit**

```bash
jj commit -m "fix(frontend): defer numeric setting commits"
```

### Task 5: Preserve diagnostic failures instead of reporting zero

**Files:**
- Modify: `apps/tauri-gui/src-tauri/src/commands/library.rs`
- Modify: `apps/tauri-gui/src-tauri/src/commands/settings.rs`

- [ ] **Step 1: Write failing Library status tests**

Add a configured-source fixture with a deliberately invalid required SQLite table. Assert `build_library_source_status` returns an error rather than `sqlite_rows == 0`. Prove missing `library.tsv` is zero but a directory at the TSV path is an error.

- [ ] **Step 2: Write failing diagnostics tests**

Corrupt sources/current/backend storage tables independently and assert `sources_status=error`, `current_status=error`, or `last_backend_status=error`. Assert output contains neither the temp directory nor raw SQLite errors nor a misleading successful value for that section.

- [ ] **Step 3: Run RED**

```bash
cargo test -p wallpaper-console-tauri commands::library::tests::build_library_source_status -- --nocapture
cargo test -p wallpaper-console-tauri commands::settings::tests::diagnostics_ -- --nocapture
```

- [ ] **Step 4: Implement the two contracts**

Map only `ErrorKind::NotFound` for `library.tsv` to zero. Propagate SQLite and other I/O failures from interactive status. In exported diagnostics, add `*_status=ok|error`, include safe values only on success, and never include raw error text.

- [ ] **Step 5: Run focused checks**

```bash
cargo fmt --all -- --check
cargo test -p wallpaper-console-tauri commands::library::tests -- --nocapture
cargo test -p wallpaper-console-tauri commands::settings::tests -- --nocapture
cargo clippy -p wallpaper-console-tauri -- -D warnings
```

- [ ] **Step 6: Commit**

```bash
jj commit -m "fix(gui): keep Library diagnostics truthful"
```

### Task 6: Make verification fail correctly and run performance once

**Files:**
- Modify: `scripts/test_tauri_before_commands.sh`
- Create: `scripts/test_tauri_before_commands_unit.sh`
- Modify: `apps/tauri-gui/frontend/package.json`
- Modify: `xtask/src/main.rs`

- [ ] **Step 1: Add failing regressions**

Make the main shell script sourceable by moving execution into `main()`. The new unit script sources it, resets `PASS`/`FAIL`, calls `test_root_detection` with `exit 17` inside an `if`, and fails unless the helper returns non-zero with `FAIL == 1`. Add xtask assertions that there is no dedicated Rust 10k step after workspace tests and exactly one frontend performance step.

- [ ] **Step 2: Run RED**

```bash
bash scripts/test_tauri_before_commands_unit.sh
cargo test -p xtask
cd apps/tauri-gui/frontend && npm run smoke -- --list
```

Expected: exit capture or matrix assertions fail; smoke currently lists `library-perf.spec.ts`.

- [ ] **Step 3: Implement real exit capture and de-duplication**

```bash
if output=$(cd "$cwd" && timeout "$timeout_sec" sh -c "$command" 2>&1); then
  rc=0
else
  rc=$?
fi
```

Set `smoke` to run only `smoke.spec.ts`. Remove the explicit Rust 10k step because `cargo test --workspace` executes that integration test. Add the shell unit script as a Drift verification step.

- [ ] **Step 4: Run focused checks**

```bash
bash scripts/test_tauri_before_commands_unit.sh
bash scripts/test_tauri_before_commands.sh
cargo test -p xtask
cargo run -p xtask -- verify all --dry-run
cd apps/tauri-gui/frontend && npm run smoke -- --list
```

- [ ] **Step 5: Commit**

```bash
jj commit -m "test: harden and streamline verification"
```

### Task 7: Final integration review and verification

**Files:**
- Review the complete range from design commit to the final task commit.

- [ ] **Step 1: Run final spec and quality reviews**

Check every acceptance criterion in `doc/specs/2026-07-20-autonomous-reliability-optimization-design.md` against the actual diff. Fix every open P0/P1/P2 finding and re-review.

- [ ] **Step 2: Run the fresh repository gate**

```bash
cargo run -p xtask -- verify all
cargo build --workspace
git diff --check
```

Expected: exit code 0. The smoke count is lower only because performance specs run in the dedicated performance step instead of twice.

- [ ] **Step 3: Inspect history and working copy**

```bash
jj status
jj log -r '6ca60b61::@' --no-graph
```

Expected: an empty working-copy commit above clear task commits and no untracked or modified project files.

- [ ] **Step 4: Move and push the existing bookmark**

```bash
jj bookmark set codex/simple-wallpaper-console -r @-
jj git fetch --remote origin
jj git push --remote origin --bookmark codex/simple-wallpaper-console
```

Expected: a forward-only push. Fetch again and confirm local, Git, and origin bookmarks resolve to the same final commit.
