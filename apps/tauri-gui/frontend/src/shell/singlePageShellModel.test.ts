import assert from 'node:assert/strict';
import test from 'node:test';

import type { SourceDTO } from '../api/types.ts';
import type { CurrentWallpaperState } from './currentWallpaperState.ts';
import {
  currentWallpaperLabel,
  reconcileSourceFilter,
  targetArgument,
} from './singlePageShellModel.ts';

const source = (id: number): SourceDTO => ({
  id,
  path: `/walls/${id}`,
  displayName: `Source ${id}`,
  kind: 'directory',
  recursive: true,
  availability: 'available',
  addedAt: '2026-07-14T00:00:00Z',
  exists: true,
  isWE: false,
  label: `Source ${id}`,
});

test('All Displays stays an explicit omitted target while one output stays exact', () => {
  assert.equal(targetArgument({ kind: 'allDisplays' }), undefined);
  assert.equal(targetArgument({ kind: 'output', output: '  DP-2  ' }), 'DP-2');
});

test('a saved source filter survives reload only while that source still exists', () => {
  assert.deepEqual(
    reconcileSourceFilter({ kind: 'source', sourceId: 7 }, [source(3), source(7)]),
    { kind: 'source', sourceId: 7 },
  );
  assert.deepEqual(
    reconcileSourceFilter({ kind: 'source', sourceId: 7 }, [source(3)]),
    { kind: 'all' },
  );
});

test('bottom status distinguishes confirmed, mixed, and unknown runtime evidence', () => {
  const confirmed: CurrentWallpaperState = {
    kind: 'confirmed',
    wallpaperPath: '/walls/quiet-lake.jpg',
    outputs: ['DP-1'],
  };
  const mixed: CurrentWallpaperState = {
    kind: 'mixed',
    outputs: [
      { output: 'DP-1', wallpaperPath: '/walls/a.jpg' },
      { output: 'HDMI-A-1', wallpaperPath: '/walls/b.jpg' },
    ],
  };
  const unknown: CurrentWallpaperState = { kind: 'unknown', outputs: ['DP-1'] };

  assert.equal(currentWallpaperLabel(confirmed), 'Current: quiet-lake.jpg');
  assert.equal(currentWallpaperLabel(mixed), 'Current: displays use different wallpapers');
  assert.equal(currentWallpaperLabel(unknown), 'Current: not verified');
});
