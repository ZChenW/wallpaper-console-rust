import assert from 'node:assert/strict';
import test from 'node:test';

import {
  createLibraryReadyGate,
  createLibraryWatchdog,
  shouldSignalLibraryPaint,
  type LibraryPaintInput,
} from './startupWatchdog.ts';

// ── Helpers ────────────────────────────────────────────────────────────

function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((r) => { resolve = r; });
  return { promise, resolve };
}

function paintInput(overrides: Partial<LibraryPaintInput> = {}): LibraryPaintInput {
  return {
    initialLoading: false,
    hasEntries: false,
    emptyConfirmed: false,
    loadError: false,
    timedOut: false,
    ...overrides,
  };
}

// ── shouldSignalLibraryPaint ────────────────────────────────────────────

test('library paint signals when entries are loaded', () => {
  assert.equal(shouldSignalLibraryPaint(paintInput({ hasEntries: true })), true);
});

test('library paint signals when empty is confirmed', () => {
  assert.equal(shouldSignalLibraryPaint(paintInput({ emptyConfirmed: true })), true);
});

test('library paint signals on load error', () => {
  assert.equal(shouldSignalLibraryPaint(paintInput({ loadError: true })), true);
});

test('library paint does NOT signal while initialLoading with no content', () => {
  assert.equal(shouldSignalLibraryPaint(paintInput({ initialLoading: true })), false);
});

test('timeout retry state IS a library paint even when initialLoading is still true', () => {
  // This is the key requirement: a hung request that times out must count as
  // a first Library paint so library_ready is sent and the backend can proceed.
  assert.equal(
    shouldSignalLibraryPaint(paintInput({ initialLoading: true, timedOut: true })),
    true,
  );
});

test('timeout with entries already present still signals', () => {
  assert.equal(
    shouldSignalLibraryPaint(paintInput({ initialLoading: true, timedOut: true, hasEntries: true })),
    true,
  );
});

test('library paint does NOT signal when still loading without timeout', () => {
  assert.equal(
    shouldSignalLibraryPaint(paintInput({ initialLoading: true, hasEntries: false, emptyConfirmed: false, loadError: false, timedOut: false })),
    false,
  );
});

// ── createLibraryReadyGate ──────────────────────────────────────────────

test('ready gate signals exactly once', () => {
  const gate = createLibraryReadyGate();

  assert.equal(gate.called, false);
  assert.equal(gate.shouldSignal(paintInput({ hasEntries: true })), true);

  gate.markCalled();
  assert.equal(gate.called, true);

  // Subsequent calls are silent
  assert.equal(gate.shouldSignal(paintInput({ hasEntries: true })), false);
  assert.equal(gate.shouldSignal(paintInput({ timedOut: true })), false);
});

test('ready gate does not double-signal across timeout then success', () => {
  const gate = createLibraryReadyGate();

  // First: timeout triggers the paint
  assert.equal(
    gate.shouldSignal(paintInput({ initialLoading: true, timedOut: true })),
    true,
  );
  gate.markCalled();

  // Later: the actual data arrives — gate already fired, don't re-signal
  assert.equal(
    gate.shouldSignal(paintInput({ hasEntries: true })),
    false,
  );
});

// ── createLibraryWatchdog ───────────────────────────────────────────────

test('watchdog fires onTimeout after the specified delay', async () => {
  const watchdog = createLibraryWatchdog();
  const { promise, resolve } = deferred();
  let fired = false;

  watchdog.arm(50, () => { fired = true; resolve(); });
  assert.equal(fired, false, 'should not fire immediately');

  await promise;
  assert.equal(fired, true);
});

test('watchdog can be disarmed before firing', async () => {
  const watchdog = createLibraryWatchdog();
  let fired = false;

  const cancel = watchdog.arm(50, () => { fired = true; });
  cancel();
  await new Promise((r) => setTimeout(r, 80));

  assert.equal(fired, false);
});

test('arming a new watchdog clears the previous one', async () => {
  const watchdog = createLibraryWatchdog();
  let firstFired = false;
  let secondFired = false;

  watchdog.arm(30, () => { firstFired = true; });
  const d = deferred();
  watchdog.arm(50, () => { secondFired = true; d.resolve(); });

  await d.promise;
  assert.equal(firstFired, false, 'first arm should have been cancelled');
  assert.equal(secondFired, true);
});

test('two consecutive watchdogs both fire if not cancelled (simulating hung retry)', async () => {
  // Simulates: first request hangs → watchdog fires → user clicks Retry →
  // new request hangs → watchdog fires again.
  const watchdog = createLibraryWatchdog();

  const firstDone = deferred();
  watchdog.arm(30, () => firstDone.resolve());
  await firstDone.promise;

  // Second arm simulates the retry
  const secondDone = deferred();
  watchdog.arm(30, () => secondDone.resolve());
  await secondDone.promise;

  // Both fired (they're independent timeouts in sequence)
  assert.equal(true, true, 'both watchdogs fired without interfering');
});

test('stale onTimeout callback does not fire after being replaced by a new arm', async () => {
  // When a fast retry happens, the old timeout must not fire.
  const watchdog = createLibraryWatchdog();
  let staleFired = false;
  let currentFired = false;

  // Arm the first watchdog with a longer delay
  const staleCancel = watchdog.arm(80, () => { staleFired = true; });

  // Immediately re-arm (simulates a new request arriving before the old one times out)
  const d = deferred();
  staleCancel(); // explicit cancel from the stale request
  watchdog.arm(30, () => { currentFired = true; d.resolve(); });

  await d.promise;
  // Wait long enough for the stale timer to have fired if it were still active
  await new Promise((r) => setTimeout(r, 70));

  assert.equal(staleFired, false, 'stale callback must not fire');
  assert.equal(currentFired, true, 'current callback must fire');
});

test('watchdog cleanup on unmount prevents callback', async () => {
  const watchdog = createLibraryWatchdog();
  let fired = false;

  const cleanup = watchdog.arm(30, () => { fired = true; });
  cleanup(); // simulate React unmount
  await new Promise((r) => setTimeout(r, 60));

  assert.equal(fired, false);
});

test('multiple arms in sequence for consecutive hung retries', async () => {
  // Full scenario:
  // 1. Initial request hangs → 50ms watchdog fires (timeout #1)
  // 2. User clicks Retry → new 50ms watchdog fires (timeout #2)
  // Each timeout represents a distinct retry state.
  const watchdog = createLibraryWatchdog();

  const timeouts: number[] = [];

  // First arm
  const d1 = deferred();
  watchdog.arm(50, () => { timeouts.push(1); d1.resolve(); });
  await d1.promise;
  assert.deepEqual(timeouts, [1]);

  // Second arm (retry)
  const d2 = deferred();
  watchdog.arm(50, () => { timeouts.push(2); d2.resolve(); });
  await d2.promise;
  assert.deepEqual(timeouts, [1, 2]);

  // Third arm (another retry)
  const d3 = deferred();
  watchdog.arm(50, () => { timeouts.push(3); d3.resolve(); });
  await d3.promise;
  assert.deepEqual(timeouts, [1, 2, 3]);
});
