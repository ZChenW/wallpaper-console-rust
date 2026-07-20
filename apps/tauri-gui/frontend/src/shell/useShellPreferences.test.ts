import assert from 'node:assert/strict';
import test from 'node:test';

import {
  SHELL_PREFERENCES_CONFIG_KEY,
  createShellPreferencesLoader,
  createShellPreferencesSaveQueue,
  loadShellPreferences,
  resolveShellPreferencesUpdate,
  saveShellPreferences,
  type ShellPreferencesClient,
  type ShellPreferencesLoader,
} from './useShellPreferences.ts';
import {
  DEFAULT_SHELL_PREFERENCES,
  type ShellPreferences,
} from './shellPreferences.ts';

const ok = { success: true, stdout: '', stderr: '', exitCode: 0 };

function preferences(
  overrides: Partial<ShellPreferences> = {},
): ShellPreferences {
  return {
    ...DEFAULT_SHELL_PREFERENCES,
    ...overrides,
  };
}

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

async function nextTask(): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, 0));
}

test('load uses the fixed key and treats a missing value as defaults', async () => {
  const keys: string[] = [];
  let writes = 0;
  const client: ShellPreferencesClient = {
    configGet: async (key) => {
      keys.push(key);
      return '';
    },
    configSet: async () => {
      writes += 1;
      return ok;
    },
  };

  assert.equal(SHELL_PREFERENCES_CONFIG_KEY, 'gui_shell_preferences');
  assert.deepEqual(await loadShellPreferences(client), DEFAULT_SHELL_PREFERENCES);
  assert.deepEqual(keys, ['gui_shell_preferences']);
  assert.equal(writes, 0, 'loading a missing key must not persist defaults');
});

test('load repairs malformed JSON and malformed fields through the existing parser', async () => {
  const malformedJson: ShellPreferencesClient = {
    configGet: async () => '{broken',
    configSet: async () => ok,
  };
  const malformedFields: ShellPreferencesClient = {
    configGet: async () => JSON.stringify({
      typeFilter: 'all',
      favoritesOnly: 'true',
      sort: 'newest',
      theme: 'dark',
    }),
    configSet: async () => ok,
  };

  assert.deepEqual(await loadShellPreferences(malformedJson), DEFAULT_SHELL_PREFERENCES);
  assert.deepEqual(await loadShellPreferences(malformedFields), {
    ...DEFAULT_SHELL_PREFERENCES,
    theme: 'dark',
  });
});

test('load propagates config transport failures', async () => {
  const client: ShellPreferencesClient = {
    configGet: async () => {
      throw new Error('config unavailable');
    },
    configSet: async () => ok,
  };

  await assert.rejects(loadShellPreferences(client), /config unavailable/);
});

test('save writes only allow-listed preferences to the fixed key', async () => {
  const writes: Array<{ key: string; value: string }> = [];
  const client: ShellPreferencesClient = {
    configGet: async () => '',
    configSet: async (key, value) => {
      writes.push({ key, value });
      return ok;
    },
  };
  const runtimeValue = {
    ...preferences({ theme: 'dark', favoritesOnly: true }),
    search: 'transient',
    selectedPath: '/private/wallpaper.jpg',
    scrollTop: 900,
  } as ShellPreferences;

  await saveShellPreferences(client, runtimeValue);

  assert.equal(writes.length, 1);
  assert.equal(writes[0]?.key, 'gui_shell_preferences');
  assert.deepEqual(Object.keys(JSON.parse(writes[0]!.value)).sort(), [
    'applyGesture',
    'cardSize',
    'displayTarget',
    'favoritesOnly',
    'libraryViewMode',
    'sort',
    'sourceFilter',
    'theme',
    'typeFilter',
    'version',
  ]);
  assert.equal(writes[0]?.value.includes('transient'), false);
  assert.equal(writes[0]?.value.includes('/private/wallpaper.jpg'), false);
});

test('save turns an unsuccessful CommandResult into a specific error', async () => {
  const client: ShellPreferencesClient = {
    configGet: async () => '',
    configSet: async () => ({
      success: false,
      stdout: '',
      stderr: 'database is read-only',
      exitCode: 1,
    }),
  };

  await assert.rejects(
    saveShellPreferences(client, preferences()),
    /Failed to save shell preferences: database is read-only/,
  );
});

test('functional updates are normalized without performing persistence', () => {
  let calls = 0;
  const next = resolveShellPreferencesUpdate(
    preferences(),
    (current) => {
      calls += 1;
      return {
        ...current,
        theme: 'dark',
        search: 'not part of ShellPreferences',
      } as ShellPreferences;
    },
  );

  assert.equal(calls, 1);
  assert.deepEqual(next, preferences({ theme: 'dark' }));
  assert.equal('search' in next, false);
});

test('save queue starts writes serially so an older slow write cannot finish last', async () => {
  const starts: string[] = [];
  const writes: Array<ReturnType<typeof deferred<typeof ok>>> = [];
  const client: ShellPreferencesClient = {
    configGet: async () => '',
    configSet: async (_key, value) => {
      starts.push((JSON.parse(value) as { theme: string }).theme);
      const write = deferred<typeof ok>();
      writes.push(write);
      return write.promise;
    },
  };
  const queue = createShellPreferencesSaveQueue(client);

  const older = queue.enqueue(preferences({ theme: 'light' }));
  const newer = queue.enqueue(preferences({ theme: 'dark' }));
  await nextTask();
  assert.deepEqual(starts, ['light']);

  writes[0]!.resolve(ok);
  await older;
  await nextTask();
  assert.deepEqual(starts, ['light', 'dark']);

  writes[1]!.resolve(ok);
  await newer;
  assert.equal(queue.latestError, null);
});

test('save queue deduplicates the same snapshot for StrictMode-safe effects', async () => {
  let writes = 0;
  const pending = deferred<typeof ok>();
  const client: ShellPreferencesClient = {
    configGet: async () => '',
    configSet: async () => {
      writes += 1;
      return pending.promise;
    },
  };
  const queue = createShellPreferencesSaveQueue(client);
  const snapshot = preferences({ favoritesOnly: true });

  const first = queue.enqueue(snapshot);
  const duplicate = queue.enqueue({ ...snapshot });
  await nextTask();
  assert.equal(writes, 1);

  pending.resolve(ok);
  await Promise.all([first, duplicate]);
  assert.equal(writes, 1);
});

test('save queue reports only the latest request error and clears it on latest success', async () => {
  const writes: Array<ReturnType<typeof deferred<typeof ok>>> = [];
  const client: ShellPreferencesClient = {
    configGet: async () => '',
    configSet: async () => {
      const write = deferred<typeof ok>();
      writes.push(write);
      return write.promise;
    },
  };
  const observed: Array<string | null> = [];
  const queue = createShellPreferencesSaveQueue(
    client,
    (error) => observed.push(error?.message ?? null),
  );

  const stale = queue.enqueue(preferences({ theme: 'light' }));
  const latest = queue.enqueue(preferences({ theme: 'dark' }));
  await nextTask();
  writes[0]!.reject(new Error('stale write failed'));
  await assert.rejects(stale, /stale write failed/);
  await nextTask();
  assert.ok(queue.latestError === null);

  writes[1]!.reject(new Error('latest write failed'));
  await assert.rejects(latest, /latest write failed/);
  const latestWriteError = queue.latestError as Error | null;
  assert.equal(latestWriteError?.message, 'latest write failed');

  const recovery = queue.enqueue(preferences({ theme: 'light' }));
  assert.equal(queue.latestError, null);
  await nextTask();
  writes[2]!.resolve(ok);
  await recovery;
  assert.equal(queue.latestError, null);
  assert.deepEqual(observed, ['latest write failed', null]);
});

// ── ShellPreferencesLoader (bounded configGet + retry) ──────────────────

test('loader succeeds immediately when configGet resolves within timeout', async () => {
  const loaded = preferences({ theme: 'dark', favoritesOnly: true });
  const client: ShellPreferencesClient = {
    configGet: async () => JSON.stringify(loaded),
    configSet: async () => ok,
  };
  const loader = createShellPreferencesLoader();

  const result = await new Promise<{
    resolved: ShellPreferences | null;
    loadError: Error | null;
  }>((resolve) => {
    loader.load(client, {
      loadTimeoutMs: 100,
      retryDelayMs: 500,
      onDefaults: (err) => resolve({ resolved: null, loadError: err }),
      onSuccess: (prefs) => resolve({ resolved: prefs, loadError: null }),
    });
  });

  assert.deepEqual(result.resolved, loaded);
  assert.equal(result.loadError, null);
});

test('loader falls back to defaults when configGet times out', async () => {
  // A promise that never resolves
  const client: ShellPreferencesClient = {
    configGet: () => new Promise(() => {}),
    configSet: async () => ok,
  };
  const loader = createShellPreferencesLoader();

  const result = await new Promise<{
    resolved: ShellPreferences | null;
    loadError: Error | null;
  }>((resolve) => {
    loader.load(client, {
      loadTimeoutMs: 50,
      retryDelayMs: 30, // small for test
      onDefaults: (err) => resolve({ resolved: null, loadError: err }),
      onSuccess: (prefs) => resolve({ resolved: prefs, loadError: null }),
    });
  });

  assert.equal(result.resolved, null);
  assert.notEqual(result.loadError, null);
  assert.match(result.loadError!.message, /timed out/);
});

test('loader retries after timeout and applies persisted value on success', async () => {
  let callCount = 0;
  const persisted = preferences({ theme: 'dark' });
  const client: ShellPreferencesClient = {
    configGet: async () => {
      callCount += 1;
      if (callCount === 1) {
        // First call hangs
        return new Promise<string>(() => {});
      }
      // Retry succeeds
      return JSON.stringify(persisted);
    },
    configSet: async () => ok,
  };
  const loader = createShellPreferencesLoader();

  const events: Array<{ kind: string; prefs?: ShellPreferences; error?: string }> = [];

  await new Promise<void>((resolve) => {
    loader.load(client, {
      loadTimeoutMs: 50,
      retryDelayMs: 50,
      onDefaults: (err) => {
        events.push({ kind: 'defaults', error: err.message });
        // Don't resolve yet — wait for retry
      },
      onSuccess: (prefs) => {
        events.push({ kind: 'success', prefs });
        resolve();
      },
    });
  });

  assert.equal(callCount, 2);
  assert.equal(events.length, 2);
  assert.equal(events[0]!.kind, 'defaults');
  assert.match(events[0]!.error!, /timed out/);
  assert.equal(events[1]!.kind, 'success');
  assert.deepEqual(events[1]!.prefs, persisted);
});

test('loader retries on configGet transport error and applies persisted value', async () => {
  let callCount = 0;
  const persisted = preferences({ theme: 'light' });
  const client: ShellPreferencesClient = {
    configGet: async () => {
      callCount += 1;
      if (callCount === 1) {
        throw new Error('config service unavailable');
      }
      return JSON.stringify(persisted);
    },
    configSet: async () => ok,
  };
  const loader = createShellPreferencesLoader();

  const events: Array<{ kind: string; prefs?: ShellPreferences; error?: string }> = [];

  await new Promise<void>((resolve) => {
    loader.load(client, {
      loadTimeoutMs: 200,
      retryDelayMs: 50,
      onDefaults: (err) => {
        events.push({ kind: 'defaults', error: err.message });
      },
      onSuccess: (prefs) => {
        events.push({ kind: 'success', prefs });
        resolve();
      },
    });
  });

  assert.equal(callCount, 2);
  assert.equal(events.length, 2);
  assert.equal(events[0]!.kind, 'defaults');
  assert.equal(events[1]!.kind, 'success');
  assert.deepEqual(events[1]!.prefs, persisted);
});

test('loader cancel prevents retry callback from firing', async () => {
  const client: ShellPreferencesClient = {
    configGet: () => new Promise(() => {}), // hangs forever
    configSet: async () => ok,
  };
  const loader = createShellPreferencesLoader();

  let retryFired = false;
  const cancel = loader.load(client, {
    loadTimeoutMs: 30,
    retryDelayMs: 50,
    onDefaults: () => {},
    onSuccess: () => { retryFired = true; },
  });

  // Cancel after timeout but before retry
  await new Promise((r) => setTimeout(r, 50));
  cancel();

  // Wait long enough for retry to have fired if not cancelled
  await new Promise((r) => setTimeout(r, 100));
  assert.equal(retryFired, false);
});

test('loader old request sequence id does not overwrite newer load', async () => {
  // Start first load with a slow client
  let resolveFirst: (value: string) => void = () => {};
  const slowClient: ShellPreferencesClient = {
    configGet: () => new Promise<string>((r) => { resolveFirst = r; }),
    configSet: async () => ok,
  };
  const loader = createShellPreferencesLoader();

  const firstEvents: string[] = [];
  const cancel1 = loader.load(slowClient, {
    loadTimeoutMs: 500,
    retryDelayMs: 500,
    onDefaults: (err) => firstEvents.push('defaults:' + err.message),
    onSuccess: () => firstEvents.push('success'),
  });

  // Start a second load with a fast client (simulates re-mount or prop change)
  const secondPrefs = preferences({ theme: 'dark' });
  const fastClient: ShellPreferencesClient = {
    configGet: async () => JSON.stringify(secondPrefs),
    configSet: async () => ok,
  };
  const secondEvents: string[] = [];
  const cancel2 = loader.load(fastClient, {
    loadTimeoutMs: 100,
    retryDelayMs: 500,
    onDefaults: (err) => secondEvents.push('defaults:' + err.message),
    onSuccess: (prefs) => {
      secondEvents.push('success');
      assert.deepEqual(prefs, secondPrefs);
    },
  });

  // Now resolve the first (stale) request
  const stalePrefs = preferences({ theme: 'light' });
  resolveFirst(JSON.stringify(stalePrefs));

  // Wait a bit
  await new Promise((r) => setTimeout(r, 50));

  // The first load's success callback should NOT have fired
  assert.deepEqual(firstEvents, []);
  // The second load should have succeeded
  assert.deepEqual(secondEvents, ['success']);

  cancel1();
  cancel2();
});

test('loader default values are never auto-written back to storage', async () => {
  let writes = 0;
  const client: ShellPreferencesClient = {
    configGet: () => new Promise(() => {}), // hangs
    configSet: async () => {
      writes += 1;
      return ok;
    },
  };
  const loader = createShellPreferencesLoader();

  await new Promise<void>((resolve) => {
    loader.load(client, {
      loadTimeoutMs: 30,
      retryDelayMs: 500, // long enough to not retry during test
      onDefaults: () => resolve(),
      onSuccess: () => {},
    });
  });

  // The loader itself never writes — only the hook does via the save queue
  assert.equal(writes, 0, 'defaults must never be auto-persisted');
});

// ── Save-order guarantees: degraded → retry → user writes ────────────────

test('preferences write order: user save A in degraded state then retry B ensures B last', async () => {
  // Scenario:
  //   1. Load times out → defaults shown (degraded). Defaults NOT written.
  //   2. User changes to A — enqueued in save queue. A starts writing.
  //   3. Retry succeeds with B — hook enqueues B after A.
  //   4. A finishes → B starts → B finishes.
  //   Assertion: B was the LAST write to storage.
  const writes: string[] = [];
  const pendingWrites: Array<(result: typeof ok) => void> = [];

  const client: ShellPreferencesClient = {
    configGet: async () => '',
    configSet: async (_key, value) => {
      const theme = (JSON.parse(value) as { theme: string }).theme;
      writes.push(theme);
      return new Promise<typeof ok>((resolve) => {
        pendingWrites.push(resolve);
      });
    },
  };

  const queue = createShellPreferencesSaveQueue(client);

  // User changes to A while degraded
  const aPromise = queue.enqueue(preferences({ theme: 'dark' }));
  await nextTask();
  assert.deepEqual(writes, ['dark'], 'A must be the first write');

  // Retry succeeds — hook calls saveQueue.enqueue(B)
  const bPromise = queue.enqueue(preferences({ theme: 'light' }));
  await nextTask();
  // B is queued but A is still in-flight — no new write yet
  assert.deepEqual(writes, ['dark'], 'B must wait for A to finish');

  // Complete A → B should then start
  pendingWrites[0]!(ok);
  await aPromise;
  await nextTask();
  assert.deepEqual(writes, ['dark', 'light'], 'B must start after A completes');

  // Complete B
  pendingWrites[1]!(ok);
  await bPromise;

  assert.deepEqual(writes, ['dark', 'light'],
    'final writes must be [A, B] with B last');
  assert.equal(queue.latestError, null);
});

test('preferences write order: degraded A, retry B, user C ensures C is last', async () => {
  // Scenario:
  //   1. Load times out → defaults (degraded).
  //   2. User changes to A — enqueue A (in-flight).
  //   3. Retry succeeds with B — hook enqueues B after A.
  //   4. User changes to C — hook effect enqueues C after B.
  //   Assertion: final writes are [A, B, C] with C last.
  const writes: string[] = [];
  const pendingWrites: Array<(result: typeof ok) => void> = [];

  const client: ShellPreferencesClient = {
    configGet: async () => '',
    configSet: async (_key, value) => {
      const theme = (JSON.parse(value) as { theme: string }).theme;
      writes.push(theme);
      return new Promise<typeof ok>((resolve) => {
        pendingWrites.push(resolve);
      });
    },
  };

  const queue = createShellPreferencesSaveQueue(client);

  // User changes to A while degraded
  const aPromise = queue.enqueue(preferences({ theme: 'dark' }));
  await nextTask();
  assert.deepEqual(writes, ['dark']);

  // Retry loads B → enqueued by hook
  const bPromise = queue.enqueue(preferences({ theme: 'light' }));
  await nextTask();
  // Only A is in-flight
  assert.deepEqual(writes, ['dark']);

  // User changes to C → enqueued by hook effect
  const cPromise = queue.enqueue(preferences({ theme: 'system' }));
  await nextTask();
  assert.deepEqual(writes, ['dark']);

  // Complete A → B starts
  pendingWrites[0]!(ok);
  await aPromise;
  await nextTask();
  assert.deepEqual(writes, ['dark', 'light']);

  // Complete B → C starts
  pendingWrites[1]!(ok);
  await bPromise;
  await nextTask();
  assert.deepEqual(writes, ['dark', 'light', 'system']);

  // Complete C
  pendingWrites[2]!(ok);
  await cPromise;

  assert.deepEqual(writes, ['dark', 'light', 'system'],
    'final writes must be [A, B, C] with C last');
  assert.equal(queue.latestError, null);
});
