import assert from 'node:assert/strict';
import test from 'node:test';

import {
  anchoredScrollTopForLayoutChange,
  visibleThumbnailPaths,
} from './wallpaperGridHelpers.ts';
import type { WallpaperDTO } from '../api/bridge.ts';

const entry = (path: string, previewPath?: string): WallpaperDTO => ({
  path,
  type: 'image',
  ext: 'jpg',
  backend: 'awww',
  size: 1,
  mtime: 1,
  resolution: '1x1',
  ...(previewPath ? { previewPath } : {}),
});

test('visibleThumbnailPaths falls back to first rows when range is null', () => {
  const entries = Array.from({ length: 20 }, (_, i) => entry(`p${i}`));
  assert.deepEqual(
    visibleThumbnailPaths(entries, 4, null, 3),
    ['p0', 'p1', 'p2', 'p3', 'p4', 'p5', 'p6', 'p7', 'p8', 'p9', 'p10', 'p11'],
  );
});

test('visibleThumbnailPaths uses virtual range when available', () => {
  const entries = Array.from({ length: 20 }, (_, i) => entry(`p${i}`));
  assert.deepEqual(
    visibleThumbnailPaths(entries, 4, { startIndex: 2, endIndex: 3 }, 3),
    ['p8', 'p9', 'p10', 'p11', 'p12', 'p13', 'p14', 'p15'],
  );
});

test('visibleThumbnailPaths excludes entries with previewPath', () => {
  const entries = [entry('a'), entry('b', '/preview.gif'), entry('c')];
  assert.deepEqual(visibleThumbnailPaths(entries, 3, null, 1), ['a', 'c']);
});

test('anchoredScrollTopForLayoutChange preserves the first visible item when columns change', () => {
  assert.equal(
    anchoredScrollTopForLayoutChange({
      scrollTop: 408,
      previousColumns: 4,
      previousRowHeight: 188,
      nextColumns: 5,
      nextRowHeight: 188,
    }),
    188,
  );
});

test('anchoredScrollTopForLayoutChange preserves the first visible item when card size changes', () => {
  assert.equal(
    anchoredScrollTopForLayoutChange({
      scrollTop: 408,
      previousColumns: 4,
      previousRowHeight: 188,
      nextColumns: 4,
      nextRowHeight: 164,
    }),
    328,
  );
});

test('anchoredScrollTopForLayoutChange handles simultaneous size and column changes', () => {
  assert.equal(
    anchoredScrollTopForLayoutChange({
      scrollTop: 408,
      previousColumns: 4,
      previousRowHeight: 188,
      nextColumns: 3,
      nextRowHeight: 232,
    }),
    464,
  );
});
