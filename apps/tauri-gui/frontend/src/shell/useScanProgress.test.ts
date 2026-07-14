import assert from 'node:assert/strict';
import test from 'node:test';

import type { ScanProgressSnapshot } from './scanProgressController.ts';
import { EMPTY_SCAN_STATE, scanReducer } from './feedbackState.ts';

test('scan hook adapter exposes ScanActivity presentation from the observed backend snapshot', async () => {
  const module = await import('./useScanProgress.ts').catch(() => null);
  assert.ok(module, 'expected the single-page scan hook module');
  assert.equal(typeof module.toScanProgressView, 'function');
  assert.equal(typeof module.useScanProgress, 'function');

  const scanState = scanReducer(EMPTY_SCAN_STATE, { type: 'started', nowMs: 1_000 });
  const snapshot: ScanProgressSnapshot = {
    progress: null,
    scanState,
    observedAtMs: 1_500,
    transportError: null,
    pollingMode: 'active',
  };

  assert.deepEqual(module.toScanProgressView(snapshot), {
    progress: null,
    scanState,
    presentation: {
      kind: 'running',
      nonModal: true,
      canCancel: true,
      elapsedMs: 500,
    },
    scanError: null,
    transportError: null,
    pollingMode: 'active',
  });
});

test('scan hook adapter keeps backend failures separate from recoverable bridge failures', async () => {
  const { toScanProgressView } = await import('./useScanProgress.ts');
  const snapshot: ScanProgressSnapshot = {
    progress: {
      running: false,
      stage: 'walking files',
      scanned: 12,
      reusedMetadata: 8,
      probedMetadata: 4,
      insertedSqlite: 11,
      staged: 0,
      skipped: 1,
      metadataErrors: 0,
      cancelRequested: false,
      error: 'source went offline',
    },
    scanState: EMPTY_SCAN_STATE,
    observedAtMs: 2_000,
    transportError: 'progress bridge reconnected',
    pollingMode: 'idle',
  };

  const view = toScanProgressView(snapshot);
  assert.equal(view.scanError, 'source went offline');
  assert.equal(view.transportError, 'progress bridge reconnected');
  assert.deepEqual(view.presentation, { kind: 'hidden' });
});
