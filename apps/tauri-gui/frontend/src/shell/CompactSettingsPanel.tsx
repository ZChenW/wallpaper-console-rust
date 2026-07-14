import type { ChangeEvent, CSSProperties, KeyboardEvent } from 'react';

import type { BackendStatusDTO, RendererStatusesDTO } from '../api/types.ts';
import DisplayTargetSelector from './DisplayTargetSelector.tsx';
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
  readonly connectedOutputs: readonly string[];
  readonly behaviorSettings: WallpaperBehaviorSettings;
  readonly updateBehaviorSettings: (update: WallpaperBehaviorSettingsUpdate) => void;
  readonly behaviorReady: boolean;
  readonly loadError: Error | null;
  readonly saveError: Error | null;
  readonly rendererStatuses: RendererStatusesDTO | null;
  readonly rendererStatusesLoading: boolean;
  readonly rendererStatusesError: string | null;
  readonly sourceCount: number;
  readonly offlineSourceCount: number;
  readonly onOpenSources: () => void;
  readonly onClose: () => void;
}

const overlayStyle: CSSProperties = {
  position: 'fixed',
  inset: 0,
  zIndex: 50,
  display: 'flex',
  justifyContent: 'flex-end',
  background: 'rgb(0 0 0 / 0.38)',
};

const panelStyle: CSSProperties = {
  width: 'min(30rem, 100%)',
  height: '100%',
  overflowY: 'auto',
  background: 'var(--surface, #171717)',
  color: 'var(--text, #f5f5f5)',
  boxShadow: '-0.75rem 0 2rem rgb(0 0 0 / 0.28)',
  padding: '1rem',
};

const headerStyle: CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'space-between',
  gap: '1rem',
};

const sectionStyle: CSSProperties = {
  display: 'grid',
  gap: '0.75rem',
  padding: '1rem 0',
  borderTop: '1px solid rgb(128 128 128 / 0.3)',
};

const fieldStyle: CSSProperties = {
  display: 'grid',
  gridTemplateColumns: 'minmax(8rem, 1fr) minmax(9rem, 1fr)',
  alignItems: 'center',
  gap: '0.75rem',
};

const errorStyle: CSSProperties = {
  margin: 0,
  color: 'var(--danger, #ffb4ab)',
};

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

function rendererStatusLabel(status: BackendStatusDTO | undefined): string {
  if (!status) return 'Unknown';
  return status.available ? 'Installed' : 'Unavailable';
}

export function CompactSettingsPanelView({
  open,
  preferences,
  updatePreferences,
  connectedOutputs,
  behaviorSettings,
  updateBehaviorSettings,
  behaviorReady,
  loadError,
  saveError,
  rendererStatuses,
  rendererStatusesLoading,
  rendererStatusesError,
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
  const updateImageRenderer = (event: ChangeEvent<HTMLSelectElement>) => {
    const imageBackend = event.currentTarget.value as ImageRenderer;
    updateBehaviorSettings((current) => ({ ...current, imageBackend }));
  };
  const updateGifRenderer = (event: ChangeEvent<HTMLSelectElement>) => {
    const gifBackend = event.currentTarget.value as GifRenderer;
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

  return (
    <div onKeyDown={handleKeyDown} style={overlayStyle}>
      <aside
        aria-label="Settings"
        aria-modal="true"
        role="dialog"
        style={panelStyle}
      >
        <header style={headerStyle}>
          <h2>Settings</h2>
          <button autoFocus type="button" aria-label="Close settings" onClick={onClose}>
            Close
          </button>
        </header>

        <section
          aria-labelledby="settings-appearance-heading"
          data-settings-group="appearance-interaction"
          style={sectionStyle}
        >
          <h3 id="settings-appearance-heading">Appearance &amp; interaction</h3>
          <label style={fieldStyle}>
            <span>Theme</span>
            <select aria-label="Theme" value={preferences.theme} onChange={updateTheme}>
              <option value="system">System</option>
              <option value="light">Light</option>
              <option value="dark">Dark</option>
            </select>
          </label>
          <label style={fieldStyle}>
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
          <label style={fieldStyle}>
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
          style={sectionStyle}
        >
          <h3 id="settings-wallpaper-heading">Wallpaper behavior</h3>
          <div style={fieldStyle}>
            <span>Default display</span>
            <DisplayTargetSelector
              ariaLabel="Default display target"
              connectedOutputs={connectedOutputs}
              value={preferences.displayTarget}
              onChange={(displayTarget) => {
                updatePreferences((current) => ({ ...current, displayTarget }));
              }}
            />
          </div>
          {!behaviorReady && loadError === null ? (
            <p role="status">Loading wallpaper behavior settings…</p>
          ) : null}
          {!behaviorReady && loadError !== null ? (
            <p>Wallpaper behavior controls are disabled until configuration can be read.</p>
          ) : null}
          {loadError !== null && <p role="alert" style={errorStyle}>{errorMessage(loadError)}</p>}
          {saveError !== null && <p role="alert" style={errorStyle}>{errorMessage(saveError)}</p>}
          <div aria-label="Renderer installation status">
            <strong>Renderer installation status</strong>
            <ul>
              {([
                ['awww', rendererStatuses?.awww],
                ['mpvpaper', rendererStatuses?.mpvpaper],
                ['linux-wallpaperengine', rendererStatuses?.linuxWallpaperEngine],
              ] as const).map(([name, status]) => (
                <li key={name} title={status?.detail}>
                  <span>{name}</span>: <span>{rendererStatusLabel(status)}</span>
                </li>
              ))}
            </ul>
            {rendererStatusesLoading ? <p role="status">Checking renderer installation…</p> : null}
            {rendererStatusesError ? (
              <p role="status">Renderer status is Unknown: {rendererStatusesError}</p>
            ) : null}
          </div>
          <label style={fieldStyle}>
            <span>Image renderer</span>
            <select
              aria-label="Image renderer"
              data-behavior-control={true}
              disabled={!behaviorReady}
              value={behaviorSettings.imageBackend}
              onChange={updateImageRenderer}
            >
              <option disabled={awwwUnavailable} value="awww">awww renderer</option>
              <option disabled={mpvpaperUnavailable} value="mpvpaper">mpvpaper renderer</option>
            </select>
          </label>
          <label style={fieldStyle}>
            <span>GIF renderer</span>
            <select
              aria-label="GIF renderer"
              data-behavior-control={true}
              disabled={!behaviorReady}
              value={behaviorSettings.gifBackend}
              onChange={updateGifRenderer}
            >
              <option disabled={awwwUnavailable} value="awww">awww renderer</option>
              <option disabled={mpvpaperUnavailable} value="mpvpaper">mpvpaper renderer</option>
            </select>
          </label>
          <div style={fieldStyle}>
            <span>Video renderer</span>
            <output>
              mpvpaper (required for video){mpvpaperUnavailable ? ' · unavailable' : ''}
            </output>
          </div>
          {usesAwww ? (
            <>
              <label style={fieldStyle}>
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
              <label style={fieldStyle}>
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
              <label style={fieldStyle}>
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
              <label style={fieldStyle}>
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
          <label style={fieldStyle}>
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
          <label style={fieldStyle}>
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
          <label style={fieldStyle}>
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
          <label style={fieldStyle}>
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
          <label style={fieldStyle}>
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
            Renderer availability is checked when applying. Missing dependencies are reported
            with installation guidance.
          </p>
        </section>

        <section
          aria-labelledby="settings-sources-heading"
          data-settings-group="sources"
          style={sectionStyle}
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
  return CompactSettingsPanelView(props);
}
