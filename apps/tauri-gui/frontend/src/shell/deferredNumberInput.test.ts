import assert from 'node:assert/strict';
import test from 'node:test';

import { committedNumberDraft } from './deferredNumberInput.ts';

test('committedNumberDraft preserves confirmed for empty and invalid drafts', () => {
  assert.equal(committedNumberDraft('', 60), 60);
  assert.equal(committedNumberDraft('   ', 60), 60);
  assert.equal(committedNumberDraft('NaN', 60), 60);
  assert.equal(committedNumberDraft('-', 60), 60);
  assert.equal(committedNumberDraft('.', 60), 60);
  assert.equal(committedNumberDraft('Infinity', 60), 60);
});

test('committedNumberDraft accepts finite decimal and integer drafts', () => {
  assert.equal(committedNumberDraft('144', 60), 144);
  assert.equal(committedNumberDraft('2.5', 1), 2.5);
  assert.equal(committedNumberDraft('-3', 0), -3);
  assert.equal(committedNumberDraft(' 12 ', 0), 12);
});
