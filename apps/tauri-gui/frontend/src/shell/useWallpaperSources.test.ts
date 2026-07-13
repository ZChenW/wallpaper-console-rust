import assert from 'node:assert/strict';
import test from 'node:test';

import {
  executeSourceMutation,
  formatSourceLoadError,
} from './useWallpaperSources.ts';

const success = {
  success: true,
  stdout: 'saved',
  stderr: '',
  exitCode: 0,
};

test('source mutations reconcile sources and library even when command reports partial failure', async () => {
  const calls: string[] = [];
  const partialFailure = {
    success: false,
    stdout: 'source saved',
    stderr: 'refresh cancelled',
    exitCode: 1,
  };

  const result = await executeSourceMutation(
    async () => {
      calls.push('mutate');
      return partialFailure;
    },
    async () => {
      calls.push('reconcile');
    },
  );

  assert.equal(result, partialFailure);
  assert.deepEqual(calls, ['mutate', 'reconcile']);
});

test('source mutation preserves the primary exception after best-effort reconciliation', async () => {
  const primary = new Error('transport disconnected');
  let reconciled = false;

  await assert.rejects(
    executeSourceMutation(
      async () => Promise.reject(primary),
      async () => {
        reconciled = true;
        throw new Error('secondary reload failure');
      },
    ),
    (error) => error === primary,
  );
  assert.equal(reconciled, true);
});

test('successful source mutation is not masked by reconciliation failure', async () => {
  assert.equal(
    await executeSourceMutation(
      async () => success,
      async () => Promise.reject(new Error('reload failed')),
    ),
    success,
  );
});

test('source load errors preserve useful text and provide a stable fallback', () => {
  assert.equal(formatSourceLoadError(new Error('database is locked')), 'database is locked');
  assert.equal(formatSourceLoadError('permission denied'), 'permission denied');
  assert.equal(formatSourceLoadError({ code: 1 }), 'Failed to load wallpaper sources');
});
