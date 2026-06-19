import assert from 'node:assert/strict';
import test from 'node:test';
import {
  mergePagedWallpaperItems,
  resolveRequestKind,
  loadingStateForKind,
} from './usePagedWallpapers.ts';

const item = (path: string) => ({
  path,
  type: 'image',
  ext: 'jpg',
  backend: 'awww',
  size: 1,
  mtime: 1,
  resolution: '1x1',
});

test('mergePagedWallpaperItems replaces first page', () => {
  assert.deepEqual(
    mergePagedWallpaperItems([item('/old.jpg')], [item('/new.jpg')], false).map((entry) => entry.path),
    ['/new.jpg'],
  );
});

test('mergePagedWallpaperItems appends later pages', () => {
  assert.deepEqual(
    mergePagedWallpaperItems([item('/a.jpg')], [item('/b.jpg')], true).map((entry) => entry.path),
    ['/a.jpg', '/b.jpg'],
  );
});

test('mergePagedWallpaperItems treats null items as an empty page', () => {
  assert.deepEqual(
    mergePagedWallpaperItems([item('/a.jpg')], null, false).map((entry) => entry.path),
    [],
  );
});

test('resolveRequestKind returns initial for first non-append load', () => {
  assert.equal(resolveRequestKind(false, false), 'initial');
});

test('resolveRequestKind returns refresh for non-append load after first page', () => {
  assert.equal(resolveRequestKind(false, true), 'refresh');
});

test('resolveRequestKind returns append regardless of hasLoadedOnce', () => {
  assert.equal(resolveRequestKind(true, false), 'append');
  assert.equal(resolveRequestKind(true, true), 'append');
});

test('loadingStateForKind sets initialLoading only for initial kind', () => {
  assert.deepEqual(loadingStateForKind('initial'), { initialLoading: true, refreshing: false });
  assert.deepEqual(loadingStateForKind('refresh'), { initialLoading: false, refreshing: true });
  assert.deepEqual(loadingStateForKind('append'), { initialLoading: false, refreshing: false });
});
