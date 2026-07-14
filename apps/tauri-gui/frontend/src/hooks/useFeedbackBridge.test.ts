import assert from 'node:assert/strict';
import test from 'node:test';

import type { CommandFeedback } from '../api/feedback.ts';
import { APP_EVENTS } from '../events/appEvents.ts';
import { subscribeFeedbackEvent } from './useFeedbackBridge.ts';

test('feedback event subscription ignores unavailable Tauri bridges', async () => {
  const received: CommandFeedback[] = [];
  const syncFailure = (() => {
    throw new Error('missing Tauri internals');
  }) as Parameters<typeof subscribeFeedbackEvent>[1];
  const asyncFailure = (() => Promise.reject(new Error('listen rejected'))) as
    Parameters<typeof subscribeFeedbackEvent>[1];

  subscribeFeedbackEvent((feedback) => received.push(feedback), syncFailure)();
  subscribeFeedbackEvent((feedback) => received.push(feedback), asyncFailure)();
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.deepEqual(received, []);
});

test('feedback event subscription forwards payload and cleans up after deferred install', async () => {
  const received: CommandFeedback[] = [];
  let resolveListen: ((unlisten: () => void) => void) | undefined;
  let handler: ((event: { payload: CommandFeedback }) => void) | undefined;
  let unlistenCalls = 0;
  const listenFn = ((_event: string, next: typeof handler) => {
    handler = next;
    return new Promise<() => void>((resolve) => { resolveListen = resolve; });
  }) as Parameters<typeof subscribeFeedbackEvent>[1];

  const unsubscribe = subscribeFeedbackEvent((feedback) => received.push(feedback), listenFn);
  handler?.({ payload: { state: 'success', label: 'Applied' } });
  unsubscribe();
  resolveListen?.(() => { unlistenCalls += 1; });
  await new Promise((resolve) => setTimeout(resolve, 0));

  assert.deepEqual(received, [{ state: 'success', label: 'Applied' }]);
  assert.equal(unlistenCalls, 1);
  assert.equal(APP_EVENTS.feedback, 'wc-feedback');
});
