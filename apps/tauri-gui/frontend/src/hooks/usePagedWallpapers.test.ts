import assert from 'node:assert/strict';
import test from 'node:test';
import { mergePagedWallpaperItems } from './usePagedWallpapers.ts';

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
