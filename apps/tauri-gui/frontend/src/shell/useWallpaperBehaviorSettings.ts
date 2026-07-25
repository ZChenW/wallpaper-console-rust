import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react';

import type {
  AwwwTransitionTypeDTO,
  BehaviorSettingsDTO,
  BehaviorSettingsPatchDTO,
  BehaviorSettingsSnapshotDTO,
  GifRendererDTO,
  ImageRendererDTO,
  LweScalingModeDTO,
  OpenProjectLocationModeDTO,
  VideoRendererDTO,
  WallpaperFillModeDTO,
} from '../api/types.ts';

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
] as const satisfies readonly AwwwTransitionTypeDTO[]);

export const LWE_SCALING_MODES = Object.freeze([
  'default',
  'fill',
  'fit',
  'stretch',
] as const satisfies readonly LweScalingModeDTO[]);

export type ImageRenderer = ImageRendererDTO;
export type GifRenderer = GifRendererDTO;
export type VideoRenderer = VideoRendererDTO;
export type WallpaperFillMode = WallpaperFillModeDTO;
export type AwwwTransitionType = AwwwTransitionTypeDTO;
export type LweScalingMode = LweScalingModeDTO;
export type OpenProjectLocationMode = OpenProjectLocationModeDTO;
export type WallpaperBehaviorSettings = BehaviorSettingsDTO;

export interface WallpaperBehaviorSettingsClient {
  behaviorSettingsGet(): Promise<BehaviorSettingsSnapshotDTO>;
  behaviorSettingsUpdate(
    expectedRevision: string,
    patch: BehaviorSettingsPatchDTO,
  ): Promise<BehaviorSettingsSnapshotDTO>;
}

export type WallpaperBehaviorSettingsUpdate =
  | WallpaperBehaviorSettings
  | ((current: WallpaperBehaviorSettings) => WallpaperBehaviorSettings);

export interface UseWallpaperBehaviorSettingsResult {
  readonly settings: WallpaperBehaviorSettings | null;
  readonly ready: boolean;
  readonly loadError: Error | null;
  readonly saveError: Error | null;
  readonly updateSettings: (update: WallpaperBehaviorSettingsUpdate) => void;
}

const SETTING_FIELDS = Object.freeze([
  'imageBackend',
  'gifBackend',
  'videoBackend',
  'fillMode',
  'awwwTransitionType',
  'awwwTransitionDuration',
  'awwwTransitionFps',
  'lweScaling',
  'lweFps',
  'lweMuted',
  'lweVolume',
  'restoreOnLogin',
  'openProjectLocationMode',
] as const satisfies readonly (keyof WallpaperBehaviorSettings)[]);

function errorFromUnknown(error: unknown, fallback: string): Error {
  if (error instanceof Error) return error;
  if (typeof error === 'string' && error.trim().length > 0) return new Error(error.trim());
  return new Error(fallback);
}

function settingsPatch(
  current: WallpaperBehaviorSettings,
  next: WallpaperBehaviorSettings,
): BehaviorSettingsPatchDTO {
  const patch: Partial<Record<keyof WallpaperBehaviorSettings, unknown>> = {};
  for (const field of SETTING_FIELDS) {
    if (!Object.is(current[field], next[field])) patch[field] = next[field];
  }
  return patch as BehaviorSettingsPatchDTO;
}

function patchIsEmpty(patch: BehaviorSettingsPatchDTO): boolean {
  return Object.keys(patch).length === 0;
}

function isRevisionConflict(error: unknown): boolean {
  const message = error instanceof Error ? error.message : String(error);
  return message.includes('config_revision_changed');
}

export async function loadWallpaperBehaviorSettings(
  client: WallpaperBehaviorSettingsClient,
): Promise<BehaviorSettingsSnapshotDTO> {
  return client.behaviorSettingsGet();
}

export class WallpaperBehaviorWriter {
  private readonly client: WallpaperBehaviorSettingsClient;
  private snapshot: BehaviorSettingsSnapshotDTO | null = null;
  private tail: Promise<unknown> = Promise.resolve();

  constructor(client: WallpaperBehaviorSettingsClient) {
    this.client = client;
  }

  reset(snapshot: BehaviorSettingsSnapshotDTO | null): void {
    this.snapshot = snapshot;
  }

  persist(patch: BehaviorSettingsPatchDTO): Promise<BehaviorSettingsSnapshotDTO> {
    if (patchIsEmpty(patch)) {
      return this.snapshot === null
        ? Promise.reject(new Error('Behavior settings have not loaded.'))
        : Promise.resolve(this.snapshot);
    }
    const run = this.tail.then(async () => {
      let current = this.snapshot;
      if (current === null) throw new Error('Behavior settings have not loaded.');
      try {
        current = await this.client.behaviorSettingsUpdate(current.revision, patch);
      } catch (error) {
        if (!isRevisionConflict(error)) throw error;
        current = await this.client.behaviorSettingsGet();
        current = await this.client.behaviorSettingsUpdate(current.revision, patch);
      }
      this.snapshot = current;
      return current;
    });
    this.tail = run.catch(() => {});
    return run;
  }
}

export function resolveWallpaperBehaviorSettingsUpdate(
  current: WallpaperBehaviorSettings,
  update: WallpaperBehaviorSettingsUpdate,
): WallpaperBehaviorSettings {
  return typeof update === 'function' ? update(current) : update;
}

export function useWallpaperBehaviorSettings(
  client: WallpaperBehaviorSettingsClient,
): UseWallpaperBehaviorSettingsResult {
  const [settings, setSettings] = useState<WallpaperBehaviorSettings | null>(null);
  const settingsRef = useRef<WallpaperBehaviorSettings | null>(null);
  const [ready, setReady] = useState(false);
  const [loadError, setLoadError] = useState<Error | null>(null);
  const [saveError, setSaveError] = useState<Error | null>(null);
  const writer = useMemo(() => new WallpaperBehaviorWriter(client), [client]);

  useEffect(() => {
    let active = true;
    writer.reset(null);
    settingsRef.current = null;
    setSettings(null);
    setReady(false);
    setLoadError(null);
    setSaveError(null);

    void loadWallpaperBehaviorSettings(client).then(
      (snapshot) => {
        if (!active) return;
        writer.reset(snapshot);
        settingsRef.current = snapshot.settings;
        setSettings(snapshot.settings);
        setReady(true);
      },
      (failure: unknown) => {
        if (!active) return;
        writer.reset(null);
        settingsRef.current = null;
        setSettings(null);
        setLoadError(errorFromUnknown(failure, 'Failed to load wallpaper behavior settings.'));
      },
    );

    return () => {
      active = false;
    };
  }, [client, writer]);

  const updateSettings = useCallback((update: WallpaperBehaviorSettingsUpdate) => {
    const current = settingsRef.current;
    if (current === null) return;
    const next = resolveWallpaperBehaviorSettingsUpdate(current, update);
    const patch = settingsPatch(current, next);
    if (patchIsEmpty(patch)) return;
    settingsRef.current = next;
    setSettings(next);
    setSaveError(null);
    void writer.persist(patch).catch((failure: unknown) => {
      setSaveError(errorFromUnknown(failure, 'Failed to save wallpaper behavior settings.'));
    });
  }, [writer]);

  return {
    settings,
    ready,
    loadError,
    saveError,
    updateSettings,
  };
}
