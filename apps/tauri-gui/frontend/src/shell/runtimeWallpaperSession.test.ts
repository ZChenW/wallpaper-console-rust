import assert from 'node:assert/strict';
import test from 'node:test';

import {
  createRuntimeWallpaperSession,
  reduceRuntimeWallpaperSession,
  toRuntimeDisplayWallpapers,
} from './runtimeWallpaperSession.ts';

const result = (
  statePath: string,
  requestId = 'request-1',
  appliedOutputs: string[] = ['eDP-1', 'HDMI-A-1'],
) => ({
  requestId,
  appliedPath: '/resolved/applied.jpg',
  statePath,
  backend: 'awww',
  fileType: 'image',
  preview: false,
  appliedOutputs,
});

test('successful target-omitted apply confirms its statePath on every connected output', () => {
  const initial = createRuntimeWallpaperSession([' eDP-1 ', 'HDMI-A-1', 'eDP-1']);
  const next = reduceRuntimeWallpaperSession(initial, {
    type: 'applySucceeded',
    transport: 'targeted',
    request: { path: '/requested.jpg', requestId: 'request-1' },
    result: result('/resolved/state.jpg'),
  });

  assert.deepEqual(toRuntimeDisplayWallpapers(next), [
    { output: 'eDP-1', wallpaperPath: '/resolved/state.jpg', status: 'confirmed' },
    { output: 'HDMI-A-1', wallpaperPath: '/resolved/state.jpg', status: 'confirmed' },
  ]);
});

test('successful named apply confirms only that connected output and preserves other evidence', () => {
  const initial = createRuntimeWallpaperSession(['eDP-1', 'HDMI-A-1']);
  const shared = reduceRuntimeWallpaperSession(initial, {
    type: 'applySucceeded',
    transport: 'targeted',
    request: { path: '/shared.jpg', requestId: 'all' },
    result: result('/shared.jpg', 'all'),
  });
  const next = reduceRuntimeWallpaperSession(shared, {
    type: 'applySucceeded',
    transport: 'targeted',
    request: { path: '/monitor.jpg', target: ' HDMI-A-1 ', requestId: 'monitor' },
    result: result('/resolved/monitor.jpg', 'monitor', ['HDMI-A-1']),
  });

  assert.deepEqual(toRuntimeDisplayWallpapers(next), [
    { output: 'eDP-1', wallpaperPath: '/shared.jpg', status: 'confirmed' },
    { output: 'HDMI-A-1', wallpaperPath: '/resolved/monitor.jpg', status: 'confirmed' },
  ]);
});

test('legacy apply success cannot create targeted runtime confirmation', () => {
  const initial = createRuntimeWallpaperSession(['eDP-1']);
  const next = reduceRuntimeWallpaperSession(initial, {
    type: 'applySucceeded',
    transport: 'action',
    request: { kind: 'apply', path: '/legacy.jpg', requestId: 'legacy' },
    result: result('/legacy.jpg', 'legacy'),
  });

  assert.equal(next, initial);
  assert.deepEqual(toRuntimeDisplayWallpapers(next), [
    { output: 'eDP-1', wallpaperPath: null, status: 'unknown' },
  ]);
});

test('missing or malformed apply result cannot create runtime confirmation', () => {
  const initial = createRuntimeWallpaperSession(['eDP-1']);
  const request = { path: '/requested.jpg', requestId: 'targeted' };
  const missing = reduceRuntimeWallpaperSession(initial, {
    type: 'applySucceeded',
    transport: 'targeted',
    request,
    result: undefined,
  });
  const malformed = reduceRuntimeWallpaperSession(initial, {
    type: 'applySucceeded',
    transport: 'targeted',
    request,
    result: { ...result('/valid.jpg', 'targeted'), statePath: '   ' },
  });

  assert.equal(missing, initial);
  assert.equal(malformed, initial);
});

test('blank, unknown, or disconnected named target cannot create confirmation', () => {
  const initial = createRuntimeWallpaperSession(['eDP-1']);
  const blank = reduceRuntimeWallpaperSession(initial, {
    type: 'applySucceeded',
    transport: 'targeted',
    request: { path: '/requested.jpg', target: '   ', requestId: 'blank' },
    result: result('/blank.jpg', 'blank'),
  });
  const disconnected = reduceRuntimeWallpaperSession(initial, {
    type: 'applySucceeded',
    transport: 'targeted',
    request: { path: '/requested.jpg', target: 'DP-9', requestId: 'disconnected' },
    result: result('/disconnected.jpg', 'disconnected'),
  });

  assert.equal(blank, initial);
  assert.equal(disconnected, initial);
});

test('disconnect drops evidence and reconnect cannot revive an old confirmation', () => {
  const initial = createRuntimeWallpaperSession(['eDP-1', 'HDMI-A-1']);
  const confirmed = reduceRuntimeWallpaperSession(initial, {
    type: 'applySucceeded',
    transport: 'targeted',
    request: { path: '/shared.jpg', requestId: 'shared' },
    result: result('/shared.jpg', 'shared'),
  });
  const disconnected = reduceRuntimeWallpaperSession(confirmed, {
    type: 'connectedOutputsChanged',
    connectedOutputs: ['eDP-1'],
  });
  const reconnected = reduceRuntimeWallpaperSession(disconnected, {
    type: 'connectedOutputsChanged',
    connectedOutputs: [' eDP-1 ', 'HDMI-A-1', 'HDMI-A-1'],
  });

  assert.deepEqual(toRuntimeDisplayWallpapers(disconnected), [
    { output: 'eDP-1', wallpaperPath: '/shared.jpg', status: 'confirmed' },
  ]);
  assert.deepEqual(toRuntimeDisplayWallpapers(reconnected), [
    { output: 'eDP-1', wallpaperPath: '/shared.jpg', status: 'confirmed' },
    { output: 'HDMI-A-1', wallpaperPath: null, status: 'unknown' },
  ]);
});

test('All Displays success confirms only outputs actually applied before a hotplug change', () => {
  const initial = createRuntimeWallpaperSession(['eDP-1', 'HDMI-A-1']);
  const afterHotplug = reduceRuntimeWallpaperSession(initial, {
    type: 'connectedOutputsChanged',
    connectedOutputs: ['eDP-1', 'DP-2'],
  });
  const next = reduceRuntimeWallpaperSession(afterHotplug, {
    type: 'applySucceeded',
    transport: 'targeted',
    request: { path: '/requested.jpg', requestId: 'hotplug' },
    result: result('/resolved/state.jpg', 'hotplug', ['eDP-1', 'HDMI-A-1']),
  });

  assert.deepEqual(toRuntimeDisplayWallpapers(next), [
    { output: 'eDP-1', wallpaperPath: '/resolved/state.jpg', status: 'confirmed' },
    { output: 'DP-2', wallpaperPath: null, status: 'unknown' },
  ]);
});

test('mismatched request evidence and outputs outside a named target are ignored', () => {
  const initial = createRuntimeWallpaperSession(['eDP-1', 'HDMI-A-1']);
  const mismatch = reduceRuntimeWallpaperSession(initial, {
    type: 'applySucceeded',
    transport: 'targeted',
    request: { path: '/requested.jpg', target: 'eDP-1', requestId: 'expected' },
    result: result('/wrong.jpg', 'different', ['eDP-1']),
  });
  const wrongOutput = reduceRuntimeWallpaperSession(initial, {
    type: 'applySucceeded',
    transport: 'targeted',
    request: { path: '/requested.jpg', target: 'eDP-1', requestId: 'named' },
    result: result('/wrong-output.jpg', 'named', ['HDMI-A-1']),
  });
  const blankRequestIdWithoutResponseId = reduceRuntimeWallpaperSession(initial, {
    type: 'applySucceeded',
    transport: 'targeted',
    request: { path: '/requested.jpg', target: 'eDP-1', requestId: '' },
    result: result('/wrong-blank.jpg', undefined, ['eDP-1']),
  });

  assert.equal(mismatch, initial);
  assert.equal(wrongOutput, initial);
  assert.equal(blankRequestIdWithoutResponseId, initial);
});

test('runtime reconciliation replaces session confirmations and clears stopped renderers', () => {
  const initial = createRuntimeWallpaperSession(['eDP-1', 'HDMI-A-1']);
  const applied = reduceRuntimeWallpaperSession(initial, {
    type: 'applySucceeded',
    transport: 'targeted',
    request: { path: '/shared.jpg', requestId: 'shared' },
    result: result('/shared.jpg', 'shared'),
  });

  const reconciled = reduceRuntimeWallpaperSession(applied, {
    type: 'runtimeReconciled',
    observations: [
      { output: 'eDP-1', wallpaperPath: '/observed.jpg', status: 'confirmed' },
      { output: 'HDMI-A-1', wallpaperPath: null, status: 'unknown' },
    ],
  });

  assert.deepEqual(toRuntimeDisplayWallpapers(reconciled), [
    { output: 'eDP-1', wallpaperPath: '/observed.jpg', status: 'confirmed' },
    { output: 'HDMI-A-1', wallpaperPath: null, status: 'unknown' },
  ]);
});
