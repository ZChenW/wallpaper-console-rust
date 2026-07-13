import assert from 'node:assert/strict';
import test from 'node:test';

import { resolveCardPointerInteraction } from './cardInteraction.ts';

test('single-click mode selects and applies exactly once across a physical double click', () => {
  assert.deepEqual(resolveCardPointerInteraction({
    gesture: 'single',
    clickCount: 1,
    canApply: true,
    fromControl: false,
  }), { select: true, apply: true });
  assert.deepEqual(resolveCardPointerInteraction({
    gesture: 'single',
    clickCount: 2,
    canApply: true,
    fromControl: false,
  }), { select: false, apply: false });
});

test('double-click mode selects on the first click and applies on the second', () => {
  assert.deepEqual(resolveCardPointerInteraction({
    gesture: 'double',
    clickCount: 1,
    canApply: true,
    fromControl: false,
  }), { select: true, apply: false });
  assert.deepEqual(resolveCardPointerInteraction({
    gesture: 'double',
    clickCount: 2,
    canApply: true,
    fromControl: false,
  }), { select: true, apply: true });
});

test('unsupported cards remain selectable but never apply', () => {
  for (const gesture of ['single', 'double'] as const) {
    assert.deepEqual(resolveCardPointerInteraction({
      gesture,
      clickCount: gesture === 'single' ? 1 : 2,
      canApply: false,
      fromControl: false,
    }), { select: true, apply: false });
  }
});

test('card controls and malformed click counts never select or apply', () => {
  for (const clickCount of [0, -1, 1.5, Number.NaN, Number.POSITIVE_INFINITY]) {
    assert.deepEqual(resolveCardPointerInteraction({
      gesture: 'single',
      clickCount,
      canApply: true,
      fromControl: false,
    }), { select: false, apply: false });
  }
  assert.deepEqual(resolveCardPointerInteraction({
    gesture: 'single',
    clickCount: 1,
    canApply: true,
    fromControl: true,
  }), { select: false, apply: false });
});

test('third and later clicks in one sequence are ignored', () => {
  assert.deepEqual(resolveCardPointerInteraction({
    gesture: 'double',
    clickCount: 3,
    canApply: true,
    fromControl: false,
  }), { select: false, apply: false });
});
