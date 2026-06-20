import assert from 'node:assert/strict';
import test from 'node:test';

import { ThumbnailRequestQueue } from './thumbnailQueueCore.ts';
import type { ThumbnailDTO } from '../api/bridge.ts';

const thumb = (path: string): ThumbnailDTO => ({ path, thumbnail: `thumb:${path}`, cacheHit: false });

function makeHandlers() {
  const thumbnails: Record<string, string> = {};
  const failures: Record<string, string | undefined> = {};
  return {
    onThumbnail: (path: string, t: string) => { thumbnails[path] = t; },
    onFailure: (path: string, reason?: string) => { failures[path] = reason; },
    thumbnails,
    failures,
  };
}

test('thumbnail queue deduplicates repeated paths', async () => {
  const loaded: string[] = [];
  const h = makeHandlers();
  const queue = new ThumbnailRequestQueue({
    concurrency: 2,
    load: async (path) => { loaded.push(path); return thumb(path); },
    onThumbnail: h.onThumbnail,
    onFailure: h.onFailure,
  });

  queue.enqueue(['a', 'a', 'b']);
  await new Promise((resolve) => setTimeout(resolve, 20));

  assert.deepEqual(loaded.sort(), ['a', 'b']);
});

test('thumbnail queue prioritizes front items before pending back items', async () => {
  const loaded: string[] = [];
  let releaseFirst: (() => void) | undefined;
  const firstLoad = new Promise<void>((resolve) => { releaseFirst = resolve; });
  const h = makeHandlers();
  const queue = new ThumbnailRequestQueue({
    concurrency: 1,
    load: async (path) => { loaded.push(path); if (path === 'a') await firstLoad; return thumb(path); },
    onThumbnail: h.onThumbnail,
    onFailure: h.onFailure,
  });

  queue.enqueue(['a', 'b']);
  queue.enqueue(['visible'], { priority: 'front' });
  releaseFirst?.();
  await new Promise((resolve) => setTimeout(resolve, 20));

  assert.deepEqual(loaded, ['a', 'visible', 'b']);
});

test('thumbnail queue forget removes pending item before re-enqueue', async () => {
  const loaded: string[] = [];
  let releaseFirst: (() => void) | undefined;
  const firstLoad = new Promise<void>((resolve) => { releaseFirst = resolve; });
  const h = makeHandlers();
  const queue = new ThumbnailRequestQueue({
    concurrency: 1,
    load: async (path) => { loaded.push(path); if (path === 'a') await firstLoad; return thumb(path); },
    onThumbnail: h.onThumbnail,
    onFailure: h.onFailure,
  });

  queue.enqueue(['a', 'x']);
  queue.forget(['x']);
  queue.enqueue(['x'], { priority: 'front' });
  releaseFirst?.();
  await new Promise((resolve) => setTimeout(resolve, 20));

  assert.deepEqual(loaded, ['a', 'x']);
});

test('thumbnail queue ignores in-flight completion after reset', async () => {
  let release: (() => void) | undefined;
  const blocked = new Promise<void>((resolve) => { release = resolve; });
  const h = makeHandlers();
  const queue = new ThumbnailRequestQueue({
    concurrency: 1,
    load: async (path) => { await blocked; return thumb(path); },
    onThumbnail: h.onThumbnail,
    onFailure: h.onFailure,
  });

  queue.enqueue(['stale']);
  queue.reset();
  release?.();
  await new Promise((resolve) => setTimeout(resolve, 20));

  assert.deepEqual(h.thumbnails, {});
});

test('thumbnail queue ignores in-flight completion after forget', async () => {
  let release: (() => void) | undefined;
  const blocked = new Promise<void>((resolve) => { release = resolve; });
  const h = makeHandlers();
  const queue = new ThumbnailRequestQueue({
    concurrency: 1,
    load: async (path) => { await blocked; return thumb(path); },
    onThumbnail: h.onThumbnail,
    onFailure: h.onFailure,
  });

  queue.enqueue(['x']);
  queue.forget(['x']);
  release?.();
  await new Promise((resolve) => setTimeout(resolve, 20));

  assert.deepEqual(h.thumbnails, {});
});

test('thumbnail queue re-enqueues after forget of in-flight path when version is newer', async () => {
  const loaded: string[] = [];
  let releaseFirst: (() => void) | undefined;
  const firstBlock = new Promise<void>((resolve) => { releaseFirst = resolve; });
  const h = makeHandlers();

  const queue = new ThumbnailRequestQueue({
    concurrency: 1,
    load: async (path) => { loaded.push(path); if (path === 'x' && loaded.length === 1) await firstBlock; return thumb(path); },
    onThumbnail: h.onThumbnail,
    onFailure: h.onFailure,
  });

  queue.enqueue(['x']);
  queue.forget(['x']);
  queue.enqueue(['x']);
  releaseFirst?.();
  await new Promise((resolve) => setTimeout(resolve, 20));

  assert.deepEqual(loaded, ['x', 'x'], 'load should be called twice');
  assert.deepEqual(h.thumbnails, { x: 'thumb:x' }, 'should have re-enqueued thumbnail');
});

test('thumbnail queue re-enqueues forgotten in-flight path with default concurrency', async () => {
  const loaded: string[] = [];
  let releaseFirst: (() => void) | undefined;
  const firstBlock = new Promise<void>((resolve) => { releaseFirst = resolve; });
  const h = makeHandlers();

  const queue = new ThumbnailRequestQueue({
    concurrency: 2,
    load: async (path) => { loaded.push(path); if (path === 'x' && loaded.length === 1) await firstBlock; return thumb(path); },
    onThumbnail: h.onThumbnail,
    onFailure: h.onFailure,
  });

  queue.enqueue(['x']);
  queue.forget(['x']);
  queue.enqueue(['x']);
  releaseFirst?.();
  await new Promise((resolve) => setTimeout(resolve, 20));

  assert.deepEqual(loaded, ['x', 'x'], 'load should be called twice');
  assert.deepEqual(h.thumbnails, { x: 'thumb:x' }, 'should have re-enqueued thumbnail');
});

test('thumbnail queue snapshot reports cached thumbnails', async () => {
  const h = makeHandlers();
  const queue = new ThumbnailRequestQueue({
    concurrency: 1,
    load: async (path) => thumb(path),
    onThumbnail: h.onThumbnail,
    onFailure: h.onFailure,
  });

  queue.enqueue(['a']);
  await new Promise((resolve) => setTimeout(resolve, 20));

  assert.deepEqual(h.thumbnails, { a: 'thumb:a' });
  assert.equal(queue.snapshot().cached, 1);
});

test('thumbnail queue get returns cached value synchronously', async () => {
  const h = makeHandlers();
  const queue = new ThumbnailRequestQueue({
    concurrency: 1,
    load: async (path) => thumb(path),
    onThumbnail: h.onThumbnail,
    onFailure: h.onFailure,
  });

  queue.enqueue(['a']);
  await new Promise((resolve) => setTimeout(resolve, 20));

  assert.equal(queue.get('a'), 'thumb:a');
  assert.equal(queue.get('b'), undefined);
});

test('thumbnail queue onThumbnail called per-path on completion', async () => {
  const h = makeHandlers();
  const queue = new ThumbnailRequestQueue({
    concurrency: 4,
    load: async (path) => thumb(path),
    onThumbnail: h.onThumbnail,
    onFailure: h.onFailure,
  });

  queue.enqueue(['a', 'b', 'c', 'd']);
  await new Promise((resolve) => setTimeout(resolve, 30));

  assert.deepEqual(h.thumbnails, {
    a: 'thumb:a', b: 'thumb:b', c: 'thumb:c', d: 'thumb:d',
  });
});

test('thumbnail queue keeps duplicate checks cheap with a large backlog', async () => {
  let releaseFirst: (() => void) | undefined;
  const firstLoad = new Promise<void>((resolve) => { releaseFirst = resolve; });
  const loaded: string[] = [];
  const h = makeHandlers();
  const queue = new ThumbnailRequestQueue({
    concurrency: 1,
    load: async (path) => {
      loaded.push(path);
      if (path === 'hold') await firstLoad;
      return thumb(path);
    },
    onThumbnail: h.onThumbnail,
    onFailure: h.onFailure,
  });

  queue.enqueue(['hold', ...Array.from({ length: 500 }, (_, i) => `p-${i}`)]);
  queue.enqueue(Array.from({ length: 500 }, (_, i) => `p-${i}`), { priority: 'front' });

  const snap = queue.stats();
  assert.equal(snap.pending, 500);

  releaseFirst?.();
  await new Promise((resolve) => setTimeout(resolve, 30));
  assert.equal(loaded[0], 'hold');
});

test('thumbnail queue keeps queuedPaths in sync when re-enqueueing forgotten in-flight path', async () => {
  let releaseX: (() => void) | undefined;
  const xBlock = new Promise<void>((resolve) => { releaseX = resolve; });
  const h = makeHandlers();
  const queue = new ThumbnailRequestQueue({
    concurrency: 2,
    load: async (path) => {
      if (path === 'x') await xBlock;
      return thumb(path);
    },
    onThumbnail: h.onThumbnail,
    onFailure: h.onFailure,
  });

  queue.enqueue(['x']);
  await new Promise((resolve) => setTimeout(resolve, 0));
  queue.forget(['x']);
  queue.enqueue(['x']);
  queue.enqueue(['x']);

  assert.equal(queue.stats().pending, 1);
  assert.deepEqual(queue.snapshot().pending, ['x']);

  releaseX?.();
  await new Promise((resolve) => setTimeout(resolve, 30));
});

test('thumbnail queue still emits when completions land in separate frames', async () => {
  const h = makeHandlers();
  let releaseFirst: (() => void) | undefined;
  const firstBlock = new Promise<void>((resolve) => { releaseFirst = resolve; });
  const queue = new ThumbnailRequestQueue({
    concurrency: 2,
    load: async (path) => { if (path === 'a') await firstBlock; return thumb(path); },
    onThumbnail: h.onThumbnail,
    onFailure: h.onFailure,
  });

  queue.enqueue(['a', 'b']);
  await new Promise((resolve) => setTimeout(resolve, 20));
  assert.deepEqual(h.thumbnails, { b: 'thumb:b' });

  releaseFirst?.();
  await new Promise((resolve) => setTimeout(resolve, 20));
  assert.deepEqual(h.thumbnails, { a: 'thumb:a', b: 'thumb:b' });
});
