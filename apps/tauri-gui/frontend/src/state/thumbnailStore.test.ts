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
