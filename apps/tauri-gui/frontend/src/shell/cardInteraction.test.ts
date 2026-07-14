import assert from 'node:assert/strict';
import test from 'node:test';

import {
  cardInteractionClassName,
  resolveCardKeyboardInteraction,
  resolveCardPointerInteraction,
} from './cardInteraction.ts';

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

test('card visual state classes expose selection, pending, and current independently', () => {
  assert.equal(
    cardInteractionClassName({ selected: true, pending: true, current: false }),
    'wallpaper-card selected pending',
  );
  assert.equal(
    cardInteractionClassName({ selected: false, pending: false, current: true }),
    'wallpaper-card current',
  );
  assert.equal(
    cardInteractionClassName({ selected: false, pending: false, current: false }),
    'wallpaper-card',
  );
});

test('keyboard activation applies once and exposes the context menu key', () => {
  assert.deepEqual(resolveCardKeyboardInteraction({
    key: 'Enter',
    shiftKey: false,
    canApply: true,
  }), { select: true, apply: true, contextMenu: false });
  assert.deepEqual(resolveCardKeyboardInteraction({
    key: ' ',
    shiftKey: false,
    canApply: false,
  }), { select: true, apply: false, contextMenu: false });
  assert.deepEqual(resolveCardKeyboardInteraction({
    key: 'F10',
    shiftKey: true,
    canApply: true,
  }), { select: false, apply: false, contextMenu: true });
  assert.deepEqual(resolveCardKeyboardInteraction({
    key: 'ArrowRight',
    shiftKey: false,
    canApply: true,
  }), { select: false, apply: false, contextMenu: false });
});
