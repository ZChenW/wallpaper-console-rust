import assert from 'node:assert/strict';
import test from 'node:test';

import {
  shouldTickFeedbackClock,
  translateCommandFeedback,
} from './useShellFeedback.ts';
import { EMPTY_FEEDBACK_STATE, feedbackReducer } from './feedbackState.ts';

test('running command feedback stays in the compact status channel', () => {
  assert.deepEqual(translateCommandFeedback({
    state: 'running',
    label: 'Starting renderer',
    detail: 'linux-wallpaperengine is settling',
  }, 'apply'), {
    kind: 'running',
    channel: 'apply',
    message: 'Starting renderer',
    technicalDetails: 'linux-wallpaperengine is settling',
  });
});

test('completed command feedback becomes a concise notice with details separated', () => {
  assert.deepEqual(translateCommandFeedback({
    state: 'success',
    label: 'Applied',
    detail: 'scene-preview.gif',
  }, 'apply'), {
    kind: 'notice',
    channel: 'apply',
    severity: 'success',
    message: 'Applied: scene-preview.gif',
    technicalDetails: undefined,
  });

  assert.deepEqual(translateCommandFeedback({
    state: 'error',
    label: 'Apply failed',
    detail: 'renderer exited 1\nInstall linux-wallpaperengine',
  }, 'apply'), {
    kind: 'notice',
    channel: 'apply',
    severity: 'error',
    message: 'Apply failed',
    technicalDetails: 'renderer exited 1\nInstall linux-wallpaperengine',
  });

  assert.deepEqual(translateCommandFeedback({
    state: 'success',
    label: 'Source refreshed',
    detail: 'Indexed 20 wallpapers\nReused 19 metadata records',
  }, 'settings'), {
    kind: 'notice',
    channel: 'settings',
    severity: 'success',
    message: 'Source refreshed',
    technicalDetails: 'Indexed 20 wallpapers\nReused 19 metadata records',
  });
});

test('idle command feedback clears its channel', () => {
  assert.deepEqual(translateCommandFeedback({ state: 'idle' }, 'settings'), {
    kind: 'clear',
    channel: 'settings',
  });
});

test('feedback clock runs only while a timed notice exists', () => {
  const persistent = feedbackReducer(EMPTY_FEEDBACK_STATE, {
    type: 'show',
    channel: 'apply',
    severity: 'error',
    message: 'Apply failed',
    nowMs: 0,
  });
  assert.equal(shouldTickFeedbackClock(persistent), false);

  const withTimed = feedbackReducer(persistent, {
    type: 'show',
    channel: 'settings',
    severity: 'success',
    message: 'Saved',
    nowMs: 0,
  });
  assert.equal(shouldTickFeedbackClock(withTimed), true);
});
