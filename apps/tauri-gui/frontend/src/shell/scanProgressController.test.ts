import assert from 'node:assert/strict';
import test from 'node:test';

import type { CommandResult, ScanProgressDTO } from '../api/types.ts';
import { scanPresentation } from './feedbackState.ts';
import { ScanProgressController } from './scanProgressController.ts';

const ok: CommandResult = {
  success: true,
  stdout: 'Cancel requested.',
  stderr: '',
  exitCode: 0,
};

function progress(overrides: Partial<ScanProgressDTO> = {}): ScanProgressDTO {
  return {
    running: false,
    stage: 'idle',
    scanned: 0,
    reusedMetadata: 0,
    probedMetadata: 0,
    insertedSqlite: 0,
    staged: 0,
    skipped: 0,
    metadataErrors: 0,
    cancelRequested: false,
    ...overrides,
  };
}

class ManualClock {
  nowMs = 1_000;
  private nextId = 1;
  private tasks = new Map<number, { dueAtMs: number; callback: () => void }>();

  readonly now = () => this.nowMs;

  readonly setTimer = (callback: () => void, delayMs: number): unknown => {
    const id = this.nextId++;
    this.tasks.set(id, {
      dueAtMs: this.nowMs + delayMs,
      callback,
    });
    return id;
  };

  readonly clearTimer = (handle: unknown): void => {
    if (typeof handle === 'number') this.tasks.delete(handle);
  };

  nextDelay(): number | null {
    const next = [...this.tasks.values()].sort((a, b) => a.dueAtMs - b.dueAtMs)[0];
    return next ? Math.max(0, next.dueAtMs - this.nowMs) : null;
  }

  advanceBy(delayMs: number): void {
    this.nowMs += delayMs;
    const due = [...this.tasks.entries()]
      .filter(([, task]) => task.dueAtMs <= this.nowMs)
      .sort((a, b) => a[1].dueAtMs - b[1].dueAtMs);
    for (const [id, task] of due) {
      this.tasks.delete(id);
      task.callback();
    }
  }
}

async function settle(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
}

function createController(input: {
  clock: ManualClock;
  read: () => Promise<ScanProgressDTO>;
  cancel?: () => Promise<CommandResult>;
}) {
  return new ScanProgressController({
    api: {
      scanProgress: input.read,
      scanCancel: input.cancel ?? (async () => ok),
    },
    now: input.clock.now,
    scheduler: input.clock,
    activePollMs: 100,
    idlePollMs: 1_000,
  });
}

test('initial polling discovers a backend-started scan and downshifts after its terminal state', async () => {
  const clock = new ManualClock();
  let backend = progress({ running: true, stage: 'walking files', scanned: 3 });
  const controller = createController({ clock, read: async () => backend });

  controller.start();
  await settle();

  assert.equal(controller.getSnapshot().scanState.kind, 'running');
  assert.equal(controller.getSnapshot().progress?.scanned, 3);
  assert.equal(controller.getSnapshot().pollingMode, 'active');
  assert.equal(clock.nextDelay(), 100);

  backend = progress({ scanned: 8 });
  clock.advanceBy(100);
  await settle();

  assert.equal(controller.getSnapshot().scanState.kind, 'idle');
  assert.equal(controller.getSnapshot().pollingMode, 'idle');
  assert.equal(clock.nextDelay(), 1_000);
});

test('progress polling preserves the reducer 500ms presentation delay without resetting start time', async () => {
  const clock = new ManualClock();
  const controller = createController({
    clock,
    read: async () => progress({ running: true, stage: 'indexing' }),
  });

  controller.start();
  await settle();
  const startedAtMs = controller.getSnapshot().scanState.kind === 'running'
    ? controller.getSnapshot().scanState.startedAtMs
    : assert.fail('scan should be running');

  clock.nowMs = startedAtMs + 499;
  await controller.refresh();
  assert.deepEqual(
    scanPresentation(controller.getSnapshot().scanState, controller.getSnapshot().observedAtMs),
    { kind: 'hidden' },
  );

  clock.nowMs = startedAtMs + 500;
  await controller.refresh();
  assert.deepEqual(
    scanPresentation(controller.getSnapshot().scanState, controller.getSnapshot().observedAtMs),
    { kind: 'running', nonModal: true, canCancel: true, elapsedMs: 500 },
  );
});

test('source action signals cover a quick scan without flashing stale progress', async () => {
  const clock = new ManualClock();
  const controller = createController({ clock, read: async () => progress() });
  controller.start();
  await settle();

  controller.signalStarted();
  assert.equal(controller.getSnapshot().scanState.kind, 'running');
  assert.deepEqual(
    scanPresentation(controller.getSnapshot().scanState, clock.now()),
    { kind: 'hidden' },
  );

  clock.nowMs += 100;
  controller.signalFinished();
  await settle();

  assert.equal(controller.getSnapshot().scanState.kind, 'idle');
  assert.deepEqual(
    scanPresentation(controller.getSnapshot().scanState, clock.now()),
    { kind: 'hidden' },
  );
});

test('cancel remains pending until backend terminal progress and cannot be requested twice', async () => {
  const clock = new ManualClock();
  let backend = progress({ running: true, stage: 'walking files' });
  let cancelCalls = 0;
  let resolveCancel: ((result: CommandResult) => void) | undefined;
  const cancelResult = new Promise<CommandResult>((resolve) => { resolveCancel = resolve; });
  const controller = createController({
    clock,
    read: async () => backend,
    cancel: async () => {
      cancelCalls += 1;
      return cancelResult;
    },
  });
  controller.start();
  await settle();

  const first = controller.requestCancel();
  const second = controller.requestCancel();
  assert.equal(cancelCalls, 1);
  assert.equal(controller.getSnapshot().scanState.kind, 'running');
  assert.notEqual(
    controller.getSnapshot().scanState.kind === 'running'
      ? controller.getSnapshot().scanState.cancelRequestedAtMs
      : null,
    null,
  );

  resolveCancel?.(ok);
  await Promise.all([first, second]);
  backend = progress({
    running: true,
    stage: 'walking files',
    cancelRequested: true,
  });
  await controller.refresh();
  assert.equal(controller.getSnapshot().scanState.kind, 'running');

  backend = progress({
    stage: 'walking files',
    cancelRequested: true,
    error: 'scan cancelled',
  });
  await controller.refresh();
  assert.equal(controller.getSnapshot().scanState.kind, 'cancelled');

  controller.dismissCancelled();
  assert.equal(controller.getSnapshot().scanState.kind, 'idle');
});

test('a failed cancel request restores the cancel action and polling can recover from transport errors', async () => {
  const clock = new ManualClock();
  let readCalls = 0;
  let cancelCalls = 0;
  const controller = createController({
    clock,
    read: async () => {
      readCalls += 1;
      if (readCalls === 1) throw new Error('bridge temporarily unavailable');
      return progress({ running: true, stage: 'indexing' });
    },
    cancel: async () => {
      cancelCalls += 1;
      if (cancelCalls === 1) throw new Error('cancel transport failed');
      return ok;
    },
  });

  controller.start();
  await settle();
  assert.match(controller.getSnapshot().transportError ?? '', /bridge temporarily unavailable/);
  assert.equal(controller.getSnapshot().pollingMode, 'idle');

  await controller.refresh();
  assert.equal(controller.getSnapshot().transportError, null);
  assert.equal(controller.getSnapshot().scanState.kind, 'running');

  assert.equal(await controller.requestCancel(), null);
  assert.equal(
    controller.getSnapshot().scanState.kind === 'running'
      ? controller.getSnapshot().scanState.cancelRequestedAtMs
      : null,
    null,
  );

  assert.equal(await controller.requestCancel(), ok);
  assert.equal(cancelCalls, 2);
});

test('stopping ignores a late progress response and leaves no timer behind', async () => {
  const clock = new ManualClock();
  let resolveRead: ((value: ScanProgressDTO) => void) | undefined;
  const pendingRead = new Promise<ScanProgressDTO>((resolve) => { resolveRead = resolve; });
  const controller = createController({ clock, read: async () => pendingRead });

  controller.start();
  controller.stop();
  resolveRead?.(progress({ running: true, stage: 'late result' }));
  await settle();

  assert.equal(controller.getSnapshot().scanState.kind, 'idle');
  assert.equal(controller.getSnapshot().pollingMode, 'stopped');
  assert.equal(clock.nextDelay(), null);
});
