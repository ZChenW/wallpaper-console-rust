import assert from 'node:assert/strict';
import test from 'node:test';

import { ApplyQueueController } from './applyQueueController.ts';
import type { ApplyQueueDeps } from './applyQueueController.ts';

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
}): ApplyQueueDeps {
  return {
    applyAction: opts.applyAction,
    refreshStatus: async () => {},
    invalidateHistory: () => {},
    setFeedback: (value) => { opts.feedback.push(value); },
    makeErrorFeedback: (label) => ({ state: 'error', label, detail: 'test error' }),
    recordMetric: (name) => { opts.metrics?.push(name); },
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
  });

  const controller = new ApplyQueueController(deps, (value) => applyingStates.push(value));
  controller.enqueue(req('a'));
  controller.enqueue(req('b'));
  controller.enqueue(req('c'));
  releaseFirst?.();
  await new Promise((resolve) => setTimeout(resolve, 30));

  assert.deepEqual(calls, ['a', 'c'], 'should execute current then latest pending only; drop superseded middle');
  assert.deepEqual(applyingStates, [true, false]);
  assert(feedback.some((f) => f.state === 'running' && f.detail?.startsWith('Queued')));
  assert(feedback.some((f) => f.state === 'running' && f.detail?.startsWith('Starting')));
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
  assert.ok(stages.some((d) => d?.startsWith('Starting')), 'starting backend stage emitted');
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
