import assert from 'node:assert/strict';
import test from 'node:test';

import type { LibraryBrowserItemDTO } from '../api/types.ts';
import {
  formatAddedDate,
  presentWallpaper,
  wallpaperTypeLabel,
} from './wallpaperPresentation.ts';

function wallpaper(
  overrides: Partial<LibraryBrowserItemDTO> = {},
): LibraryBrowserItemDTO {
  return {
    path: '/walls/night-drive/scene.pkg',
    type: 'we_scene',
    ext: 'pkg',
    backend: 'linux-wallpaperengine',
    size: 12 * 1024 * 1024,
    mtime: 1_700_000_000,
    resolution: '3840x2160',
    title: 'Night Drive',
    workshopId: '123456',
    rendererCompatibility: 'Rendered with limited particle support',
    wallpaperId: 42,
    favorite: true,
    author: '  Ada Artist  ',
    addedAt: '2026-07-14T10:30:00Z',
    sources: [
      { id: 1, displayName: ' Workshop ' },
      { id: 2, displayName: 'Local' },
      { id: 3, displayName: 'Workshop' },
    ],
    ...overrides,
  };
}

test('presents complete wallpaper metadata with stable user-facing formatting', () => {
  assert.deepEqual(presentWallpaper(wallpaper()), {
    name: 'Night Drive',
    sources: 'Workshop, Local',
    type: 'Wallpaper Engine Scene',
    resolution: '3840 × 2160',
    size: '12.0 MB',
    addedDate: 'Jul 14, 2026',
    author: 'Ada Artist',
    workshopId: '123456',
    backend: 'linux-wallpaperengine',
    compatibility: 'Rendered with limited particle support',
  });
});

test('uses null for unavailable optional metadata and preserves an honest compatibility warning', () => {
  assert.deepEqual(presentWallpaper(wallpaper({
    title: undefined,
    workshopId: undefined,
    path: '/walls/still.jpg',
    type: 'image',
    backend: '  ',
    size: Number.NaN,
    resolution: 'unknown',
    rendererCompatibility: undefined,
    applyReason: '  Renderer is unavailable  ',
    author: ' ',
    addedAt: 'not-a-date',
    sources: [{ id: 1, displayName: ' ' }],
  })), {
    name: 'still.jpg',
    sources: null,
    type: 'Image',
    resolution: null,
    size: null,
    addedDate: null,
    author: null,
    workshopId: null,
    backend: null,
    compatibility: 'Renderer is unavailable',
  });
});

test('treats SQLite addedAt timestamps without a zone as UTC', () => {
  const originalTimeZone = process.env.TZ;
  process.env.TZ = 'Pacific/Kiritimati';
  try {
    assert.equal(formatAddedDate('2026-07-14 00:30:00'), 'Jul 14, 2026');
    assert.equal(formatAddedDate('2026-07-14T00:30:00'), 'Jul 14, 2026');
    assert.equal(formatAddedDate('2026-07-14 00:30:00.125'), 'Jul 14, 2026');
    assert.equal(formatAddedDate('2026-07-14T00:30:00Z'), 'Jul 14, 2026');
    assert.equal(formatAddedDate('2026-02-31 00:30:00'), null);
  } finally {
    if (originalTimeZone === undefined) delete process.env.TZ;
    else process.env.TZ = originalTimeZone;
  }
});

test('prefers an actionable renderer limitation over generic compatibility copy', () => {
  assert.equal(presentWallpaper(wallpaper({
    rendererCompatibility: 'Rendering may differ from Wallpaper Engine.',
    applyReason: 'Scene backend is unavailable.',
    unsupportedReason: 'Unsupported project type.',
    backendErrorMessage: 'Renderer process exited.',
  })).compatibility, 'Scene backend is unavailable.');
  assert.equal(presentWallpaper(wallpaper({
    rendererCompatibility: 'Rendering may differ from Wallpaper Engine.',
    applyReason: undefined,
    unsupportedReason: 'Unsupported project type.',
    backendErrorMessage: 'Renderer process exited.',
  })).compatibility, 'Unsupported project type.');
  assert.equal(presentWallpaper(wallpaper({
    rendererCompatibility: 'Rendering may differ from Wallpaper Engine.',
    applyReason: undefined,
    unsupportedReason: undefined,
    backendErrorMessage: 'Renderer process exited.',
  })).compatibility, 'Rendering may differ from Wallpaper Engine.');
  assert.equal(presentWallpaper(wallpaper({
    rendererCompatibility: undefined,
    applyReason: undefined,
    unsupportedReason: undefined,
    backendErrorMessage: 'Renderer process exited with private diagnostics.',
  })).compatibility, null);
});

test('omits compatibility when no renderer or apply limitation is available', () => {
  assert.equal(presentWallpaper(wallpaper({
    rendererCompatibility: undefined,
    applyReason: undefined,
    unsupportedReason: undefined,
    backendErrorMessage: undefined,
  })).compatibility, null);
});

test('labels every supported wallpaper type and keeps unknown types readable', () => {
  assert.equal(wallpaperTypeLabel('image'), 'Image');
  assert.equal(wallpaperTypeLabel('gif'), 'GIF');
  assert.equal(wallpaperTypeLabel('video'), 'Video');
  assert.equal(wallpaperTypeLabel('we_scene'), 'Wallpaper Engine Scene');
  assert.equal(wallpaperTypeLabel('we_web'), 'Wallpaper Engine Web');
  assert.equal(wallpaperTypeLabel('unsupported'), 'Unsupported');
  assert.equal(wallpaperTypeLabel('custom_renderer'), 'custom renderer');
  assert.equal(wallpaperTypeLabel(''), 'Unknown');
});
