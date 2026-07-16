import assert from 'node:assert/strict';
import test from 'node:test';

import {
  animatedPreviewPath,
  anchoredScrollTopForLayoutChange,
  captureStableViewportAnchor,
  previewAssetPath,
  shouldPauseThumbnailReveal,
  shouldStartAnimatedHover,
  shouldRequestNextPage,
  restoreStableViewportAnchor,
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

test('visibleThumbnailPaths generates static thumbnails from preview assets', () => {
  const entries = [entry('a'), entry('b', '/preview.gif'), entry('c')];
  assert.deepEqual(visibleThumbnailPaths(entries, 3, null, 1), ['a', '/preview.gif', 'c']);
});

test('previewAssetPath prefers a project preview over the wallpaper path', () => {
  assert.equal(previewAssetPath(entry('/project', '/project/preview.gif')), '/project/preview.gif');
  assert.equal(previewAssetPath(entry('/picture.jpg')), '/picture.jpg');
});

test('animated preview is exposed only for a hovered GIF while scrolling is idle', () => {
  const gif = entry('/project', '/project/preview.GIF');
  assert.equal(animatedPreviewPath(gif, true, false), '/project/preview.GIF');
  assert.equal(animatedPreviewPath(gif, false, false), null);
  assert.equal(animatedPreviewPath(gif, true, true), null);
  assert.equal(animatedPreviewPath(entry('/project', '/project/preview.jpg'), true, false), null);
});

test('thumbnail reveal pauses while the grid is inactive or scrolling', () => {
  assert.equal(shouldPauseThumbnailReveal(false, false), true);
  assert.equal(shouldPauseThumbnailReveal(true, true), true);
  assert.equal(shouldPauseThumbnailReveal(true, false), false);
});

test('hover starts an animated preview only while scrolling is idle', () => {
  assert.equal(shouldStartAnimatedHover(false), true);
  assert.equal(shouldStartAnimatedHover(true), false);
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

test('next page is requested only near the loaded virtual tail', () => {
  assert.equal(shouldRequestNextPage({
    rowCount: 30,
    visibleEndRow: 27,
    hasMore: true,
    loadingMore: false,
  }), true);
  assert.equal(shouldRequestNextPage({
    rowCount: 30,
    visibleEndRow: 26,
    hasMore: true,
    loadingMore: false,
  }), false);
  assert.equal(shouldRequestNextPage({
    rowCount: 30,
    visibleEndRow: 29,
    hasMore: false,
    loadingMore: false,
  }), false);
  assert.equal(shouldRequestNextPage({
    rowCount: 30,
    visibleEndRow: 29,
    hasMore: true,
    loadingMore: true,
  }), false);
  assert.equal(shouldRequestNextPage({
    rowCount: 0,
    visibleEndRow: null,
    hasMore: true,
    loadingMore: false,
  }), false);
});

test('stable wallpaper ID restores the viewport across revision replacement', () => {
  const before = [1, 2, 3, 4, 5, 6].map((wallpaperId) => ({ wallpaperId }));
  const anchor = captureStableViewportAnchor(before, 2, 100, 225);
  assert.deepEqual(anchor, { wallpaperId: 5, rowOffset: 25 });

  const after = [9, 5, 6, 7].map((wallpaperId) => ({ wallpaperId }));
  assert.equal(restoreStableViewportAnchor(after, anchor, 2, 100), 25);
  assert.equal(restoreStableViewportAnchor([{ wallpaperId: 9 }], anchor, 2, 100), null);
});
