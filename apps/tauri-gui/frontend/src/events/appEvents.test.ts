import assert from 'node:assert/strict';
import test from 'node:test';
import { APP_EVENTS, emitFeedback, onFeedback } from './appEvents.ts';
import type { CommandFeedback } from '../api/feedback';

test('APP_EVENTS preserves public event names', () => {
  assert.equal(APP_EVENTS.feedback, 'wc-feedback');
  assert.equal(APP_EVENTS.applyStage, 'wc-apply-stage');
  assert.equal(APP_EVENTS.configChanged, 'wc-config-changed');
  assert.equal(APP_EVENTS.favoritesInvalidated, 'favorites-cache-invalidated');
});

test('emitFeedback notifies typed feedback listeners', () => {
  const globals = globalThis as typeof globalThis & { window?: EventTarget };
  const originalWindow = globals.window;
  const target = new EventTarget();
  globals.window = target as typeof globals.window;
  const seen: CommandFeedback[] = [];

  try {
    const off = onFeedback((feedback) => seen.push(feedback));
    emitFeedback({ state: 'success', label: 'Saved', detail: 'ok' });
    off();
  } finally {
    globals.window = originalWindow;
  }

  assert.deepEqual(seen, [{ state: 'success', label: 'Saved', detail: 'ok' }]);
});
