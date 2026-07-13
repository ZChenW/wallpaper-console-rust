import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react';

export const WALLPAPER_BEHAVIOR_CONFIG_KEYS = Object.freeze([
  'image_backend',
  'gif_backend',
  'video_backend',
  'awww_resize',
] as const);

export type ImageRenderer = 'awww' | 'mpvpaper';
export type GifRenderer = 'awww' | 'mpvpaper';
export type VideoRenderer = 'mpvpaper';
export type WallpaperFillMode = 'crop' | 'fit' | 'stretch';

export interface WallpaperBehaviorSettings {
  readonly imageBackend: ImageRenderer;
  readonly gifBackend: GifRenderer;
  readonly videoBackend: VideoRenderer;
  readonly fillMode: WallpaperFillMode;
}

export const DEFAULT_WALLPAPER_BEHAVIOR_SETTINGS: Readonly<WallpaperBehaviorSettings> =
  Object.freeze({
    imageBackend: 'awww',
    gifBackend: 'awww',
    videoBackend: 'mpvpaper',
    fillMode: 'crop',
  });

export interface WallpaperBehaviorCommandResult {
  readonly success: boolean;
  readonly stdout?: string;
  readonly stderr?: string;
  readonly exitCode?: number;
  readonly error?: {
    readonly message?: string;
    readonly detail?: string;
    readonly suggestion?: string;
  };
}

export interface WallpaperBehaviorSettingsClient {
  configGetMany(keys: string[]): Promise<Record<string, string>>;
  configSet(key: string, value: string): Promise<WallpaperBehaviorCommandResult>;
}

export type WallpaperBehaviorSettingsUpdate =
  | WallpaperBehaviorSettings
  | ((current: WallpaperBehaviorSettings) => WallpaperBehaviorSettings);

export interface WallpaperBehaviorSaveQueue {
  readonly latestError: Error | null;
  enqueue(settings: WallpaperBehaviorSettings): Promise<void>;
}

export interface UseWallpaperBehaviorSettingsResult {
  readonly settings: WallpaperBehaviorSettings;
  readonly ready: boolean;
  readonly loadError: Error | null;
  readonly saveError: Error | null;
  readonly updateSettings: (update: WallpaperBehaviorSettingsUpdate) => void;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function renderer(value: unknown): ImageRenderer {
  return value === 'mpvpaper' ? 'mpvpaper' : 'awww';
}

function fillMode(value: unknown): WallpaperFillMode {
  return value === 'fit' || value === 'stretch' ? value : 'crop';
}

function defaultSettings(): WallpaperBehaviorSettings {
  return { ...DEFAULT_WALLPAPER_BEHAVIOR_SETTINGS };
}

/** Normalize the four raw config values exposed by the compact settings UI. */
export function normalizeWallpaperBehaviorConfig(
  values: Readonly<Record<string, string | undefined>>,
): WallpaperBehaviorSettings {
  return {
    imageBackend: renderer(values.image_backend),
    gifBackend: renderer(values.gif_backend),
    // mpvpaper is the only compatible video renderer. Legacy or malformed
    // values are deliberately repaired instead of being offered in the UI.
    videoBackend: 'mpvpaper',
    fillMode: fillMode(values.awww_resize),
  };
}

/** Repair an untrusted in-memory update before it reaches state or storage. */
export function normalizeWallpaperBehaviorSettings(value: unknown): WallpaperBehaviorSettings {
  const record = isRecord(value) ? value : {};
  return {
    imageBackend: renderer(record.imageBackend),
    gifBackend: renderer(record.gifBackend),
    videoBackend: 'mpvpaper',
    fillMode: fillMode(record.fillMode),
  };
}

function configEntries(
  value: WallpaperBehaviorSettings,
): ReadonlyArray<readonly [string, string]> {
  const normalized = normalizeWallpaperBehaviorSettings(value);
  return [
    ['image_backend', normalized.imageBackend],
    ['gif_backend', normalized.gifBackend],
    ['video_backend', normalized.videoBackend],
    ['awww_resize', normalized.fillMode],
  ];
}

function settingsSnapshot(value: WallpaperBehaviorSettings): string {
  return JSON.stringify(configEntries(value));
}

function errorFromUnknown(error: unknown, fallback: string): Error {
  if (error instanceof Error) return error;
  if (typeof error === 'string' && error.trim().length > 0) return new Error(error.trim());
  return new Error(fallback);
}

function commandFailureDetail(result: WallpaperBehaviorCommandResult): string {
  const candidates = [
    result.error?.message,
    result.error?.detail,
    result.stderr,
    result.stdout,
    result.error?.suggestion,
  ];
  for (const candidate of candidates) {
    if (typeof candidate === 'string' && candidate.trim().length > 0) {
      return candidate.trim();
    }
  }
  return typeof result.exitCode === 'number'
    ? `configuration write exited with code ${result.exitCode}`
    : 'configuration service rejected the write';
}

export async function loadWallpaperBehaviorSettings(
  client: WallpaperBehaviorSettingsClient,
): Promise<WallpaperBehaviorSettings> {
  const values = await client.configGetMany([...WALLPAPER_BEHAVIOR_CONFIG_KEYS]);
  return normalizeWallpaperBehaviorConfig(values);
}

export async function saveWallpaperBehaviorSettings(
  client: WallpaperBehaviorSettingsClient,
  value: WallpaperBehaviorSettings,
): Promise<void> {
  for (const [key, settingValue] of configEntries(value)) {
    const result = await client.configSet(key, settingValue);
    if (!result.success) {
      throw new Error(
        `Failed to save wallpaper behavior setting "${key}": ${commandFailureDetail(result)}`,
      );
    }
  }
}

export function resolveWallpaperBehaviorSettingsUpdate(
  current: WallpaperBehaviorSettings,
  update: WallpaperBehaviorSettingsUpdate,
): WallpaperBehaviorSettings {
  const next = typeof update === 'function' ? update(current) : update;
  return normalizeWallpaperBehaviorSettings(next);
}

export function createWallpaperBehaviorSaveQueue(
  client: WallpaperBehaviorSettingsClient,
  onLatestError?: (error: Error | null) => void,
): WallpaperBehaviorSaveQueue {
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

  const queue: WallpaperBehaviorSaveQueue = {
    get latestError() {
      return latestError;
    },
    enqueue(value) {
      const normalized = normalizeWallpaperBehaviorSettings(value);
      const payload = settingsSnapshot(normalized);
      if (payload === lastPayload && lastPromise !== null) return lastPromise;

      const requestId = ++latestRequestId;
      if (latestError !== null) setLatestError(null);

      const run = tail.then(() => saveWallpaperBehaviorSettings(client, normalized));
      const observed = run.then(
        () => {
          if (requestId === latestRequestId) setLatestError(null);
        },
        (failure: unknown) => {
          const error = errorFromUnknown(failure, 'Failed to save wallpaper behavior settings.');
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

export function useWallpaperBehaviorSettings(
  client: WallpaperBehaviorSettingsClient,
): UseWallpaperBehaviorSettingsResult {
  const [settings, setSettings] = useState<WallpaperBehaviorSettings>(defaultSettings);
  const [ready, setReady] = useState(false);
  const [loadError, setLoadError] = useState<Error | null>(null);
  const [saveError, setSaveError] = useState<Error | null>(null);
  const persistedSnapshotRef = useRef<string | null>(null);
  const saveQueue = useMemo(
    () => createWallpaperBehaviorSaveQueue(client, setSaveError),
    [client],
  );

  useEffect(() => {
    let active = true;
    persistedSnapshotRef.current = null;
    setReady(false);
    setLoadError(null);
    setSaveError(null);

    void loadWallpaperBehaviorSettings(client).then(
      (loaded) => {
        if (!active) return;
        persistedSnapshotRef.current = settingsSnapshot(loaded);
        setSettings(loaded);
        setReady(true);
      },
      (failure: unknown) => {
        if (!active) return;
        const fallback = defaultSettings();
        persistedSnapshotRef.current = settingsSnapshot(fallback);
        setSettings(fallback);
        setLoadError(errorFromUnknown(failure, 'Failed to load wallpaper behavior settings.'));
        setReady(true);
      },
    );

    return () => {
      active = false;
    };
  }, [client]);

  useEffect(() => {
    // Initial defaults are render-only. Writes begin only after a load (or a
    // load failure with an explicit fallback baseline) has completed.
    if (!ready || persistedSnapshotRef.current === null) return;
    const snapshot = settingsSnapshot(settings);
    if (snapshot === persistedSnapshotRef.current) return;
    persistedSnapshotRef.current = snapshot;
    void saveQueue.enqueue(settings).catch(() => {});
  }, [ready, saveQueue, settings]);

  const updateSettings = useCallback((update: WallpaperBehaviorSettingsUpdate) => {
    // React can evaluate functional updaters more than once in StrictMode, so
    // persistence belongs to the effect above rather than this callback.
    setSettings((current) => resolveWallpaperBehaviorSettingsUpdate(current, update));
  }, []);

  return {
    settings,
    ready,
    loadError,
    saveError,
    updateSettings,
  };
}
