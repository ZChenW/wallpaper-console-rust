import { useState, useEffect, useCallback, useRef } from 'react';
import {
  api,
  CommandResult,
  LibrarySourceStatusDTO,
  LinuxWallpaperEngineStatusDTO,
  ThumbnailCacheDTO,
  WebWallpaperStatusDTO,
} from '../api/bridge';
import { CommandFeedback, commandErrorFeedback, commandSuccessFeedback } from '../api/feedback';
import ConfirmDialog from '../components/ConfirmDialog';
import { useAppState } from '../state/AppStateContext';
import { Loader } from 'lucide-react';

interface Props {
  onRefresh: () => void;
  onFeedback: (fb: CommandFeedback) => void;
}

interface ConfigGroup {
  key: string;
  label: string;
  type: 'select' | 'text' | 'number';
  options?: string[];
  placeholder?: string;
}

const BACKEND_CONFIGS: ConfigGroup[] = [
  { key: 'image_backend', label: 'Image backend', type: 'select', options: ['awww', 'mpvpaper'] },
  { key: 'gif_backend', label: 'GIF backend', type: 'select', options: ['awww', 'mpvpaper'] },
  { key: 'video_backend', label: 'Video backend', type: 'select', options: ['mpvpaper', 'awww'] },
  { key: 'mpvpaper_options', label: 'mpvpaper options', type: 'text', placeholder: 'no-audio --loop-file=inf' },
  { key: 'mpvpaper_output', label: 'mpvpaper output', type: 'text', placeholder: '*' },
  { key: 'awww_transition_type', label: 'awww transition', type: 'select', options: ['fade', 'slide', 'wipe'] },
  { key: 'awww_transition_duration', label: 'awww duration (s)', type: 'text', placeholder: '1' },
  { key: 'awww_resize', label: 'awww resize', type: 'select', options: ['crop', 'fit', 'stretch'] },
];

const WE_BACKEND_CONFIGS: ConfigGroup[] = [
  { key: 'linux_wallpaperengine_enabled', label: 'Enable scene backend', type: 'select', options: ['on', 'off'] },
  { key: 'linux_wallpaperengine_path', label: 'linux-wallpaperengine path', type: 'text', placeholder: 'auto' },
  { key: 'linux_wallpaperengine_target_mode', label: 'Target mode', type: 'select', options: ['auto', 'screen-root', 'screen-span', 'window'] },
  { key: 'linux_wallpaperengine_target', label: 'Output/window target', type: 'text', placeholder: 'eDP-1 or HDMI-A-1' },
  { key: 'linux_wallpaperengine_scaling', label: 'Scaling', type: 'select', options: ['default', 'fill', 'fit', 'stretch'] },
  { key: 'linux_wallpaperengine_fps', label: 'FPS', type: 'select', options: ['30', '60'] },
  { key: 'linux_wallpaperengine_muted', label: 'Muted', type: 'select', options: ['off', 'on'] },
  { key: 'linux_wallpaperengine_volume', label: 'Volume', type: 'number', placeholder: '100' },
  { key: 'linux_wallpaperengine_assets_dir', label: 'Assets dir', type: 'text', placeholder: 'auto' },
];

const WEB_WALLPAPER_CONFIGS: ConfigGroup[] = [
  { key: 'web_wallpaper_enabled', label: 'Enable web wallpaper backend', type: 'select', options: ['on', 'off'] },
  { key: 'web_wallpaper_browser', label: 'Web browser path', type: 'text', placeholder: 'auto' },
  { key: 'web_wallpaper_audio', label: 'Audio', type: 'select', options: ['on', 'off'] },
  { key: 'web_wallpaper_window_width', label: 'Window width', type: 'number', placeholder: '1920' },
  { key: 'web_wallpaper_window_height', label: 'Window height', type: 'number', placeholder: '1080' },
  { key: 'web_wallpaper_extra_args', label: 'Extra browser args', type: 'text', placeholder: '' },
];

const LIBRARY_CONFIGS: ConfigGroup[] = [
  { key: 'min_wallpaper_width', label: 'Min width', type: 'number', placeholder: '1280' },
  { key: 'min_wallpaper_height', label: 'Min height', type: 'number', placeholder: '720' },
  { key: 'gui_thumbnail_mode', label: 'Thumbnail mode', type: 'select', options: ['cache', 'original', 'icon'] },
  { key: 'gui_thumbnail_cleanup_days', label: 'Clear thumbnail cache after days', type: 'number', placeholder: '30' },
  { key: 'gui_thumbnail_failure_ttl_secs', label: 'Retry failed thumbnails after seconds', type: 'number', placeholder: '900' },
  { key: 'gui_debug_logs', label: 'Debug logs', type: 'select', options: ['off', 'on'] },
  { key: 'preview_metadata', label: 'fzf preview', type: 'select', options: ['compact', 'visual', 'full'] },
];

type DbAction = 'verify' | 'rebuild' | 'backup' | 'export' | 'restore';

export default function SettingsView({ onRefresh: _onRefresh, onFeedback }: Props) {
  const { invalidateLibrary } = useAppState();
  const [configs, setConfigs] = useState<Record<string, string>>({});
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState<string | null>(null);
  const [confirmAction, setConfirmAction] = useState<{ title: string; msg: string; fn: () => Promise<void> } | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [thumbCache, setThumbCache] = useState<ThumbnailCacheDTO | null>(null);
  const [dbAction, setDbAction] = useState<DbAction | null>(null);
  const [libraryStatus, setLibraryStatus] = useState<LibrarySourceStatusDTO | null>(null);
  const [weStatus, setWeStatus] = useState<LinuxWallpaperEngineStatusDTO | null>(null);
  const [webStatus, setWebStatus] = useState<WebWallpaperStatusDTO | null>(null);
  const restoreInputRef = useRef<HTMLInputElement>(null);

  const loadConfigs = useCallback(async () => {
    setLoading(true);
    const allKeys = [...BACKEND_CONFIGS, ...WE_BACKEND_CONFIGS, ...WEB_WALLPAPER_CONFIGS, ...LIBRARY_CONFIGS].map((c) => c.key);
    const map: Record<string, string> = {};
    for (const key of allKeys) {
      try { map[key] = await api.configGet(key); } catch { map[key] = ''; }
    }
    setConfigs(map);
    setLoading(false);
  }, []);

  const loadThumbCache = useCallback(async () => {
    try { setThumbCache(await api.thumbnailCacheStatus()); } catch { /* */ }
  }, []);

  const loadWeStatus = useCallback(async () => {
    try { setWeStatus(await api.linuxWallpaperEngineStatus()); } catch { /* */ }
  }, []);

  const loadWebStatus = useCallback(async () => {
    try { setWebStatus(await api.webWallpaperStatus()); } catch { /* */ }
  }, []);

  useEffect(() => {
    loadConfigs();
    loadThumbCache();
    loadWeStatus();
    loadWebStatus();
    void api.librarySourceStatus().then(setLibraryStatus);
  }, [loadConfigs, loadThumbCache, loadWeStatus, loadWebStatus]);

  const handleSet = async (key: string, value: string): Promise<boolean> => {
    const normalized = normalizeConfigValue(key, value);
    setSaving(key);
    try {
      const r = await api.configSet(key, normalized);
      if (r.success) {
        setConfigs((prev) => ({ ...prev, [key]: normalized }));
        window.dispatchEvent(new CustomEvent('wc-config-changed', { detail: { key, value: normalized } }));
        if (key.startsWith('linux_wallpaperengine_')) {
          void loadWeStatus();
        }
        if (key.startsWith('web_wallpaper_')) {
          void loadWebStatus();
        }
        onFeedback({ state: 'success', label: 'Setting saved', detail: key });
        return true;
      } else {
        onFeedback(commandErrorFeedback('Setting', r));
        return false;
      }
    } catch (e) {
      onFeedback(commandErrorFeedback('Setting', e));
      return false;
    } finally {
      setSaving(null);
    }
  };

  const runDbAction = async (action: DbAction, label: string, fn: () => Promise<CommandResult>) => {
    setConfirming(true);
    setDbAction(action);
    onFeedback({ state: 'running', label });
    try {
      const r = await fn();
      if (r.success) {
        if (r.stdout && r.stdout.includes('WITH WARNINGS')) {
          onFeedback({ state: 'warning', label: `${label} complete`, detail: r.stdout });
        } else {
          onFeedback(commandSuccessFeedback(label, r));
        }
      } else {
        onFeedback(commandErrorFeedback(label, r));
      }
    } catch (e) {
      onFeedback(commandErrorFeedback(label, e));
    } finally {
      setDbAction(null);
      setConfirming(false);
      setConfirmAction(null);
    }
  };

  const handleRestore = () => {
    restoreInputRef.current?.click();
  };

  const handleRestoreFileSelected = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;
    const path = (file as unknown as { path?: string }).path ?? file.name;
    setConfirmAction({
      title: 'Restore Database',
      msg: `Restore wallpapers.db from:\n${path}\n\nCurrent database will be backed up first.`,
      fn: async () => {
        setConfirming(true);
        onFeedback({ state: 'running', label: 'Restore' });
        try {
          const r = await api.sqliteRestore(path);
          if (r.success) {
            onFeedback(commandSuccessFeedback('Restore', r));
          } else {
            onFeedback(commandErrorFeedback('Restore', r));
          }
        } catch (e) {
          onFeedback(commandErrorFeedback('Restore', e));
        } finally {
          setConfirming(false);
          setConfirmAction(null);
        }
      },
    });
    e.target.value = '';
  };

  return (
    <div className="view settings-view">
      <h2>Settings</h2>
      {loading ? <div className="loading">Loading...</div> : (
        <div className="settings-sections">
          <section className="settings-group">
            <h3>Wallpaper Backends</h3>
            {BACKEND_CONFIGS.map((c) => (
              <ConfigRow key={c.key} config={c} value={configs[c.key] ?? ''} saving={saving === c.key} onSet={(v) => handleSet(c.key, v)} />
            ))}
          </section>

          <section className="settings-group">
            <h3>Wallpaper Engine Backend</h3>
            <div className="config-row">
              <div className="config-info">
                <span className="config-label">
                  {weStatus?.available ? 'Ready' : 'Missing'}
                </span>
                <span className="config-desc">
                  {weStatus?.available && weStatus.path
                    ? `linux-wallpaperengine found at ${weStatus.path}`
                    : weStatus?.message ?? 'Checking linux-wallpaperengine...'}
                </span>
                {!weStatus?.available && weStatus?.detail && (
                  <span className="config-desc">{weStatus.detail}</span>
                )}
              </div>
            </div>
            {WE_BACKEND_CONFIGS.map((c) => (
              <ConfigRow key={c.key} config={c} value={configs[c.key] ?? ''} saving={saving === c.key} onSet={(v) => handleSet(c.key, v)} />
            ))}
            <div className="config-desc" style={{ fontSize: 11, color: '#666', marginTop: 4 }}>
              linux-wallpaperengine supports many Wallpaper Engine <strong>scene</strong> wallpapers.
              Some scene wallpapers may use unsupported projection effects and will show
              a compatibility warning. <strong>Web</strong> wallpapers use the experimental Chromium preview.
            </div>
          </section>

          <section className="settings-group">
            <h3>Chromium Preview (Experimental)</h3>
            <div className="config-row">
              <div className="config-info">
                <span className="config-label">
                  {webStatus?.available ? 'Chromium preview: Available' : (webStatus?.message ?? 'Checking...')}
                </span>
                {webStatus?.detail && (
                  <span className="config-desc">{webStatus.detail}</span>
                )}
              </div>
            </div>
            {WEB_WALLPAPER_CONFIGS.map((c) => (
              <ConfigRow key={c.key} config={c} value={configs[c.key] ?? ''} saving={saving === c.key} onSet={(v) => handleSet(c.key, v)} />
            ))}
            <div className="config-desc" style={{ fontSize: 11, color: '#c96', marginTop: 4 }}>
              <strong>Experimental only.</strong> Chromium preview opens Web wallpapers in a normal
              browser window. It is <strong>not a real desktop wallpaper backend</strong> on Niri/Wayland.
              A native WebKitGTK layer-shell renderer is required for true Web wallpaper support.
            </div>
            <div className="config-desc" style={{ fontSize: 11, color: '#666', marginTop: 4 }}>
              Auto-detects chromium, google-chrome, brave, or vivaldi.
              {webStatus?.available && webStatus.path && (
                <><br />Browser: <code>{webStatus.path}</code></>
              )}
            </div>
            <details style={{ marginTop: 4 }}>
              <summary>niri compositor rule example</summary>
              <pre style={{
                fontSize: 11,
                background: '#1a1a1a',
                padding: '8px 12px',
                borderRadius: 4,
                overflowX: 'auto',
                marginTop: 4,
              }}>
{`// Add to ~/.config/niri/config.kdl:
window-rule {
    match app-id="^web-wallpaper-console$"
    default-column-width {}
    open-floating false
    open-maximized true
    block-out-from "screenshot"
    block-out-from "screen-capture"
}`}
              </pre>
            </details>
          </section>

          <section className="settings-group">
            <h3>Library & Thumbnails</h3>
            {LIBRARY_CONFIGS.map((c) => (
              <ConfigRow key={c.key} config={c} value={configs[c.key] ?? ''} saving={saving === c.key} onSet={(v) => handleSet(c.key, v)} />
            ))}
            <div className="config-row">
              <div className="config-info">
                <span className="config-label">Thumbnail cache</span>
                <span className="config-desc">
                  {thumbCache
                    ? `${thumbCache.entries} thumbnails, ${thumbCache.size}` +
                      ` · ${thumbCache.failureEntries} failed` +
                      (thumbCache.oldestMtime
                        ? ` · oldest: ${new Date(thumbCache.oldestMtime * 1000).toLocaleDateString()}`
                        : '') +
                      (thumbCache.newestMtime
                        ? ` · newest: ${new Date(thumbCache.newestMtime * 1000).toLocaleDateString()}`
                        : '') +
                      ` · cleanup: older than ${cleanupDays(configs, thumbCache)} days`
                    : '...'}
                </span>
              </div>
              <div className="db-actions">
                <button
                  className="btn small"
                  onClick={async () => {
                    const days = cleanupDays(configs, thumbCache);
                    onFeedback({ state: 'running', label: 'Cleanup' });
                    try {
                      const r = await api.thumbnailCacheCleanupOld(days);
                      if (r.success) onFeedback(commandSuccessFeedback('Cleanup', r));
                      else onFeedback(commandErrorFeedback('Cleanup', r));
                      loadThumbCache();
                    } catch (e) {
                      onFeedback(commandErrorFeedback('Cleanup', e));
                    }
                  }}
                  title={`Remove cached thumbnails and failure records older than ${cleanupDays(configs, thumbCache)} days`}
                >
                  Cleanup old
                </button>
                <button
                  className="btn small danger"
                  onClick={() => setConfirmAction({
                    title: 'Clear Thumbnail Cache',
                    msg: `Delete all ${thumbCache?.entries ?? 0} cached thumbnails?`,
                    fn: async () => {
                      setConfirming(true);
                      onFeedback({ state: 'running', label: 'Clearing thumbnail cache' });
                      try {
                        const r = await api.thumbnailCacheClear();
                        if (r.success) {
                          onFeedback(commandSuccessFeedback('Thumbnail cache clear', r));
                        } else {
                          onFeedback(commandErrorFeedback('Thumbnail cache clear', r));
                        }
                        loadThumbCache();
                      } catch (e) {
                        onFeedback(commandErrorFeedback('Thumbnail cache clear', e));
                      } finally {
                        setConfirming(false);
                        setConfirmAction(null);
                      }
                    },
                  })}
                >
                  Clear
                </button>
              </div>
            </div>
          </section>

          <section className="settings-group">
            <h3>Library Database</h3>
            <div className="config-row">
              <div className="config-info">
                <span className="config-label">SQLite active</span>
                <span className="config-desc">{libraryStatus != null ? `${libraryStatus.sqliteRows} wallpapers indexed` : '...'}</span>
              </div>
              <div className="db-actions">
                <button
                  className={`btn small ${dbAction === 'verify' ? 'running' : ''}`}
                  onClick={() => runDbAction('verify', 'Verify', async () => {
                    const result = await api.sqliteVerify();
                    if (result.success && result.stdout && result.stdout.includes('WITH WARNINGS')) {
                      onFeedback({ state: 'warning', label: 'Verify complete', detail: result.stdout });
                    }
                    return result;
                  })}
                  disabled={dbAction !== null}
                >
                  {dbAction === 'verify' && <Loader size={12} className="spin" style={{ marginRight: 4 }} />}
                  Verify Database
                </button>
                <button
                  className={`btn small ${dbAction === 'backup' ? 'running' : ''}`}
                  onClick={() => runDbAction('backup', 'Backup', () => api.sqliteBackup())}
                  disabled={dbAction !== null}
                >
                  {dbAction === 'backup' && <Loader size={12} className="spin" style={{ marginRight: 4 }} />}
                  Backup Database
                </button>
              </div>
            </div>
            <details className="settings-advanced">
              <summary>Advanced Database Maintenance</summary>
              <div className="db-actions" style={{ marginTop: 8 }}>
                <button
                  className={`btn small ${dbAction === 'rebuild' ? 'running' : ''}`}
                  onClick={() => setConfirmAction({
                    title: 'Rebuild Database',
                    msg: 'Re-scan all configured source directories and rebuild the library database?',
                    fn: async () => {
                      const result = await api.rescan();
                      if (result.success) {
                        invalidateLibrary();
                        onFeedback(commandSuccessFeedback('Rebuild', result));
                      } else {
                        onFeedback(commandErrorFeedback('Rebuild', result));
                      }
                    },
                  })}
                  disabled={dbAction !== null}
                >
                  {dbAction === 'rebuild' && <Loader size={12} className="spin" style={{ marginRight: 4 }} />}
                  Rebuild Database
                </button>
                <button
                  className={`btn small ${dbAction === 'export' ? 'running' : ''}`}
                  onClick={() => setConfirmAction({
                    title: 'Export Legacy Files',
                    msg: 'Export SQLite back to flat files?',
                    fn: () => runDbAction('export', 'Export', () => api.sqliteExportFlat()),
                  })}
                  disabled={dbAction !== null}
                >
                  {dbAction === 'export' && <Loader size={12} className="spin" style={{ marginRight: 4 }} />}
                  Export Legacy Files
                </button>
                <button
                  className="btn small"
                  onClick={handleRestore}
                  disabled={dbAction !== null}
                >
                  Restore Backup
                </button>
                <input
                  ref={restoreInputRef}
                  type="file"
                  accept=".db,.db.bak*"
                  style={{ display: 'none' }}
                  onChange={handleRestoreFileSelected}
                />
              </div>
            </details>
            <button
              className="btn small"
              onClick={async () => {
                onFeedback({ state: 'running', label: 'Exporting diagnostics' });
                try {
                  const r = await api.exportDiagnostics();
                  if (r.success) {
                    onFeedback({ state: 'success', label: 'Diagnostics exported', detail: r.stdout });
                  } else {
                    onFeedback(commandErrorFeedback('Export diagnostics', r));
                  }
                } catch (e) {
                  onFeedback(commandErrorFeedback('Export diagnostics', e));
                }
              }}
              style={{ marginTop: 8 }}
            >
              Export diagnostics
            </button>
          </section>
        </div>
      )}

      {confirmAction && (
        <ConfirmDialog
          title={confirmAction.title}
          message={confirmAction.msg}
          onConfirm={confirmAction.fn}
          onCancel={() => { setConfirmAction(null); setConfirming(false); }}
          danger={confirmAction.title.includes('Clear') || confirmAction.title.includes('Export') || confirmAction.title.includes('Restore') || confirmAction.title.includes('Stop')}
          confirming={confirming}
        />
      )}
    </div>
  );
}

function normalizeConfigValue(key: string, value: string): string {
  if (key === 'gui_thumbnail_cleanup_days') {
    return clampIntString(value, 1, 3650, 30);
  }
  if (key === 'gui_thumbnail_failure_ttl_secs') {
    return clampIntString(value, 60, 86_400, 900);
  }
  if (key === 'linux_wallpaperengine_fps') {
    return clampIntString(value, 1, 240, 60);
  }
  if (key === 'linux_wallpaperengine_volume') {
    return clampIntString(value, 0, 100, 100);
  }
  if (key === 'linux_wallpaperengine_path' || key === 'linux_wallpaperengine_assets_dir') {
    return value.trim() || 'auto';
  }
  if (key === 'linux_wallpaperengine_target') {
    return value.trim();
  }
  return value;
}

function clampIntString(value: string, min: number, max: number, fallback: number): string {
  const parsed = Number.parseInt(value, 10);
  if (!Number.isFinite(parsed)) return String(fallback);
  return String(Math.min(max, Math.max(min, parsed)));
}

function cleanupDays(configs: Record<string, string>, cache: ThumbnailCacheDTO | null): number {
  const configured = Number.parseInt(configs['gui_thumbnail_cleanup_days'] ?? '', 10);
  if (Number.isFinite(configured)) return Math.min(3650, Math.max(1, configured));
  return cache?.cleanupDays ?? 30;
}

function ConfigRow({
  config,
  value,
  saving,
  onSet,
}: {
  config: ConfigGroup;
  value: string;
  saving: boolean;
  onSet: (v: string) => Promise<boolean>;
}) {
  const [edit, setEdit] = useState(value);
  const submitting = useRef(false);

  useEffect(() => { setEdit(value); }, [value]);

  const submit = async (v: string) => {
    if (submitting.current) return;
    submitting.current = true;
    setEdit(v);
    const ok = await onSet(v);
    if (!ok) setEdit(value);
    submitting.current = false;
  };

  return (
    <div className="config-row">
      <div className="config-info">
        <span className="config-label">{config.label}</span>
        <span className="config-key">{config.key}</span>
      </div>
      <div className="config-input">
        {config.type === 'select' && config.options ? (
          <select
            value={edit}
            onChange={(e) => submit(e.target.value)}
            disabled={saving}
          >
            {config.options.map((o) => (
              <option key={o} value={o}>{o}</option>
            ))}
          </select>
        ) : (
          <input
            type={config.type === 'number' ? 'number' : 'text'}
            value={edit}
            placeholder={config.placeholder}
            onChange={(e) => setEdit(e.target.value)}
            onBlur={() => { if (edit !== value && !submitting.current) submit(edit); }}
            onKeyDown={(e) => { if (e.key === 'Enter' && edit !== value && !submitting.current) submit(edit); }}
            disabled={saving}
          />
        )}
        {saving && <Loader size={12} className="spin" />}
      </div>
    </div>
  );
}
