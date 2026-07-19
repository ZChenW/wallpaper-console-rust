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

// ── Bounded configGet with retry ────────────────────────────────────────

function configTimeoutError(timeoutMs: number): Error {
  return new Error(`configGet timed out after ${timeoutMs}ms`);
}

async function configGetWithTimeout(
  client: ShellPreferencesClient,
  timeoutMs: number,
): Promise<string> {
  if (!Number.isFinite(timeoutMs) || timeoutMs <= 0) {
    return client.configGet(SHELL_PREFERENCES_CONFIG_KEY);
  }
  return new Promise<string>((resolve, reject) => {
    const timer = setTimeout(() => {
      reject(configTimeoutError(timeoutMs));
    }, timeoutMs);
    client.configGet(SHELL_PREFERENCES_CONFIG_KEY).then(
      (value) => { clearTimeout(timer); resolve(value); },
      (err) => { clearTimeout(timer); reject(err); },
    );
  });
}

export interface ShellPreferencesLoadOptions {
  /** Timeout in ms for the initial configGet call. */
  loadTimeoutMs: number;
  /** Delay in ms before retrying after a timeout or error. */
  retryDelayMs: number;
  /** Called when the initial load fails (timeout or error) — built-in defaults are used. */
  onDefaults: (loadError: Error) => void;
  /** Called when persisted preferences are successfully loaded (initial or retry). */
  onSuccess: (preferences: ShellPreferences) => void;
}

export interface ShellPreferencesLoader {
  /**
   * Start loading preferences with a bounded timeout and scheduled retry.
   * Returns a cleanup function that cancels any pending retry and prevents
   * any in-flight callback from overwriting a subsequent load.
   */
  load(
    client: ShellPreferencesClient,
    options: ShellPreferencesLoadOptions,
  ): () => void;
}

export function createShellPreferencesLoader(): ShellPreferencesLoader {
  let requestId = 0;

  return {
    load(client, options) {
      const currentRequestId = ++requestId;
      let cancelled = false;
      let retryTimer: ReturnType<typeof setTimeout> | null = null;

      const isCurrent = () => requestId === currentRequestId && !cancelled;

      const scheduleRetry = () => {
        if (!isCurrent()) return;
        retryTimer = setTimeout(async () => {
          if (!isCurrent()) return;
          try {
            const raw = await configGetWithTimeout(client, options.loadTimeoutMs);
            if (!isCurrent()) return;
            const prefs = parseShellPreferences(raw);
            if (!isCurrent()) return;
            options.onSuccess(prefs);
          } catch {
            // Retry failed — do not retry again automatically.
            // The caller can choose to retry manually.
          }
        }, options.retryDelayMs);
      };

      // Fire the initial load immediately.
      void configGetWithTimeout(client, options.loadTimeoutMs).then(
        (raw) => {
          if (!isCurrent()) return;
          const prefs = parseShellPreferences(raw);
          if (!isCurrent()) return;
          options.onSuccess(prefs);
        },
        (error) => {
          if (!isCurrent()) return;
          const loadError = error instanceof Error ? error : new Error(String(error));
          options.onDefaults(loadError);
          scheduleRetry();
        },
      );

      return () => {
        cancelled = true;
        if (retryTimer !== null) {
          clearTimeout(retryTimer);
          retryTimer = null;
        }
      };
    },
  };
}

export function useShellPreferences(
  client: ShellPreferencesClient,
  loadTimeoutMs = 3_000,
  retryDelayMs = 5_000,
): UseShellPreferencesResult {
  const [preferences, setPreferences] = useState<ShellPreferences>(defaultShellPreferences);
  const [ready, setReady] = useState(false);
  const [loadError, setLoadError] = useState<Error | null>(null);
  const [saveError, setSaveError] = useState<Error | null>(null);
  const persistedSnapshotRef = useRef<string | null>(null);
  const loaderRef = useRef(createShellPreferencesLoader());
  const saveQueue = useMemo(
    () => createShellPreferencesSaveQueue(client, setSaveError),
    [client],
  );

  // Tracks whether the current load cycle fell back to defaults (degraded).
  // When a retry-after-degraded succeeds, the loaded preferences must be
  // written back to storage so they are not overwritten by an in-flight
  // user save that started while defaults were shown.
  const wasDegradedRef = useRef(false);

  useEffect(() => {
    persistedSnapshotRef.current = null;
    wasDegradedRef.current = false;
    setReady(false);
    setLoadError(null);
    setSaveError(null);

    const cancel = loaderRef.current.load(client, {
      loadTimeoutMs,
      retryDelayMs,
      onDefaults: (err) => {
        const fallback = defaultShellPreferences();
        // Ensure defaults are the render baseline but never auto-persisted.
        persistedSnapshotRef.current = serializeShellPreferences(fallback);
        wasDegradedRef.current = true;
        setPreferences(fallback);
        setLoadError(err);
        setReady(true);
      },
      onSuccess: (loaded) => {
        persistedSnapshotRef.current = serializeShellPreferences(loaded);
        setPreferences(loaded);
        setLoadError(null);
        setReady(true);

        // Retry-after-degraded: the UI is showing defaults and the user may
        // have already enqueued a save (A).  Enqueue the loaded snapshot (B)
        // so the save queue order is [A, B] and B finishes last.  Normal
        // initial loads (wasDegraded == false) must NOT write — the snapshot
        // already matches and the write effect is a no-op.
        if (wasDegradedRef.current) {
          wasDegradedRef.current = false;
          void saveQueue.enqueue(loaded).catch(() => {});
        }
      },
    });

    return () => {
      cancel();
    };
  }, [client, loadTimeoutMs, retryDelayMs, saveQueue]);

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
