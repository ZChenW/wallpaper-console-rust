import assert from 'node:assert/strict';
import test from 'node:test';

import {
  clampIndexDialogActive,
  resolveIndexDialogKey,
} from './flowIndexDialogModel.ts';
import * as indexDialogModel from './flowIndexDialogModel.ts';

test('expanded index keyboard movement is finite and direct', () => {
  assert.deepEqual(resolveIndexDialogKey('ArrowDown', 2, 5), { kind: 'focus', index: 3 });
  assert.deepEqual(resolveIndexDialogKey('ArrowDown', 4, 5), { kind: 'focus', index: 4 });
  assert.deepEqual(resolveIndexDialogKey('ArrowUp', 0, 5), { kind: 'focus', index: 0 });
  assert.deepEqual(resolveIndexDialogKey('Home', 3, 5), { kind: 'focus', index: 0 });
  assert.deepEqual(resolveIndexDialogKey('End', 1, 5), { kind: 'focus', index: 4 });
  assert.deepEqual(resolveIndexDialogKey('Enter', 3, 5), { kind: 'activate', index: 3 });
  assert.deepEqual(resolveIndexDialogKey(' ', 3, 5), { kind: 'activate', index: 3 });
});

test('expanded index closes only on Escape and leaves Tab traversal native', () => {
  assert.deepEqual(resolveIndexDialogKey('Escape', 2, 5), { kind: 'close' });
  assert.equal(resolveIndexDialogKey('Tab', 2, 5), null);
  assert.equal(resolveIndexDialogKey('F10', 2, 5), null);
});

test('expanded index safely handles empty and replaced loaded collections', () => {
  assert.equal(clampIndexDialogActive(9, 3), 2);
  assert.equal(clampIndexDialogActive(-4, 3), 0);
  assert.equal(clampIndexDialogActive(4, 0), -1);
  assert.equal(resolveIndexDialogKey('Enter', -1, 0), null);
  assert.equal(resolveIndexDialogKey('ArrowDown', -1, 0), null);
});

test('expanded index initializes focus only on the closed-to-open transition', () => {
  const model = indexDialogModel as unknown as {
    shouldInitializeIndexDialogFocus?: (wasOpen: boolean, open: boolean) => boolean;
  };
  assert.equal(typeof model.shouldInitializeIndexDialogFocus, 'function');
  const shouldInitialize = model.shouldInitializeIndexDialogFocus!;

  assert.equal(shouldInitialize(false, false), false);
  assert.equal(shouldInitialize(false, true), true);
  assert.equal(shouldInitialize(true, true), false, 'an append while open must preserve focus');
  assert.equal(shouldInitialize(true, false), false);
});
