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

test('forget clears cached thumbnails and notifies subscribers', async () => {
  const store = new ThumbnailStore(1, async (path) => ({
    path,
    cacheHit: false,
    thumbnail: `thumb:${path}`,
  }));
  let notified = 0;
  store.subscribe('a', () => { notified += 1; });

  store.enqueueVisible(['a']);
  await new Promise((resolve) => setTimeout(resolve, 20));
  assert.equal(store.get('a'), 'thumb:a');
  const notifiedBeforeForget = notified;

  store.forget(['a']);
  assert.equal(store.get('a'), undefined);
  await new Promise((resolve) => setTimeout(resolve, 20));
  assert.ok(notified > notifiedBeforeForget);
});

test('reset clears cache and notifies listeners without removing subscriptions', async () => {
  const store = new ThumbnailStore(1, async (path) => ({
    path,
    cacheHit: false,
    thumbnail: `thumb:${path}`,
  }));
  let notified = 0;
  store.subscribe('same', () => { notified += 1; });

  store.enqueueVisible(['same']);
  await new Promise((resolve) => setTimeout(resolve, 20));
  assert.equal(store.get('same'), 'thumb:same');
  assert.equal(store.listenerPathCount(), 1);

  store.reset();
  assert.equal(store.get('same'), undefined);
  assert.equal(store.listenerPathCount(), 1);
  await new Promise((resolve) => setTimeout(resolve, 20));
  assert.ok(notified >= 2);

  store.enqueueVisible(['same']);
  await new Promise((resolve) => setTimeout(resolve, 20));
  assert.equal(store.get('same'), 'thumb:same');
});

test('refresh keeps subscribed thumbnails visible until replacements arrive', async () => {
  let callCount = 0;
  let releaseRefresh: (() => void) | undefined;
  const refreshBlocked = new Promise<void>((resolve) => { releaseRefresh = resolve; });
  const store = new ThumbnailStore(1, async (path) => {
    callCount += 1;
    if (callCount === 2) await refreshBlocked;
    return { path, cacheHit: false, thumbnail: `thumb:${path}:${callCount}` };
  });
  let notified = 0;
  store.subscribe('same', () => { notified += 1; });

  store.enqueueVisible(['same']);
  await new Promise((resolve) => setTimeout(resolve, 20));
  assert.equal(store.get('same'), 'thumb:same:1');

  store.refreshSubscribed();
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(store.get('same'), 'thumb:same:1');
  assert.equal(store.stats().active, 1);

  releaseRefresh?.();
  await new Promise((resolve) => setTimeout(resolve, 20));
  assert.equal(store.get('same'), 'thumb:same:2');
  assert.ok(notified >= 2);
});

test('refresh removes a subscribed fallback when regeneration explicitly fails', async () => {
  let callCount = 0;
  const store = new ThumbnailStore(1, async (path) => {
    callCount += 1;
    return callCount === 1
      ? { path, cacheHit: false, thumbnail: `thumb:${path}:old` }
      : { path, cacheHit: false, failureReason: 'decode_failed' };
  });
  store.subscribe('same', () => undefined);

  store.enqueueVisible(['same']);
  await new Promise((resolve) => setTimeout(resolve, 20));
  assert.equal(store.get('same'), 'thumb:same:old');

  store.refreshSubscribed();
  await new Promise((resolve) => setTimeout(resolve, 20));

  assert.equal(store.get('same'), undefined, 'an explicit refresh failure must not leave stale media');
  assert.equal(store.getFailure('same'), 'decode_failed');
});

test('refresh drops unobserved cache while preserving subscribed fallback', async () => {
  let generation = 0;
  let releaseRefresh: (() => void) | undefined;
  const refreshBlocked = new Promise<void>((resolve) => { releaseRefresh = resolve; });
  const store = new ThumbnailStore(1, async (path) => {
    generation += 1;
    if (generation === 3) await refreshBlocked;
    return { path, cacheHit: false, thumbnail: `thumb:${path}:${generation}` };
  });
  store.subscribe('keep', () => undefined);

  store.enqueueVisible(['keep', 'stale']);
  await new Promise((resolve) => setTimeout(resolve, 30));
  assert.equal(store.get('keep'), 'thumb:keep:1');
  assert.equal(store.get('stale'), 'thumb:stale:2');

  store.refreshSubscribed();
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(store.get('keep'), 'thumb:keep:1');
  assert.equal(store.get('stale'), undefined);

  releaseRefresh?.();
  await new Promise((resolve) => setTimeout(resolve, 20));
  assert.equal(store.get('keep'), 'thumb:keep:3');
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

test('thumbnail store bounds long-session cache and reloads an evicted path', async () => {
  const loads: string[] = [];
  const store = new ThumbnailStore(1, async (path) => {
    loads.push(path);
    return { path, cacheHit: false, thumbnail: `thumb:${path}:${loads.length}` };
  }, 2);

  store.enqueueVisible(['a', 'b', 'c']);
  await new Promise((resolve) => setTimeout(resolve, 30));

  assert.equal(store.stats().cached, 2);
  assert.equal(store.snapshot().cached, 2, 'queue cache must evict with the public cache');
  assert.equal(store.get('a'), undefined);
  assert.equal(store.get('b'), 'thumb:b:2');
  assert.equal(store.get('c'), 'thumb:c:3');

  store.enqueueVisible(['a']);
  await new Promise((resolve) => setTimeout(resolve, 20));
  assert.equal(loads.filter((path) => path === 'a').length, 2);
  assert.equal(store.stats().cached, 2);
});
