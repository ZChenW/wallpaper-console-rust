import assert from 'node:assert/strict';
import test from 'node:test';

import {
  resetSettingsContentScroll,
  scheduleSettingsContentScrollReset,
} from './settingsScroll.ts';

test('resetSettingsContentScroll moves the settings content back to top', () => {
  const target = { scrollTop: 480 };

  resetSettingsContentScroll(target);

  assert.equal(target.scrollTop, 0);
});

test('resetSettingsContentScroll ignores null targets', () => {
  assert.doesNotThrow(() => resetSettingsContentScroll(null));
});

test('scheduleSettingsContentScrollReset resets after animation frame', () => {
  const originalWindow = globalThis.window;
  const hadWindow = 'window' in globalThis;
  const target = { scrollTop: 320 };

  globalThis.window = {
    requestAnimationFrame: (cb: FrameRequestCallback) => {
      cb(0);
      return 1;
    },
  } as Window & typeof globalThis;

  try {
    scheduleSettingsContentScrollReset(target);
    assert.equal(target.scrollTop, 0);
  } finally {
    if (hadWindow) {
      globalThis.window = originalWindow;
    } else {
      delete (globalThis as { window?: Window }).window;
    }
  }
});
