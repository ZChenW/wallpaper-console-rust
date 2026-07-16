import assert from 'node:assert/strict';
import test, { beforeEach } from 'node:test';

import { api } from './mockBridge.ts';
import type { LibraryBrowserQueryDTO } from './types.ts';

const ctrl = api.__mockControl;

function browserQuery(
  overrides: Partial<LibraryBrowserQueryDTO> = {},
): LibraryBrowserQueryDTO {
  return {
    typeFilter: 'usable',
    favoritesOnly: false,
    search: '',
    sort: 'recentlyAdded',
    cursor: null,
    limit: 200,
    ...overrides,
  };
}

beforeEach(() => {
  ctrl.resetAll();
});

test('history is not exposed by the wallpaper console API', () => {
  assert.equal('historyPage' in api, false);
  assert.equal('historyClear' in api, false);
});

test('runtime observation mock exposes replaceable positive evidence and reset restores defaults', async () => {
  const defaults = await api.runtimeWallpaperObservations();
  assert.equal(defaults.length, 2);
  assert.ok(defaults.every((entry) => entry.status === 'confirmed'));

  ctrl.setRuntimeWallpaperObservations([
    {
      output: 'eDP-1',
      wallpaperPath: null,
      status: 'unknown',
      reason: 'mock renderer stopped',
    },
  ]);
  const changed = await api.runtimeWallpaperObservations();
  assert.deepEqual(changed, [{
    output: 'eDP-1',
    wallpaperPath: null,
    status: 'unknown',
    reason: 'mock renderer stopped',
  }]);

  ctrl.resetAll();
  assert.deepEqual(await api.runtimeWallpaperObservations(), defaults);
});

test('renderer status mock is replaceable and reset restores installation defaults', async () => {
  const defaults = await api.rendererStatuses();
  assert.equal(defaults.awww.available, true);
  assert.equal(defaults.mpvpaper.available, true);
  assert.equal(defaults.linuxWallpaperEngine.available, false);

  ctrl.setRendererStatuses({
    awww: { available: false, message: 'awww missing', detail: 'not in PATH' },
    mpvpaper: { available: true, message: 'mpvpaper installed' },
    linuxWallpaperEngine: {
      available: true,
      message: 'linux-wallpaperengine installed',
      path: '/mock/bin/linux-wallpaperengine',
    },
  });
  assert.equal((await api.rendererStatuses()).awww.available, false);

  ctrl.resetAll();
  assert.deepEqual(await api.rendererStatuses(), defaults);
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

test('sources expose stable rich metadata while preserving legacy fields', async () => {
  const sources = await api.sourcesList();

  assert.deepEqual(
    sources.map(({ id, displayName, kind, recursive, availability, addedAt }) => ({
      id,
      displayName,
      kind,
      recursive,
      availability,
      addedAt,
    })),
    [
      {
        id: 1,
        displayName: 'Pictures',
        kind: 'directory',
        recursive: true,
        availability: 'available',
        addedAt: '2026-07-01T00:00:00Z',
      },
      {
        id: 2,
        displayName: 'Wallpapers',
        kind: 'directory',
        recursive: true,
        availability: 'available',
        addedAt: '2026-07-02T00:00:00Z',
      },
      {
        id: 3,
        displayName: 'Steam Workshop: 12345',
        kind: 'wallpaper_engine_workshop',
        recursive: false,
        availability: 'available',
        addedAt: '2026-07-03T00:00:00Z',
      },
    ],
  );
  assert.equal(sources[0]?.label, sources[0]?.displayName);
  assert.equal(sources[2]?.isWE, true);
});

test('setSourceAvailability changes one source and resetAll restores it', async () => {
  ctrl.setSourceAvailability(2, 'offline');

  let sources = await api.sourcesList();
  assert.equal(sources.find((source) => source.id === 1)?.availability, 'available');
  assert.equal(sources.find((source) => source.id === 2)?.availability, 'offline');

  ctrl.resetAll();
  sources = await api.sourcesList();
  assert.equal(sources.find((source) => source.id === 2)?.availability, 'available');
});

test('first-run suggestions are explicit mock state and reset with all scenarios', async () => {
  const ctrl = api.__mockControl;
  assert.deepEqual(await api.firstRunSourceSuggestions(), []);

  ctrl.setFirstRunSourceSuggestions([
    { kind: 'directory', label: 'Downloads', path: '/mock/Downloads' },
    { kind: 'wallpaperEngine', roots: ['/mock/Steam/workshop/content/431960'] },
  ]);
  assert.deepEqual(await api.firstRunSourceSuggestions(), [
    { kind: 'directory', label: 'Downloads', path: '/mock/Downloads' },
    { kind: 'wallpaperEngine', roots: ['/mock/Steam/workshop/content/431960'] },
  ]);

  ctrl.resetAll();
  assert.deepEqual(await api.firstRunSourceSuggestions(), []);
});

test('source mutations are stable-id scoped and resettable', async () => {
  const initial = await api.sourcesList();
  const first = initial[0]!;
  const second = initial[1]!;

  assert.equal((await api.sourceRename(first.id, 'Renamed collection')).success, true);
  assert.equal((await api.sourceSetRecursive(first.id, false)).success, true);
  assert.equal((await api.sourceRefresh(first.id)).success, true);
  let changed = await api.sourcesList();
  assert.equal(changed.find((source) => source.id === first.id)?.displayName, 'Renamed collection');
  assert.equal(changed.find((source) => source.id === first.id)?.label, 'Renamed collection');
  assert.equal(changed.find((source) => source.id === first.id)?.recursive, false);
  assert.equal(changed.find((source) => source.id === second.id)?.displayName, 'Wallpapers');

  assert.equal((await api.sourceRemoveById(first.id)).success, true);
  changed = await api.sourcesList();
  assert.equal(changed.some((source) => source.id === first.id), false);
  assert.equal(changed.some((source) => source.id === second.id), true);

  ctrl.resetAll();
  const reset = await api.sourcesList();
  assert.equal(reset.some((source) => source.id === first.id), true);
  assert.equal(reset.find((source) => source.id === first.id)?.displayName, 'Pictures');
});

test('sourceAdd never reuses a removed stable ID', async () => {
  assert.equal((await api.sourceRemoveById(3)).success, true);
  assert.equal((await api.sourceAdd('/mock/New')).success, true);

  const added = (await api.sourcesList()).find((source) => source.path === '/mock/New');
  assert.equal(added?.id, 4);
});

test('sourceAdd recognizes Wallpaper Engine workshop sources', async () => {
  assert.equal(
    (await api.sourceAdd('/mock/steamapps/workshop/content/431960/67890/')).success,
    true,
  );

  const source = (await api.sourcesList()).find((candidate) => candidate.id === 4);
  assert.equal(source?.kind, 'wallpaper_engine_workshop');
  assert.equal(source?.recursive, false);
  assert.equal(source?.isWE, true);
});

test('sourceAdd treats paths that differ only by trailing slashes as one identity', async () => {
  assert.equal((await api.sourceAdd('/mock/New/')).success, true);
  assert.equal((await api.sourceAdd('/mock/New')).success, true);

  const added = (await api.sourcesList()).filter((source) => source.path === '/mock/New');
  assert.equal(added.length, 1);
  assert.equal(added[0]?.id, 4);
});

test('displaysList returns connected display names', async () => {
  const displays = await api.displaysList();

  assert.deepEqual(displays, {
    outputs: [{ name: 'eDP-1' }, { name: 'HDMI-A-1' }],
  });
});

test('displayStateList returns typed camelCase display state', async () => {
  const state = await api.displayStateList();

  assert.deepEqual(state, [
    {
      targetKey: '__all_displays__',
      kind: 'allDisplays',
      output: null,
      wallpaperPath: '/mock/path/wallpaper-001.jpg',
      backend: 'awww',
      updatedAt: '2026-07-13T00:00:00Z',
    },
  ]);
});

test('applyToDisplay defaults an omitted target to All Displays', async () => {
  const result = await api.applyToDisplay({
    path: '/mock/path/new-wallpaper.jpg',
    requestId: 'req-all',
  });

  assert.equal(result.success, true);
  assert.deepEqual(JSON.parse(result.stdout), {
    requestId: 'req-all',
    appliedPath: '/mock/path/new-wallpaper.jpg',
    statePath: '/mock/path/new-wallpaper.jpg',
    backend: 'awww',
    fileType: 'image',
    preview: false,
    appliedOutputs: ['eDP-1', 'HDMI-A-1'],
  });
  assert.deepEqual(ctrl.lastTargetedApplyRequest(), {
    path: '/mock/path/new-wallpaper.jpg',
    requestId: 'req-all',
  });
});

test('applyToDisplay forwards a named display target', async () => {
  const result = await api.applyToDisplay({
    path: '/mock/path/targeted.jpg',
    target: 'eDP-1',
    requestId: 'req-edp',
  });

  assert.equal(result.success, true);
  assert.deepEqual(JSON.parse(result.stdout).appliedOutputs, ['eDP-1']);
  assert.deepEqual(ctrl.lastTargetedApplyRequest(), {
    path: '/mock/path/targeted.jpg',
    target: 'eDP-1',
    requestId: 'req-edp',
  });
});

test('applyToDisplay preserves preview kind on targeted transport', async () => {
  const request = {
    kind: 'apply_preview' as const,
    path: '/mock/Steam/steamapps/workshop/content/431960/3558034522',
    target: 'HDMI-A-1',
    requestId: 'req-preview',
  };

  const result = await api.applyToDisplay(request);
  const parsed = JSON.parse(result.stdout);

  assert.equal(parsed.preview, true);
  assert.equal(parsed.fileType, 'gif');
  assert.deepEqual(parsed.appliedOutputs, ['HDMI-A-1']);
  assert.deepEqual(ctrl.lastTargetedApplyRequest(), request);
  assert.equal(ctrl.lastApplyActionRequest(), null, 'targeted preview must not use applyAction');
});

test('restoreDisplays accepts discovered or explicit connected outputs', async () => {
  const discovered = await api.restoreDisplays();
  const explicit = await api.restoreDisplays({ outputs: ['eDP-1', 'HDMI-A-1'] });

  assert.equal(discovered.success, true);
  assert.equal(explicit.success, true);
});

test('library browser unsupported category contains only WE Web and unsupported projects', async () => {
  const page = await api.libraryBrowserPage(
    browserQuery({ typeFilter: 'unsupported' }),
  );

  assert.deepEqual(
    new Set(page.items.map((item) => item.type)),
    new Set(['we_web', 'unsupported']),
  );
  assert.equal(page.items.length, 2);
});

test('library browser composes source, type, favorite, and AND search filters', async () => {
  const page = await api.libraryBrowserPage(
    browserQuery({
      sourceId: 1,
      typeFilter: 'video',
      favoritesOnly: true,
      search: 'wallpaper 050 mock',
    }),
  );

  assert.equal(page.items.length, 1);
  assert.equal(page.items[0]?.path, '/mock/path/wallpaper-050.mp4');
  assert.equal(page.items[0]?.favorite, true);
  assert.deepEqual(page.items[0]?.sources.map((source) => source.id), [1]);
});

test('library browser search uses filename, title, author, and source display name only', async () => {
  const acrossFields = await api.libraryBrowserPage(
    browserQuery({
      typeFilter: 'weScene',
      search: 'scene ada workshop',
    }),
  );
  assert.deepEqual(
    acrossFields.items.map((item) => item.title),
    ['Scene title'],
  );

  for (const excludedField of ['steamapps', 'linux-wallpaperengine', 'scene.json']) {
    const excluded = await api.libraryBrowserPage(
      browserQuery({ typeFilter: 'weScene', search: excludedField }),
    );
    assert.equal(excluded.items.length, 0, `${excludedField} must not be searchable`);
  }
});

test('library browser name sorts are stable and pagination uses filtered total', async () => {
  const firstAsc = await api.libraryBrowserPage(
    browserQuery({ typeFilter: 'image', sort: 'nameAsc', limit: 1 }),
  );
  const asc = await api.libraryBrowserPage(
    browserQuery({
      typeFilter: 'image',
      sort: 'nameAsc',
      cursor: firstAsc.nextCursor,
      limit: 1,
    }),
  );
  const desc = await api.libraryBrowserPage(
    browserQuery({ typeFilter: 'image', sort: 'nameDesc', limit: 2 }),
  );
  assert.equal(firstAsc.items[0]?.path, '/mock/path/wallpaper-001.jpg');
  assert.equal(desc.items[0]?.path, '/mock/path/wallpaper-149.jpg');
  assert.equal(asc.items[0]?.path, '/mock/path/wallpaper-002.jpg');
});

test('library browser recentlyAdded sort uses newest metadata first', async () => {
  const page = await api.libraryBrowserPage(
    browserQuery({ sort: 'recentlyAdded', limit: 2 }),
  );

  assert.deepEqual(
    page.items.map((item) => item.path),
    [
      '/mock/Steam/steamapps/workshop/content/431960/3558034522',
      '/mock/Steam/steamapps/workshop/content/431960/3589454154',
    ],
  );
});

test('library browser page caps oversized requests at 500 items', async () => {
  ctrl.setBrowserFixtureCopies(4);

  const page = await api.libraryBrowserPage(browserQuery({ limit: 10_000 }));
  const total = await api.libraryBrowserTotal(browserQuery({ limit: 10_000 }), page.revision);

  assert.ok(total.total > 500, 'fixture must prove the cap rather than exhaust the result set');
  assert.equal(page.items.length, 500);
});

test('library browser random returns only a matching candidate and null for no match', async () => {
  const match = await api.libraryBrowserRandom(
    browserQuery({
      sourceId: 3,
      typeFilter: 'weScene',
      favoritesOnly: true,
      search: 'scene ada',
      limit: 0,
    }),
  );
  assert.equal(match?.path, '/mock/Steam/steamapps/workshop/content/431960/3558034522');
  assert.equal(match?.favorite, true);
  assert.equal(match?.author, 'Ada Lovelace');

  const missing = await api.libraryBrowserRandom(
    browserQuery({ search: 'definitely-missing-wallpaper' }),
  );
  assert.equal(missing, null);
});

test('favorite mutations immediately update unified page and random results', async () => {
  const path = '/mock/path/wallpaper-002.jpg';
  const query = browserQuery({
    sourceId: 1,
    typeFilter: 'image',
    favoritesOnly: true,
    search: 'wallpaper-002',
  });

  assert.equal((await api.libraryBrowserPage(query)).items.length, 0);
  assert.equal(await api.libraryBrowserRandom(query), null);

  assert.equal((await api.favoriteAdd(path)).success, true);
  const addedPage = await api.libraryBrowserPage(query);
  assert.equal(addedPage.items.length, 1);
  assert.equal(addedPage.items[0]?.path, path);
  assert.equal(addedPage.items[0]?.favorite, true);
  assert.equal((await api.libraryBrowserRandom(query))?.path, path);

  assert.equal((await api.favoriteRemove(path)).success, true);
  assert.equal((await api.libraryBrowserPage(query)).items.length, 0);
  assert.equal(await api.libraryBrowserRandom(query), null);
});

test('failed favorite additions leave unified browser state unchanged', async () => {
  const path = '/mock/Steam/steamapps/workshop/content/431960/3650880224';
  const query = browserQuery({
    sourceId: 3,
    typeFilter: 'unsupported',
    favoritesOnly: true,
    search: 'web title',
  });

  assert.equal((await api.favoriteAdd(path)).success, false);
  assert.equal((await api.libraryBrowserPage(query)).items.length, 0);
  assert.equal(await api.libraryBrowserRandom(query), null);
});

test('resetAll restores initial favorites after add and remove mutations', async () => {
  const initialFavorite = '/mock/path/wallpaper-001.jpg';
  const addedFavorite = '/mock/path/wallpaper-002.jpg';

  await api.favoriteRemove(initialFavorite);
  await api.favoriteAdd(addedFavorite);
  ctrl.resetAll();

  const favorites = await api.libraryBrowserPage(browserQuery({
    typeFilter: 'image',
    favoritesOnly: true,
    sort: 'nameAsc',
  }));
  assert.equal(
    favorites.items.some((item) => item.path === initialFavorite && item.favorite),
    true,
  );
  assert.equal(favorites.items.some((item) => item.path === addedFavorite), false);
});

test('large browser fixture has at least 1000 unique paths across stable pages', async () => {
  ctrl.setBrowserFixtureCopies(7);
  const query = browserQuery({ limit: 500 });
  const first = await api.libraryBrowserPage(query);
  const total = await api.libraryBrowserTotal(query, first.revision);
  const allItems = [...first.items];

  let cursor = first.nextCursor;
  while (cursor !== null) {
    const page = await api.libraryBrowserPage({ ...query, cursor });
    assert.equal(page.revision, first.revision);
    allItems.push(...page.items);
    cursor = page.nextCursor;
  }

  assert.ok(total.total >= 1_000);
  assert.equal(allItems.length, total.total);
  assert.equal(new Set(allItems.map((item) => item.path)).size, total.total);
  assert.equal(new Set(allItems.map((item) => item.wallpaperId)).size, total.total);
});

test('large fixture filtering, favorites, and random share the same unique entries', async () => {
  ctrl.setBrowserFixtureCopies(7);
  const matchingQuery = browserQuery({
    sourceId: 1,
    typeFilter: 'video',
    search: 'wallpaper-050',
    sort: 'recentlyAdded',
  });
  const matching = await api.libraryBrowserPage(matchingQuery);

  assert.equal(matching.items.length, 7);
  assert.equal(new Set(matching.items.map((item) => item.path)).size, 7);

  const initialPath = '/mock/path/wallpaper-050.mp4';
  const copiedPath = matching.items.find((item) => item.path !== initialPath)?.path;
  assert.ok(copiedPath);
  await api.favoriteRemove(initialPath);
  await api.favoriteAdd(copiedPath);

  const favoriteQuery = { ...matchingQuery, favoritesOnly: true };
  const favorites = await api.libraryBrowserPage(favoriteQuery);
  assert.deepEqual(favorites.items.map((item) => item.path), [copiedPath]);
  assert.equal((await api.libraryBrowserRandom(favoriteQuery))?.path, copiedPath);
});
