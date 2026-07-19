import assert from 'node:assert/strict';
import test from 'node:test';

import {
  cardHoverLabel,
  displayName,
  editorialActionLabel,
  formatSize,
  metaLine,
  typeIcon,
  weBadge,
  weBadgeClass,
} from './wallpaperCardHelpers.ts';

test('Editorial action copy reflects the configured apply gesture', () => {
  assert.equal(editorialActionLabel(true, 'single'), 'Select / apply');
  assert.equal(editorialActionLabel(true, 'double'), 'Select / double-click apply');
  assert.equal(editorialActionLabel(false, 'single'), 'View details');
});

const baseEntry = (overrides: Record<string, unknown> = {}) => ({
  path: '/walls/a.jpg',
  type: 'image',
  ext: 'jpg',
  backend: 'awww',
  size: 1024,
  mtime: 1,
  resolution: '1920x1080',
  ...overrides,
});

test('displayName prefers title, then workshopId, then filename, then path', () => {
  assert.equal(displayName(baseEntry({ path: '/w/x.jpg' })), 'x.jpg');
  assert.equal(displayName(baseEntry({ path: '/w/x.jpg', workshopId: '123' })), '123');
  assert.equal(
    displayName(baseEntry({ path: '/w/x.jpg', workshopId: '123', title: 'My Wall' })),
    'My Wall',
  );
});

test('typeIcon maps known types and falls back for unknown', () => {
  assert.equal(typeIcon('image'), '\u{1F5BC}');
  assert.equal(typeIcon('video'), '\u{1F3AC}');
  assert.equal(typeIcon('we_scene'), 'WE');
  assert.equal(typeIcon('unknown'), '\u{1F4C4}');
});

test('weBadge returns null for regular types', () => {
  assert.equal(weBadge(baseEntry({ type: 'image' })), null);
});

test('weBadge marks incompatible scene', () => {
  assert.equal(
    weBadge(baseEntry({ type: 'we_scene', backendStatus: 'failed' })),
    'Scene incompatible',
  );
  assert.equal(weBadge(baseEntry({ type: 'we_scene' })), 'WE Scene');
});

test('weBadge shows renderer limitation for renderer_limitation status', () => {
  assert.equal(
    weBadge(baseEntry({ type: 'we_scene', backendStatus: 'renderer_limitation' })),
    'Renderer limitation',
  );
});

test('weBadge shows Web browse only for we_web', () => {
  assert.equal(weBadge(baseEntry({ type: 'we_web' })), 'Web · browse only');
});

test('weBadgeClass uses danger class for Web browse only', () => {
  assert.equal(
    weBadgeClass(baseEntry({ type: 'we_web' })),
    'wallpaper-badge wallpaper-badge-danger',
  );
});

test('weBadgeClass uses danger class for renderer_limitation', () => {
  assert.equal(
    weBadgeClass(baseEntry({ type: 'we_scene', backendStatus: 'renderer_limitation' })),
    'wallpaper-badge wallpaper-badge-danger',
  );
});

test('weBadgeClass uses danger class for failed backend', () => {
  assert.equal(
    weBadgeClass(baseEntry({ type: 'we_scene', backendStatus: 'failed' })),
    'wallpaper-badge wallpaper-badge-danger',
  );
  assert.equal(weBadgeClass(baseEntry({ type: 'we_scene' })), 'wallpaper-badge');
});

test('metaLine shows resolution/type/size for regular types', () => {
  assert.equal(
    metaLine(baseEntry({ type: 'image', resolution: '1920x1080', size: 1048576 })),
    '1920x1080 · image · 1.0 MB',
  );
});

test('metaLine uses unsupportedReason for unsupported type', () => {
  assert.equal(
    metaLine(baseEntry({ type: 'unsupported', unsupportedReason: 'bad project' })),
    'bad project',
  );
});

test('formatSize scales across units', () => {
  assert.equal(formatSize(0), '0 B');
  assert.equal(formatSize(2048), '2 KB');
  assert.equal(formatSize(5 << 20), '5.0 MB');
  assert.equal(formatSize(3 * (1 << 30)), '3.0 GB');
});

test('hover label uses the title and source names without exposing the absolute path', () => {
  assert.equal(cardHoverLabel(baseEntry({
    path: '/private/walls/secret.jpg',
    title: 'Quiet Lake',
    sources: [
      { id: 1, displayName: 'Pictures' },
      { id: 2, displayName: 'Downloaded' },
    ],
  })), 'Quiet Lake · Pictures, Downloaded');
  assert.equal(cardHoverLabel(baseEntry({ path: '/private/walls/secret.jpg' })), 'secret.jpg');
});
