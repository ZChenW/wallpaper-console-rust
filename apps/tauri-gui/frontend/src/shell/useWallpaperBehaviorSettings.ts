import {
  useCallback,
  useEffect,
  useMemo,
  useState,
} from 'react';

export const WALLPAPER_BEHAVIOR_CONFIG_KEYS = Object.freeze([
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
] as const);

export const AWWW_TRANSITION_TYPES = Object.freeze([
  'simple',
  'fade',
  'left',
  'right',
  'top',
  'bottom',
  'wipe',
  'grow',
  'center',
  'outer',
  'random',
  'wave',
] as const);

export const LWE_SCALING_MODES = Object.freeze([
  'default',
  'fill',
  'fit',
  'stretch',
] as const);

export type ImageRenderer = 'awww' | 'mpvpaper';
export type GifRenderer = 'awww' | 'mpvpaper';
export type VideoRenderer = 'mpvpaper';
export type WallpaperFillMode = 'crop' | 'fit' | 'stretch';
export type AwwwTransitionType = (typeof AWWW_TRANSITION_TYPES)[number];
export type LweScalingMode = (typeof LWE_SCALING_MODES)[number];

export interface WallpaperBehaviorSettings {
  readonly imageBackend: ImageRenderer;
  readonly gifBackend: GifRenderer;
  readonly videoBackend: VideoRenderer;
  readonly fillMode: WallpaperFillMode;
  readonly awwwTransitionType: AwwwTransitionType;
  readonly awwwTransitionDuration: number;
  readonly awwwTransitionFps: number;
  readonly lweScaling: LweScalingMode;
  readonly lweFps: number;
  readonly lweMuted: boolean;
  readonly lweVolume: number;
  readonly restoreOnLogin: boolean;
}

export const DEFAULT_WALLPAPER_BEHAVIOR_SETTINGS: Readonly<WallpaperBehaviorSettings> =
  Object.freeze({
    imageBackend: 'awww',
    gifBackend: 'awww',
    videoBackend: 'mpvpaper',
    fillMode: 'crop',
    awwwTransitionType: 'fade',
    awwwTransitionDuration: 1,
    awwwTransitionFps: 60,
    lweScaling: 'default',
    lweFps: 60,
    lweMuted: false,
    lweVolume: 100,
    restoreOnLogin: false,
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

export interface WallpaperBehaviorPersistence {
  reset(settings: WallpaperBehaviorSettings | null): void;
  persist(settings: WallpaperBehaviorSettings): Promise<void>;
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

function awwwTransitionType(value: unknown): AwwwTransitionType {
  // Preserve the two legacy aliases accepted by the renderer layer.
  if (value === 'slide') return 'left';
  if (value === 'none') return 'simple';
  return typeof value === 'string'
    && (AWWW_TRANSITION_TYPES as readonly string[]).includes(value)
    ? value as AwwwTransitionType
    : 'fade';
}

function boundedNumber(
  value: unknown,
  minimum: number,
  maximum: number,
  fallback: number,
): number {
  if (typeof value !== 'number' && typeof value !== 'string') return fallback;
  if (typeof value === 'string' && value.trim().length === 0) return fallback;
  const parsed = Number(value);
  return Number.isFinite(parsed) && parsed >= minimum && parsed <= maximum
    ? parsed
    : fallback;
}

function boundedInteger(
  value: unknown,
  minimum: number,
  maximum: number,
  fallback: number,
): number {
  if (typeof value !== 'number' && typeof value !== 'string') return fallback;
  if (typeof value === 'string' && value.trim().length === 0) return fallback;
  const parsed = Number(value);
  if (!Number.isInteger(parsed)) return fallback;
  return Math.min(maximum, Math.max(minimum, parsed));
}

function lweScaling(value: unknown): LweScalingMode {
  return typeof value === 'string'
    && (LWE_SCALING_MODES as readonly string[]).includes(value)
    ? value as LweScalingMode
    : 'default';
}

function enabled(value: unknown): boolean {
  return value === true || value === 'on';
}

function defaultSettings(): WallpaperBehaviorSettings {
  return { ...DEFAULT_WALLPAPER_BEHAVIOR_SETTINGS };
}

/** Normalize the raw config values exposed by the compact settings UI. */
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
    awwwTransitionType: awwwTransitionType(values.awww_transition_type),
    awwwTransitionDuration: boundedNumber(values.awww_transition_duration, 0, 60, 1),
    awwwTransitionFps: boundedInteger(values.wallpaper_transition_fps, 1, 240, 60),
    lweScaling: lweScaling(values.linux_wallpaperengine_scaling),
    lweFps: boundedInteger(values.linux_wallpaperengine_fps, 1, 240, 60),
    lweMuted: enabled(values.linux_wallpaperengine_muted),
    lweVolume: boundedInteger(values.linux_wallpaperengine_volume, 0, 100, 100),
    restoreOnLogin: enabled(values.restore_on_login),
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
    awwwTransitionType: awwwTransitionType(record.awwwTransitionType),
    awwwTransitionDuration: boundedNumber(record.awwwTransitionDuration, 0, 60, 1),
    awwwTransitionFps: boundedInteger(record.awwwTransitionFps, 1, 240, 60),
    lweScaling: lweScaling(record.lweScaling),
    lweFps: boundedInteger(record.lweFps, 1, 240, 60),
    lweMuted: enabled(record.lweMuted),
    lweVolume: boundedInteger(record.lweVolume, 0, 100, 100),
    restoreOnLogin: enabled(record.restoreOnLogin),
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
    ['awww_transition_type', normalized.awwwTransitionType],
    ['awww_transition_duration', String(normalized.awwwTransitionDuration)],
    ['wallpaper_transition_fps', String(normalized.awwwTransitionFps)],
    ['linux_wallpaperengine_scaling', normalized.lweScaling],
    ['linux_wallpaperengine_fps', String(normalized.lweFps)],
    ['linux_wallpaperengine_muted', normalized.lweMuted ? 'on' : 'off'],
    ['linux_wallpaperengine_volume', String(normalized.lweVolume)],
    ['restore_on_login', normalized.restoreOnLogin ? 'on' : 'off'],
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

export function createWallpaperBehaviorPersistence(
  saveQueue: WallpaperBehaviorSaveQueue,
): WallpaperBehaviorPersistence {
  // One immediate retry covers transient failures without creating an
  // unbounded write loop when configuration storage remains unavailable.
  const maximumAttempts = 2;
  let generation = 0;
  let baselineEstablished = false;
  let confirmedSnapshot: string | null = null;
  let latestDesired: {
    readonly settings: WallpaperBehaviorSettings;
    readonly snapshot: string;
    readonly revision: number;
  } | null = null;
  let nextRevision = 0;
  let storageUncertain = false;
  let activePromise: Promise<void> | null = null;

  class TerminalPersistenceFailure {
    readonly cause: unknown;
    readonly generation: number;
    readonly revision: number;

    constructor(
      cause: unknown,
      failureGeneration: number,
      failureRevision: number,
    ) {
      this.cause = cause;
      this.generation = failureGeneration;
      this.revision = failureRevision;
    }
  }

  const needsPersistence = (): boolean => baselineEstablished
    && latestDesired !== null
    && (storageUncertain || latestDesired.snapshot !== confirmedSnapshot);

  const drain = async (runGeneration: number): Promise<void> => {
    let attemptedSnapshot: string | null = null;
    let attempts = 0;

    while (runGeneration === generation && baselineEstablished && latestDesired !== null) {
      const desired = latestDesired;
      if (!storageUncertain && desired.snapshot === confirmedSnapshot) return;
      if (desired.snapshot !== attemptedSnapshot) {
        attemptedSnapshot = desired.snapshot;
        attempts = 0;
      }

      try {
        await saveQueue.enqueue(desired.settings);
      } catch (failure) {
        if (runGeneration !== generation) return;
        // A failed multi-key save may already have changed earlier keys. The
        // last confirmed snapshot is no longer proof of what is on disk.
        storageUncertain = true;
        if (latestDesired?.snapshot !== desired.snapshot) {
          attemptedSnapshot = null;
          attempts = 0;
          continue;
        }
        attempts += 1;
        if (attempts >= maximumAttempts) {
          throw new TerminalPersistenceFailure(
            failure,
            runGeneration,
            desired.revision,
          );
        }
        continue;
      }

      if (runGeneration !== generation) return;
      confirmedSnapshot = desired.snapshot;
      storageUncertain = false;
      attemptedSnapshot = null;
      attempts = 0;
    }
  };

  const ensureDrain = (): Promise<void> => {
    if (activePromise !== null) return activePromise;
    const runGeneration = generation;
    const run = drain(runGeneration);
    let observed!: Promise<void>;
    observed = run.then(
      () => {
        if (activePromise !== observed) return;
        activePromise = null;
        // A desired snapshot can arrive after drain decides it has no work but
        // before this completion callback runs. Recheck once ownership clears.
        if (needsPersistence()) return ensureDrain();
      },
      (failure: unknown) => {
        if (activePromise === observed) activePromise = null;
        if (failure instanceof TerminalPersistenceFailure) {
          const desiredChanged = failure.generation !== generation
            || failure.revision !== latestDesired?.revision;
          if (desiredChanged && needsPersistence()) return ensureDrain();
          throw failure.cause;
        }
        throw failure;
      },
    );
    activePromise = observed;
    return observed;
  };

  return {
    reset(value) {
      generation += 1;
      const writeInFlight = activePromise !== null;
      baselineEstablished = value !== null;
      if (value === null) {
        confirmedSnapshot = null;
        latestDesired = null;
        storageUncertain = writeInFlight;
        return;
      }

      const normalized = normalizeWallpaperBehaviorSettings(value);
      const snapshot = settingsSnapshot(normalized);
      confirmedSnapshot = snapshot;
      if (writeInFlight) {
        latestDesired = {
          settings: normalized,
          snapshot,
          revision: ++nextRevision,
        };
        // The old generation can finish after this new baseline was read, so
        // rewrite it once the old active promise releases the queue.
        storageUncertain = true;
      } else {
        latestDesired = null;
        storageUncertain = false;
      }
    },
    persist(value) {
      if (!baselineEstablished) return Promise.resolve();
      const normalized = normalizeWallpaperBehaviorSettings(value);
      const snapshot = settingsSnapshot(normalized);
      latestDesired = {
        settings: normalized,
        snapshot,
        revision: latestDesired?.snapshot === snapshot
          ? latestDesired.revision
          : ++nextRevision,
      };
      return ensureDrain();
    },
  };
}

export function useWallpaperBehaviorSettings(
  client: WallpaperBehaviorSettingsClient,
): UseWallpaperBehaviorSettingsResult {
  const [settings, setSettings] = useState<WallpaperBehaviorSettings>(defaultSettings);
  const [ready, setReady] = useState(false);
  const [loadError, setLoadError] = useState<Error | null>(null);
  const [saveError, setSaveError] = useState<Error | null>(null);
  const saveQueue = useMemo(
    () => createWallpaperBehaviorSaveQueue(client, setSaveError),
    [client],
  );
  const persistence = useMemo(
    () => createWallpaperBehaviorPersistence(saveQueue),
    [saveQueue],
  );

  useEffect(() => {
    let active = true;
    persistence.reset(null);
    setReady(false);
    setLoadError(null);
    setSaveError(null);

    void loadWallpaperBehaviorSettings(client).then(
      (loaded) => {
        if (!active) return;
        persistence.reset(loaded);
        setSettings(loaded);
        setReady(true);
      },
      (failure: unknown) => {
        if (!active) return;
        const fallback = defaultSettings();
        // Keep fallback values render-only. Treating them as loaded would let a
        // later edit overwrite unread real configuration with defaults.
        persistence.reset(null);
        setSettings(fallback);
        setLoadError(errorFromUnknown(failure, 'Failed to load wallpaper behavior settings.'));
        setReady(false);
      },
    );

    return () => {
      active = false;
    };
  }, [client, persistence]);

  useEffect(() => {
    // Initial defaults are render-only. Writes begin only after a successful
    // load established an exact persisted baseline.
    if (!ready) return;
    void persistence.persist(settings).catch(() => {});
  }, [persistence, ready, settings]);

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
