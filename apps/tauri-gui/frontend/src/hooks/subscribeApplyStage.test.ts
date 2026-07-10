import assert from 'node:assert/strict';
import test from 'node:test';

import { APP_EVENTS, type ApplyStagePayload } from '../events/appEvents.ts';
import { createSubscribeApplyStage } from './subscribeApplyStageCore.ts';

test('subscribeApplyStage disposes before listen resolves and still calls unlisten', async () => {
  let resolveDeferred: (() => void) | undefined;
  let unlistenCalled = false;
  const deferredUnlisten = () => { unlistenCalled = true; };

  const listenFn = (async () => {
    await new Promise<void>((resolve) => {
      resolveDeferred = resolve;
    });
    return deferredUnlisten;
  }) as Parameters<typeof createSubscribeApplyStage>[1];

  const subscribe = createSubscribeApplyStage(APP_EVENTS.applyStage, listenFn);
  const unsubscribe = subscribe(() => {});

  unsubscribe();
  resolveDeferred?.();
  await new Promise((resolve) => setTimeout(resolve, 0));

  assert.equal(unlistenCalled, true, 'deferred unlisten must run when disposed before resolution');
});

test('subscribeApplyStage forwards payload events to handler', async () => {
  const payloads: ApplyStagePayload[] = [];
  const listenFn = (async (_event, handler) => {
    handler({
      event: APP_EVENTS.applyStage,
      id: 1,
      payload: {
        requestId: 'req-1',
        stage: 'EnsureAwwwDaemon',
        label: 'Starting awww daemon',
        detail: 'Starting awww daemon.',
      },
    });
    return () => {};
  }) as Parameters<typeof createSubscribeApplyStage>[1];

  const subscribe = createSubscribeApplyStage(APP_EVENTS.applyStage, listenFn);
  subscribe((payload) => { payloads.push(payload); });
  await new Promise((resolve) => setTimeout(resolve, 0));

  assert.equal(payloads.length, 1);
  assert.equal(payloads[0]?.requestId, 'req-1');
});
