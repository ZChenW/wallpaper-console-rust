import assert from 'node:assert/strict';
import test from 'node:test';

import {
  formatLoadPageError,
  mergePagedWallpaperItems,
} from './usePagedWallpapers.ts';
import type { WallpaperDTO } from '../api/bridge.ts';

interface BrowserWallpaper extends WallpaperDTO {
  wallpaperId: number;
  favorite: boolean;
  author?: string;
  sources: Array<{ id: number; displayName: string }>;
}

const browserWallpaper = (
  wallpaperId: number,
  overrides: Partial<BrowserWallpaper> = {},
): BrowserWallpaper => ({
  wallpaperId,
  path: `/wall/${wallpaperId}.jpg`,
  type: 'image',
  ext: 'jpg',
  backend: 'awww',
  size: 1024,
  mtime: wallpaperId,
  resolution: '1920x1080',
  favorite: wallpaperId % 2 === 0,
  author: `Author ${wallpaperId}`,
  sources: [{ id: 7, displayName: 'Curated' }],
  ...overrides,
});

test('rich wallpaper fields survive initial, append, and reload replacement merges', () => {
  const first = browserWallpaper(1);
  const second = browserWallpaper(2, { favorite: true, author: 'Second author' });
  const replacement = browserWallpaper(3, {
    favorite: true,
    author: 'Reloaded author',
    sources: [{ id: 9, displayName: 'Reloaded source' }],
  });

  const initial = mergePagedWallpaperItems<BrowserWallpaper>([], [first], false);
  const appended = mergePagedWallpaperItems<BrowserWallpaper>(initial, [second], true);
  const reloaded = mergePagedWallpaperItems<BrowserWallpaper>(appended, [replacement], false);

  assert.deepEqual(initial, [first]);
  assert.deepEqual(appended, [first, second]);
  assert.deepEqual(reloaded, [replacement]);
  assert.equal(appended[1].favorite, true);
  assert.equal(appended[1].author, 'Second author');
  assert.deepEqual(reloaded[0].sources, [{ id: 9, displayName: 'Reloaded source' }]);
});

test('formatLoadPageError preserves Error message text', () => {
  assert.equal(formatLoadPageError(new Error('database is locked')), 'database is locked');
});

test('formatLoadPageError preserves string errors', () => {
  assert.equal(formatLoadPageError('database is locked'), 'database is locked');
});

test('formatLoadPageError falls back for unknown error shapes', () => {
  assert.equal(formatLoadPageError({ code: 1 }), 'Failed to load library page');
});
