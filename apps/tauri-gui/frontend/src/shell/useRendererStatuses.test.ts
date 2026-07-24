import assert from 'node:assert/strict';
import test from 'node:test';

import type { RendererStatusesDTO } from '../api/types.ts';
import {
  createRendererStatusRequestSequence,
  loadRendererStatuses,
  rendererStatusErrorMessage,
} from './useRendererStatuses.ts';

const statuses: RendererStatusesDTO = {
  awww: { available: true, message: 'awww is installed.' },
  mpvpaper: { available: false, message: 'mpvpaper is unavailable.' },
  linuxWallpaperEngine: {
    available: true,
    message: 'linux-wallpaperengine is installed.',
  },
};

test('renderer status loader performs one unified read-only request', async () => {
  let calls = 0;
  const loaded = await loadRendererStatuses({
    rendererStatuses: async () => {
      calls += 1;
      return statuses;
    },
  });

  assert.equal(calls, 1);
  assert.equal(loaded, statuses);
});

test('renderer status errors preserve useful text and have a stable fallback', () => {
  assert.equal(rendererStatusErrorMessage(new Error('renderer probe failed')), 'renderer probe failed');
  assert.equal(rendererStatusErrorMessage('bridge unavailable'), 'bridge unavailable');
  assert.equal(rendererStatusErrorMessage({ code: 1 }), 'Renderer status is unavailable');
});

test('renderer status reads have a bounded detection deadline', async () => {
  await assert.rejects(
    loadRendererStatuses(
      { rendererStatuses: () => new Promise(() => {}) },
      10,
    ),
    /Renderer detection timed out after 10ms/,
  );
});

test('renderer status request sequence rejects stale and invalidated loads', () => {
  const sequence = createRendererStatusRequestSequence();
  const first = sequence.begin();
  const second = sequence.begin();

  assert.equal(sequence.isLatest(first), false);
  assert.equal(sequence.isLatest(second), true);
  sequence.invalidate();
  assert.equal(sequence.isLatest(second), false);
});
