import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import {
  DEFAULT_SHELL_PREFERENCES,
  normalizeShellPreferences,
  parseShellPreferences,
  serializeShellPreferences,
  type ShellPreferences,
} from './shellPreferences.ts';

export const SHELL_PREFERENCES_CONFIG_KEY = 'gui_shell_preferences';

export interface ShellPreferencesCommandResult {
  readonly success: boolean;
  readonly stdout?: string;
  readonly stderr?: string;
  readonly exitCode?: number;
  readonly error?: {
    readonly message?: string;
  };
}

export interface ShellPreferencesClient {
  configGet(key: string): Promise<string>;
  configSet(key: string, value: string): Promise<ShellPreferencesCommandResult>;
}

export type ShellPreferencesUpdate =
  | ShellPreferences
  | ((current: ShellPreferences) => ShellPreferences);

export interface ShellPreferencesSaveQueue {
  readonly latestError: Error | null;
  enqueue(preferences: ShellPreferences): Promise<void>;
}

export interface UseShellPreferencesResult {
  readonly preferences: ShellPreferences;
  readonly ready: boolean;
  readonly loadError: Error | null;
  readonly saveError: Error | null;
  readonly updatePreferences: (update: ShellPreferencesUpdate) => void;
}

function defaultShellPreferences(): ShellPreferences {
  return normalizeShellPreferences(DEFAULT_SHELL_PREFERENCES);
}

function errorFromUnknown(error: unknown, fallback: string): Error {
  if (error instanceof Error) return error;
  if (typeof error === 'string' && error.trim().length > 0) {
    return new Error(error.trim());
  }
  return new Error(fallback);
}

function commandFailureDetail(result: ShellPreferencesCommandResult): string {
  const candidates = [result.error?.message, result.stderr, result.stdout];
  for (const candidate of candidates) {
    if (typeof candidate === 'string' && candidate.trim().length > 0) {
      return candidate.trim();
    }
  }
  return typeof result.exitCode === 'number'
    ? `configuration write exited with code ${result.exitCode}`
    : 'configuration service rejected the write';
}

async function saveSerializedShellPreferences(
  client: ShellPreferencesClient,
  value: string,
): Promise<void> {
  const result = await client.configSet(SHELL_PREFERENCES_CONFIG_KEY, value);
  if (!result.success) {
    throw new Error(`Failed to save shell preferences: ${commandFailureDetail(result)}`);
  }
}

export async function loadShellPreferences(
  client: ShellPreferencesClient,
): Promise<ShellPreferences> {
  const raw = await client.configGet(SHELL_PREFERENCES_CONFIG_KEY);
  return parseShellPreferences(raw);
}

export async function saveShellPreferences(
  client: ShellPreferencesClient,
  preferences: ShellPreferences,
): Promise<void> {
  await saveSerializedShellPreferences(client, serializeShellPreferences(preferences));
}

export function resolveShellPreferencesUpdate(
  current: ShellPreferences,
  update: ShellPreferencesUpdate,
): ShellPreferences {
  const next = typeof update === 'function' ? update(current) : update;
  return normalizeShellPreferences(next);
}

export function createShellPreferencesSaveQueue(
  client: ShellPreferencesClient,
  onLatestError?: (error: Error | null) => void,
): ShellPreferencesSaveQueue {
  let tail: Promise<void> = Promise.resolve();
  let latestRequestId = 0;
  let latestError: Error | null = null;
  let lastPayload: string | null = null;
  let lastPromise: Promise<void> | null = null;

  const setLatestError = (error: Error | null) => {
    if (latestError === error) return;
    latestError = error;
    onLatestError?.(error);
  };

  const queue: ShellPreferencesSaveQueue = {
    get latestError() {
      return latestError;
    },
    enqueue(preferences) {
      const payload = serializeShellPreferences(preferences);
      if (payload === lastPayload && lastPromise !== null) return lastPromise;

      const requestId = ++latestRequestId;
      if (latestError !== null) setLatestError(null);

      const run = tail.then(() => saveSerializedShellPreferences(client, payload));
      const observed = run.then(
        () => {
          if (requestId === latestRequestId) setLatestError(null);
        },
        (failure: unknown) => {
          const error = errorFromUnknown(failure, 'Failed to save shell preferences.');
          if (requestId === latestRequestId) {
            lastPayload = null;
            lastPromise = null;
            setLatestError(error);
          }
          throw error;
        },
      );
      tail = observed.catch(() => {});
      lastPayload = payload;
      lastPromise = observed;
      return observed;
    },
  };
  return queue;
}

export function useShellPreferences(
  client: ShellPreferencesClient,
): UseShellPreferencesResult {
  const [preferences, setPreferences] = useState<ShellPreferences>(defaultShellPreferences);
  const [ready, setReady] = useState(false);
  const [loadError, setLoadError] = useState<Error | null>(null);
  const [saveError, setSaveError] = useState<Error | null>(null);
  const persistedSnapshotRef = useRef<string | null>(null);
  const saveQueue = useMemo(
    () => createShellPreferencesSaveQueue(client, setSaveError),
    [client],
  );

  useEffect(() => {
    let active = true;
    persistedSnapshotRef.current = null;
    setReady(false);
    setLoadError(null);
    setSaveError(null);

    void loadShellPreferences(client).then(
      (loaded) => {
        if (!active) return;
        persistedSnapshotRef.current = serializeShellPreferences(loaded);
        setPreferences(loaded);
        setReady(true);
      },
      (failure: unknown) => {
        if (!active) return;
        const fallback = defaultShellPreferences();
        persistedSnapshotRef.current = serializeShellPreferences(fallback);
        setPreferences(fallback);
        setLoadError(errorFromUnknown(failure, 'Failed to load shell preferences.'));
        setReady(true);
      },
    );

    return () => {
      active = false;
    };
  }, [client]);

  useEffect(() => {
    // The loaded snapshot is the persistence baseline. Until it exists, the
    // initial defaults are render-only and must never be written back.
    if (!ready || persistedSnapshotRef.current === null) return;
    const snapshot = serializeShellPreferences(preferences);
    if (snapshot === persistedSnapshotRef.current) return;
    persistedSnapshotRef.current = snapshot;
    void saveQueue.enqueue(preferences).catch(() => {});
  }, [preferences, ready, saveQueue]);

  const updatePreferences = useCallback((update: ShellPreferencesUpdate) => {
    // Keep this updater free of I/O: React may evaluate state updaters more
    // than once in development StrictMode. The effect above owns persistence.
    setPreferences((current) => resolveShellPreferencesUpdate(current, update));
  }, []);

  return {
    preferences,
    ready,
    loadError,
    saveError,
    updatePreferences,
  };
}
