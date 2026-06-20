import assert from 'node:assert/strict';
import test from 'node:test';

import {
  createSettingsStatusRequestSeq,
  refreshSettingsStatusCore,
  shouldApplySettingsStatusSnapshot,
} from './refreshSettingsStatusCore.ts';

const library = {
  configured: '/lib',
  effective: '/lib',
  sqliteReady: true,
  sqliteRows: 10,
  tsvRows: 0,
  stale: false,
  message: '',
};

const thumb = {
  dir: '/cache',
  size: '1 MB',
  entries: 3,
  failureEntries: 0,
  cleanupDays: 30,
};

test('refreshSettingsStatusCore keeps fulfilled statuses when one loader rejects', async () => {
  const snapshot = await refreshSettingsStatusCore({
    librarySourceStatus: async () => library,
    linuxWallpaperEngineStatus: async () => {
      throw new Error('we ipc failed');
    },
    thumbnailCacheStatus: async () => thumb,
  });

  assert.deepEqual(snapshot.libraryStatus, library);
  assert.equal(snapshot.libraryError, null);
  assert.equal(snapshot.weStatus, null);
  assert.equal(snapshot.weError, 'we ipc failed');
  assert.deepEqual(snapshot.thumbCache, thumb);
  assert.equal(snapshot.thumbError, null);
});

test('refreshSettingsStatusCore treats synchronous loader throws as rejections', async () => {
  const snapshot = await refreshSettingsStatusCore({
    librarySourceStatus: () => {
      throw new Error('sync library');
    },
    linuxWallpaperEngineStatus: async () => ({ available: true, path: '/we', message: 'ok' }),
    thumbnailCacheStatus: async () => thumb,
  });

  assert.equal(snapshot.libraryStatus, null);
  assert.equal(snapshot.libraryError, 'sync library');
  assert.equal(snapshot.weStatus?.available, true);
  assert.deepEqual(snapshot.thumbCache, thumb);
});

test('shouldApplySettingsStatusSnapshot ignores stale request ids', () => {
  assert.equal(shouldApplySettingsStatusSnapshot(1, 2), false);
  assert.equal(shouldApplySettingsStatusSnapshot(2, 2), true);
});

test('stale settings status refresh does not apply after a newer refresh completes', async () => {
  let releaseSlow: (() => void) | undefined;
  const slowBlock = new Promise<void>((resolve) => { releaseSlow = resolve; });
  const seq = createSettingsStatusRequestSeq();
  const appliedRows: number[] = [];

  const runRefresh = async (requestId: number, rows: number, block?: Promise<void>) => {
    const snapshot = await refreshSettingsStatusCore({
      librarySourceStatus: async () => {
        if (block) await block;
        return { ...library, sqliteRows: rows };
      },
      linuxWallpaperEngineStatus: async () => ({ available: false, message: 'missing' }),
      thumbnailCacheStatus: async () => thumb,
    });
    if (!seq.isLatest(requestId)) return false;
    appliedRows.push(snapshot.libraryStatus!.sqliteRows);
    return true;
  };

  const slowId = seq.begin();
  const fastId = seq.begin();
  const slowPromise = runRefresh(slowId, 1, slowBlock);
  const fastPromise = runRefresh(fastId, 99);
  assert.equal(await fastPromise, true);
  releaseSlow?.();
  assert.equal(await slowPromise, false);
  assert.deepEqual(appliedRows, [99]);
});
