import assert from 'node:assert/strict';
import test from 'node:test';

import { addSuggestedDirectory } from './firstRunSourceActions.ts';

const success = { success: true, stdout: 'indexed', stderr: '', exitCode: 0 };

test('suggested directory add is explicitly scan-tracked and reconciled', async () => {
  const calls: string[] = [];
  const result = await addSuggestedDirectory(
    { sourceAdd: async (path) => {
      calls.push(`add:${path}`);
      return success;
    } },
    '/home/demo/Downloads',
    async () => { calls.push('reconcile'); },
    () => calls.push('started'),
    () => calls.push('finished'),
  );

  assert.equal(result, success);
  assert.deepEqual(calls, [
    'started',
    'add:/home/demo/Downloads',
    'reconcile',
    'finished',
  ]);
});

test('suggested directory add still reconciles and finishes after transport failure', async () => {
  const calls: string[] = [];
  const failure = new Error('transport disconnected');
  await assert.rejects(
    addSuggestedDirectory(
      { sourceAdd: async () => {
        calls.push('add');
        throw failure;
      } },
      '/home/demo/Downloads',
      async () => { calls.push('reconcile'); },
      () => calls.push('started'),
      () => calls.push('finished'),
    ),
    (error) => error === failure,
  );
  assert.deepEqual(calls, ['started', 'add', 'reconcile', 'finished']);
});
