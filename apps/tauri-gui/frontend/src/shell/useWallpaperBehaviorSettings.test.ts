import assert from 'node:assert/strict';
import test from 'node:test';

import {
  DEFAULT_WALLPAPER_BEHAVIOR_SETTINGS,
  WALLPAPER_BEHAVIOR_CONFIG_KEYS,
  createWallpaperBehaviorPersistence,
  createWallpaperBehaviorSaveQueue,
  loadWallpaperBehaviorSettings,
  normalizeWallpaperBehaviorConfig,
  resolveWallpaperBehaviorSettingsUpdate,
  saveWallpaperBehaviorSettings,
  type WallpaperBehaviorSettings,
  type WallpaperBehaviorSettingsClient,
} from './useWallpaperBehaviorSettings.ts';

const ok = { success: true, stdout: '', stderr: '', exitCode: 0 };

function settings(
  overrides: Partial<WallpaperBehaviorSettings> = {},
): WallpaperBehaviorSettings {
  return {
    ...DEFAULT_WALLPAPER_BEHAVIOR_SETTINGS,
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

test('normalizes missing and incompatible behavior config to safe defaults', () => {
  assert.deepEqual(WALLPAPER_BEHAVIOR_CONFIG_KEYS, [
    'image_backend',
    'gif_backend',
    'video_backend',
    'awww_resize',
    'awww_transition_type',
    'awww_transition_duration',
    'wallpaper_transition_fps',
    'linux_wallpaperengine_scaling',
    'linux_wallpaperengine_fps',
    'linux_wallpaperengine_muted',
    'linux_wallpaperengine_volume',
    'restore_on_login',
  ]);
  assert.deepEqual(normalizeWallpaperBehaviorConfig({}), DEFAULT_WALLPAPER_BEHAVIOR_SETTINGS);
  assert.deepEqual(normalizeWallpaperBehaviorConfig({
    image_backend: 'unknown',
    gif_backend: 'swww',
    video_backend: 'awww',
    awww_resize: 'center',
    awww_transition_type: 'explode',
    awww_transition_duration: '61',
    wallpaper_transition_fps: 'not-a-number',
    linux_wallpaperengine_scaling: 'tile',
    linux_wallpaperengine_fps: 'not-a-number',
    linux_wallpaperengine_muted: 'true',
    linux_wallpaperengine_volume: 'not-a-number',
    restore_on_login: 'true',
  }), DEFAULT_WALLPAPER_BEHAVIOR_SETTINGS);
  assert.deepEqual(normalizeWallpaperBehaviorConfig({
    image_backend: 'mpvpaper',
    gif_backend: 'mpvpaper',
    video_backend: 'mpvpaper',
    awww_resize: 'fit',
    awww_transition_type: 'wipe',
    awww_transition_duration: '1.5',
    wallpaper_transition_fps: '144',
    linux_wallpaperengine_scaling: 'fill',
    linux_wallpaperengine_fps: '30',
    linux_wallpaperengine_muted: 'on',
    linux_wallpaperengine_volume: '35',
    restore_on_login: 'on',
  }), settings({
    imageBackend: 'mpvpaper',
    gifBackend: 'mpvpaper',
    fillMode: 'fit',
    awwwTransitionType: 'wipe',
    awwwTransitionDuration: 1.5,
    awwwTransitionFps: 144,
    lweScaling: 'fill',
    lweFps: 30,
    lweMuted: true,
    lweVolume: 35,
    restoreOnLogin: true,
  }));
  assert.deepEqual(normalizeWallpaperBehaviorConfig({
    awww_transition_type: 'slide',
    awww_transition_duration: '60',
    wallpaper_transition_fps: '999',
    linux_wallpaperengine_scaling: 'stretch',
    linux_wallpaperengine_fps: '0',
    linux_wallpaperengine_muted: 'off',
    linux_wallpaperengine_volume: '-5',
    restore_on_login: 'off',
  }), settings({
    awwwTransitionType: 'left',
    awwwTransitionDuration: 60,
    awwwTransitionFps: 240,
    lweScaling: 'stretch',
    lweFps: 1,
    lweVolume: 0,
  }));
});

test('load requests only the behavior keys and never writes defaults before load', async () => {
  const requests: string[][] = [];
  let writes = 0;
  const pending = deferred<Record<string, string>>();
  const client: WallpaperBehaviorSettingsClient = {
    configGetMany: async (keys) => {
      requests.push([...keys]);
      return pending.promise;
    },
    configSet: async () => {
      writes += 1;
      return ok;
    },
  };

  const loading = loadWallpaperBehaviorSettings(client);
  await nextTask();
  assert.deepEqual(requests, [[...WALLPAPER_BEHAVIOR_CONFIG_KEYS]]);
  assert.equal(writes, 0);

  pending.resolve({ video_backend: 'awww', awww_resize: 'stretch' });
  assert.deepEqual(await loading, settings({ fillMode: 'stretch' }));
  assert.equal(writes, 0, 'normalizing loaded values is not an implicit write');
});

test('load propagates config transport failures', async () => {
  const client: WallpaperBehaviorSettingsClient = {
    configGetMany: async () => {
      throw new Error('configuration unavailable');
    },
    configSet: async () => ok,
  };

  await assert.rejects(loadWallpaperBehaviorSettings(client), /configuration unavailable/);
});

test('save writes a normalized snapshot in fixed order and video is always mpvpaper', async () => {
  const writes: Array<[string, string]> = [];
  const client: WallpaperBehaviorSettingsClient = {
    configGetMany: async () => ({}),
    configSet: async (key, value) => {
      writes.push([key, value]);
      return ok;
    },
  };

  await saveWallpaperBehaviorSettings(client, {
    ...settings(),
    imageBackend: 'mpvpaper',
    videoBackend: 'awww',
    fillMode: 'fit',
    awwwTransitionType: 'grow',
    awwwTransitionDuration: 2.5,
    awwwTransitionFps: 120,
    lweScaling: 'stretch',
    lweFps: 75,
    lweMuted: true,
    lweVolume: 45,
    restoreOnLogin: true,
  } as unknown as WallpaperBehaviorSettings);

  assert.deepEqual(writes, [
    ['image_backend', 'mpvpaper'],
    ['gif_backend', 'awww'],
    ['video_backend', 'mpvpaper'],
    ['awww_resize', 'fit'],
    ['awww_transition_type', 'grow'],
    ['awww_transition_duration', '2.5'],
    ['wallpaper_transition_fps', '120'],
    ['linux_wallpaperengine_scaling', 'stretch'],
    ['linux_wallpaperengine_fps', '75'],
    ['linux_wallpaperengine_muted', 'on'],
    ['linux_wallpaperengine_volume', '45'],
    ['restore_on_login', 'on'],
  ]);
});

test('save turns an unsuccessful CommandResult into a setting-specific error', async () => {
  const client: WallpaperBehaviorSettingsClient = {
    configGetMany: async () => ({}),
    configSet: async (key) => key === 'gif_backend'
      ? {
          success: false,
          stdout: '',
          stderr: 'configuration file is read-only',
          exitCode: 1,
        }
      : ok,
  };

  await assert.rejects(
    saveWallpaperBehaviorSettings(client, settings()),
    /Failed to save wallpaper behavior setting "gif_backend": configuration file is read-only/,
  );
});

test('functional updates are normalized without performing persistence', () => {
  let calls = 0;
  const next = resolveWallpaperBehaviorSettingsUpdate(
    settings(),
    (current) => {
      calls += 1;
      return {
        ...current,
        imageBackend: 'mpvpaper',
        videoBackend: 'awww',
        fillMode: 'invalid',
      } as unknown as WallpaperBehaviorSettings;
    },
  );

  assert.equal(calls, 1);
  assert.deepEqual(next, settings({ imageBackend: 'mpvpaper' }));
});

test('save queue serializes snapshots so the newest values finish last', async () => {
  const starts: string[] = [];
  const writes: Array<ReturnType<typeof deferred<typeof ok>>> = [];
  const client: WallpaperBehaviorSettingsClient = {
    configGetMany: async () => ({}),
    configSet: async (key, value) => {
      starts.push(`${key}=${value}`);
      const write = deferred<typeof ok>();
      writes.push(write);
      return write.promise;
    },
  };
  const queue = createWallpaperBehaviorSaveQueue(client);
  const older = queue.enqueue(settings({ imageBackend: 'mpvpaper' }));
  const newer = queue.enqueue(settings({ imageBackend: 'awww', fillMode: 'fit' }));

  await nextTask();
  assert.deepEqual(starts, ['image_backend=mpvpaper']);
  const snapshotSize = WALLPAPER_BEHAVIOR_CONFIG_KEYS.length;
  for (let index = 0; index < snapshotSize; index += 1) {
    writes[index]!.resolve(ok);
    await nextTask();
  }
  await older;
  assert.equal(starts[snapshotSize], 'image_backend=awww');
  for (let index = snapshotSize; index < snapshotSize * 2; index += 1) {
    writes[index]!.resolve(ok);
    await nextTask();
  }
  await newer;
  assert.equal(starts.at(-1), 'restore_on_login=off');
  assert.equal(starts[snapshotSize + 3], 'awww_resize=fit');
  assert.equal(queue.latestError, null);
});

test('save queue reports only the latest request failure and recovers on a newer success', async () => {
  let attempt = 0;
  const observed: Array<string | null> = [];
  const client: WallpaperBehaviorSettingsClient = {
    configGetMany: async () => ({}),
    configSet: async () => {
      attempt += 1;
      if (attempt === 1) throw new Error('stale transport failure');
      if (attempt === 2) throw new Error('latest transport failure');
      return ok;
    },
  };
  const queue = createWallpaperBehaviorSaveQueue(
    client,
    (error) => observed.push(error?.message ?? null),
  );

  const stale = queue.enqueue(settings({ imageBackend: 'mpvpaper' }));
  const latest = queue.enqueue(settings({ gifBackend: 'mpvpaper' }));
  await assert.rejects(stale, /stale transport failure/);
  await assert.rejects(latest, /latest transport failure/);
  assert.equal(queue.latestError?.message, 'latest transport failure');

  const recovery = queue.enqueue(settings({ fillMode: 'stretch' }));
  await recovery;
  assert.equal(queue.latestError, null);
  assert.deepEqual(observed, ['latest transport failure', null]);
});

test('persistence retries one transient failure and confirms the snapshot only on success', async () => {
  let attempts = 0;
  const client: WallpaperBehaviorSettingsClient = {
    configGetMany: async () => ({}),
    configSet: async () => {
      attempts += 1;
      if (attempts === 1) throw new Error('temporary write failure');
      return ok;
    },
  };
  const persistence = createWallpaperBehaviorPersistence(
    createWallpaperBehaviorSaveQueue(client),
  );
  const changed = settings({ imageBackend: 'mpvpaper' });
  persistence.reset(settings());

  await persistence.persist(changed);
  assert.equal(attempts, 1 + WALLPAPER_BEHAVIOR_CONFIG_KEYS.length);

  await persistence.persist({ ...changed });
  assert.equal(
    attempts,
    1 + WALLPAPER_BEHAVIOR_CONFIG_KEYS.length,
    'a successful write becomes the new persisted baseline',
  );
});

test('persistence stops after one automatic retry when failures continue', async () => {
  let attempts = 0;
  const client: WallpaperBehaviorSettingsClient = {
    configGetMany: async () => ({}),
    configSet: async () => {
      attempts += 1;
      throw new Error('persistent write failure');
    },
  };
  const persistence = createWallpaperBehaviorPersistence(
    createWallpaperBehaviorSaveQueue(client),
  );
  persistence.reset(settings());

  await assert.rejects(
    persistence.persist(settings({ imageBackend: 'mpvpaper' })),
    /persistent write failure/,
  );
  await nextTask();
  assert.equal(attempts, 2);
});

test('persistence permits an explicit retry after the bounded attempts fail', async () => {
  let attempts = 0;
  const client: WallpaperBehaviorSettingsClient = {
    configGetMany: async () => ({}),
    configSet: async () => {
      attempts += 1;
      if (attempts <= 2) throw new Error('storage temporarily unavailable');
      return ok;
    },
  };
  const persistence = createWallpaperBehaviorPersistence(
    createWallpaperBehaviorSaveQueue(client),
  );
  const changed = settings({ imageBackend: 'mpvpaper' });
  persistence.reset(settings());

  await assert.rejects(persistence.persist(changed), /storage temporarily unavailable/);
  await persistence.persist({ ...changed });

  assert.equal(attempts, 2 + WALLPAPER_BEHAVIOR_CONFIG_KEYS.length);
});

test('persistence restores the confirmed snapshot when UI reverts during an in-flight save', async () => {
  const firstWrite = deferred<typeof ok>();
  const imageWrites: string[] = [];
  let attempts = 0;
  const client: WallpaperBehaviorSettingsClient = {
    configGetMany: async () => ({}),
    configSet: async (key, value) => {
      attempts += 1;
      if (key === 'image_backend') imageWrites.push(value);
      if (attempts === 1) return firstWrite.promise;
      return ok;
    },
  };
  const persistence = createWallpaperBehaviorPersistence(
    createWallpaperBehaviorSaveQueue(client),
  );
  const original = settings();
  persistence.reset(original);

  const change = persistence.persist(settings({ imageBackend: 'mpvpaper' }));
  await nextTask();
  const revert = persistence.persist({ ...original });
  firstWrite.resolve(ok);

  await Promise.all([change, revert]);
  assert.deepEqual(imageWrites, ['mpvpaper', 'awww']);
  assert.equal(attempts, WALLPAPER_BEHAVIOR_CONFIG_KEYS.length * 2);
});

test('persistence repairs a partially failed save when UI reverts to the prior snapshot', async () => {
  const failedWrite = deferred<typeof ok>();
  const imageWrites: string[] = [];
  let gifAttempts = 0;
  const client: WallpaperBehaviorSettingsClient = {
    configGetMany: async () => ({}),
    configSet: async (key, value) => {
      if (key === 'image_backend') imageWrites.push(value);
      if (key === 'gif_backend') {
        gifAttempts += 1;
        if (gifAttempts === 1) return failedWrite.promise;
      }
      return ok;
    },
  };
  const persistence = createWallpaperBehaviorPersistence(
    createWallpaperBehaviorSaveQueue(client),
  );
  const original = settings();
  persistence.reset(original);

  const change = persistence.persist(settings({ imageBackend: 'mpvpaper' }));
  await nextTask();
  const revert = persistence.persist({ ...original });
  failedWrite.reject(new Error('write interrupted after the first key'));

  await Promise.all([change, revert]);
  assert.deepEqual(imageWrites, ['mpvpaper', 'awww']);
});

test('persistence coalesces rapid changes to the latest UI snapshot', async () => {
  const firstWrite = deferred<typeof ok>();
  const imageWrites: string[] = [];
  const resizeWrites: string[] = [];
  let attempts = 0;
  const client: WallpaperBehaviorSettingsClient = {
    configGetMany: async () => ({}),
    configSet: async (key, value) => {
      attempts += 1;
      if (key === 'image_backend') imageWrites.push(value);
      if (key === 'awww_resize') resizeWrites.push(value);
      if (attempts === 1) return firstWrite.promise;
      return ok;
    },
  };
  const persistence = createWallpaperBehaviorPersistence(
    createWallpaperBehaviorSaveQueue(client),
  );
  const original = settings();
  persistence.reset(original);

  const first = persistence.persist(settings({ imageBackend: 'mpvpaper' }));
  await nextTask();
  const superseded = persistence.persist(settings({ fillMode: 'fit' }));
  const latest = persistence.persist({ ...original });
  firstWrite.resolve(ok);

  await Promise.all([first, superseded, latest]);
  assert.deepEqual(imageWrites, ['mpvpaper', 'awww']);
  assert.deepEqual(resizeWrites, ['crop', 'crop']);
  assert.equal(attempts, WALLPAPER_BEHAVIOR_CONFIG_KEYS.length * 2);
});

test('persistence deduplicates an in-flight desired snapshot', async () => {
  const firstWrite = deferred<typeof ok>();
  let attempts = 0;
  const client: WallpaperBehaviorSettingsClient = {
    configGetMany: async () => ({}),
    configSet: async () => {
      attempts += 1;
      if (attempts === 1) return firstWrite.promise;
      return ok;
    },
  };
  const persistence = createWallpaperBehaviorPersistence(
    createWallpaperBehaviorSaveQueue(client),
  );
  persistence.reset(settings());
  const changed = settings({ imageBackend: 'mpvpaper' });

  const first = persistence.persist(changed);
  await nextTask();
  const duplicate = persistence.persist({ ...changed });
  assert.equal(attempts, 1);
  firstWrite.resolve(ok);

  await Promise.all([first, duplicate]);
  assert.equal(attempts, WALLPAPER_BEHAVIOR_CONFIG_KEYS.length);
});

test('persistence drains a newer revision arriving between terminal failure and rejection handling', async () => {
  const secondFailure = deferred<typeof ok>();
  const recoveryScheduled = deferred<void>();
  const imageWrites: string[] = [];
  let attempts = 0;
  const client: WallpaperBehaviorSettingsClient = {
    configGetMany: async () => ({}),
    configSet: async (key, value) => {
      attempts += 1;
      if (key === 'image_backend') imageWrites.push(value);
      if (attempts === 1) throw new Error('first persistent failure');
      if (attempts === 2) return secondFailure.promise;
      return ok;
    },
  };
  const persistence = createWallpaperBehaviorPersistence(
    createWallpaperBehaviorSaveQueue(client),
  );
  persistence.reset(settings());

  let terminalFailure: unknown = null;
  let recoveryPromise: Promise<void> | null = null;
  const originalSave = persistence.persist(settings({ imageBackend: 'mpvpaper' }));
  const completion = originalSave.then(
    () => {},
    (failure: unknown) => {
      terminalFailure = failure;
    },
  );
  await nextTask();
  assert.equal(attempts, 2);

  void secondFailure.promise.catch(() => {
    // The fourth microtask lands after drain has rejected but before the
    // active-promise rejection observer runs.
    queueMicrotask(() => queueMicrotask(() => queueMicrotask(() => queueMicrotask(() => {
      recoveryPromise = persistence.persist(settings({ fillMode: 'fit' }));
      recoveryScheduled.resolve();
    }))));
  });
  secondFailure.reject(new Error('second persistent failure'));

  await recoveryScheduled.promise;
  assert.equal(recoveryPromise, originalSave, 'the terminal failure observer is still pending');
  await completion;
  assert.equal(terminalFailure, null);
  assert.deepEqual(imageWrites, ['mpvpaper', 'mpvpaper', 'awww']);
  assert.equal(attempts, 2 + WALLPAPER_BEHAVIOR_CONFIG_KEYS.length);
  await nextTask();
  assert.equal(attempts, 2 + WALLPAPER_BEHAVIOR_CONFIG_KEYS.length);
});

test('reset during an in-flight save rewrites the new baseline after the old save succeeds', async () => {
  const oldWrite = deferred<typeof ok>();
  const imageWrites: string[] = [];
  const resizeWrites: string[] = [];
  let attempts = 0;
  const client: WallpaperBehaviorSettingsClient = {
    configGetMany: async () => ({}),
    configSet: async (key, value) => {
      attempts += 1;
      if (key === 'image_backend') imageWrites.push(value);
      if (key === 'awww_resize') resizeWrites.push(value);
      if (attempts === 1) return oldWrite.promise;
      return ok;
    },
  };
  const persistence = createWallpaperBehaviorPersistence(
    createWallpaperBehaviorSaveQueue(client),
  );
  persistence.reset(settings());

  const oldSave = persistence.persist(settings({ imageBackend: 'mpvpaper' }));
  await nextTask();
  persistence.reset(settings({ fillMode: 'fit' }));
  oldWrite.resolve(ok);

  await oldSave;
  assert.deepEqual(imageWrites, ['mpvpaper', 'awww']);
  assert.deepEqual(resizeWrites, ['crop', 'fit']);
  assert.equal(attempts, WALLPAPER_BEHAVIOR_CONFIG_KEYS.length * 2);
});

test('reset through an unavailable baseline rewrites the new baseline after the old save fails', async () => {
  const oldWrite = deferred<typeof ok>();
  const imageWrites: string[] = [];
  const resizeWrites: string[] = [];
  let attempts = 0;
  const client: WallpaperBehaviorSettingsClient = {
    configGetMany: async () => ({}),
    configSet: async (key, value) => {
      attempts += 1;
      if (key === 'image_backend') imageWrites.push(value);
      if (key === 'awww_resize') resizeWrites.push(value);
      if (attempts === 1) return oldWrite.promise;
      return ok;
    },
  };
  const persistence = createWallpaperBehaviorPersistence(
    createWallpaperBehaviorSaveQueue(client),
  );
  persistence.reset(settings());

  const oldSave = persistence.persist(settings({ imageBackend: 'mpvpaper' }));
  await nextTask();
  persistence.reset(null);
  persistence.reset(settings({ fillMode: 'fit' }));
  oldWrite.reject(new Error('old generation failed'));

  await oldSave;
  assert.deepEqual(imageWrites, ['mpvpaper', 'awww']);
  assert.deepEqual(resizeWrites, ['fit']);
  assert.equal(attempts, 1 + WALLPAPER_BEHAVIOR_CONFIG_KEYS.length);
});
