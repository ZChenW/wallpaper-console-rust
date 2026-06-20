import assert from 'node:assert/strict';
import test from 'node:test';

import { ThumbnailStore } from './thumbnailStore.ts';
import type { ThumbnailDTO } from '../api/bridge.ts';

test('thumbnail store batches visible notifications into one frame', async () => {
  const originalRaf = globalThis.requestAnimationFrame;
  const callbacks: FrameRequestCallback[] = [];
  globalThis.requestAnimationFrame = ((cb: FrameRequestCallback) => {
    callbacks.push(cb);
    return callbacks.length;
  }) as typeof requestAnimationFrame;

  try {
    const store = new ThumbnailStore(2, async (path): Promise<ThumbnailDTO> => ({
      path,
      thumbnail: `thumb:${path}`,
      cacheHit: false,
    }));
    let calls = 0;
    store.subscribe('a', () => { calls += 1; });
    store.subscribe('b', () => { calls += 1; });
    store.enqueueVisible(['a', 'b']);

    await new Promise((resolve) => setTimeout(resolve, 0));
    assert.equal(calls, 0);
    assert.equal(callbacks.length >= 1, true);

    for (const cb of callbacks.splice(0)) cb(performance.now());
    await new Promise((resolve) => setTimeout(resolve, 0));
    for (const cb of callbacks.splice(0)) cb(performance.now());

    assert.equal(calls, 2);
  } finally {
    globalThis.requestAnimationFrame = originalRaf;
  }
});

test('thumbnail store defers listener notifications while reveal is paused', async () => {
  const originalRaf = globalThis.requestAnimationFrame;
  const callbacks: FrameRequestCallback[] = [];
  globalThis.requestAnimationFrame = ((cb: FrameRequestCallback) => {
    callbacks.push(cb);
    return callbacks.length;
  }) as typeof requestAnimationFrame;

  try {
    const store = new ThumbnailStore(1, async (path): Promise<ThumbnailDTO> => ({
      path,
      thumbnail: `thumb:${path}`,
      cacheHit: false,
    }));
    let calls = 0;
    store.subscribe('a', () => { calls += 1; });
    store.setRevealPaused(true);
    store.enqueueVisible(['a']);
    await new Promise((resolve) => setTimeout(resolve, 0));
    for (const cb of callbacks.splice(0)) cb(performance.now());
    await new Promise((resolve) => setTimeout(resolve, 0));
    assert.equal(calls, 0);

    store.setRevealPaused(false);
    for (const cb of callbacks.splice(0)) cb(performance.now());
    await new Promise((resolve) => setTimeout(resolve, 0));
    assert.equal(calls, 1);
  } finally {
    globalThis.requestAnimationFrame = originalRaf;
  }
});

test('thumbnail store defers already scheduled rAF notifications when reveal pauses before flush', async () => {
  const originalRaf = globalThis.requestAnimationFrame;
  const callbacks: FrameRequestCallback[] = [];
  globalThis.requestAnimationFrame = ((cb: FrameRequestCallback) => {
    callbacks.push(cb);
    return callbacks.length;
  }) as typeof requestAnimationFrame;

  try {
    const store = new ThumbnailStore(1, async (path): Promise<ThumbnailDTO> => ({
      path,
      thumbnail: `thumb:${path}`,
      cacheHit: false,
    }));
    let calls = 0;
    store.subscribe('a', () => { calls += 1; });

    store.enqueueVisible(['a']);
    await new Promise((resolve) => setTimeout(resolve, 0));
    for (const cb of callbacks.splice(0)) cb(performance.now());
    await new Promise((resolve) => setTimeout(resolve, 0));

    assert.equal(callbacks.length >= 1, true);
    store.setRevealPaused(true);
    for (const cb of callbacks.splice(0)) cb(performance.now());
    await new Promise((resolve) => setTimeout(resolve, 0));
    assert.equal(calls, 0);

    store.setRevealPaused(false);
    for (const cb of callbacks.splice(0)) cb(performance.now());
    await new Promise((resolve) => setTimeout(resolve, 0));
    assert.equal(calls, 1);
  } finally {
    globalThis.requestAnimationFrame = originalRaf;
  }
});

test('thumbnail store reveals paused paths across multiple frames', async () => {
  const originalRaf = globalThis.requestAnimationFrame;
  const callbacks: FrameRequestCallback[] = [];
  globalThis.requestAnimationFrame = ((cb: FrameRequestCallback) => {
    callbacks.push(cb);
    return callbacks.length;
  }) as typeof requestAnimationFrame;

  const flushRaf = () => {
    for (const cb of callbacks.splice(0)) cb(performance.now());
  };

  try {
    const paths = Array.from({ length: 30 }, (_, i) => `p${i}`);
    const store = new ThumbnailStore(30, async (path): Promise<ThumbnailDTO> => ({
      path,
      thumbnail: `thumb:${path}`,
      cacheHit: false,
    }));
    const calls = new Map<string, number>();
    for (const path of paths) {
      store.subscribe(path, () => {
        calls.set(path, (calls.get(path) ?? 0) + 1);
      });
    }

    store.setRevealPaused(true);
    store.enqueueVisible(paths);
    await new Promise((resolve) => setTimeout(resolve, 0));
    flushRaf();
    await new Promise((resolve) => setTimeout(resolve, 0));
    flushRaf();
    await new Promise((resolve) => setTimeout(resolve, 0));
    assert.equal(calls.size, 0);

    store.setRevealPaused(false);
    flushRaf();
    assert.equal(calls.size, 12);

    flushRaf();
    assert.equal(calls.size, 24);

    flushRaf();
    assert.equal(calls.size, 30);
    for (const path of paths) {
      assert.equal(calls.get(path), 1);
    }
  } finally {
    globalThis.requestAnimationFrame = originalRaf;
  }
});
