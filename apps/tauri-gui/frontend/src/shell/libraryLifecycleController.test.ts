import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import type { CommandFeedback } from '../api/feedback.ts';
import type {
  CommandResult,
  FirstRunSourceSuggestionDTO,
} from '../api/types.ts';
import {
  LibraryLifecycleController,
  type LibraryLifecycleApi,
} from './libraryLifecycleController.ts';
import type { LibraryWatchdog } from './startupWatchdog.ts';
import type { ShellNoticeInput } from './useShellFeedback.ts';

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function commandResult(
  success: boolean,
  error?: CommandResult['error'],
): CommandResult {
  return {
    success,
    stdout: '',
    stderr: '',
    exitCode: success ? 0 : 1,
    ...(error ? { error } : {}),
  };
}

function suggestion(path: string): FirstRunSourceSuggestionDTO {
  return {
    path,
    label: path,
    kind: 'directory',
  };
}

interface Harness {
  api: LibraryLifecycleApi;
  controller: LibraryLifecycleController;
  notices: ShellNoticeInput[];
  feedback: CommandFeedback[];
  reloadLibraryCalls: number;
  reloadSourcesCalls: number;
}

function createHarness(overrides: Partial<LibraryLifecycleApi> = {}): Harness {
  const notices: ShellNoticeInput[] = [];
  const feedback: CommandFeedback[] = [];
  const harness = {
    reloadLibraryCalls: 0,
    reloadSourcesCalls: 0,
  };
  const api: LibraryLifecycleApi = {
    firstRunSourceSuggestions: async () => [],
    libraryReady: async () => {},
    sqliteRepair: async () => commandResult(true),
    sqliteVerify: async () => commandResult(true),
    ...overrides,
  };
  const controller = new LibraryLifecycleController({
    api,
    reloadLibrary: async () => {
      harness.reloadLibraryCalls += 1;
    },
    reloadSources: async () => {
      harness.reloadSourcesCalls += 1;
    },
    showNotice: (notice) => notices.push(notice),
    setSystemFeedback: (item) => feedback.push(item),
  });
  return {
    api,
    controller,
    notices,
    feedback,
    get reloadLibraryCalls() {
      return harness.reloadLibraryCalls;
    },
    get reloadSourcesCalls() {
      return harness.reloadSourcesCalls;
    },
  };
}

test('first-run suggestions ignore an obsolete request and publish only the latest result', async () => {
  const first = deferred<FirstRunSourceSuggestionDTO[]>();
  const second = deferred<FirstRunSourceSuggestionDTO[]>();
  const requests = [first, second];
  const harness = createHarness({
    firstRunSourceSuggestions: () => requests.shift()!.promise,
  });

  const cancelFirst = harness.controller.requestFirstRunSuggestions(true);
  cancelFirst();
  harness.controller.requestFirstRunSuggestions(true);
  first.resolve([suggestion('/stale')]);
  await first.promise;

  assert.deepEqual(harness.controller.snapshot.firstRunSuggestions, []);

  second.resolve([suggestion('/current')]);
  await second.promise;
  assert.deepEqual(
    harness.controller.snapshot.firstRunSuggestions.map(({ path }) => path),
    ['/current'],
  );
});

test('leaving first run clears optional suggestions and invalidates their request', async () => {
  const request = deferred<FirstRunSourceSuggestionDTO[]>();
  const harness = createHarness({
    firstRunSourceSuggestions: () => request.promise,
  });

  harness.controller.requestFirstRunSuggestions(true);
  harness.controller.requestFirstRunSuggestions(false);
  request.resolve([suggestion('/late')]);
  await request.promise;

  assert.deepEqual(harness.controller.snapshot.firstRunSuggestions, []);
  assert.equal(harness.controller.snapshot.firstRunSuggestionsError, null);
});

test('no-op lifecycle transitions do not publish redundant React snapshots', () => {
  const harness = createHarness();
  let notifications = 0;
  harness.controller.subscribe(() => {
    notifications += 1;
  });

  harness.controller.requestFirstRunSuggestions(false);
  harness.controller.clearTimeoutIf(true);

  assert.equal(notifications, 0);
});

test('watchdog timeout and retry stay behind the controller interface', () => {
  let timeout: (() => void) | null = null;
  let cleanupCalls = 0;
  const watchdog: LibraryWatchdog = {
    arm(_timeoutMs, onTimeout) {
      timeout = onTimeout;
      return () => {
        cleanupCalls += 1;
        timeout = null;
      };
    },
  };
  const harness = createHarness();
  const controller = new LibraryLifecycleController({
    api: harness.api,
    reloadLibrary: async () => {},
    reloadSources: async () => {},
    showNotice: () => {},
    setSystemFeedback: () => {},
    watchdog,
  });

  controller.watchInitialRequest(false, 3_000);
  assert.ok(timeout);
  (timeout as () => void)();
  assert.equal(controller.snapshot.initialRequestTimedOut, true);

  controller.retryInitialRequest();
  assert.equal(controller.snapshot.initialRequestTimedOut, false);
  assert.equal(controller.snapshot.watchdogRetry, 1);

  controller.watchInitialRequest(true, 3_000);
  assert.equal(cleanupCalls, 0, 'a fired watchdog no longer needs cleanup');
});

test('integrity verification publishes confirmed corruption through the interface', async () => {
  const harness = createHarness({
    sqliteVerify: async () => commandResult(false, {
      kind: 'sqlite_integrity',
      message: 'database is malformed',
      recoverable: true,
    }),
  });

  harness.controller.verifyIntegrity(true);
  await Promise.resolve();
  await Promise.resolve();

  assert.equal(harness.controller.snapshot.repairFault?.message, 'Library database needs repair');
  assert.match(
    harness.controller.snapshot.repairFault?.technicalDetails ?? '',
    /database is malformed/,
  );
});

test('successful repair verifies before refreshing both catalogs and reporting success', async () => {
  const harness = createHarness();

  await harness.controller.repairLibrary();

  assert.equal(harness.controller.snapshot.repairPending, false);
  assert.equal(harness.controller.snapshot.repairFault, null);
  assert.equal(harness.reloadLibraryCalls, 1);
  assert.equal(harness.reloadSourcesCalls, 1);
  assert.deepEqual(harness.notices, [{
    channel: 'system',
    severity: 'success',
    message: 'Library index repaired',
  }]);
  assert.deepEqual(harness.feedback, []);
});

test('failed repair reports command feedback without refreshing Library state', async () => {
  const harness = createHarness({
    sqliteRepair: async () => commandResult(false, {
      kind: 'storage_error',
      message: 'repair failed',
      recoverable: true,
    }),
  });

  await harness.controller.repairLibrary();

  assert.equal(harness.reloadLibraryCalls, 0);
  assert.equal(harness.reloadSourcesCalls, 0);
  assert.equal(harness.feedback.length, 1);
  assert.equal(harness.feedback[0]?.state, 'error');
  assert.equal(harness.controller.snapshot.repairPending, false);
});

test('SinglePageShell consumes the lifecycle interface instead of its internals', async () => {
  const shell = await readFile(
    new URL('./SinglePageShell.tsx', import.meta.url),
    'utf8',
  );

  assert.match(shell, /useLibraryLifecycle/);
  assert.doesNotMatch(shell, /@tauri-apps\/api\/event/);
  assert.doesNotMatch(shell, /startupWatchdog/);
  assert.doesNotMatch(shell, /libraryRepair\.ts/);
  assert.doesNotMatch(shell, /LIBRARY_REFRESH_EVENT/);
  assert.doesNotMatch(shell, /firstRunSuggestionRequest|libraryVerificationRequest/);
});
