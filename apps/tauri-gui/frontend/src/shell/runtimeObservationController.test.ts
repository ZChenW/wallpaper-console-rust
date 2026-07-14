import assert from 'node:assert/strict';
import test from 'node:test';

import type { RuntimeWallpaperObservationDTO } from '../api/types.ts';
import { RuntimeObservationController } from './runtimeObservationController.ts';

class ManualScheduler {
  private nextId = 1;
  private tasks = new Map<number, { delayMs: number; callback: () => void }>();

  readonly setTimer = (callback: () => void, delayMs: number): unknown => {
    const id = this.nextId++;
    this.tasks.set(id, { delayMs, callback });
    return id;
  };

  readonly clearTimer = (handle: unknown): void => {
    if (typeof handle === 'number') this.tasks.delete(handle);
  };

  nextDelay(): number | null {
    return this.tasks.values().next().value?.delayMs ?? null;
  }

  runNext(): void {
    const entry = this.tasks.entries().next().value as
      | [number, { delayMs: number; callback: () => void }]
      | undefined;
    assert.ok(entry, 'expected a scheduled poll');
    this.tasks.delete(entry[0]);
    entry[1].callback();
  }
}

async function settle(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
}

function deferred<T>() {
  let resolve: ((value: T) => void) | undefined;
  let reject: ((error: unknown) => void) | undefined;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return {
    promise,
    resolve: (value: T) => resolve?.(value),
    reject: (error: unknown) => reject?.(error),
  };
}

const confirmed = (path: string): RuntimeWallpaperObservationDTO[] => [{
  output: 'eDP-1',
  wallpaperPath: path,
  status: 'confirmed',
}];

test('slow runtime probes stay single-flight and schedule from completion', async () => {
  const scheduler = new ManualScheduler();
  const reads = [deferred<RuntimeWallpaperObservationDTO[]>(), deferred<RuntimeWallpaperObservationDTO[]>()];
  let readCalls = 0;
  const observed: RuntimeWallpaperObservationDTO[][] = [];
  const controller = new RuntimeObservationController({
    api: { runtimeWallpaperObservations: () => reads[readCalls++].promise },
    connectedOutputs: ['eDP-1'],
    onObservations: (value) => observed.push([...value]),
    scheduler,
    pollMs: 5_000,
  });

  controller.start();
  assert.equal(readCalls, 1);
  assert.equal(scheduler.nextDelay(), null);

  reads[0].resolve(confirmed('/walls/a.jpg'));
  await settle();
  assert.deepEqual(observed, [confirmed('/walls/a.jpg')]);
  assert.equal(scheduler.nextDelay(), 5_000);

  scheduler.runNext();
  assert.equal(readCalls, 2);
  assert.equal(scheduler.nextDelay(), null);
  reads[1].resolve(confirmed('/walls/b.jpg'));
  await settle();
});

test('apply invalidation drops an older in-flight observation and repolls immediately', async () => {
  const scheduler = new ManualScheduler();
  const first = deferred<RuntimeWallpaperObservationDTO[]>();
  const second = deferred<RuntimeWallpaperObservationDTO[]>();
  const reads = [first, second];
  let readCalls = 0;
  const observed: RuntimeWallpaperObservationDTO[][] = [];
  const controller = new RuntimeObservationController({
    api: { runtimeWallpaperObservations: () => reads[readCalls++].promise },
    connectedOutputs: ['eDP-1'],
    onObservations: (value) => observed.push([...value]),
    scheduler,
  });

  controller.start();
  controller.invalidateAndRefresh();
  first.resolve(confirmed('/walls/old.jpg'));
  await settle();

  assert.deepEqual(observed, []);
  assert.equal(readCalls, 2);
  second.resolve(confirmed('/walls/new.jpg'));
  await settle();
  assert.deepEqual(observed, [confirmed('/walls/new.jpg')]);
});

test('probe failure clears stale current evidence for every connected output', async () => {
  const scheduler = new ManualScheduler();
  const observed: RuntimeWallpaperObservationDTO[][] = [];
  const controller = new RuntimeObservationController({
    api: { runtimeWallpaperObservations: async () => { throw new Error('bridge unavailable'); } },
    connectedOutputs: ['eDP-1', 'HDMI-A-1'],
    onObservations: (value) => observed.push([...value]),
    scheduler,
  });

  controller.start();
  await settle();

  assert.deepEqual(observed, [[
    { output: 'eDP-1', wallpaperPath: null, status: 'unknown' },
    { output: 'HDMI-A-1', wallpaperPath: null, status: 'unknown' },
  ]]);
});

test('stop ignores a late observation and leaves no scheduled poll', async () => {
  const scheduler = new ManualScheduler();
  const read = deferred<RuntimeWallpaperObservationDTO[]>();
  const observed: RuntimeWallpaperObservationDTO[][] = [];
  const controller = new RuntimeObservationController({
    api: { runtimeWallpaperObservations: async () => read.promise },
    connectedOutputs: ['eDP-1'],
    onObservations: (value) => observed.push([...value]),
    scheduler,
  });

  controller.start();
  controller.stop();
  read.resolve(confirmed('/walls/late.jpg'));
  await settle();

  assert.deepEqual(observed, []);
  assert.equal(scheduler.nextDelay(), null);
});
