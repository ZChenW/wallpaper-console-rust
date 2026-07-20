import assert from 'node:assert/strict';
import test from 'node:test';

import {
  instantiateActiveLibraryAdapter,
  libraryEntryApplyAvailable,
  libraryEntryApplyDisabledReason,
  resolveLibraryFlowStartupAnchor,
  resolveLibraryModeSwitchAnchor,
  resolveLibraryQueryResetAnchor,
} from './libraryViewModel.ts';
import type { LibraryBrowserItemDTO } from '../api/types.ts';

const entries = [
  { wallpaperId: 11, path: '/wallpapers/first.jpg' },
  { wallpaperId: 22, path: '/wallpapers/current.jpg' },
  { wallpaperId: 33, path: '/wallpapers/selected.jpg' },
];

test('LibraryViewport instantiates only the active adapter factory', () => {
  const calls: string[] = [];
  const factories = {
    grid: () => {
      calls.push('grid');
      return 'grid-adapter';
    },
    flow: () => {
      calls.push('flow');
      return 'flow-adapter';
    },
  };

  assert.equal(instantiateActiveLibraryAdapter('flow', factories), 'flow-adapter');
  assert.deepEqual(calls, ['flow']);

  calls.length = 0;
  assert.equal(instantiateActiveLibraryAdapter('grid', factories), 'grid-adapter');
  assert.deepEqual(calls, ['grid']);
});

test('mode switching anchors Selected, then outgoing center, then first loaded item', () => {
  assert.deepEqual(resolveLibraryModeSwitchAnchor(entries, 33, 22), {
    wallpaperId: 33,
    index: 2,
  });
  assert.deepEqual(resolveLibraryModeSwitchAnchor(entries, 404, 22), {
    wallpaperId: 22,
    index: 1,
  });
  assert.deepEqual(resolveLibraryModeSwitchAnchor(entries, 404, 405), {
    wallpaperId: 11,
    index: 0,
  });
  assert.equal(resolveLibraryModeSwitchAnchor([], 33, 22), null);
});

test('direct Flow startup anchors explicit ID, then loaded Current, then first', () => {
  assert.deepEqual(
    resolveLibraryFlowStartupAnchor(entries, 33, '/wallpapers/current.jpg'),
    { wallpaperId: 33, index: 2 },
  );
  assert.deepEqual(
    resolveLibraryFlowStartupAnchor(entries, 404, '/wallpapers/current.jpg'),
    { wallpaperId: 22, index: 1 },
  );
  assert.deepEqual(
    resolveLibraryFlowStartupAnchor(entries, null, '/wallpapers/missing.jpg'),
    { wallpaperId: 11, index: 0 },
  );
  assert.equal(resolveLibraryFlowStartupAnchor([], 33, '/wallpapers/current.jpg'), null);
});

test('a query reset always anchors the first loaded item', () => {
  assert.deepEqual(resolveLibraryQueryResetAnchor(entries), {
    wallpaperId: 11,
    index: 0,
  });
  assert.equal(resolveLibraryQueryResetAnchor([]), null);
});

const applicableEntry = {
  wallpaperId: 1,
  path: '/wallpapers/live.webm',
  type: 'video',
} as LibraryBrowserItemDTO;

test('libraryEntryApplyAvailable requires both display and entry eligibility', () => {
  const isApplicable = () => true;
  assert.equal(
    libraryEntryApplyAvailable(false, isApplicable, applicableEntry),
    false,
  );
  assert.equal(
    libraryEntryApplyAvailable(true, () => false, applicableEntry),
    false,
  );
  assert.equal(
    libraryEntryApplyAvailable(true, isApplicable, applicableEntry),
    true,
  );
});

test('libraryEntryApplyDisabledReason prefers display blocking over entry reasons', () => {
  const entry = {
    ...applicableEntry,
    applyReason: 'Compatible renderer unavailable',
  } as LibraryBrowserItemDTO;
  assert.equal(
    libraryEntryApplyDisabledReason(
      false,
      'The selected display is unavailable.',
      entry,
    ),
    'The selected display is unavailable.',
  );
  assert.equal(
    libraryEntryApplyDisabledReason(true, null, entry),
    'Compatible renderer unavailable',
  );
});
