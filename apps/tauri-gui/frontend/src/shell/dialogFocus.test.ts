import assert from 'node:assert/strict';
import test from 'node:test';

import { wrappedDialogFocusIndex } from './dialogFocus.ts';

test('dialog focus wraps only at its keyboard boundaries', () => {
  assert.equal(wrappedDialogFocusIndex(0, 4, true), 3);
  assert.equal(wrappedDialogFocusIndex(3, 4, false), 0);
  assert.equal(wrappedDialogFocusIndex(1, 4, false), null);
  assert.equal(wrappedDialogFocusIndex(2, 4, true), null);
});

test('dialog focus enters from outside and safely handles empty dialogs', () => {
  assert.equal(wrappedDialogFocusIndex(-1, 3, false), 0);
  assert.equal(wrappedDialogFocusIndex(-1, 3, true), 2);
  assert.equal(wrappedDialogFocusIndex(-1, 0, false), null);
});
