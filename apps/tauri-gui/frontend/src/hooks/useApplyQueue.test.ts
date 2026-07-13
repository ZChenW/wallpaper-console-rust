import assert from 'node:assert/strict';
import test from 'node:test';

import { ApplyQueueController, createApplyQueueHandlers } from './applyQueueController.ts';
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
  applyToDisplay?: ApplyQueueDeps['applyToDisplay'];
  feedback: CommandFeedback[];
  metrics?: string[];
  subscribeApplyStage?: ApplyQueueDeps['subscribeApplyStage'];
  invalidateLibrary?: () => void;
  refreshStatus?: () => Promise<void>;
  onApplied?: ApplyQueueDeps['onApplied'];
}): ApplyQueueDeps {
  return {
    applyAction: opts.applyAction,
    applyToDisplay: opts.applyToDisplay ?? (async () => {
      throw new Error('unexpected targeted apply');
    }),
    refreshStatus: opts.refreshStatus ?? (async () => {}),
    invalidateLibrary: opts.invalidateLibrary ?? (() => {}),
    setFeedback: (value) => { opts.feedback.push(value); },
    makeErrorFeedback: (label) => ({ state: 'error', label, detail: 'test error' }),
    recordMetric: (name) => { opts.metrics?.push(name); },
    subscribeApplyStage: opts.subscribeApplyStage,
    onApplied: opts.onApplied,
  };
}

test('apply handlers preserve legacy actions and construct targeted requests', () => {
  const actionRequests: ApplyRequestDTO[] = [];
  const targetedRequests: Array<{ path: string; target?: string; requestId?: string }> = [];
  const handlers = createApplyQueueHandlers(
    {
      enqueue: (request) => { actionRequests.push(request); },
      enqueueTargeted: (request) => { targetedRequests.push(request); },
    },
    (() => {
      let next = 0;
      return () => `request-${++next}`;
    })(),
  );
  const explicitAction = req('explicit');

  handlers.handleApplyAction(explicitAction);
  handlers.handleApply('/wall/legacy.jpg');
  handlers.handleApplyToDisplay('/wall/all.jpg');
  handlers.handleApplyToDisplay('/wall/named.jpg', 'HDMI-A-1');

  assert.deepEqual(actionRequests, [
    explicitAction,
    { kind: 'apply', path: '/wall/legacy.jpg', requestId: 'request-1' },
  ]);
  assert.deepEqual(targetedRequests, [
    { path: '/wall/all.jpg', requestId: 'request-2' },
    { path: '/wall/named.jpg', target: 'HDMI-A-1', requestId: 'request-3' },
  ]);
});

test('targeted apply sends omitted and named targets only through applyToDisplay', async () => {
  const actionCalls: ApplyRequestDTO[] = [];
  const targetedCalls: Array<{ path: string; target?: string; requestId?: string }> = [];
  const feedback: CommandFeedback[] = [];
  const deps = makeDeps({
    applyAction: async (request) => {
      actionCalls.push(request);
      return { success: true, stdout: '{}', stderr: '' };
    },
    applyToDisplay: async (request) => {
      targetedCalls.push(request);
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
  });

  const controller = new ApplyQueueController(deps, () => {});
  controller.enqueueTargeted({ path: '/wall/all.jpg', requestId: 'all' });
  await new Promise((resolve) => setTimeout(resolve, 20));
  controller.enqueueTargeted({ path: '/wall/named.jpg', target: 'HDMI-A-1', requestId: 'named' });
  await new Promise((resolve) => setTimeout(resolve, 20));

  assert.deepEqual(actionCalls, [], 'targeted apply must never fall back to applyAction');
  assert.deepEqual(targetedCalls, [
    { path: '/wall/all.jpg', requestId: 'all' },
    { path: '/wall/named.jpg', target: 'HDMI-A-1', requestId: 'named' },
  ]);
});

test('queue state immediately replaces a superseded pending path', async () => {
  let releaseFirst: (() => void) | undefined;
  const firstBlock = new Promise<void>((resolve) => { releaseFirst = resolve; });
  const feedback: CommandFeedback[] = [];
  const states: Array<{ applying: boolean; activePath?: string; pendingPath?: string }> = [];
  const deps = makeDeps({
    applyAction: async (request) => {
      if (request.requestId === 'a') await firstBlock;
      return { success: true, stdout: '{}', stderr: '' };
    },
    feedback,
  });
  const controller = new ApplyQueueController(deps, () => {}, (state) => states.push(state));

  controller.enqueue(req('a'));
  controller.enqueue(req('b'));
  assert.deepEqual(controller.getState(), {
    applying: true,
    activePath: '/wall/a.jpg',
    pendingPath: '/wall/b.jpg',
  });
  controller.enqueue(req('c'));
  assert.deepEqual(controller.getState(), {
    applying: true,
    activePath: '/wall/a.jpg',
    pendingPath: '/wall/c.jpg',
  });
  assert(states.some((state) => state.pendingPath === '/wall/b.jpg'));
  assert.equal(states.at(-1)?.pendingPath, '/wall/c.jpg');

  releaseFirst?.();
  await new Promise((resolve) => setTimeout(resolve, 30));
  assert.ok(
    !states.some((state) => state.applying && state.activePath === undefined),
    'observable applying state always identifies the active card',
  );
  assert.deepEqual(controller.getState(), {
    applying: false,
    activePath: undefined,
    pendingPath: undefined,
  });
});

test('successful apply reports its original request and parsed result', async () => {
  const feedback: CommandFeedback[] = [];
  const applied: Array<{ request: unknown; result: unknown }> = [];
  const request = { path: '/wall/targeted.jpg', target: 'eDP-1', requestId: 'targeted-success' };
  const result = {
    requestId: request.requestId,
    appliedPath: request.path,
    statePath: request.path,
    backend: 'awww',
    fileType: 'image',
    preview: false,
  };
  const deps = makeDeps({
    applyAction: async () => ({ success: false, stdout: '', stderr: 'unexpected' }),
    applyToDisplay: async () => ({ success: true, stdout: JSON.stringify(result), stderr: '' }),
    feedback,
    onApplied: (originalRequest, parsedResult) => {
      applied.push({ request: originalRequest, result: parsedResult });
    },
  });

  const controller = new ApplyQueueController(deps, () => {});
  controller.enqueueTargeted(request);
  await new Promise((resolve) => setTimeout(resolve, 30));

  assert.deepEqual(applied, [{ request, result }]);
});

test('successful apply reports undefined result for malformed stdout', async () => {
  const feedback: CommandFeedback[] = [];
  const applied: unknown[] = [];
  const deps = makeDeps({
    applyAction: async () => ({ success: true, stdout: 'not-json', stderr: '' }),
    feedback,
    onApplied: (_request, result) => { applied.push(result); },
  });

  const controller = new ApplyQueueController(deps, () => {});
  controller.enqueue(req('malformed'));
  await new Promise((resolve) => setTimeout(resolve, 30));

  assert.deepEqual(applied, [undefined]);
});

test('successful apply accepts backend null requestId as a valid result', async () => {
  const feedback: CommandFeedback[] = [];
  const applied: unknown[] = [];
  const result = {
    requestId: null,
    appliedPath: '/wall/no-id.jpg',
    statePath: '/wall/no-id.jpg',
    backend: 'awww',
    fileType: 'image',
    preview: false,
  };
  const deps = makeDeps({
    applyAction: async () => ({ success: true, stdout: JSON.stringify(result), stderr: '' }),
    feedback,
    onApplied: (_request, parsed) => { applied.push(parsed); },
  });

  const controller = new ApplyQueueController(deps, () => {});
  controller.enqueue({ kind: 'apply', path: result.appliedPath });
  await new Promise((resolve) => setTimeout(resolve, 30));

  assert.deepEqual(applied, [{
    appliedPath: result.appliedPath,
    statePath: result.statePath,
    backend: result.backend,
    fileType: result.fileType,
    preview: result.preview,
  }]);
});

test('refresh failure after backend success keeps success and onApplied', async () => {
  const feedback: CommandFeedback[] = [];
  const applied: string[] = [];
  let invalidations = 0;
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
    refreshStatus: async () => { throw new Error('status unavailable'); },
    invalidateLibrary: () => { invalidations += 1; },
    onApplied: (request) => { applied.push(request.path); },
  });

  const controller = new ApplyQueueController(deps, () => {});
  controller.enqueue(req('refresh-fails'));
  await new Promise((resolve) => setTimeout(resolve, 30));

  assert.deepEqual(applied, ['/wall/refresh-fails.jpg']);
  assert.equal(invalidations, 0, 'status refresh failure is not an apply failure');
  assert.ok(feedback.some((value) => value.state === 'success' && value.label === 'Applied'));
  assert.ok(!feedback.some((value) => value.state === 'error' && value.label === 'Apply'));
});

test('failed apply never calls onApplied', async () => {
  const feedback: CommandFeedback[] = [];
  let appliedCount = 0;
  const deps = makeDeps({
    applyAction: async () => ({ success: false, stdout: '', stderr: 'renderer failed' }),
    feedback,
    onApplied: () => { appliedCount += 1; },
  });

  const controller = new ApplyQueueController(deps, () => {});
  controller.enqueue(req('failure'));
  await new Promise((resolve) => setTimeout(resolve, 30));

  assert.equal(appliedCount, 0);
});

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

test('apply queue invalidates library on failure so cards show retryable state', async () => {
  const feedback: CommandFeedback[] = [];
  let libraryInvalidations = 0;

  const deps = makeDeps({
    applyAction: async () => ({ success: false, stdout: '', stderr: 'renderer failed' }),
    feedback,
    invalidateLibrary: () => { libraryInvalidations += 1; },
  });

  const controller = new ApplyQueueController(deps, () => {});
  controller.enqueue(req('a'));
  await new Promise((resolve) => setTimeout(resolve, 30));

  assert.equal(libraryInvalidations, 1, 'failed apply should refresh library cards immediately');
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
