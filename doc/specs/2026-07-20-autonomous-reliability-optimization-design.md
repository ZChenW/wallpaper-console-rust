# Autonomous Reliability Optimization Design

## Purpose

This optimization pass improves reliability, user feedback, diagnosability, and
developer verification without redesigning the accepted Grid, Flow, Editorial,
or settings visuals. The repository is healthy at baseline, so each change must
fix a demonstrated failure mode and preserve the existing interfaces wherever
possible.

The user explicitly authorized autonomous design approval and continuous
execution. This document is therefore the reviewable design gate for the pass;
implementation still proceeds through tests, per-task review, and reversible
`jj` commits.

## Audit result and priority

No P0 issue was found. The selected work is ordered by impact, then by risk and
cost:

1. **P1 — reliable `library_ready` delivery.** A rejected Tauri invocation is
   currently marked complete before acknowledgement. That permanently prevents
   the backend Library observer, scheduler, and FTS builder from starting.
2. **P1 — self-healing Library source watchers.** A failed inotify worker stays
   represented in the watcher registry, so an unchanged source configuration is
   never watched again.
3. **P2 — consistent display applicability in Grid and Flow.** Flow prevents an
   apply to a disconnected display before activation, while Grid currently lets
   the user attempt it and rejects only later.
4. **P2 — editable numeric behavior settings.** Controlled number inputs commit
   every intermediate string; clearing a field becomes `0` and is immediately
   clamped, which makes ordinary replacement typing unreliable.
5. **P2 — truthful Library diagnostics.** SQLite and non-missing TSV I/O errors
   are currently converted to zero counts, producing an incorrect “empty index”
   diagnosis.
6. **P2 — trustworthy and non-duplicated verification.** A shell test loses the
   real exit code through `|| true`, while both Rust and Playwright performance
   gates run twice during `verify all`.

Two small shell-state defects share the first task's ownership and are included
there: a recovered scan error must be reportable again if it recurs, and a
startup timeout flag must clear after a resolved Library state.

## Considered approaches

### A. Broad structural rewrite

Split the largest Rust and React files, replace storage interfaces, and redesign
startup orchestration in one pass. This could improve long-term organization,
but its regression surface is much larger than the evidence justifies. It also
mixes behavior changes with file movement, making review and rollback harder.

### B. Evidence-backed reliability slices (selected)

Implement six independent tracer bullets. Each places a small interface at the
actual failure seam, proves the old failure with a test, and changes only the
minimum production path. This gives high user and maintainer value while keeping
every commit independently reversible.

### C. Tests and documentation only

Record the findings and strengthen gates without changing runtime behavior. This
has the lowest immediate risk but leaves known session-long failure modes in
production, so it does not satisfy the request to implement meaningful
optimizations.

## Architecture

### 1. Frontend shell reliability controller

`startupWatchdog.ts` will own a deep `library_ready` delivery module rather than
exposing a boolean that React must mutate in the correct order. Its interface is
small: activate delivery after first paint and dispose it on unmount. Internally
it guarantees one in-flight request, retries rejected requests with capped
backoff, marks success only after acknowledgement, and cancels pending timers on
dispose.

`SinglePageShell` remains the adapter: it supplies `api.libraryReady`, activates
the controller when the existing first-paint predicate becomes true, and does
not add render state for retries. The backend command remains idempotent.

A separate pure recurring-error gate will distinguish a continuous duplicate
from a new occurrence after recovery. The startup timeout flag will clear only
when the browser reaches a resolved state (entries, confirmed empty, or error),
not merely when another transient request begins.

### 2. Watcher lifecycle seam

`library_scheduler.rs` will make watcher retirement explicit. Receiving
`WatchMessage::Failed(source_id)` will stop and remove the matching handle before
marking the source dirty. The existing two-second catalog reconciliation then
recreates the watcher from the unchanged source signature. A pure helper around
the registry operation will make this lifecycle behavior testable without real
inotify.

Initial recursive-watch setup failure must terminate the worker after sending
`Failed`; it must not continue with an unusable descriptor. Dynamic recursive
watch expansion errors will also emit `Failed` and terminate. Warnings must not
include wallpaper paths.

### 3. Shared apply applicability

`LibraryViewModel` will expose the display-unavailable reason alongside
`canApplyToDisplay`. Both adapters will combine display availability with the
existing per-entry applicability. Grid cards still permit selection and details,
but do not dispatch apply, and their warning uses the display reason rather than
claiming the wallpaper itself is unsupported.

No card geometry, theme CSS, animation, scroll behavior, media lifecycle, or
selection semantics changes.

### 4. Deferred numeric settings

Numeric settings will use uncontrolled input drafts (`defaultValue`) and commit
only on blur or Enter. Empty or syntactically invalid drafts restore the last
confirmed value; valid drafts flow through the existing normalization and
persistence module, which remains the single owner of range clamping.

A small pure parser/commit helper hides number-input edge cases from the settings
view. Escape restores the confirmed value without persisting. The rendered
control structure and CSS remain unchanged.

### 5. Truthful diagnostics

`build_library_source_status` will treat a missing legacy TSV as zero rows, but
propagate every other TSV I/O error and every SQLite count error. It must never
turn a failed count into an apparently valid empty count.

The exported diagnostics file must stay available even when individual reads
fail. Each fallible section will emit an explicit privacy-safe status such as
`sources_status=error` while omitting raw paths and operating-system messages.
Successful sections continue to emit their current values. This separates the
interactive status command's fail-fast contract from the diagnostic bundle's
best-effort contract.

### 6. Verification integrity

The before-command shell test will capture the command's real exit status inside
an `if` branch that is compatible with `set -e`. A lightweight injected command
case will prove that an otherwise-unrecognized non-zero result fails.

`npm run smoke` will explicitly run `smoke.spec.ts`; `perf:library` remains the
sole Playwright performance entry point. `cargo test --workspace` remains the
sole Rust execution of the 10k integration test, so the redundant dedicated
xtask step is removed. Unit tests will lock the resulting verification matrix.

## Error handling and observability

- Library readiness retries are internal and non-modal; repeated transient IPC
  errors do not create notification spam.
- Watcher failures mark the source dirty so current contents are reconciled even
  before the watcher is recreated.
- User-facing apply feedback explains a disconnected display and never labels a
  compatible wallpaper as unsupported.
- Diagnostic status errors remain structured at the command seam; exported
  diagnostics report error categories without sensitive filesystem content.
- Verification commands must fail on real child-command failures.

## Testing strategy

Every implementation task follows red-green-refactor:

- Node unit tests for readiness retry/disposal, recurring error reset, resolved
  timeout cleanup, display applicability, and numeric draft commit behavior.
- Playwright coverage for disconnected-display Grid behavior and numeric input
  replacement, without visual snapshot changes.
- Rust unit tests for watcher retirement/recreation eligibility and truthful
  Library status/diagnostic output under corrupted or unreadable fixtures.
- Shell/xtask tests for exit-code propagation and a non-duplicated verification
  matrix.
- Per-task focused checks, then the repository gate:
  `cargo run -p xtask -- verify all`, `cargo build --workspace`, and
  `git diff --check`.

## Non-goals

- No redesign of Grid, Flow, Editorial, Liquid Glass, settings layout, or CSS.
- No broad split of large files solely to reduce line counts.
- No storage facade migration, runtime-state persistence redesign, or checked
  backend-stop rewrite in this pass; those need separate behavioral designs.
- No change to renderer compatibility claims or Wallpaper Engine parity.
- No PR creation or merge; the existing `codex/simple-wallpaper-console`
  bookmark is updated and pushed after final verification.

## Acceptance criteria

- A failed `library_ready` invocation retries and a later acknowledgement starts
  backend maintenance; success is never sent twice.
- A failed watcher is removed and becomes eligible for automatic recreation.
- Grid and Flow both prevent apply to a disconnected display while preserving
  selection/details and showing the correct reason.
- All four numeric behavior fields can be cleared and replaced naturally, and
  only committed values are persisted.
- Library status and exported diagnostics distinguish real read failures from
  a valid zero-row Library without leaking sensitive paths.
- Verification rejects unknown non-zero child exits and runs each performance
  gate once.
- All focused and full repository checks pass, reviews have no unresolved
  P0/P1/P2 findings, `jj status` is clean, and the final bookmark matches GitHub.
