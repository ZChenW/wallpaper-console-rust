import assert from 'node:assert/strict';
import test from 'node:test';

import type { DisplayStateDTO, SourceDTO } from '../api/types.ts';
import { loadShellCatalogSnapshot } from './useShellCatalog.ts';

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
