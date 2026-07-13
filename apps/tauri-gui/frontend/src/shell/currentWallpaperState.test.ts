import assert from 'node:assert/strict';
import test from 'node:test';

import {
  resolveCurrentWallpaperState,
  type CurrentWallpaperSnapshot,
} from './currentWallpaperState.ts';

const snapshot = (
  overrides: Partial<CurrentWallpaperSnapshot> = {},
): CurrentWallpaperSnapshot => ({
  activeTarget: { kind: 'allDisplays' },
  connectedOutputs: ['eDP-1', 'HDMI-A-1'],
  runtime: [],
  persisted: [],
  ...overrides,
});

test('a named target is current only with confirmed runtime evidence for that output', () => {
  assert.deepEqual(resolveCurrentWallpaperState(snapshot({
    activeTarget: { kind: 'output', output: 'HDMI-A-1' },
    runtime: [
      { output: 'eDP-1', wallpaperPath: '/walls/laptop.jpg', status: 'confirmed' },
      { output: 'HDMI-A-1', wallpaperPath: '/walls/monitor.jpg', status: 'confirmed' },
    ],
  })), {
    kind: 'confirmed',
    wallpaperPath: '/walls/monitor.jpg',
    outputs: ['HDMI-A-1'],
  });
});

test('All Displays is confirmed when every connected output confirms the same wallpaper', () => {
  assert.deepEqual(resolveCurrentWallpaperState(snapshot({
    runtime: [
      { output: 'eDP-1', wallpaperPath: '/walls/shared.jpg', status: 'confirmed' },
      { output: 'HDMI-A-1', wallpaperPath: '/walls/shared.jpg', status: 'confirmed' },
    ],
  })), {
    kind: 'confirmed',
    wallpaperPath: '/walls/shared.jpg',
    outputs: ['eDP-1', 'HDMI-A-1'],
  });
});

test('All Displays reports mixed outputs without inventing a single current card', () => {
  assert.deepEqual(resolveCurrentWallpaperState(snapshot({
    runtime: [
      { output: 'eDP-1', wallpaperPath: '/walls/laptop.jpg', status: 'confirmed' },
      { output: 'HDMI-A-1', wallpaperPath: '/walls/monitor.jpg', status: 'confirmed' },
    ],
  })), {
    kind: 'mixed',
    outputs: [
      { output: 'eDP-1', wallpaperPath: '/walls/laptop.jpg' },
      { output: 'HDMI-A-1', wallpaperPath: '/walls/monitor.jpg' },
    ],
  });
});

test('All Displays remains unknown until every connected output is confirmed', () => {
  assert.deepEqual(resolveCurrentWallpaperState(snapshot({
    runtime: [
      { output: 'eDP-1', wallpaperPath: '/walls/laptop.jpg', status: 'confirmed' },
      { output: 'HDMI-A-1', wallpaperPath: null, status: 'unknown' },
    ],
  })), {
    kind: 'unknown',
    outputs: ['eDP-1', 'HDMI-A-1'],
  });
});

test('persisted mappings alone never masquerade as realtime current wallpaper', () => {
  assert.deepEqual(resolveCurrentWallpaperState(snapshot({
    activeTarget: { kind: 'output', output: 'eDP-1' },
    persisted: [
      { target: { kind: 'output', output: 'eDP-1' }, wallpaperPath: '/walls/saved.jpg' },
    ],
  })), {
    kind: 'unknown',
    outputs: ['eDP-1'],
  });

  assert.deepEqual(resolveCurrentWallpaperState(snapshot({
    persisted: [
      { target: { kind: 'allDisplays' }, wallpaperPath: '/walls/saved-for-all.jpg' },
    ],
  })), {
    kind: 'unknown',
    outputs: ['eDP-1', 'HDMI-A-1'],
  });
});

test('a disconnected named target and an empty display set are unknown', () => {
  assert.deepEqual(resolveCurrentWallpaperState(snapshot({
    activeTarget: { kind: 'output', output: 'DP-9' },
    runtime: [{ output: 'DP-9', wallpaperPath: '/walls/stale.jpg', status: 'confirmed' }],
  })), {
    kind: 'unknown',
    outputs: ['DP-9'],
  });

  assert.deepEqual(resolveCurrentWallpaperState(snapshot({ connectedOutputs: [] })), {
    kind: 'unknown',
    outputs: [],
  });
});

test('conflicting runtime observations are unknown instead of choosing by array order', () => {
  assert.deepEqual(resolveCurrentWallpaperState(snapshot({
    activeTarget: { kind: 'output', output: 'eDP-1' },
    runtime: [
      { output: 'eDP-1', wallpaperPath: '/walls/old.jpg', status: 'confirmed' },
      { output: 'eDP-1', wallpaperPath: '/walls/new.jpg', status: 'confirmed' },
    ],
  })), {
    kind: 'unknown',
    outputs: ['eDP-1'],
  });
});
