import assert from 'node:assert/strict';
import test from 'node:test';

import {
  SHELL_PREFERENCES_CONFIG_KEY,
  createShellPreferencesSaveQueue,
  loadShellPreferences,
  resolveShellPreferencesUpdate,
  saveShellPreferences,
  type ShellPreferencesClient,
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
  assert.equal(queue.latestError, null);

  writes[1]!.reject(new Error('latest write failed'));
  await assert.rejects(latest, /latest write failed/);
  assert.equal(queue.latestError?.message, 'latest write failed');

  const recovery = queue.enqueue(preferences({ theme: 'light' }));
  assert.equal(queue.latestError, null);
  await nextTask();
  writes[2]!.resolve(ok);
  await recovery;
  assert.equal(queue.latestError, null);
  assert.deepEqual(observed, ['latest write failed', null]);
});
