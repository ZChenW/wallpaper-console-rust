import assert from 'node:assert/strict';
import test, { beforeEach } from 'node:test';

import { api } from './mockBridge.ts';

const ctrl = api.__mockControl;

beforeEach(() => {
  ctrl.resetAll();
});

test('setScanProgress changes the returned scanProgress state', async () => {
  ctrl.setScanProgress({ running: true, scanned: 5, totalHint: 100, stage: 'walking files' });
  const p = await api.scanProgress();
  assert.equal(p.running, true);
  assert.equal(p.scanned, 5);
  assert.equal(p.totalHint, 100);
  assert.equal(p.stage, 'walking files');
});

test('resetScanProgress restores the default idle scanProgress', async () => {
  ctrl.setScanProgress({ running: true, scanned: 42, stage: 'walking files' });
  ctrl.resetScanProgress();
  const p = await api.scanProgress();
  assert.equal(p.running, false);
  assert.equal(p.scanned, 0);
  assert.equal(p.stage, 'idle');
});

test('setScanAutoAdvance increments scanned on each scanProgress call while running', async () => {
  ctrl.setScanProgress({ running: true, scanned: 0 });
  ctrl.setScanAutoAdvance(true, 5);
  const a = await api.scanProgress();
  const b = await api.scanProgress();
  const c = await api.scanProgress();
  assert.equal(a.scanned, 5);
  assert.equal(b.scanned, 10);
  assert.equal(c.scanned, 15);
});

test('scanProgress does not auto-advance when not running', async () => {
  ctrl.setScanAutoAdvance(true, 5);
  const a = await api.scanProgress();
  const b = await api.scanProgress();
  assert.equal(a.running, false);
  assert.equal(a.scanned, 0);
  assert.equal(b.scanned, 0);
});

test('scanCancel marks running as false so the UI can dismiss the cancel button', async () => {
  ctrl.setScanProgress({ running: true, scanned: 7, stage: 'walking files' });
  const before = await api.scanProgress();
  assert.equal(before.running, true);
  const r = await api.scanCancel();
  assert.equal(r.success, true);
  const after = await api.scanProgress();
  assert.equal(after.running, false);
});

test('injectCommandFailure makes sqliteVerify return a failure result', async () => {
  const before = await api.sqliteVerify();
  assert.equal(before.success, true);
  ctrl.injectCommandFailure('sqliteVerify');
  const failed = await api.sqliteVerify();
  assert.equal(failed.success, false);
  assert.equal(failed.exitCode, 1);
  ctrl.clearCommandFailure('sqliteVerify');
  const restored = await api.sqliteVerify();
  assert.equal(restored.success, true);
});

test('injectCommandFailure for exportDiagnostics returns failure after delay', async () => {
  ctrl.injectCommandFailure('exportDiagnostics');
  const r = await api.exportDiagnostics();
  assert.equal(r.success, false);
  assert.equal(r.stderr, 'mock failure');
});

test('configSet persists across configGet calls', async () => {
  assert.equal(await api.configGet('awww_transition_type'), 'fade');
  const r = await api.configSet('awww_transition_type', 'wipe');
  assert.equal(r.success, true);
  assert.equal(await api.configGet('awww_transition_type'), 'wipe');
  assert.equal(await api.configGet('gui_theme'), 'light');
  await api.configSet('gui_theme', 'obsidian_warm');
  assert.equal(await api.configGet('gui_theme'), 'obsidian_warm');
});

test('configGet falls back to defaults when no override is set', async () => {
  assert.equal(await api.configGet('image_backend'), 'awww');
  assert.equal(await api.configGet('video_backend'), 'mpvpaper');
  assert.equal(await api.configGet('nonexistent_key'), '');
});

test('resetConfig clears overrides but keeps defaults', async () => {
  await api.configSet('gui_theme', 'obsidian_warm');
  assert.equal(await api.configGet('gui_theme'), 'obsidian_warm');
  ctrl.resetConfig();
  assert.equal(await api.configGet('gui_theme'), 'light');
});

test('configGetMany reflects persisted overrides', async () => {
  await api.configSet('awww_transition_type', 'grow');
  const many = await api.configGetMany(['awww_transition_type', 'gui_theme']);
  assert.equal(many.awww_transition_type, 'grow');
  assert.equal(many.gui_theme, 'light');
});

test('setThumbnailFailure makes thumbnailFor return failureReason', async () => {
  const path = '/mock/path/wallpaper-001.jpg';
  const before = await api.thumbnailFor(path);
  assert.equal(before.failureReason, undefined);
  ctrl.setThumbnailFailure(path);
  const failed = await api.thumbnailFor(path);
  assert.equal(failed.cacheHit, false);
  assert.equal(failed.failureReason, 'mock thumbnail failure');
  ctrl.clearThumbnailFailure(path);
  const restored = await api.thumbnailFor(path);
  assert.equal(restored.failureReason, undefined);
});

test('thumbnail failure is path-scoped and does not affect other paths', async () => {
  const failing = '/mock/path/wallpaper-001.jpg';
  const other = '/mock/path/wallpaper-002.jpg';
  ctrl.setThumbnailFailure(failing);
  const a = await api.thumbnailFor(failing);
  const b = await api.thumbnailFor(other);
  assert.equal(a.failureReason, 'mock thumbnail failure');
  assert.equal(b.failureReason, undefined);
});

test('resetAll restores scan progress, config, command failures, and thumbnail failures', async () => {
  ctrl.setScanProgress({ running: true, scanned: 9 });
  ctrl.setScanAutoAdvance(true, 3);
  await api.configSet('gui_theme', 'obsidian_warm');
  ctrl.injectCommandFailure('sqliteVerify');
  ctrl.setThumbnailFailure('/mock/path/wallpaper-001.jpg');

  ctrl.resetAll();

  const scan = await api.scanProgress();
  assert.equal(scan.running, false);
  assert.equal(scan.scanned, 0);
  assert.equal(await api.configGet('gui_theme'), 'light');
  assert.equal((await api.sqliteVerify()).success, true);
  const thumb = await api.thumbnailFor('/mock/path/wallpaper-001.jpg');
  assert.equal(thumb.failureReason, undefined);
});

test('setLibraryFirstPageEmpty returns empty first page then filled on subsequent calls', async () => {
  ctrl.setLibraryFirstPageEmpty(true);
  const first = await api.libraryPage('all', 'newest', '', 0, 120);
  assert.equal(first.total, 0);
  assert.deepEqual(first.items, []);

  const second = await api.libraryPage('all', 'newest', '', 0, 120);
  assert.ok(second.total > 0, 'second call should be filled');
  assert.ok(second.items.length > 0, 'filled page should have items');
});

test('setLibraryFirstPageEmpty does not affect appended (offset>0) pages', async () => {
  ctrl.setLibraryFirstPageEmpty(true);
  const appendPage = await api.libraryPage('all', 'newest', '', 120, 120);
  assert.ok(appendPage.total > 0, 'append page should be filled despite scenario');
});

test('resetAll clears the library first-page-empty scenario', async () => {
  ctrl.setLibraryFirstPageEmpty(true);
  ctrl.resetAll();
  const page = await api.libraryPage('all', 'newest', '', 0, 120);
  assert.ok(page.total > 0, 'scenario should be cleared after resetAll');
});
