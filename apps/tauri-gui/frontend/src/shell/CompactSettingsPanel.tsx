import { useEffect, type ChangeEvent, type KeyboardEvent, type MouseEvent } from 'react';
import { X } from 'lucide-react';

import type { RendererStatusesDTO } from '../api/types.ts';
import type {
  ApplyGesture,
  ShellPreferences,
  ShellTheme,
} from './shellPreferences.ts';
import type { ShellPreferencesUpdate } from './useShellPreferences.ts';
import {
  AWWW_TRANSITION_TYPES,
  LWE_SCALING_MODES,
  type AwwwTransitionType,
  GifRenderer,
  ImageRenderer,
  type LweScalingMode,
  WallpaperBehaviorSettings,
  WallpaperBehaviorSettingsUpdate,
  WallpaperFillMode,
} from './useWallpaperBehaviorSettings.ts';
import type { WallpaperCardSize } from '../utils/layout.ts';

export interface CompactSettingsPanelProps {
  readonly open: boolean;
  readonly preferences: ShellPreferences;
  readonly updatePreferences: (update: ShellPreferencesUpdate) => void;
  readonly behaviorSettings: WallpaperBehaviorSettings;
  readonly updateBehaviorSettings: (update: WallpaperBehaviorSettingsUpdate) => void;
  readonly behaviorReady: boolean;
  readonly loadError: Error | null;
  readonly saveError: Error | null;
  readonly rendererStatuses: RendererStatusesDTO | null;
  readonly sourceCount: number;
  readonly offlineSourceCount: number;
  readonly onOpenSources: () => void;
  readonly onClose: () => void;
}

function errorMessage(error: Error): string {
  return error.message.trim() || 'Unknown configuration error';
}

function sourceSummary(sourceCount: number, offlineSourceCount: number): string {
  const total = Math.max(0, Math.trunc(sourceCount));
  const offline = Math.min(total, Math.max(0, Math.trunc(offlineSourceCount)));
  const sourceLabel = total === 1 ? 'source' : 'sources';
  return offline > 0
    ? `${total} ${sourceLabel} · ${offline} offline`
    : `${total} ${sourceLabel} · all available`;
}

export function lockBodyScroll(body: { style: { overflow: string } }): () => void {
  const previousOverflow = body.style.overflow;
  body.style.overflow = 'hidden';
  return () => {
    body.style.overflow = previousOverflow;
  };
}

export function CompactSettingsPanelView({
  open,
  preferences,
  updatePreferences,
  behaviorSettings,
  updateBehaviorSettings,
  behaviorReady,
  loadError,
  saveError,
  rendererStatuses,
  sourceCount,
  offlineSourceCount,
  onOpenSources,
  onClose,
}: CompactSettingsPanelProps) {
  if (!open) return null;

  const usesAwww = behaviorSettings.imageBackend === 'awww'
    || behaviorSettings.gifBackend === 'awww';
  const awwwUnavailable = rendererStatuses?.awww.available === false;
  const mpvpaperUnavailable = rendererStatuses?.mpvpaper.available === false;
  const lweUnavailable = rendererStatuses?.linuxWallpaperEngine.available === false;
  const updateTheme = (event: ChangeEvent<HTMLSelectElement>) => {
    const theme = event.currentTarget.value as ShellTheme;
    updatePreferences((current) => ({ ...current, theme }));
  };
  const updateGesture = (event: ChangeEvent<HTMLSelectElement>) => {
    const applyGesture = event.currentTarget.value as ApplyGesture;
    updatePreferences((current) => ({ ...current, applyGesture }));
  };
  const updateCardSize = (event: ChangeEvent<HTMLSelectElement>) => {
    const cardSize = event.currentTarget.value as WallpaperCardSize;
    updatePreferences((current) => ({ ...current, cardSize }));
  };
  const updateImageRenderer = (imageBackend: ImageRenderer) => {
    updateBehaviorSettings((current) => ({ ...current, imageBackend }));
  };
  const updateGifRenderer = (gifBackend: GifRenderer) => {
    updateBehaviorSettings((current) => ({ ...current, gifBackend }));
  };
  const updateFillMode = (event: ChangeEvent<HTMLSelectElement>) => {
    const fillMode = event.currentTarget.value as WallpaperFillMode;
    updateBehaviorSettings((current) => ({ ...current, fillMode }));
  };
  const updateAwwwTransitionType = (event: ChangeEvent<HTMLSelectElement>) => {
    const awwwTransitionType = event.currentTarget.value as AwwwTransitionType;
    updateBehaviorSettings((current) => ({ ...current, awwwTransitionType }));
  };
  const updateAwwwTransitionDuration = (event: ChangeEvent<HTMLInputElement>) => {
    const awwwTransitionDuration = Number(event.currentTarget.value);
    updateBehaviorSettings((current) => ({ ...current, awwwTransitionDuration }));
  };
  const updateAwwwTransitionFps = (event: ChangeEvent<HTMLInputElement>) => {
    const awwwTransitionFps = Number(event.currentTarget.value);
    updateBehaviorSettings((current) => ({ ...current, awwwTransitionFps }));
  };
  const updateLweScaling = (event: ChangeEvent<HTMLSelectElement>) => {
    const lweScaling = event.currentTarget.value as LweScalingMode;
    updateBehaviorSettings((current) => ({ ...current, lweScaling }));
  };
  const updateLweFps = (event: ChangeEvent<HTMLInputElement>) => {
    const lweFps = Number(event.currentTarget.value);
    updateBehaviorSettings((current) => ({ ...current, lweFps }));
  };
  const updateLweMuted = (event: ChangeEvent<HTMLInputElement>) => {
    const lweMuted = event.currentTarget.checked;
    updateBehaviorSettings((current) => ({ ...current, lweMuted }));
  };
  const updateLweVolume = (event: ChangeEvent<HTMLInputElement>) => {
    const lweVolume = Number(event.currentTarget.value);
    updateBehaviorSettings((current) => ({ ...current, lweVolume }));
  };
  const updateRestoreOnLogin = (event: ChangeEvent<HTMLInputElement>) => {
    const restoreOnLogin = event.currentTarget.checked;
    updateBehaviorSettings((current) => ({ ...current, restoreOnLogin }));
  };
  const handleKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    if (event.key !== 'Escape') return;
    event.preventDefault();
    onClose();
  };
  const handleBackdropMouseDown = (event: MouseEvent<HTMLDivElement>) => {
    if (event.target === event.currentTarget) onClose();
  };
  const rendererCard = (
    renderer: 'awww' | 'mpvpaper',
    selected: boolean,
    unavailable: boolean,
    onClick?: () => void,
  ) => (
    <button
      aria-disabled={onClick ? undefined : true}
      aria-pressed={selected}
      className="settings-renderer-card"
      data-behavior-control={onClick ? true : undefined}
      data-renderer={renderer}
      data-unavailable={unavailable || undefined}
      disabled={onClick ? !behaviorReady || unavailable : undefined}
      tabIndex={onClick ? undefined : -1}
      title={unavailable ? `${renderer} is unavailable` : undefined}
      type="button"
      onClick={onClick}
    >
      {renderer}
    </button>
  );

  return (
    <div
      className="settings-overlay"
      data-settings-overlay={true}
      onKeyDown={handleKeyDown}
      onMouseDown={handleBackdropMouseDown}
      style={{ zIndex: 1100 }}
    >
      <aside
        aria-label="Settings"
        aria-modal="true"
        role="dialog"
        className="settings-panel"
      >
        <header className="settings-panel__header">
          <h2>Settings</h2>
          <button
            autoFocus
            aria-label="Close settings"
            className="settings-panel__close"
            data-icon-button={true}
            type="button"
            onClick={onClose}
          >
            <X aria-hidden="true" size={19} />
          </button>
        </header>

        <section
          aria-labelledby="settings-appearance-heading"
          data-settings-group="appearance-interaction"
          className="settings-section"
        >
          <h3 id="settings-appearance-heading">Appearance &amp; interaction</h3>
          <label className="settings-field">
            <span>Theme</span>
            <select aria-label="Theme" value={preferences.theme} onChange={updateTheme}>
              <option value="system">System</option>
              <option value="light">Light</option>
              <option value="dark">Dark</option>
            </select>
          </label>
          <label className="settings-field">
            <span>Apply gesture</span>
            <select
              aria-label="Apply gesture"
              value={preferences.applyGesture}
              onChange={updateGesture}
            >
              <option value="single">Single click</option>
              <option value="double">Double click</option>
            </select>
          </label>
          <label className="settings-field">
            <span>Card size</span>
            <select aria-label="Card size" value={preferences.cardSize} onChange={updateCardSize}>
              <option value="small">Small</option>
              <option value="medium">Medium</option>
              <option value="large">Large</option>
            </select>
          </label>
        </section>

        <section
          aria-labelledby="settings-wallpaper-heading"
          data-settings-group="wallpaper-behavior"
          className="settings-section"
        >
          <h3 id="settings-wallpaper-heading">Wallpaper behavior</h3>
          {!behaviorReady && loadError === null ? (
            <p role="status">Loading wallpaper behavior settings…</p>
          ) : null}
          {!behaviorReady && loadError !== null ? (
            <p>Wallpaper behavior controls are disabled until configuration can be read.</p>
          ) : null}
          {loadError !== null && <p role="alert" className="settings-error">{errorMessage(loadError)}</p>}
          {saveError !== null && <p role="alert" className="settings-error">{errorMessage(saveError)}</p>}
          <div aria-label="Image" className="settings-renderer-field" role="group">
            <span>Image</span>
            <div className="settings-renderer-cards">
              {rendererCard('awww', behaviorSettings.imageBackend === 'awww', awwwUnavailable,
                () => updateImageRenderer('awww'))}
              {rendererCard('mpvpaper', behaviorSettings.imageBackend === 'mpvpaper', mpvpaperUnavailable,
                () => updateImageRenderer('mpvpaper'))}
            </div>
          </div>
          <div aria-label="GIF" className="settings-renderer-field" role="group">
            <span>GIF</span>
            <div className="settings-renderer-cards">
              {rendererCard('awww', behaviorSettings.gifBackend === 'awww', awwwUnavailable,
                () => updateGifRenderer('awww'))}
              {rendererCard('mpvpaper', behaviorSettings.gifBackend === 'mpvpaper', mpvpaperUnavailable,
                () => updateGifRenderer('mpvpaper'))}
            </div>
          </div>
          <div aria-label="Video" className="settings-renderer-field" role="group">
            <span>Video</span>
            <div className="settings-renderer-cards settings-renderer-cards--single">
              {rendererCard('mpvpaper', true, mpvpaperUnavailable)}
            </div>
          </div>
          {usesAwww ? (
            <>
              <label className="settings-field">
                <span>Fill behavior</span>
                <select
                  aria-label="Fill behavior"
                  data-behavior-control={true}
                  disabled={!behaviorReady}
                  value={behaviorSettings.fillMode}
                  onChange={updateFillMode}
                >
                  <option value="crop">Crop to fill</option>
                  <option value="fit">Fit inside</option>
                  <option value="stretch">Stretch</option>
                </select>
              </label>
              <label className="settings-field">
                <span>Transition</span>
                <select
                  aria-label="awww transition type"
                  data-behavior-control={true}
                  disabled={!behaviorReady}
                  value={behaviorSettings.awwwTransitionType}
                  onChange={updateAwwwTransitionType}
                >
                  {AWWW_TRANSITION_TYPES.map((transitionType) => (
                    <option key={transitionType} value={transitionType}>{transitionType}</option>
                  ))}
                </select>
              </label>
              <label className="settings-field">
                <span>Transition duration</span>
                <input
                  aria-label="awww transition duration"
                  data-behavior-control={true}
                  disabled={!behaviorReady}
                  max={60}
                  min={0}
                  onChange={updateAwwwTransitionDuration}
                  step={0.1}
                  type="number"
                  value={behaviorSettings.awwwTransitionDuration}
                />
              </label>
              <label className="settings-field">
                <span>Transition FPS</span>
                <input
                  aria-label="awww transition FPS"
                  data-behavior-control={true}
                  disabled={!behaviorReady}
                  max={240}
                  min={1}
                  onChange={updateAwwwTransitionFps}
                  step={1}
                  type="number"
                  value={behaviorSettings.awwwTransitionFps}
                />
              </label>
            </>
          ) : (
            <p>Fill and transition controls are available when awww handles images or GIFs.</p>
          )}
          <h4>Wallpaper Engine scenes</h4>
          <label className="settings-field">
            <span>Scene scaling</span>
            <select
              aria-label="Wallpaper Engine scaling"
              data-behavior-control={true}
              disabled={!behaviorReady || lweUnavailable}
              value={behaviorSettings.lweScaling}
              onChange={updateLweScaling}
            >
              {LWE_SCALING_MODES.map((scaling) => (
                <option key={scaling} value={scaling}>{scaling}</option>
              ))}
            </select>
          </label>
          <label className="settings-field">
            <span>Scene FPS</span>
            <input
              aria-label="Wallpaper Engine FPS"
              data-behavior-control={true}
              disabled={!behaviorReady || lweUnavailable}
              max={240}
              min={1}
              onChange={updateLweFps}
              step={1}
              type="number"
              value={behaviorSettings.lweFps}
            />
          </label>
          <label className="settings-field">
            <span>Mute scene audio</span>
            <input
              aria-label="Mute Wallpaper Engine audio"
              checked={behaviorSettings.lweMuted}
              data-behavior-control={true}
              disabled={!behaviorReady || lweUnavailable}
              onChange={updateLweMuted}
              type="checkbox"
            />
          </label>
          <label className="settings-field">
            <span>Scene volume</span>
            <input
              aria-label="Wallpaper Engine volume"
              data-behavior-control={true}
              disabled={!behaviorReady || lweUnavailable || behaviorSettings.lweMuted}
              max={100}
              min={0}
              onChange={updateLweVolume}
              step={1}
              type="number"
              value={behaviorSettings.lweVolume}
            />
          </label>
          <h4>Session restore</h4>
          <label className="settings-field">
            <span>Restore on login</span>
            <input
              aria-label="Restore on login"
              checked={behaviorSettings.restoreOnLogin}
              data-behavior-control={true}
              disabled={!behaviorReady}
              onChange={updateRestoreOnLogin}
              type="checkbox"
            />
          </label>
          <p>
            Add <code>wallpaper-console-rust restore-at-login</code> to your desktop session
            startup. This switch allows that command to restore saved display wallpapers.
          </p>
          <p>
            Availability is checked when applying. Missing dependencies are reported with
            installation guidance.
          </p>
        </section>

        <section
          aria-labelledby="settings-sources-heading"
          data-settings-group="sources"
          className="settings-section"
        >
          <h3 id="settings-sources-heading">Sources</h3>
          <p>{sourceSummary(sourceCount, offlineSourceCount)}</p>
          <p>Add, refresh, rename, or remove folders in the dedicated source panel.</p>
          <button
            type="button"
            aria-label="Manage wallpaper sources"
            data-source-management-action={true}
            onClick={onOpenSources}
          >
            Manage sources
          </button>
        </section>
      </aside>
    </div>
  );
}

export default function CompactSettingsPanel(props: CompactSettingsPanelProps) {
  useEffect(() => {
    if (!props.open) return undefined;

    const unlockBodyScroll = lockBodyScroll(document.body);
    const closeOnEscape = (event: globalThis.KeyboardEvent) => {
      if (event.key !== 'Escape' || event.defaultPrevented) return;
      event.preventDefault();
      props.onClose();
    };
    document.addEventListener('keydown', closeOnEscape);
    return () => {
      unlockBodyScroll();
      document.removeEventListener('keydown', closeOnEscape);
    };
  }, [props.open, props.onClose]);

  return CompactSettingsPanelView(props);
}
