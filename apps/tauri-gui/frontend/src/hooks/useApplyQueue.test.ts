import assert from 'node:assert/strict';
import test from 'node:test';

import { ApplyQueueController } from './applyQueueController.ts';
import type { ApplyQueueDeps } from './applyQueueController.ts';
import type { ApplyStagePayload } from '../events/appEvents.ts';

type ApplyRequestDTO = {
  kind: string;
  path: string;
  requestId?: string | null;
};

type CommandFeedback =
  | { state: 'idle' }
  | { state: 'running'; label: string; detail?: string }
  | { state: 'success'; label: string; detail?: string }
  | { state: 'warning'; label: string; detail: string }
  | { state: 'error'; label: string; detail: string };

const req = (id: string, path = `/wall/${id}.jpg`): ApplyRequestDTO => ({
  kind: 'apply',
  path,
  requestId: id,
});

function makeDeps(opts: {
  applyAction: ApplyQueueDeps['applyAction'];
  feedback: CommandFeedback[];
  metrics?: string[];
  subscribeApplyStage?: ApplyQueueDeps['subscribeApplyStage'];
}): ApplyQueueDeps {
  return {
    applyAction: opts.applyAction,
    refreshStatus: async () => {},
    invalidateHistory: () => {},
    setFeedback: (value) => { opts.feedback.push(value); },
    makeErrorFeedback: (label) => ({ state: 'error', label, detail: 'test error' }),
    recordMetric: (name) => { opts.metrics?.push(name); },
    subscribeApplyStage: opts.subscribeApplyStage,
  };
}

test('apply queue runs current request then latest pending request only', async () => {
  const calls: string[] = [];
  let releaseFirst: (() => void) | undefined;
  const firstBlock = new Promise<void>((resolve) => { releaseFirst = resolve; });
  const feedback: CommandFeedback[] = [];
  const applyingStates: boolean[] = [];

  const deps = makeDeps({
    applyAction: async (request) => {
      calls.push(request.requestId ?? '');
      if (request.requestId === 'a') await firstBlock;
      return {
        success: true,
        stdout: JSON.stringify({
          requestId: request.requestId,
          appliedPath: request.path,
          statePath: request.path,
          backend: 'awww',
          fileType: 'image',
          preview: false,
        }),
        stderr: '',
      };
    },
    feedback,
    subscribeApplyStage: (handler) => {
      handler({
        requestId: 'a',
        stage: 'EnsureAwwwDaemon',
        label: 'Starting awww daemon',
        detail: 'Starting awww daemon.',
      });
      return () => {};
    },
  });

  const controller = new ApplyQueueController(deps, (value) => applyingStates.push(value));
  controller.enqueue(req('a'));
  controller.enqueue(req('b'));
  controller.enqueue(req('c'));
  releaseFirst?.();
  await new Promise((resolve) => setTimeout(resolve, 30));

  assert.deepEqual(calls, ['a', 'c'], 'should execute current then latest pending only; drop superseded middle');
  assert.deepEqual(applyingStates, [true, false]);
  assert(
    feedback.some((f) => f.state === 'running' && f.label === 'Starting awww daemon'),
    'apply stage feedback surfaced for active request',
  );
  assert(
    !feedback.some((f) => f.state === 'running' && f.detail?.startsWith('Queued')),
    'queued must not overwrite the active apply stage',
  );
  assert(
    feedback.some((f) => f.state === 'running' && f.detail?.includes('Next wallpaper queued.')),
    'queued suffix appended to active stage when a request is pending',
  );
  assert(feedback.some((f) => f.state === 'success' && f.label === 'Applied'));
});

test('apply queue emits settling stage before success', async () => {
  const feedback: CommandFeedback[] = [];

  const deps = makeDeps({
    applyAction: async (request) => ({
      success: true,
      stdout: JSON.stringify({
        requestId: request.requestId,
        appliedPath: request.path,
        statePath: request.path,
        backend: 'awww',
        fileType: 'image',
        preview: false,
      }),
      stderr: '',
    }),
    feedback,
  });

  const controller = new ApplyQueueController(deps, () => {});
  controller.enqueue(req('a'));
  await new Promise((resolve) => setTimeout(resolve, 30));

  const stages = feedback.filter((f) => f.state === 'running').map((f) => f.detail);
  assert.ok(stages.some((d) => d?.startsWith('Settling')), 'settling stage emitted');
});

test('apply queue records request duration metric on success', async () => {
  const metrics: string[] = [];
  const feedback: CommandFeedback[] = [];

  const deps = makeDeps({
    applyAction: async () => ({ success: true, stdout: '{}', stderr: '' }),
    feedback,
    metrics,
  });

  const controller = new ApplyQueueController(deps, () => {});
  controller.enqueue(req('a'));
  await new Promise((resolve) => setTimeout(resolve, 30));

  assert.ok(metrics.includes('apply.request.ms'), 'should record apply.request.ms');
});

test('apply queue does not record metric on failure', async () => {
  const metrics: string[] = [];
  const feedback: CommandFeedback[] = [];

  const deps = makeDeps({
    applyAction: async () => ({ success: false, stdout: '', stderr: 'boom' }),
    feedback,
    metrics,
  });

  const controller = new ApplyQueueController(deps, () => {});
  controller.enqueue(req('a'));
  await new Promise((resolve) => setTimeout(resolve, 30));

  assert.equal(metrics.length, 0, 'no metric recorded for failed apply');
  assert.ok(feedback.some((f) => f.state === 'error'));
});

test('apply queue updates feedback from wc-apply-stage and unsubscribes on success', async () => {
  const feedback: CommandFeedback[] = [];
  let unsubscribeCount = 0;
  let emitStage: ((event: ApplyStagePayload) => void) | undefined;

  const deps = makeDeps({
    applyAction: async () => {
      emitStage?.({
        requestId: 'stage-success',
        stage: 'StartLwe',
        label: 'Starting linux-wallpaperengine',
        detail: 'Starting linux-wallpaperengine.',
      });
      return { success: true, stdout: '{}', stderr: '' };
    },
    feedback,
    subscribeApplyStage: (handler) => {
      emitStage = handler;
      return () => { unsubscribeCount += 1; };
    },
  });

  const controller = new ApplyQueueController(deps, () => {});
  controller.enqueue(req('stage-success'));
  await new Promise((resolve) => setTimeout(resolve, 30));

  assert.ok(
    feedback.some((f) => f.state === 'running' && f.detail?.includes('linux-wallpaperengine')),
    'stage detail should update running feedback',
  );
  assert.equal(unsubscribeCount, 1, 'should unsubscribe after successful apply');
});

test('apply queue unsubscribes from wc-apply-stage on failure', async () => {
  const feedback: CommandFeedback[] = [];
  let unsubscribeCount = 0;

  const deps = makeDeps({
    applyAction: async () => ({ success: false, stdout: '', stderr: 'failed' }),
    feedback,
    subscribeApplyStage: () => {
      return () => { unsubscribeCount += 1; };
    },
  });

  const controller = new ApplyQueueController(deps, () => {});
  controller.enqueue(req('stage-fail'));
  await new Promise((resolve) => setTimeout(resolve, 30));

  assert.equal(unsubscribeCount, 1, 'should unsubscribe after failed apply');
  assert.ok(feedback.some((f) => f.state === 'error'));
});

test('apply queue ignores wc-apply-stage events with null requestId when current request has id', async () => {
  const feedback: CommandFeedback[] = [];
  let emitStage: ((event: ApplyStagePayload) => void) | undefined;

  const deps = makeDeps({
    applyAction: async () => {
      emitStage?.({
        requestId: null,
        stage: 'StartLwe',
        label: 'Stale listener',
        detail: 'Should be ignored.',
      });
      return { success: true, stdout: '{}', stderr: '' };
    },
    feedback,
    subscribeApplyStage: (handler) => {
      emitStage = handler;
      return () => {};
    },
  });

  const controller = new ApplyQueueController(deps, () => {});
  controller.enqueue(req('current-request'));
  await new Promise((resolve) => setTimeout(resolve, 30));

  assert.ok(
    !feedback.some((f) => f.detail?.includes('Should be ignored')),
    'null requestId events must not update a request that has requestId',
  );
});

test('apply queue ignores wc-apply-stage events for other request ids', async () => {
  const feedback: CommandFeedback[] = [];
  let emitStage: ((event: ApplyStagePayload) => void) | undefined;

  const deps = makeDeps({
    applyAction: async () => {
      emitStage?.({
        requestId: 'other-request',
        stage: 'StartLwe',
        label: 'Other request',
        detail: 'Should be ignored.',
      });
      emitStage?.({
        requestId: 'current-request',
        stage: 'WaitRendererAlive',
        label: 'Waiting for renderer',
        detail: 'Waiting for linux-wallpaperengine to start.',
      });
      return { success: true, stdout: '{}', stderr: '' };
    },
    feedback,
    subscribeApplyStage: (handler) => {
      emitStage = handler;
      return () => {};
    },
  });

  const controller = new ApplyQueueController(deps, () => {});
  controller.enqueue(req('current-request'));
  await new Promise((resolve) => setTimeout(resolve, 30));

  assert.ok(
    feedback.some((f) => f.state === 'running' && f.detail?.includes('linux-wallpaperengine to start')),
    'should apply stage for matching request id only',
  );
  assert.ok(
    !feedback.some((f) => f.detail?.includes('Should be ignored')),
    'should ignore stages for other request ids',
  );
});

test('apply queue surfaces different preview and scene stage details', async () => {
  const feedback: CommandFeedback[] = [];
  let emitStage: ((event: ApplyStagePayload) => void) | undefined;

  const deps = makeDeps({
    applyAction: async (request) => {
      if (request.requestId === 'preview') {
        emitStage?.({
          requestId: 'preview',
          stage: 'WaitRendererAlive',
          label: 'Waiting for renderer',
          detail: 'Waiting for Awww to display the preview.',
        });
      } else {
        emitStage?.({
          requestId: 'scene',
          stage: 'WaitRendererAlive',
          label: 'Waiting for renderer',
          detail: 'Waiting for linux-wallpaperengine to start.',
        });
      }
      return { success: true, stdout: '{}', stderr: '' };
    },
    feedback,
    subscribeApplyStage: (handler) => {
      emitStage = handler;
      return () => {};
    },
  });

  const controller = new ApplyQueueController(deps, () => {});
  controller.enqueue(req('preview'));
  await new Promise((resolve) => setTimeout(resolve, 20));
  controller.enqueue(req('scene'));
  await new Promise((resolve) => setTimeout(resolve, 20));

  assert.ok(feedback.some((f) => f.detail?.includes('Awww to display the preview')));
  assert.ok(feedback.some((f) => f.detail?.includes('linux-wallpaperengine to start')));
});
