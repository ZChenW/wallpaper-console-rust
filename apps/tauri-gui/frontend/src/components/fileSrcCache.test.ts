import assert from 'node:assert/strict';
import test from 'node:test';

import { BoundedFileSrcCache, DEFAULT_FILE_SRC_CACHE_MAX } from './fileSrcCache.ts';

test('BoundedFileSrcCache caches resolved values across gets', () => {
  let calls = 0;
  const cache = new BoundedFileSrcCache((p) => { calls += 1; return `src:${p}`; });
  assert.equal(cache.get('/a.jpg'), 'src:/a.jpg');
  assert.equal(cache.get('/a.jpg'), 'src:/a.jpg');
  assert.equal(calls, 1, 'resolver called once for repeated path');
  assert.equal(cache.size, 1);
});

test('BoundedFileSrcCache evicts oldest entry when exceeding max', () => {
  const cache = new BoundedFileSrcCache((p) => `src:${p}`, 3);
  cache.get('/a');
  cache.get('/b');
  cache.get('/c');
  assert.equal(cache.size, 3);
  cache.get('/d');
  assert.equal(cache.size, 3, 'should not exceed max');
  // '/a' was the oldest and should have been evicted; re-getting it calls resolver again.
  let resolved = false;
  const cache2 = new BoundedFileSrcCache((p) => { resolved = true; return `src:${p}`; }, 3);
  cache2.get('/a');
  cache2.get('/b');
  cache2.get('/c');
  cache2.get('/d');
  resolved = false;
  cache2.get('/a');
  assert.equal(resolved, true, 'evicted oldest entry is re-resolved');
});

test('BoundedFileSrcCache promotes recently accessed entries (LRU)', () => {
  const cache = new BoundedFileSrcCache((p) => `src:${p}`, 3);
  cache.get('/a');
  cache.get('/b');
  cache.get('/c');
  // Access '/a' to promote it as most-recently-used.
  cache.get('/a');
  cache.get('/d'); // should evict '/b' (now oldest), not '/a'
  let resolved = false;
  const probe = new BoundedFileSrcCache((p) => { resolved = true; return `src:${p}`; }, 3);
  probe.get('/a');
  probe.get('/b');
  probe.get('/c');
  probe.get('/a');
  probe.get('/d');
  resolved = false;
  probe.get('/a');
  assert.equal(resolved, false, 'promoted /a should not be evicted by /d insert');
});

test('BoundedFileSrcCache clear empties the cache', () => {
  const cache = new BoundedFileSrcCache((p) => `src:${p}`);
  cache.get('/a');
  cache.get('/b');
  assert.equal(cache.size, 2);
  cache.clear();
  assert.equal(cache.size, 0);
});

test('DEFAULT_FILE_SRC_CACHE_MAX is a positive number', () => {
  assert.ok(DEFAULT_FILE_SRC_CACHE_MAX > 0);
});
