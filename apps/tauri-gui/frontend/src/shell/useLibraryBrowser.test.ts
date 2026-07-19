import assert from 'node:assert/strict';
import test from 'node:test';

import {
  createLibraryBrowserQuery,
  createRandomLibraryBrowserQuery,
  formatRandomWallpaperError,
  isCurrentQueryEmpty,
  isLibraryCriteriaPending,
  isRandomRequestCurrent,
  randomWallpaperErrorOutcome,
} from './useLibraryBrowser.ts';

const criteria = {
  sourceFilter: { kind: 'source' as const, sourceId: 17 },
  typeFilter: 'weScene' as const,
  favoritesOnly: true,
  sort: 'nameDesc' as const,
  search: '  neon   city  ',
};

test('browser query maps remembered filters while paging stays owned by the paging hook', () => {
  assert.deepEqual(createLibraryBrowserQuery(criteria, 'opaque-cursor', 120), {
    sourceId: 17,
    typeFilter: 'weScene',
    favoritesOnly: true,
    search: 'neon   city',
    sort: 'nameDesc',
    cursor: 'opaque-cursor',
    limit: 120,
  });
});

test('all-sources query omits sourceId instead of inventing a sentinel', () => {
  assert.deepEqual(createLibraryBrowserQuery({
    ...criteria,
    sourceFilter: { kind: 'all' },
  }, null, 60), {
    typeFilter: 'weScene',
    favoritesOnly: true,
    search: 'neon   city',
    sort: 'nameDesc',
    cursor: null,
    limit: 60,
  });
});

test('random query uses the same filters without a paging cursor', () => {
  assert.deepEqual(createRandomLibraryBrowserQuery(criteria), {
    sourceId: 17,
    typeFilter: 'weScene',
    favoritesOnly: true,
    search: 'neon   city',
    sort: 'nameDesc',
    cursor: null,
    limit: 1,
  });
});

test('random errors preserve useful messages and provide a stable fallback', () => {
  assert.equal(formatRandomWallpaperError(new Error('database is locked')), 'database is locked');
  assert.equal(formatRandomWallpaperError('backend unavailable'), 'backend unavailable');
  assert.equal(formatRandomWallpaperError({ code: 1 }), 'Failed to choose a random wallpaper');
});

test('random command failures return an explicit outcome instead of looking empty', () => {
  assert.deepEqual(randomWallpaperErrorOutcome(new Error('database is locked')), {
    kind: 'error',
    message: 'database is locked',
  });
});

test('confirmed emptiness is used only for the query that produced it', () => {
  assert.equal(isCurrentQueryEmpty(true, 'all|usable|false|', 'all|usable|false|'), true);
  assert.equal(isCurrentQueryEmpty(true, 'source:7|usable|false|', 'all|usable|false|'), false);
  assert.equal(isCurrentQueryEmpty(false, 'all|usable|false|', 'all|usable|false|'), false);
});

test('criteria stay pending until a page resolves for the current query key', () => {
  assert.equal(isLibraryCriteriaPending(null, 'all|usable|false|'), true);
  assert.equal(
    isLibraryCriteriaPending('source:7|usable|false|', 'all|usable|false|'),
    true,
  );
  assert.equal(
    isLibraryCriteriaPending('all|usable|false|', 'all|usable|false|'),
    false,
  );
});

test('a random result becomes stale as soon as raw search text changes', () => {
  assert.equal(isRandomRequestCurrent(4, 4, 'city', 'city'), true);
  assert.equal(isRandomRequestCurrent(4, 4, '', 'n'), false);
  assert.equal(isRandomRequestCurrent(4, 5, 'city', 'city'), false);
});
