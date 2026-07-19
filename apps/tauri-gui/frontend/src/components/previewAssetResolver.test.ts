import assert from 'node:assert/strict';
import test from 'node:test';

import { PreviewAssetResolver } from './previewAssetResolver.ts';

test('deduplicates concurrent authorization for one path', async () => {
  let calls = 0;
  const resolver = new PreviewAssetResolver(async (path) => {
    calls += 1;
    await Promise.resolve();
    return `/canonical${path}`;
  }, 8);

  assert.deepEqual(await Promise.all([
    resolver.resolve('/walls/a.jpg'),
    resolver.resolve('/walls/a.jpg'),
  ]), ['/canonical/walls/a.jpg', '/canonical/walls/a.jpg']);
  assert.equal(calls, 1);
});

test('forwards the indexed owner path used to authorize a recorded preview', async () => {
  const calls: Array<[string, string]> = [];
  const resolver = new PreviewAssetResolver(async (path, wallpaperPath) => {
    calls.push([path, wallpaperPath]);
    return path;
  });

  await resolver.resolve('/orphan/preview.jpg', '/orphan/project.json');

  assert.deepEqual(calls, [['/orphan/preview.jpg', '/orphan/project.json']]);
});

test('evicts a rejected authorization so the path can be retried', async () => {
  let calls = 0;
  const resolver = new PreviewAssetResolver(async (path) => {
    calls += 1;
    if (calls === 1) throw new Error('temporary authorization failure');
    return path;
  }, 8);

  await assert.rejects(resolver.resolve('/walls/retry.jpg'), /temporary/);
  assert.equal(await resolver.resolve('/walls/retry.jpg'), '/walls/retry.jpg');
  assert.equal(calls, 2);
});

test('evicts the oldest resolved entry when the cache reaches its bound', async () => {
  const calls: string[] = [];
  const resolver = new PreviewAssetResolver(async (path) => {
    calls.push(path);
    return path;
  }, 2);

  await resolver.resolve('/walls/a.jpg');
  await resolver.resolve('/walls/b.jpg');
  await resolver.resolve('/walls/c.jpg');
  await resolver.resolve('/walls/a.jpg');

  assert.deepEqual(calls, [
    '/walls/a.jpg',
    '/walls/b.jpg',
    '/walls/c.jpg',
    '/walls/a.jpg',
  ]);
});
