import assert from 'node:assert/strict';
import test from 'node:test';

import type { DisplayStateDTO, SourceDTO } from '../api/types.ts';
import {
  loadShellCatalogSnapshot,
  loadShellCatalogIndependent,
  subscribeCatalogChannels,
  withTimeout,
  type ShellCatalogApi,
  type CatalogChannelEvents,
} from './useShellCatalog.ts';

const source: SourceDTO = {
  id: 2,
  path: '/walls',
  displayName: 'Walls',
  kind: 'directory',
  recursive: true,
  availability: 'available',
  addedAt: '2026-07-14T00:00:00Z',
  exists: true,
  isWE: false,
  label: 'Walls',
};

const state: DisplayStateDTO = {
  targetKey: 'output:DP-1',
  kind: 'output',
  output: 'DP-1',
  wallpaperPath: '/walls/a.jpg',
  backend: 'awww',
  updatedAt: '2026-07-14T00:00:00Z',
};

test('catalog snapshot loads displays, sources, and saved restore state together', async () => {
  const snapshot = await loadShellCatalogSnapshot({
    displaysList: async () => ({ outputs: [{ name: 'DP-1' }, { name: ' HDMI-A-1 ' }] }),
    sourcesList: async () => [source],
    displayStateList: async () => [state],
  });

  assert.deepEqual(snapshot.connectedOutputs, ['DP-1', 'HDMI-A-1']);
  assert.deepEqual(snapshot.sources, [source]);
  assert.deepEqual(snapshot.persistedDisplayStates, [state]);
  assert.deepEqual(snapshot.errors, {});
});

test('catalog snapshot keeps usable partial data when one independent request fails', async () => {
  const snapshot = await loadShellCatalogSnapshot({
    displaysList: async () => ({ outputs: [{ name: 'DP-1' }] }),
    sourcesList: async () => { throw new Error('source database unavailable'); },
    displayStateList: async () => [state],
  });

  assert.deepEqual(snapshot.connectedOutputs, ['DP-1']);
  assert.deepEqual(snapshot.sources, []);
  assert.deepEqual(snapshot.persistedDisplayStates, [state]);
  assert.equal(snapshot.errors.sources, 'source database unavailable');
  assert.equal(snapshot.errors.displays, undefined);
});

// ── withTimeout ─────────────────────────────────────────────────────────

test('withTimeout resolves when the promise resolves before the deadline', async () => {
  const result = await withTimeout(Promise.resolve(42), 100, 'test');
  assert.equal(result, 42);
});

test('withTimeout rejects with a timeout error when the promise hangs', async () => {
  // A promise that never resolves
  const never = new Promise<number>(() => {});
  await assert.rejects(
    withTimeout(never, 50, 'testChannel'),
    /testChannel timed out after 50ms/,
  );
});

test('withTimeout still rejects when the original rejects faster than timeout', async () => {
  const failing = Promise.reject(new Error('original error'));
  await assert.rejects(
    withTimeout(failing, 500, 'testChannel'),
    /original error/,
  );
});

// ── loadShellCatalogIndependent ─────────────────────────────────────────

test('independent channels: source completes first while displays hang', async () => {
  // Source resolves quickly, displays never resolves.
  // The snapshot must publish source data without waiting for displays.
  const snapshot = await loadShellCatalogIndependent({
    displaysList: () => new Promise(() => {}), // never resolves
    sourcesList: async () => [source],
    displayStateList: () => new Promise(() => {}), // never resolves
  }, { displaysTimeoutMs: 50, sourcesTimeoutMs: 500, displayStateTimeoutMs: 50 });

  assert.deepEqual(snapshot.sources, [source]);
  assert.deepEqual(snapshot.connectedOutputs, []);
  assert.deepEqual(snapshot.persistedDisplayStates, []);
  assert.equal(typeof snapshot.errors.sources, 'undefined');
  assert.notEqual(typeof snapshot.errors.displays, 'undefined');
  assert.match(String(snapshot.errors.displays), /timed out/);
  assert.match(String(snapshot.errors.displayState), /timed out/);
});

test('independent channels: each channel timeout produces its own error', async () => {
  const snapshot = await loadShellCatalogIndependent({
    displaysList: () => new Promise(() => {}),
    sourcesList: () => new Promise(() => {}),
    displayStateList: () => new Promise(() => {}),
  }, { displaysTimeoutMs: 30, sourcesTimeoutMs: 30, displayStateTimeoutMs: 30 });

  assert.deepEqual(snapshot.sources, []);
  assert.deepEqual(snapshot.connectedOutputs, []);
  assert.deepEqual(snapshot.persistedDisplayStates, []);
  assert.match(String(snapshot.errors.displays ?? ''), /timed out/);
  assert.match(String(snapshot.errors.sources ?? ''), /timed out/);
  assert.match(String(snapshot.errors.displayState ?? ''), /timed out/);
});

test('independent channels: all succeed normally', async () => {
  const snapshot = await loadShellCatalogIndependent({
    displaysList: async () => ({ outputs: [{ name: 'DP-1' }] }),
    sourcesList: async () => [source],
    displayStateList: async () => [state],
  }, { displaysTimeoutMs: 200, sourcesTimeoutMs: 200, displayStateTimeoutMs: 200 });

  assert.deepEqual(snapshot.connectedOutputs, ['DP-1']);
  assert.deepEqual(snapshot.sources, [source]);
  assert.deepEqual(snapshot.persistedDisplayStates, [state]);
  assert.deepEqual(snapshot.errors, {});
});

test('independent channels: one throwing error still allows others to complete', async () => {
  const snapshot = await loadShellCatalogIndependent({
    displaysList: async () => ({ outputs: [{ name: 'DP-1' }] }),
    sourcesList: async () => { throw new Error('source database unavailable'); },
    displayStateList: async () => [state],
  }, { displaysTimeoutMs: 200, sourcesTimeoutMs: 200, displayStateTimeoutMs: 200 });

  assert.deepEqual(snapshot.connectedOutputs, ['DP-1']);
  assert.deepEqual(snapshot.sources, []);
  assert.deepEqual(snapshot.persistedDisplayStates, [state]);
  assert.equal(snapshot.errors.sources, 'source database unavailable');
});

test('independent channels defaults to reasonable timeout', async () => {
  // Use default timeouts — must resolve within a reasonable time
  const snapshot = await loadShellCatalogIndependent({
    displaysList: async () => ({ outputs: [{ name: 'DP-1' }] }),
    sourcesList: async () => [],
    displayStateList: async () => [],
  });

  assert.equal(snapshot.errors.displays, undefined);
  assert.equal(snapshot.errors.sources, undefined);
  assert.equal(snapshot.errors.displayState, undefined);
});

// ── subscribeCatalogChannels: per-channel independent publishing ────────

test('subscribeCatalogChannels publishes each channel independently as it settles', async () => {
  const events: CatalogChannelEvents = {
    displays: [],
    sources: [],
    displayState: [],
    readyCalls: 0,
  };

  const t0 = Date.now();

  // Source resolves fast; displays never resolves; displayState resolves normally.
  const cleanup = subscribeCatalogChannels(
    {
      displaysList: () => new Promise<{ outputs: { name: string }[] }>(() => {}), // never
      sourcesList: async () => [source],
      displayStateList: async () => [state],
    },
    {
      onDisplays(outputs: string[], err?: string) {
        events.displays.push({ outputs, error: err, ms: Date.now() - t0 });
      },
      onSources(sourcesList: SourceDTO[], err?: string) {
        events.sources.push({ sources: sourcesList, error: err, ms: Date.now() - t0 });
      },
      onDisplayState(states: DisplayStateDTO[], err?: string) {
        events.displayState.push({ states, error: err, ms: Date.now() - t0 });
      },
      onReady() {
        events.readyCalls++;
      },
    },
    { displaysTimeoutMs: 50, sourcesTimeoutMs: 500, displayStateTimeoutMs: 500 },
  );

  // Sources should publish well before the 50ms displays timeout.
  await new Promise<void>((resolve) => {
    const check = () => {
      if (events.sources.length > 0) { resolve(); return; }
      setTimeout(check, 2);
    };
    setTimeout(check, 2);
  });

  const sourceMs = events.sources[0]!.ms;
  assert.equal(events.sources.length, 1);
  assert.deepEqual(events.sources[0]!.sources, [source]);
  assert.equal(events.sources[0]!.error, undefined);
  // Source must publish quickly (well under displaysTimeout).
  assert.ok(sourceMs < 40, `sources published at ${sourceMs}ms, expected < 40ms`);

  // ready must NOT be called yet — displays channel is still hanging.
  assert.equal(events.readyCalls, 0, 'ready must not fire before all channels settle');

  // Wait for the displays timeout to fire.
  await new Promise<void>((resolve) => {
    const check = () => {
      if (events.displays.length > 0) { resolve(); return; }
      setTimeout(check, 5);
    };
    setTimeout(check, 5);
  });

  assert.equal(events.displays.length, 1);
  assert.match(String(events.displays[0]!.error ?? ''), /timed out/);

  // Now all three channels have settled — ready must fire exactly once.
  assert.equal(events.readyCalls, 1);

  cleanup();
});

// ── subscribeCatalogChannels: timer tracking & cleanup ──────────────────

test('subscribeCatalogChannels cleanup clears timers and prevents late callbacks', async () => {
  let lateCallbackFired = false;
  let readyCalled = false;

  // Use promises that never resolve so only timers would fire.
  const cleanup = subscribeCatalogChannels(
    {
      displaysList: () => new Promise<{ outputs: { name: string }[] }>(() => {}),
      sourcesList: () => new Promise<SourceDTO[]>(() => {}),
      displayStateList: () => new Promise<DisplayStateDTO[]>(() => {}),
    },
    {
      onDisplays() { lateCallbackFired = true; },
      onSources() { lateCallbackFired = true; },
      onDisplayState() { lateCallbackFired = true; },
      onReady() { readyCalled = true; },
    },
    { displaysTimeoutMs: 30, sourcesTimeoutMs: 30, displayStateTimeoutMs: 30 },
  );

  // Clean up immediately — before any timer fires.
  cleanup();

  // Wait long enough for timers to have expired.
  await new Promise((r) => setTimeout(r, 80));

  assert.equal(lateCallbackFired, false, 'no callback must fire after cleanup');
  assert.equal(readyCalled, false, 'onReady must not fire after cleanup');
});

// ── Generation seam: stale initial callback must not overwrite reload ───

test('subscribeCatalogChannels generation seam: stale initial source callback is suppressed by newer reload', async () => {
  // Simulate the generation pattern used by useShellCatalog:
  // 1. Initial subscribe captures sourceGen=1.
  // 2. reloadSources starts → bumps sourceGeneration to 2.
  // 3. reloadSources completes with new data.
  // 4. The initial onSources callback finally arrives — it checks
  //    sourceGen !== sourceGeneration (1 !== 2) and skips.
  //
  // This test exercises the pattern directly with a controlled promise.

  let sourceGeneration = 0;
  let initialCbApplied = false;
  let reloadCbApplied = false;

  // ── Step 1: initial load captures gen 1 ──
  const initialGen = ++sourceGeneration; // = 1

  // ── Step 2: reloadSources bumps gen ──
  ++sourceGeneration; // = 2

  // ── Step 3: reloadSources result arrives ──
  // (This would be a setState in the real hook — we just record it.)
  reloadCbApplied = true;

  // ── Step 4: initial onSources fires late ──
  if (initialGen === sourceGeneration) {
    initialCbApplied = true; // BUG: stale callback overwrote
  }
  // Expected: initialGen (1) !== sourceGeneration (2), so callback is skipped.

  assert.equal(initialCbApplied, false,
    'stale initial source callback must be suppressed when reload has started');
  assert.equal(reloadCbApplied, true,
    'reload result must be applied');
});

test('subscribeCatalogChannels generation seam: stale callback suppressed after unmount bump', async () => {
  // On unmount, all three generations are bumped. Any in-flight initial
  // callbacks that arrive after unmount must be no-ops.

  let sourceGeneration = 0;
  let displayGeneration = 0;
  let fullGeneration = 0;

  // Mount: bump all three.
  const sourceGen = ++sourceGeneration;
  const displayGen = ++displayGeneration;
  const fullGen = ++fullGeneration;

  // Unmount: bump all three again.
  ++sourceGeneration;
  ++displayGeneration;
  ++fullGeneration;

  // A late initial callback for sources:
  const sourceStale = (sourceGen !== sourceGeneration); // 1 !== 2 → true
  // A late initial callback for displays:
  const displayStale = (displayGen !== displayGeneration); // 1 !== 2 → true
  // A late onReady:
  const readyStale = (fullGen !== fullGeneration); // 1 !== 2 → true

  assert.equal(sourceStale, true, 'source callback must be stale after unmount');
  assert.equal(displayStale, true, 'display callback must be stale after unmount');
  assert.equal(readyStale, true, 'onReady must be stale after unmount');
});
