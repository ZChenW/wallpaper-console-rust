import assert from 'node:assert/strict';
import test from 'node:test';

import {
  DEFAULT_LIBRARY_QUERY_STATE,
  createLibraryQueryState,
  libraryQueryReducer,
  normalizeLibraryQueryState,
  type LibraryQueryAction,
} from './libraryQueryState.ts';
import { DEFAULT_SHELL_PREFERENCES } from './shellPreferences.ts';

test('library query defaults to Usable and Recently Added with transient search', () => {
  assert.deepEqual(DEFAULT_LIBRARY_QUERY_STATE, {
    sourceFilter: { kind: 'all' },
    typeFilter: 'usable',
    favoritesOnly: false,
    sort: 'recentlyAdded',
    search: '',
    offset: 0,
    limit: 120,
  });

  assert.deepEqual(createLibraryQueryState({
    ...DEFAULT_SHELL_PREFERENCES,
    sourceFilter: { kind: 'source', sourceId: 7 },
    favoritesOnly: true,
    sort: 'nameAsc',
    search: 'legacy search must be ignored',
  }), {
    ...DEFAULT_LIBRARY_QUERY_STATE,
    sourceFilter: { kind: 'source', sourceId: 7 },
    favoritesOnly: true,
    sort: 'nameAsc',
  });
});

test('library query normalizer repairs invalid values without coercion', () => {
  assert.deepEqual(normalizeLibraryQueryState({
    sourceFilter: { kind: 'source', sourceId: '7' },
    typeFilter: 'all',
    favoritesOnly: 'yes',
    sort: 'newest',
    search: 12,
    offset: -10,
    limit: Number.NaN,
  }), DEFAULT_LIBRARY_QUERY_STATE);
});

test('advancing paging uses the current limit', () => {
  const state = libraryQueryReducer(
    { ...DEFAULT_LIBRARY_QUERY_STATE, offset: 80, limit: 40 },
    { type: 'nextPage' },
  );

  assert.equal(state.offset, 120);
  assert.equal(state.limit, 40);
});

test('every query condition change resets paging', () => {
  const cases: ReadonlyArray<LibraryQueryAction> = [
    { type: 'setSearch', search: 'forest' },
    { type: 'setSourceFilter', sourceFilter: { kind: 'source', sourceId: 3 } },
    { type: 'setTypeFilter', typeFilter: 'gif' },
    { type: 'setFavoritesOnly', favoritesOnly: true },
    { type: 'setSort', sort: 'nameDesc' },
    { type: 'setLimit', limit: 60 },
  ];

  for (const action of cases) {
    const next = libraryQueryReducer({ ...DEFAULT_LIBRARY_QUERY_STATE, offset: 360 }, action);
    assert.equal(next.offset, 0, `${action.type} should reset paging`);
  }
});

test('setting an unchanged query condition preserves paging', () => {
  const state = {
    ...DEFAULT_LIBRARY_QUERY_STATE,
    search: 'forest',
    sourceFilter: { kind: 'source' as const, sourceId: 3 },
    typeFilter: 'gif' as const,
    favoritesOnly: true,
    sort: 'nameDesc' as const,
    offset: 240,
    limit: 60,
  };
  const actions: ReadonlyArray<LibraryQueryAction> = [
    { type: 'setSearch', search: 'forest' },
    { type: 'setSourceFilter', sourceFilter: { kind: 'source', sourceId: 3 } },
    { type: 'setTypeFilter', typeFilter: 'gif' },
    { type: 'setFavoritesOnly', favoritesOnly: true },
    { type: 'setSort', sort: 'nameDesc' },
    { type: 'setLimit', limit: 60 },
  ];

  for (const action of actions) {
    assert.equal(libraryQueryReducer(state, action).offset, 240, action.type);
  }
});

test('resetPaging is explicit and malformed reducer values are repaired', () => {
  assert.equal(
    libraryQueryReducer({ ...DEFAULT_LIBRARY_QUERY_STATE, offset: 240 }, { type: 'resetPaging' }).offset,
    0,
  );
  assert.deepEqual(
    libraryQueryReducer(DEFAULT_LIBRARY_QUERY_STATE, { type: 'setLimit', limit: 0 }),
    DEFAULT_LIBRARY_QUERY_STATE,
  );
});
