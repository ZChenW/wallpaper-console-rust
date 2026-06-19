import assert from 'node:assert/strict';
import test from 'node:test';

import { ThumbnailRequestQueue } from './thumbnailQueueCore.ts';
import type { ThumbnailDTO } from '../api/bridge.ts';

const thumb = (path: string): ThumbnailDTO => ({ path, thumbnail: `thumb:${path}`, cacheHit: false });

test('thumbnail queue deduplicates repeated paths', async () => {
  const loaded: string[] = [];
  const queue = new ThumbnailRequestQueue({
    concurrency: 2,
    load: async (path) => {
      loaded.push(path);
      return thumb(path);
    },
    onUpdate: () => {},
  });

  queue.enqueue(['a', 'a', 'b']);
  await new Promise((resolve) => setTimeout(resolve, 0));

  assert.deepEqual(loaded.sort(), ['a', 'b']);
});

test('thumbnail queue prioritizes front items before pending back items', async () => {
  const loaded: string[] = [];
  let releaseFirst: (() => void) | undefined;
  const firstLoad = new Promise<void>((resolve) => {
    releaseFirst = resolve;
  });
  const queue = new ThumbnailRequestQueue({
    concurrency: 1,
    load: async (path) => {
      loaded.push(path);
      if (path === 'a') await firstLoad;
      return thumb(path);
    },
    onUpdate: () => {},
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
  const firstLoad = new Promise<void>((resolve) => {
    releaseFirst = resolve;
  });
  const queue = new ThumbnailRequestQueue({
    concurrency: 1,
    load: async (path) => {
      loaded.push(path);
      if (path === 'a') await firstLoad;
      return thumb(path);
    },
    onUpdate: () => {},
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
  let latestState: Record<string, string> = {};
  const queue = new ThumbnailRequestQueue({
    concurrency: 1,
    load: async (path) => {
      await blocked;
      return thumb(path);
    },
    onUpdate: (state) => { latestState = state; },
  });

  queue.enqueue(['stale']);
  queue.reset();
  release?.();
  await new Promise((resolve) => setTimeout(resolve, 20));

  assert.deepEqual(latestState, {});
});

test('thumbnail queue ignores in-flight completion after forget', async () => {
  let release: (() => void) | undefined;
  const blocked = new Promise<void>((resolve) => { release = resolve; });
  let latestState: Record<string, string> = {};
  const queue = new ThumbnailRequestQueue({
    concurrency: 1,
    load: async (path) => {
      await blocked;
      return thumb(path);
    },
    onUpdate: (state) => { latestState = state; },
  });

  queue.enqueue(['x']);
  queue.forget(['x']);
  release?.();
  await new Promise((resolve) => setTimeout(resolve, 20));

  assert.deepEqual(latestState, {});
});

test('thumbnail queue re-enqueues after forget of in-flight path when version is newer', async () => {
  const loaded: string[] = [];
  let releaseFirst: (() => void) | undefined;
  const firstBlock = new Promise<void>((resolve) => { releaseFirst = resolve; });
  let latestState: Record<string, string> = {};

  const queue = new ThumbnailRequestQueue({
    concurrency: 1,
    load: async (path) => {
      loaded.push(path);
      if (path === 'x' && loaded.length === 1) await firstBlock;
      return thumb(path);
    },
    onUpdate: (state) => { latestState = state; },
  });

  queue.enqueue(['x']);
  queue.forget(['x']);
  queue.enqueue(['x']);
  releaseFirst?.();
  await new Promise((resolve) => setTimeout(resolve, 20));

  assert.deepEqual(loaded, ['x', 'x'], 'load should be called twice');
  assert.deepEqual(latestState, { x: 'thumb:x' }, 'latest state should have re-enqueued thumbnail');
});

test('thumbnail queue re-enqueues forgotten in-flight path with default concurrency', async () => {
  const loaded: string[] = [];
  let releaseFirst: (() => void) | undefined;
  const firstBlock = new Promise<void>((resolve) => { releaseFirst = resolve; });
  let latestState: Record<string, string> = {};

  const queue = new ThumbnailRequestQueue({
    concurrency: 2,
    load: async (path) => {
      loaded.push(path);
      if (path === 'x' && loaded.length === 1) await firstBlock;
      return thumb(path);
    },
    onUpdate: (state) => { latestState = state; },
  });

  queue.enqueue(['x']);
  queue.forget(['x']);
  queue.enqueue(['x']);
  releaseFirst?.();
  await new Promise((resolve) => setTimeout(resolve, 20));

  assert.deepEqual(loaded, ['x', 'x'], 'load should be called twice');
  assert.deepEqual(latestState, { x: 'thumb:x' }, 'latest state should have re-enqueued thumbnail');
});

test('thumbnail queue snapshot reports cached thumbnails', async () => {
  let latestState: Record<string, string> = {};
  const queue = new ThumbnailRequestQueue({
    concurrency: 1,
    load: async (path) => thumb(path),
    onUpdate: (state) => { latestState = state; },
  });

  queue.enqueue(['a']);
  await new Promise((resolve) => setTimeout(resolve, 20));

  assert.deepEqual(latestState, { a: 'thumb:a' });
  assert.equal(queue.snapshot().cached, 1);
});

test('thumbnail queue batches same-frame completions into a single emit', async () => {
  let emitCount = 0;
  let latestState: Record<string, string> = {};
  const queue = new ThumbnailRequestQueue({
    concurrency: 4,
    load: async (path) => thumb(path),
    onUpdate: (state) => { emitCount += 1; latestState = state; },
  });

  queue.enqueue(['a', 'b', 'c', 'd']);
  await new Promise((resolve) => setTimeout(resolve, 30));

  assert.deepEqual(
    latestState,
    { a: 'thumb:a', b: 'thumb:b', c: 'thumb:c', d: 'thumb:d' },
  );
  assert.equal(emitCount, 1, 'all same-frame completions should coalesce into one emit');
});

test('thumbnail queue still emits when completions land in separate frames', async () => {
  let emitCount = 0;
  let latestState: Record<string, string> = {};
  let releaseFirst: (() => void) | undefined;
  const firstBlock = new Promise<void>((resolve) => { releaseFirst = resolve; });
  const queue = new ThumbnailRequestQueue({
    concurrency: 2,
    load: async (path) => {
      if (path === 'a') await firstBlock;
      return thumb(path);
    },
    onUpdate: (state) => { emitCount += 1; latestState = state; },
  });

  queue.enqueue(['a', 'b']);
  await new Promise((resolve) => setTimeout(resolve, 20));
  // 'b' completed in the first frame; 'a' is still blocked.
  assert.deepEqual(latestState, { b: 'thumb:b' });

  releaseFirst?.();
  await new Promise((resolve) => setTimeout(resolve, 20));
  assert.deepEqual(latestState, { a: 'thumb:a', b: 'thumb:b' });
  assert.ok(emitCount >= 2, 'separate-frame completions each emit');
});
