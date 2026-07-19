import assert from 'node:assert/strict';
import test from 'node:test';

import type { SourceDTO } from '../api/types.ts';
import type { CurrentWallpaperState } from './currentWallpaperState.ts';
import {
  canChooseRandomWallpaper,
  currentWallpaperLabel,
  effectiveSourceFilter,
  reconcileSelectedEntry,
  reconcileSelectedEntryByStableId,
  reconcileSourceFilter,
  shouldOfferFirstRun,
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

test('source read failures never masquerade as a first run', () => {
  assert.equal(shouldOfferFirstRun([], undefined), true);
  assert.equal(shouldOfferFirstRun([], 'database unavailable'), false);
  assert.equal(shouldOfferFirstRun([source(1)], undefined), false);
});

test('random waits for the debounced search and an applicable display', () => {
  assert.equal(canChooseRandomWallpaper({
    searchSettled: true,
    randomPending: false,
    total: 1,
    canApply: true,
  }), true);
  assert.equal(canChooseRandomWallpaper({
    searchSettled: false,
    randomPending: false,
    total: 1,
    canApply: true,
  }), false);
  assert.equal(canChooseRandomWallpaper({
    searchSettled: true,
    randomPending: false,
    total: 0,
    canApply: true,
  }), false);
});

test('selection follows a replacement entry and clears when the replacement omits it', () => {
  const selected = { path: '/walls/a.jpg', title: 'Old' };
  const refreshed = { path: '/walls/a.jpg', title: 'Refreshed' };

  assert.equal(
    reconcileSelectedEntry(selected, new Map([[refreshed.path, refreshed]])),
    refreshed,
  );
  assert.equal(reconcileSelectedEntry(selected, new Map()), null);
  assert.equal(reconcileSelectedEntry(null, new Map()), null);
});

test('revision replacement refreshes selection by stable ID and retains deep-page selection', () => {
  const selected = { wallpaperId: 7, path: '/old.jpg', title: 'Old' };
  const refreshed = { wallpaperId: 7, path: '/new.jpg', title: 'New' };
  assert.equal(
    reconcileSelectedEntryByStableId(selected, [refreshed]),
    refreshed,
  );
  assert.equal(reconcileSelectedEntryByStableId(selected, []), selected);
  assert.equal(reconcileSelectedEntryByStableId(null, [refreshed]), null);
});

test('source failure disables filtering but Library still loads with all-sources view', () => {
  // When sources fail to load, the source list is empty but the Library should
  // still render results using a default all-sources filter.
  // The sourceFilter reconciliation already handles this: an empty source list
  // forces sourceFilter back to 'all'.
  assert.deepEqual(reconcileSourceFilter({ kind: 'source', sourceId: 7 }, []), { kind: 'all' });
  assert.deepEqual(reconcileSourceFilter({ kind: 'all' }, []), { kind: 'all' });
});

test('source error is distinct from empty source list for first-run detection', () => {
  // An error is an error state, not evidence of fresh install
  assert.equal(shouldOfferFirstRun([], 'source database unavailable'), false);
  // But an empty list without error IS a first-run
  assert.equal(shouldOfferFirstRun([], undefined), true);
  // A populated list is never a first-run
  assert.equal(shouldOfferFirstRun([source(1)], undefined), false);
});

// ── effectiveSourceFilter ──────────────────────────────────────────────

test('effectiveSourceFilter returns the persisted filter when sources load successfully', () => {
  assert.deepEqual(
    effectiveSourceFilter({ kind: 'source', sourceId: 7 }, undefined),
    { kind: 'source', sourceId: 7 },
  );
  assert.deepEqual(
    effectiveSourceFilter({ kind: 'all' }, undefined),
    { kind: 'all' },
  );
});

test('effectiveSourceFilter forces all when source catalog errors', () => {
  // A source catalog error means we cannot know which sources exist.
  // The Library must still render with all sources; the persisted preference
  // is NOT overwritten — only the effective value passed to the browser changes.
  assert.deepEqual(
    effectiveSourceFilter({ kind: 'source', sourceId: 7 }, 'database unavailable'),
    { kind: 'all' },
  );
  assert.deepEqual(
    effectiveSourceFilter({ kind: 'all' }, 'source error'),
    { kind: 'all' },
  );
});

test('effectiveSourceFilter with null/empty error string behaves as no error', () => {
  assert.deepEqual(
    effectiveSourceFilter({ kind: 'source', sourceId: 7 }, undefined),
    { kind: 'source', sourceId: 7 },
  );
  assert.deepEqual(
    effectiveSourceFilter({ kind: 'source', sourceId: 3 }, ''),
    { kind: 'source', sourceId: 3 },
  );
});
