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
