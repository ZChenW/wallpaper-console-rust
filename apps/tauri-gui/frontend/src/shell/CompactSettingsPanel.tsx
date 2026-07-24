import { useCallback, useEffect, useRef, useState, type ChangeEvent, type KeyboardEvent, type MouseEvent } from 'react';
import { ChevronRight, FolderCog, X } from 'lucide-react';

import type { RendererStatusesDTO } from '../api/types.ts';
import SelectField from '../components/SelectField.tsx';
import type {
  ApplyGesture,
  ShellPreferences,
} from './shellPreferences.ts';
import { isShellTheme, SHELL_THEME_OPTIONS } from './shellThemes.ts';
import type { ShellPreferencesUpdate } from './useShellPreferences.ts';
import {
  AWWW_TRANSITION_TYPES,
  LWE_SCALING_MODES,
  type AwwwTransitionType,
  GifRenderer,
  ImageRenderer,
  type LweScalingMode,
  type OpenProjectLocationMode,
  WallpaperBehaviorSettings,
  WallpaperBehaviorSettingsUpdate,
  WallpaperFillMode,
} from './useWallpaperBehaviorSettings.ts';
import type { WallpaperCardSize } from '../utils/layout.ts';
import { trapDialogFocus } from './dialogFocus.ts';
import { DeferredNumberInput } from './DeferredNumberInput.tsx';

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
  readonly rendererStatusesLoading: boolean;
  readonly rendererStatusesError: string | null;
  readonly onReloadRendererStatuses: () => void;
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
  presentationPhase = 'open',
  preferences,
  updatePreferences,
  behaviorSettings,
  updateBehaviorSettings,
  behaviorReady,
  loadError,
  saveError,
  rendererStatuses,
  rendererStatusesLoading,
  rendererStatusesError,
  onReloadRendererStatuses,
  onOpenSources,
  onClose,
}: CompactSettingsPanelProps & { readonly presentationPhase?: 'open' | 'exiting' }) {
  if (!open) return null;
  const unavailableToInteraction = obscured || presentationPhase === 'exiting';

  const usesAwww = behaviorSettings.imageBackend === 'awww'
    || behaviorSettings.gifBackend === 'awww';
  const rendererDetectionReady = !rendererStatusesLoading
    && rendererStatusesError === null
    && rendererStatuses !== null;
  const awwwUnavailable = !rendererDetectionReady
    || rendererStatuses?.awww.available !== true;
  const mpvpaperUnavailable = !rendererDetectionReady
    || rendererStatuses?.mpvpaper.available !== true;
  const lweUnavailable = !rendererDetectionReady
    || rendererStatuses?.linuxWallpaperEngine.available !== true;
  const updateTheme = (value: string) => {
    if (!isShellTheme(value)) return;
    updatePreferences((current) => ({ ...current, theme: value }));
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
  const updateAwwwTransitionDuration = (awwwTransitionDuration: number) => {
    updateBehaviorSettings((current) => ({ ...current, awwwTransitionDuration }));
  };
  const updateAwwwTransitionFps = (awwwTransitionFps: number) => {
    updateBehaviorSettings((current) => ({ ...current, awwwTransitionFps }));
  };
  const updateLweScaling = (value: string) => {
    const lweScaling = value as LweScalingMode;
    updateBehaviorSettings((current) => ({ ...current, lweScaling }));
  };
  const updateLweFps = (lweFps: number) => {
    updateBehaviorSettings((current) => ({ ...current, lweFps }));
  };
  const updateLweMuted = (event: ChangeEvent<HTMLInputElement>) => {
    const lweMuted = event.currentTarget.checked;
    updateBehaviorSettings((current) => ({ ...current, lweMuted }));
  };
  const updateLweVolume = (lweVolume: number) => {
    updateBehaviorSettings((current) => ({ ...current, lweVolume }));
  };
  const updateRestoreOnLogin = (event: ChangeEvent<HTMLInputElement>) => {
    const restoreOnLogin = event.currentTarget.checked;
    updateBehaviorSettings((current) => ({ ...current, restoreOnLogin }));
  };
  const updateOpenProjectLocationMode = (value: string) => {
    const openProjectLocationMode = value as OpenProjectLocationMode;
    updateBehaviorSettings((current) => ({ ...current, openProjectLocationMode }));
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
      data-presentation-phase={presentationPhase}
      data-settings-overlay={true}
      onKeyDown={handleKeyDown}
      onMouseDown={handleBackdropMouseDown}
      style={{ zIndex: 1100 }}
    >
      <aside
        aria-label="Settings"
        aria-hidden={unavailableToInteraction ? true : undefined}
        aria-modal="true"
        inert={unavailableToInteraction}
        role="dialog"
        className="settings-panel"
        onKeyDown={(event) => trapDialogFocus(event, event.currentTarget)}
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
              <div className="settings-behavior-row">
                <span>Theme</span>
                <SelectField
                  aria-label="Theme"
                  onValueChange={updateTheme}
                  options={SHELL_THEME_OPTIONS}
                  value={preferences.theme}
                  variant="settings"
                />
              </div>
              <div className="settings-behavior-row">
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
              </div>
              <div className="settings-behavior-row">
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
              </div>
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
          {rendererStatusesLoading ? (
            <p aria-label="Renderer installation status" role="status">
              Checking renderer availability…
            </p>
          ) : null}
          {!rendererStatusesLoading && rendererStatusesError !== null ? (
            <div aria-label="Renderer installation status" className="settings-error" role="alert">
              <strong>Renderer detection failed.</strong>
              <span>{rendererStatusesError}</span>
              <button className="btn" type="button" onClick={onReloadRendererStatuses}>
                Retry
              </button>
            </div>
          ) : null}
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
                <div className="settings-behavior-row">
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
                </div>
                <div className="settings-behavior-row">
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
                </div>
                <label className="settings-behavior-row">
                  <span>Transition duration</span>
                  <DeferredNumberInput
                    aria-label="awww transition duration"
                    confirmed={behaviorSettings.awwwTransitionDuration}
                    disabled={!behaviorReady}
                    max={60}
                    min={0}
                    onCommit={updateAwwwTransitionDuration}
                    step={0.1}
                    unit="s"
                    unitKind="seconds"
                  />
                </label>
                <label className="settings-behavior-row">
                  <span>Transition FPS</span>
                  <DeferredNumberInput
                    aria-label="awww transition FPS"
                    confirmed={behaviorSettings.awwwTransitionFps}
                    disabled={!behaviorReady}
                    max={240}
                    min={1}
                    onCommit={updateAwwwTransitionFps}
                    step={1}
                    unit="FPS"
                    unitKind="transition-fps"
                  />
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
              <div className="settings-behavior-row">
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
              </div>
              <label className="settings-behavior-row">
                <span>Scene FPS</span>
                <DeferredNumberInput
                  aria-label="Wallpaper Engine FPS"
                  confirmed={behaviorSettings.lweFps}
                  disabled={!behaviorReady || lweUnavailable}
                  max={240}
                  min={1}
                  onCommit={updateLweFps}
                  step={1}
                  unit="FPS"
                  unitKind="scene-fps"
                />
              </label>
              <label className="settings-behavior-row">
                <span>Scene volume</span>
                <DeferredNumberInput
                  aria-label="Wallpaper Engine volume"
                  confirmed={behaviorSettings.lweVolume}
                  disabled={!behaviorReady || lweUnavailable || behaviorSettings.lweMuted}
                  max={100}
                  min={0}
                  onCommit={updateLweVolume}
                  step={1}
                  unit="%"
                  unitKind="scene-volume"
                />
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

          <h4 className="settings-behavior-category">Open location</h4>
          <div
            aria-labelledby="settings-open-location-heading"
            className="settings-behavior-card"
            data-behavior-card="open-location"
            role="group"
          >
            <h5 id="settings-open-location-heading">Folders</h5>
            <div className="settings-behavior-card__rows">
              <div className="settings-behavior-row">
                <span>Open project folders with</span>
                <SelectField
                  aria-label="Open project folders with"
                  dataBehaviorControl={true}
                  disabled={!behaviorReady}
                  onValueChange={updateOpenProjectLocationMode}
                  options={[
                    { value: 'file_manager', label: 'File manager' },
                    { value: 'terminal', label: 'Terminal' },
                  ]}
                  value={behaviorSettings.openProjectLocationMode}
                  variant="settings"
                />
              </div>
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
  const [prevOpen, setPrevOpen] = useState(props.open);
  const [shouldRender, setShouldRender] = useState(props.open);
  const [presentationPhase, setPresentationPhase] = useState<'open' | 'exiting'>('open');
  const exitTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Synchronously adjust state when props.open changes during render
  if (props.open !== prevOpen) {
    setPrevOpen(props.open);
    if (props.open) {
      if (exitTimerRef.current !== null) {
        clearTimeout(exitTimerRef.current);
        exitTimerRef.current = null;
      }
      setShouldRender(true);
      setPresentationPhase('open');
    } else {
      setPresentationPhase('exiting');
      const reducedMotion = typeof window !== 'undefined'
        && window.matchMedia?.('(prefers-reduced-motion: reduce)').matches === true;

      if (reducedMotion) {
        setShouldRender(false);
      } else {
        exitTimerRef.current = setTimeout(() => {
          exitTimerRef.current = null;
          setShouldRender(false);
        }, 180);
      }
    }
  }

  // Clean up exit timer on unmount.
  useEffect(() => () => {
    if (exitTimerRef.current !== null) clearTimeout(exitTimerRef.current);
  }, []);

  useEffect(() => {
    if (!props.open) return undefined;

    const unlockBodyScroll = lockBodyScroll(document.body);
    const closeOnEscape = (event: globalThis.KeyboardEvent) => {
      if (props.obscured) return;
      if (event.key !== 'Escape' || event.defaultPrevented) return;
      event.preventDefault();
      props.onClose(); // Set settingsOpen to false immediately
    };
    document.addEventListener('keydown', closeOnEscape);
    return () => {
      unlockBodyScroll();
      document.removeEventListener('keydown', closeOnEscape);
    };
  }, [props.obscured, props.open, props.onClose]);

  if (!shouldRender) return null;

  return CompactSettingsPanelView({
    ...props,
    open: shouldRender,
    onClose: props.onClose, // Trigger immediate close on click
    presentationPhase,
  });
}
