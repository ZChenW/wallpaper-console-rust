import assert from 'node:assert/strict';
import test from 'node:test';

import {
  createRuntimeWallpaperSession,
  reduceRuntimeWallpaperSession,
  toRuntimeDisplayWallpapers,
} from './runtimeWallpaperSession.ts';

const result = (statePath: string) => ({
  requestId: 'request-1',
  appliedPath: '/resolved/applied.jpg',
  statePath,
  backend: 'awww',
  fileType: 'image',
  preview: false,
});

test('successful target-omitted apply confirms its statePath on every connected output', () => {
  const initial = createRuntimeWallpaperSession([' eDP-1 ', 'HDMI-A-1', 'eDP-1']);
  const next = reduceRuntimeWallpaperSession(initial, {
    type: 'applySucceeded',
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
    request: { path: '/shared.jpg', requestId: 'all' },
    result: result('/shared.jpg'),
  });
  const next = reduceRuntimeWallpaperSession(shared, {
    type: 'applySucceeded',
    request: { path: '/monitor.jpg', target: ' HDMI-A-1 ', requestId: 'monitor' },
    result: result('/resolved/monitor.jpg'),
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
    request: { kind: 'apply', path: '/legacy.jpg', requestId: 'legacy' },
    result: result('/legacy.jpg'),
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
    request,
    result: undefined,
  });
  const malformed = reduceRuntimeWallpaperSession(initial, {
    type: 'applySucceeded',
    request,
    result: { ...result('/valid.jpg'), statePath: '   ' },
  });

  assert.equal(missing, initial);
  assert.equal(malformed, initial);
});

test('blank, unknown, or disconnected named target cannot create confirmation', () => {
  const initial = createRuntimeWallpaperSession(['eDP-1']);
  const blank = reduceRuntimeWallpaperSession(initial, {
    type: 'applySucceeded',
    request: { path: '/requested.jpg', target: '   ', requestId: 'blank' },
    result: result('/blank.jpg'),
  });
  const disconnected = reduceRuntimeWallpaperSession(initial, {
    type: 'applySucceeded',
    request: { path: '/requested.jpg', target: 'DP-9', requestId: 'disconnected' },
    result: result('/disconnected.jpg'),
  });

  assert.equal(blank, initial);
  assert.equal(disconnected, initial);
});

test('disconnect drops evidence and reconnect cannot revive an old confirmation', () => {
  const initial = createRuntimeWallpaperSession(['eDP-1', 'HDMI-A-1']);
  const confirmed = reduceRuntimeWallpaperSession(initial, {
    type: 'applySucceeded',
    request: { path: '/shared.jpg', requestId: 'shared' },
    result: result('/shared.jpg'),
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
