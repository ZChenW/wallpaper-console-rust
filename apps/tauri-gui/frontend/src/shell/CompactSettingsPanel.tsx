import type { ChangeEvent, CSSProperties } from 'react';

import DisplayTargetSelector from './DisplayTargetSelector.tsx';
import type {
  ApplyGesture,
  ShellPreferences,
  ShellTheme,
} from './shellPreferences.ts';
import type { ShellPreferencesUpdate } from './useShellPreferences.ts';
import type {
  GifRenderer,
  ImageRenderer,
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
  sourceCount,
  offlineSourceCount,
  onOpenSources,
  onClose,
}: CompactSettingsPanelProps) {
  if (!open) return null;

  const usesAwww = behaviorSettings.imageBackend === 'awww'
    || behaviorSettings.gifBackend === 'awww';
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

  return (
    <div style={overlayStyle}>
      <aside
        aria-label="Settings"
        aria-modal="true"
        role="dialog"
        style={panelStyle}
      >
        <header style={headerStyle}>
          <h2>Settings</h2>
          <button type="button" aria-label="Close settings" onClick={onClose}>
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
          {!behaviorReady && <p role="status">Loading wallpaper behavior settings…</p>}
          {loadError !== null && <p role="alert" style={errorStyle}>{errorMessage(loadError)}</p>}
          {saveError !== null && <p role="alert" style={errorStyle}>{errorMessage(saveError)}</p>}
          <label style={fieldStyle}>
            <span>Image renderer</span>
            <select
              aria-label="Image renderer"
              data-behavior-control={true}
              disabled={!behaviorReady}
              value={behaviorSettings.imageBackend}
              onChange={updateImageRenderer}
            >
              <option value="awww">awww renderer</option>
              <option value="mpvpaper">mpvpaper renderer</option>
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
              <option value="awww">awww renderer</option>
              <option value="mpvpaper">mpvpaper renderer</option>
            </select>
          </label>
          <div style={fieldStyle}>
            <span>Video renderer</span>
            <output>mpvpaper (required for video)</output>
          </div>
          {usesAwww ? (
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
          ) : (
            <p>Fill behavior is available when awww handles images or GIFs.</p>
          )}
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
