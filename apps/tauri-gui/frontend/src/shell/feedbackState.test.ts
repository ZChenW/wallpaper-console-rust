import assert from 'node:assert/strict';
import test from 'node:test';

import {
  EMPTY_FEEDBACK_STATE,
  EMPTY_SCAN_STATE,
  feedbackCountdownProgress,
  feedbackLifetimeMs,
  feedbackReducer,
  scanPresentation,
  scanReducer,
} from './feedbackState.ts';

test('feedback severities have the required auto-close lifetimes', () => {
  assert.equal(feedbackLifetimeMs('success'), 3_000);
  assert.equal(feedbackLifetimeMs('info'), 5_000);
  assert.equal(feedbackLifetimeMs('warning'), 8_000);
  assert.equal(feedbackLifetimeMs('error'), null);
});

test('countdown progress is deterministic and expired feedback is removed on tick', () => {
  const shown = feedbackReducer(EMPTY_FEEDBACK_STATE, {
    type: 'show',
    channel: 'apply',
    severity: 'success',
    message: 'Wallpaper applied',
    nowMs: 1_000,
  });
  const notice = shown.notices[0];

  assert.equal(feedbackCountdownProgress(notice, 1_000), 1);
  assert.equal(feedbackCountdownProgress(notice, 2_500), 0.5);
  assert.equal(feedbackCountdownProgress(notice, 4_000), 0);
  assert.equal(feedbackReducer(shown, { type: 'tick', nowMs: 3_999 }).notices.length, 1);
  assert.equal(feedbackReducer(shown, { type: 'tick', nowMs: 4_000 }).notices.length, 0);
});

test('hover pause and resume preserve the remaining countdown', () => {
  const shown = feedbackReducer(EMPTY_FEEDBACK_STATE, {
    type: 'show',
    channel: 'apply',
    severity: 'success',
    message: 'Wallpaper applied',
    nowMs: 1_000,
  });
  const paused = feedbackReducer(shown, { type: 'pause', channel: 'apply', nowMs: 2_000 });

  assert.equal(feedbackCountdownProgress(paused.notices[0], 99_000), 2 / 3);
  assert.equal(feedbackReducer(paused, { type: 'tick', nowMs: 99_000 }).notices.length, 1);

  const resumed = feedbackReducer(paused, { type: 'resume', channel: 'apply', nowMs: 10_000 });
  assert.equal(feedbackCountdownProgress(resumed.notices[0], 11_000), 1 / 3);
  assert.equal(feedbackReducer(resumed, { type: 'tick', nowMs: 11_999 }).notices.length, 1);
  assert.equal(feedbackReducer(resumed, { type: 'tick', nowMs: 12_000 }).notices.length, 0);
});

test('pausing at or after expiry cannot keep zero-remaining feedback alive', () => {
  const shown = feedbackReducer(EMPTY_FEEDBACK_STATE, {
    type: 'show',
    channel: 'apply',
    severity: 'success',
    message: 'Wallpaper applied',
    nowMs: 1_000,
  });
  const pausedAtExpiry = feedbackReducer(shown, {
    type: 'pause',
    channel: 'apply',
    nowMs: 4_000,
  });

  assert.equal(pausedAtExpiry.notices[0].pausedRemainingMs, null);
  assert.equal(
    feedbackReducer(pausedAtExpiry, { type: 'tick', nowMs: 4_000 }).notices.length,
    0,
  );
});

test('feedback timestamps reject non-finite advancement and clamp clock rollback to one lifetime', () => {
  const shown = feedbackReducer(EMPTY_FEEDBACK_STATE, {
    type: 'show',
    channel: 'apply',
    severity: 'success',
    message: 'Wallpaper applied',
    nowMs: Number.NaN,
  });
  assert.equal(shown.notices[0].openedAtMs, 0);
  assert.equal(shown.notices[0].expiresAtMs, 3_000);
  assert.equal(feedbackCountdownProgress(shown.notices[0], Number.NaN), 1);
  assert.equal(feedbackCountdownProgress(shown.notices[0], -10_000), 1);
  assert.deepEqual(feedbackReducer(shown, { type: 'tick', nowMs: Number.NaN }), shown);
  assert.equal(feedbackReducer(shown, { type: 'tick', nowMs: -10_000 }).notices.length, 1);

  const rolledBack = feedbackReducer(shown, {
    type: 'pause',
    channel: 'apply',
    nowMs: -10_000,
  });
  assert.equal(rolledBack.notices[0].pausedRemainingMs, 3_000);
  assert.equal(feedbackCountdownProgress(rolledBack.notices[0], 50_000), 1);

  const invalidResume = feedbackReducer(rolledBack, {
    type: 'resume',
    channel: 'apply',
    nowMs: Number.NaN,
  });
  assert.deepEqual(invalidResume, rolledBack);
});

test('application failure is persistent and has no countdown progress', () => {
  const failed = feedbackReducer(EMPTY_FEEDBACK_STATE, {
    type: 'show',
    channel: 'apply',
    severity: 'error',
    message: 'Could not start the wallpaper backend',
    nowMs: 0,
  });

  assert.equal(feedbackCountdownProgress(failed.notices[0], 86_400_000), null);
  assert.deepEqual(
    feedbackReducer(failed, { type: 'tick', nowMs: 86_400_000 }),
    failed,
  );
});

test('new apply feedback replaces old apply feedback while other channels coexist', () => {
  const applying = feedbackReducer(EMPTY_FEEDBACK_STATE, {
    type: 'show',
    channel: 'apply',
    severity: 'info',
    message: 'Applying wallpaper',
    nowMs: 0,
  });
  const withSettings = feedbackReducer(applying, {
    type: 'show',
    channel: 'settings',
    severity: 'success',
    message: 'Settings saved',
    nowMs: 50,
  });
  const applied = feedbackReducer(withSettings, {
    type: 'show',
    channel: 'apply',
    severity: 'success',
    message: 'Wallpaper applied',
    nowMs: 100,
  });

  assert.deepEqual(applied.notices.map(({ channel, message }) => ({ channel, message })), [
    { channel: 'settings', message: 'Settings saved' },
    { channel: 'apply', message: 'Wallpaper applied' },
  ]);
});

test('scan presentation waits 500ms and a fast completed scan never flashes', () => {
  const running = scanReducer(EMPTY_SCAN_STATE, { type: 'started', nowMs: 1_000 });

  assert.deepEqual(scanPresentation(running, 1_499), { kind: 'hidden' });
  assert.deepEqual(scanPresentation(running, 1_500), {
    kind: 'running',
    nonModal: true,
    canCancel: true,
    elapsedMs: 500,
  });

  const completedQuickly = scanReducer(running, { type: 'completed', nowMs: 1_100 });
  assert.deepEqual(scanPresentation(completedQuickly, 10_000), { kind: 'hidden' });
});

test('scan cancellation is explicit and non-modal even before the presentation delay', () => {
  const running = scanReducer(EMPTY_SCAN_STATE, { type: 'started', nowMs: 1_000 });
  const cancelling = scanReducer(running, { type: 'cancelRequested', nowMs: 1_100 });

  assert.deepEqual(scanPresentation(cancelling, 1_100), {
    kind: 'cancelling',
    nonModal: true,
    canCancel: false,
    elapsedMs: 100,
  });

  const cancelled = scanReducer(cancelling, { type: 'cancelled', nowMs: 1_200 });
  assert.deepEqual(scanPresentation(cancelled, 20_000), {
    kind: 'cancelled',
    nonModal: true,
  });
  assert.deepEqual(
    scanPresentation(scanReducer(cancelled, { type: 'dismissed' }), 20_000),
    { kind: 'hidden' },
  );
});

test('scan timestamps never let NaN or clock rollback bypass the presentation delay', () => {
  const invalidStart = scanReducer(EMPTY_SCAN_STATE, { type: 'started', nowMs: Number.NaN });
  assert.equal(invalidStart.kind, 'running');
  if (invalidStart.kind === 'running') assert.equal(invalidStart.startedAtMs, 0);
  assert.deepEqual(scanPresentation(invalidStart, Number.NaN), { kind: 'hidden' });

  const running = scanReducer(EMPTY_SCAN_STATE, { type: 'started', nowMs: 1_000 });
  assert.deepEqual(scanPresentation(running, -5_000), { kind: 'hidden' });
  assert.deepEqual(scanPresentation(running, Number.POSITIVE_INFINITY), { kind: 'hidden' });
});

test('a late completed event cannot erase explicit cancelled presentation', () => {
  const running = scanReducer(EMPTY_SCAN_STATE, { type: 'started', nowMs: 1_000 });
  const cancelled = scanReducer(running, { type: 'cancelled', nowMs: 1_100 });
  const completedLate = scanReducer(cancelled, { type: 'completed', nowMs: 1_200 });

  assert.deepEqual(completedLate, cancelled);
  assert.deepEqual(scanPresentation(completedLate, 20_000), {
    kind: 'cancelled',
    nonModal: true,
  });
});
