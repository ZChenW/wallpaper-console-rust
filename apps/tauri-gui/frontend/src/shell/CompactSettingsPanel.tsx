import { useEffect, type ChangeEvent, type KeyboardEvent, type MouseEvent } from 'react';
import { ChevronRight, FolderCog, X } from 'lucide-react';

import type { RendererStatusesDTO } from '../api/types.ts';
import SelectField from '../components/SelectField.tsx';
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
  readonly obscured?: boolean;
  readonly preferences: ShellPreferences;
  readonly updatePreferences: (update: ShellPreferencesUpdate) => void;
  readonly behaviorSettings: WallpaperBehaviorSettings;
  readonly updateBehaviorSettings: (update: WallpaperBehaviorSettingsUpdate) => void;
  readonly behaviorReady: boolean;
  readonly loadError: Error | null;
  readonly saveError: Error | null;
  readonly rendererStatuses: RendererStatusesDTO | null;
  readonly onOpenSources: (trigger: HTMLButtonElement) => void;
  readonly onClose: () => void;
}

function errorMessage(error: Error): string {
  return error.message.trim() || 'Unknown configuration error';
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
  obscured = false,
  preferences,
  updatePreferences,
  behaviorSettings,
  updateBehaviorSettings,
  behaviorReady,
  loadError,
  saveError,
  rendererStatuses,
  onOpenSources,
  onClose,
}: CompactSettingsPanelProps) {
  if (!open) return null;

  const usesAwww = behaviorSettings.imageBackend === 'awww'
    || behaviorSettings.gifBackend === 'awww';
  const awwwUnavailable = rendererStatuses?.awww.available === false;
  const mpvpaperUnavailable = rendererStatuses?.mpvpaper.available === false;
  const lweUnavailable = rendererStatuses?.linuxWallpaperEngine.available === false;
  const updateTheme = (value: string) => {
    const theme = value as ShellTheme;
    updatePreferences((current) => ({ ...current, theme }));
  };
  const updateGesture = (value: string) => {
    const applyGesture = value as ApplyGesture;
    updatePreferences((current) => ({ ...current, applyGesture }));
  };
  const updateCardSize = (value: string) => {
    const cardSize = value as WallpaperCardSize;
    updatePreferences((current) => ({ ...current, cardSize }));
  };
  const updateImageRenderer = (imageBackend: ImageRenderer) => {
    updateBehaviorSettings((current) => ({ ...current, imageBackend }));
  };
  const updateGifRenderer = (gifBackend: GifRenderer) => {
    updateBehaviorSettings((current) => ({ ...current, gifBackend }));
  };
  const updateFillMode = (value: string) => {
    const fillMode = value as WallpaperFillMode;
    updateBehaviorSettings((current) => ({ ...current, fillMode }));
  };
  const updateAwwwTransitionType = (value: string) => {
    const awwwTransitionType = value as AwwwTransitionType;
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
  const updateLweScaling = (value: string) => {
    const lweScaling = value as LweScalingMode;
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
      disabled={onClick ? !behaviorReady || unavailable : undefined}
      tabIndex={onClick ? undefined : -1}
      type="button"
      onClick={onClick}
    >
      {renderer}
    </button>
  );

  return (
    <div
      className="settings-overlay"
      data-obscured={obscured}
      data-settings-overlay={true}
      onKeyDown={handleKeyDown}
      onMouseDown={handleBackdropMouseDown}
      style={{ zIndex: 1100 }}
    >
      <aside
        aria-label="Settings"
        aria-hidden={obscured ? true : undefined}
        aria-modal="true"
        inert={obscured}
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
          <div
            aria-labelledby="settings-interface-heading"
            className="settings-behavior-card"
            data-settings-card="interface"
            role="group"
          >
            <h4 id="settings-interface-heading">Interface</h4>
            <div className="settings-behavior-card__rows">
              <label className="settings-behavior-row">
                <span>Theme</span>
                <SelectField
                  aria-label="Theme"
                  onValueChange={updateTheme}
                  options={[
                    { value: 'system', label: 'System' },
                    { value: 'light', label: 'Light' },
                    { value: 'dark', label: 'Dark' },
                  ]}
                  value={preferences.theme}
                  variant="settings"
                />
              </label>
              <label className="settings-behavior-row">
                <span>Apply gesture</span>
                <SelectField
                  aria-label="Apply gesture"
                  onValueChange={updateGesture}
                  options={[
                    { value: 'single', label: 'Single click' },
                    { value: 'double', label: 'Double click' },
                  ]}
                  value={preferences.applyGesture}
                  variant="settings"
                />
              </label>
              <label className="settings-behavior-row">
                <span>Card size</span>
                <SelectField
                  aria-label="Card size"
                  onValueChange={updateCardSize}
                  options={[
                    { value: 'small', label: 'Small' },
                    { value: 'medium', label: 'Medium' },
                    { value: 'large', label: 'Large' },
                  ]}
                  value={preferences.cardSize}
                  variant="settings"
                />
              </label>
            </div>
          </div>
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
          <h4 className="settings-behavior-category">Renderers</h4>
          <div
            aria-labelledby="settings-renderer-selection-heading"
            className="settings-behavior-card"
            data-behavior-card="renderer-selection"
            role="group"
          >
            <h5 id="settings-renderer-selection-heading">Renderer selection</h5>
            <div className="settings-behavior-card__rows">
              <div
                aria-label="Image"
                className="settings-behavior-row settings-renderer-field"
                role="group"
              >
                <span>Image</span>
                <div className="settings-renderer-cards">
                  {rendererCard('awww', behaviorSettings.imageBackend === 'awww', awwwUnavailable,
                    () => updateImageRenderer('awww'))}
                  {rendererCard('mpvpaper', behaviorSettings.imageBackend === 'mpvpaper', mpvpaperUnavailable,
                    () => updateImageRenderer('mpvpaper'))}
                </div>
              </div>
              <div
                aria-label="GIF"
                className="settings-behavior-row settings-renderer-field"
                role="group"
              >
                <span>GIF</span>
                <div className="settings-renderer-cards">
                  {rendererCard('awww', behaviorSettings.gifBackend === 'awww', awwwUnavailable,
                    () => updateGifRenderer('awww'))}
                  {rendererCard('mpvpaper', behaviorSettings.gifBackend === 'mpvpaper', mpvpaperUnavailable,
                    () => updateGifRenderer('mpvpaper'))}
                </div>
              </div>
              <div
                aria-label="Video"
                className="settings-behavior-row settings-renderer-field"
                role="group"
              >
                <span>Video</span>
                <div className="settings-renderer-cards settings-renderer-cards--single">
                  {rendererCard('mpvpaper', true, mpvpaperUnavailable)}
                </div>
              </div>
            </div>
          </div>
          <h4 className="settings-behavior-category">Image appearance</h4>
          {usesAwww ? (
            <div
              aria-labelledby="settings-fill-transition-heading"
              className="settings-behavior-card"
              data-behavior-card="image-appearance"
              role="group"
            >
              <h5 id="settings-fill-transition-heading">Fill &amp; transition</h5>
              <div className="settings-behavior-card__rows">
                <label className="settings-behavior-row">
                  <span>Fill behavior</span>
                  <SelectField
                    aria-label="Fill behavior"
                    dataBehaviorControl={true}
                    disabled={!behaviorReady}
                    onValueChange={updateFillMode}
                    options={[
                      { value: 'crop', label: 'Crop to fill' },
                      { value: 'fit', label: 'Fit inside' },
                      { value: 'stretch', label: 'Stretch' },
                    ]}
                    value={behaviorSettings.fillMode}
                    variant="settings"
                  />
                </label>
                <label className="settings-behavior-row">
                  <span>Transition</span>
                  <SelectField
                    aria-label="awww transition type"
                    dataBehaviorControl={true}
                    disabled={!behaviorReady}
                    onValueChange={updateAwwwTransitionType}
                    options={AWWW_TRANSITION_TYPES.map((transitionType) => ({
                      value: transitionType,
                      label: transitionType,
                    }))}
                    value={behaviorSettings.awwwTransitionType}
                    variant="settings"
                  />
                </label>
                <label className="settings-behavior-row">
                  <span>Transition duration</span>
                  <span className="settings-number-control">
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
                    <span aria-hidden="true" data-control-unit="seconds">s</span>
                  </span>
                </label>
                <label className="settings-behavior-row">
                  <span>Transition FPS</span>
                  <span className="settings-number-control">
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
                    <span aria-hidden="true" data-control-unit="transition-fps">FPS</span>
                  </span>
                </label>
              </div>
            </div>
          ) : (
            <p>Fill and transition controls are available when awww handles images or GIFs.</p>
          )}

          <h4 className="settings-behavior-category">Wallpaper Engine</h4>
          <div
            aria-labelledby="settings-scene-playback-heading"
            className="settings-behavior-card"
            data-behavior-card="scene-playback"
            role="group"
          >
            <h5 id="settings-scene-playback-heading">Scene playback</h5>
            <div className="settings-behavior-card__rows">
              <label className="settings-behavior-row">
                <span>Scene scaling</span>
                <SelectField
                  aria-label="Wallpaper Engine scaling"
                  dataBehaviorControl={true}
                  disabled={!behaviorReady || lweUnavailable}
                  onValueChange={updateLweScaling}
                  options={LWE_SCALING_MODES.map((scaling) => ({
                    value: scaling,
                    label: scaling,
                  }))}
                  value={behaviorSettings.lweScaling}
                  variant="settings"
                />
              </label>
              <label className="settings-behavior-row">
                <span>Scene FPS</span>
                <span className="settings-number-control">
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
                  <span aria-hidden="true" data-control-unit="scene-fps">FPS</span>
                </span>
              </label>
              <label className="settings-behavior-row">
                <span>Scene volume</span>
                <span className="settings-number-control">
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
                  <span aria-hidden="true" data-control-unit="scene-volume">%</span>
                </span>
              </label>
              <label className="settings-behavior-row settings-behavior-row--switch">
                <span>Mute scene audio</span>
                <input
                  aria-label="Mute Wallpaper Engine audio"
                  checked={behaviorSettings.lweMuted}
                  className="settings-switch-input"
                  data-behavior-control={true}
                  disabled={!behaviorReady || lweUnavailable}
                  onChange={updateLweMuted}
                  type="checkbox"
                />
              </label>
            </div>
          </div>

          <h4 className="settings-behavior-category">Session</h4>
          <div
            aria-labelledby="settings-session-restore-heading"
            className="settings-behavior-card"
            data-behavior-card="session-restore"
            role="group"
          >
            <h5 id="settings-session-restore-heading">Restore</h5>
            <div className="settings-behavior-card__rows">
              <label className="settings-behavior-row settings-behavior-row--switch">
                <span>Restore on login</span>
                <input
                  aria-label="Restore on login"
                  checked={behaviorSettings.restoreOnLogin}
                  className="settings-switch-input"
                  data-behavior-control={true}
                  disabled={!behaviorReady}
                  onChange={updateRestoreOnLogin}
                  type="checkbox"
                />
              </label>
            </div>
          </div>
        </section>

        <section
          aria-labelledby="settings-sources-heading"
          data-settings-group="sources"
          className="settings-section"
        >
          <h3 id="settings-sources-heading">Sources</h3>
          <button
            type="button"
            aria-label="Manage wallpaper sources"
            className="settings-navigation-card"
            data-source-management-action={true}
            onClick={(event) => onOpenSources(event.currentTarget)}
          >
            <span className="settings-navigation-card__icon" aria-hidden="true">
              <FolderCog size={17} />
            </span>
            <span className="settings-navigation-card__copy">
              <strong>Wallpaper sources</strong>
              <span>Add, rename, refresh, or remove folders.</span>
            </span>
            <ChevronRight
              aria-hidden="true"
              className="settings-navigation-card__chevron"
              size={18}
            />
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
      if (props.obscured) return;
      if (event.key !== 'Escape' || event.defaultPrevented) return;
      event.preventDefault();
      props.onClose();
    };
    document.addEventListener('keydown', closeOnEscape);
    return () => {
      unlockBodyScroll();
      document.removeEventListener('keydown', closeOnEscape);
    };
  }, [props.obscured, props.open, props.onClose]);

  return CompactSettingsPanelView(props);
}
