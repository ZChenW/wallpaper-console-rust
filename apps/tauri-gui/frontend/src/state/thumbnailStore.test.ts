import assert from 'node:assert/strict';
import test from 'node:test';

import { ThumbnailStore } from './thumbnailStore.ts';

test('thumbnail store exposes failure and clears it after success', async () => {
  let fail = true;
  const store = new ThumbnailStore(1, async (path) => {
    if (fail) return { path, cacheHit: false, failureReason: 'probe_failed' };
    return { path, cacheHit: false, thumbnail: `thumb:${path}` };
  });
  let notified = 0;
  store.subscribe('a', () => { notified += 1; });

  store.enqueueVisible(['a']);
  await new Promise((resolve) => setTimeout(resolve, 20));
  assert.equal(store.getFailure('a'), 'probe_failed');
  assert.equal(store.failureCount(), 1);

  fail = false;
  store.forget(['a']);
  store.enqueueVisible(['a']);
  await new Promise((resolve) => setTimeout(resolve, 20));
  assert.equal(store.get('a'), 'thumb:a');
  assert.equal(store.getFailure('a'), undefined);
  assert.equal(store.failureCount(), 0);
  assert.ok(notified >= 2);
});

test('thumbnail store only loads the latest range and does not retain earlier options', async () => {
  const loaded: string[] = [];
  const store = new ThumbnailStore(1, async (path) => {
    loaded.push(path);
    return { path, cacheHit: false, thumbnail: `thumb:${path}` };
  });

  store.enqueueVisible(['old-a', 'old-b'], { priority: 'front' });
  store.enqueueVisible(['new-a', 'new-b']);
  await new Promise((resolve) => setTimeout(resolve, 20));

  assert.deepEqual(loaded, ['new-a', 'new-b']);
});

test('thumbnail reveal waits while paused and resumes once', async () => {
  const store = new ThumbnailStore(1, async (path) => ({
    path,
    cacheHit: false,
    thumbnail: `thumb:${path}`,
  }));
  let notified = 0;
  store.subscribe('a', () => { notified += 1; });

  store.setRevealPaused(true);
  store.enqueueVisible(['a']);
  await new Promise((resolve) => setTimeout(resolve, 20));
  assert.equal(notified, 0);

  store.setRevealPaused(false);
  await new Promise((resolve) => setTimeout(resolve, 20));
  assert.equal(notified, 1);
});

test('thumbnail store removes a path after its last listener unsubscribes', () => {
  const store = new ThumbnailStore(1, async (path) => ({
    path,
    cacheHit: false,
    thumbnail: `thumb:${path}`,
  }));
  const unsubscribe = store.subscribe('gone', () => {});

  assert.equal(store.listenerPathCount(), 1);
  unsubscribe();
  assert.equal(store.listenerPathCount(), 0);
  unsubscribe();
  assert.equal(store.listenerPathCount(), 0);
});

test('an unsubscribe captured before reset cannot remove a new listener set', async () => {
  const store = new ThumbnailStore(1, async (path) => ({
    path,
    cacheHit: false,
    thumbnail: `thumb:${path}`,
  }));
  let notified = 0;
  const callback = () => { notified += 1; };
  const staleUnsubscribe = store.subscribe('same', callback);
  store.reset();
  store.subscribe('same', callback);

  staleUnsubscribe();
  assert.equal(store.listenerPathCount(), 1);

  store.enqueueVisible(['same']);
  await new Promise((resolve) => setTimeout(resolve, 20));
  assert.equal(notified, 1);
});
