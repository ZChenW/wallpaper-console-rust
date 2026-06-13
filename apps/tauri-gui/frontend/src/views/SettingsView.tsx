import { useState, useEffect, useCallback, useRef } from 'react';
import {
  api,
  CommandResult,
  LibrarySourceStatusDTO,
  LinuxWallpaperEngineStatusDTO,
  ThumbnailCacheDTO,
  WeDebugInfoDTO,
} from '../api/bridge';
import { CommandFeedback, commandErrorFeedback, commandSuccessFeedback } from '../api/feedback';
import ConfirmDialog from '../components/ConfirmDialog';
import { useAppState } from '../state/AppStateContext';
import { Loader } from 'lucide-react';
import {
  BACKEND_CONFIGS,
  BACKEND_ADVANCED_CONFIGS,
  ConfigGroup,
  LIBRARY_ADVANCED_CONFIGS,
  LIBRARY_CONFIGS,
  normalizeConfigValue,
  cleanupDays,
  WE_BACKEND_ADVANCED_CONFIGS,
  WE_BACKEND_CONFIGS,
} from '../settings/configSchema';

interface Props {
  onRefresh: () => void;
  onFeedback: (fb: CommandFeedback) => void;
}

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
  const [weDebugInfo, setWeDebugInfo] = useState<WeDebugInfoDTO | null>(null);
  const restoreInputRef = useRef<HTMLInputElement>(null);

  const loadConfigs = useCallback(async () => {
    setLoading(true);
    const allKeys = [
      ...BACKEND_CONFIGS,
      ...BACKEND_ADVANCED_CONFIGS,
      ...WE_BACKEND_CONFIGS,
      ...WE_BACKEND_ADVANCED_CONFIGS,
      ...LIBRARY_CONFIGS,
      ...LIBRARY_ADVANCED_CONFIGS,
    ].map((c) => c.key);
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

  const loadWeDebugInfo = useCallback(async () => {
    try { setWeDebugInfo(await api.weDebugInfo()); } catch { /* */ }
  }, []);

  useEffect(() => {
    loadConfigs();
    loadThumbCache();
    loadWeStatus();
    loadWeDebugInfo();
    void api.librarySourceStatus().then(setLibraryStatus);
  }, [loadConfigs, loadThumbCache, loadWeStatus, loadWeDebugInfo]);

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
            <details className="settings-advanced">
              <summary>Advanced backend tuning</summary>
              {BACKEND_ADVANCED_CONFIGS.map((c) => (
                <ConfigRow key={c.key} config={c} value={configs[c.key] ?? ''} saving={saving === c.key} onSet={(v) => handleSet(c.key, v)} />
              ))}
            </details>
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
            <details className="settings-advanced">
              <summary>Advanced scene backend tuning</summary>
              {WE_BACKEND_ADVANCED_CONFIGS.map((c) => (
                <ConfigRow key={c.key} config={c} value={configs[c.key] ?? ''} saving={saving === c.key} onSet={(v) => handleSet(c.key, v)} />
              ))}
            </details>
            <div className="config-desc" style={{ fontSize: 11, color: '#666', marginTop: 4 }}>
              linux-wallpaperengine supports many Wallpaper Engine <strong>scene</strong> wallpapers.
              Some scene wallpapers may use unsupported projection effects and will show
              a compatibility warning. Wallpaper Engine <strong>Web</strong> projects are indexed for browsing only.
            </div>
          </section>

          <section className="settings-group">
            <h3>Wallpaper Engine Web Projects</h3>
            <p className="config-desc">
              WE Web projects are unsupported as live wallpapers in this app. They still appear
              in the Library as project cards so you can inspect metadata, open the project folder,
              copy the Workshop ID, and apply a preview GIF when available.
            </p>
          </section>

          <section className="settings-group">
            <h3>Library & Thumbnails</h3>
            {LIBRARY_CONFIGS.map((c) => (
              <ConfigRow key={c.key} config={c} value={configs[c.key] ?? ''} saving={saving === c.key} onSet={(v) => handleSet(c.key, v)} />
            ))}
            <details className="settings-advanced">
              <summary>Advanced library tuning</summary>
              {LIBRARY_ADVANCED_CONFIGS.map((c) => (
                <ConfigRow key={c.key} config={c} value={configs[c.key] ?? ''} saving={saving === c.key} onSet={(v) => handleSet(c.key, v)} />
              ))}
            </details>
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

          {weDebugInfo && (weDebugInfo.lastStderr || weDebugInfo.lastExitStatus) && (
            <section className="settings-group">
              <h3>WE Backend Debug Info</h3>
              <p className="debug-privacy-note" style={{ fontSize: '0.8em', color: 'var(--muted)', marginBottom: 8 }}>
                Shows local paths and command lines. For diagnostics only.
              </p>
              <div className="debug-block">
                {weDebugInfo.lastCommandLine && (
                  <div className="debug-row">
                    <span className="debug-label">Last command:</span>
                    <code className="debug-value">{weDebugInfo.lastCommandLine}</code>
                  </div>
                )}
                {weDebugInfo.lastTargetConfig && (
                  <div className="debug-row">
                    <span className="debug-label">Target config:</span>
                    <code className="debug-value">{weDebugInfo.lastTargetConfig}</code>
                  </div>
                )}
                {weDebugInfo.lastExitStatus && (
                  <div className="debug-row">
                    <span className="debug-label">Exit status:</span>
                    <code className="debug-value">{weDebugInfo.lastExitStatus}</code>
                  </div>
                )}
                {weDebugInfo.lastStderr && (
                  <details className="debug-row">
                    <summary className="debug-label" style={{ cursor: 'pointer' }}>
                      Last stderr (click to expand)
                    </summary>
                    <pre className="debug-value debug-pre">{weDebugInfo.lastStderr}</pre>
                  </details>
                )}
                <div className="debug-row">
                  <span className="debug-label">Log file:</span>
                  <code className="debug-value">{weDebugInfo.logPath}</code>
                </div>
              </div>
            </section>
          )}
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
