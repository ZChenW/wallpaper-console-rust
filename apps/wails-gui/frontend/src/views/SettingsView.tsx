import { useState, useEffect, useCallback, useRef } from 'react';
import { api, CommandResult, ThumbnailCacheDTO } from '../api/bridge';
import ConfirmDialog from '../components/ConfirmDialog';
import { AlertTriangle, CheckCircle, Loader } from 'lucide-react';

interface Props {
  onRefresh: () => void;
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

const LIBRARY_CONFIGS: ConfigGroup[] = [
  { key: 'min_wallpaper_width', label: 'Min width', type: 'number', placeholder: '1280' },
  { key: 'min_wallpaper_height', label: 'Min height', type: 'number', placeholder: '720' },
  { key: 'gui_thumbnail_mode', label: 'Thumbnail mode', type: 'select', options: ['cache', 'original', 'icon'] },
  { key: 'gui_library_source', label: 'Library source', type: 'select', options: ['tsv', 'sqlite'] },
  { key: 'preview_metadata', label: 'fzf preview', type: 'select', options: ['compact', 'visual', 'full'] },
];

export default function SettingsView({ onRefresh: _onRefresh }: Props) {
  const [configs, setConfigs] = useState<Record<string, string>>({});
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState<string | null>(null);
  const [result, setResult] = useState<CommandResult | null>(null);
  const [confirmAction, setConfirmAction] = useState<{ title: string; msg: string; fn: () => void } | null>(null);
  const [thumbCache, setThumbCache] = useState<ThumbnailCacheDTO | null>(null);
  const restoreInputRef = useRef<HTMLInputElement>(null);

  const loadConfigs = useCallback(async () => {
    setLoading(true);
    const allKeys = [...BACKEND_CONFIGS, ...LIBRARY_CONFIGS].map((c) => c.key);
    allKeys.push('storage_backend');
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

  useEffect(() => { loadConfigs(); loadThumbCache(); }, [loadConfigs, loadThumbCache]);

  const handleSet = async (key: string, value: string) => {
    setSaving(key);
    setResult(null);
    const r = await api.configSet(key, value);
    setResult(r);
    if (r.success) {
      setConfigs((prev) => ({ ...prev, [key]: value }));
    }
    setSaving(null);
  };

  const handleStorageBackendChange = (newValue: string) => {
    const current = configs['storage_backend'] ?? 'file';
    if (newValue === current) return;

    // Safety gate: switching to sqlite requires successful verify first
    if (newValue === 'sqlite') {
      setConfirmAction({
        title: 'Switch to SQLite',
        msg: 'Switching to SQLite mode requires the database to be verified.\n\nA verify will run first. If it fails, the switch will be blocked.',
        fn: async () => {
          setConfirmAction(null);
          setResult(null);
          const vr = await api.sqliteVerify();
          if (vr.success) {
            await handleSet('storage_backend', 'sqlite');
          } else {
            setResult({ success: false, stdout: '', stderr: 'Verify failed — cannot switch to sqlite mode. Run migrate-to-sqlite first.', exitCode: 1 });
          }
        },
      });
      return;
    }

    // Safety gate: switching to hybrid needs DB to exist
    if (newValue === 'hybrid') {
      setConfirmAction({
        title: 'Switch to Hybrid',
        msg: 'Hybrid mode writes to both flat files and SQLite.\n\nIf wallpapers.db does not exist, it will be created via migrate first.',
        fn: async () => {
          setConfirmAction(null);
          setResult(null);
          // Try verify first; if DB missing, offer migrate
          const vr = await api.sqliteVerify();
          if (!vr.success) {
            // DB likely missing — try migrate
            const mr = await api.migrateToSqlite();
            if (!mr.success) {
              setResult({ success: false, stdout: '', stderr: 'Cannot migrate to SQLite — hybrid mode unavailable. ' + mr.stderr, exitCode: 1 });
              return;
            }
          }
          await handleSet('storage_backend', 'hybrid');
        },
      });
      return;
    }

    // Switching to file is always safe
    handleSet('storage_backend', newValue);
  };

  const handleDbAction = async (fn: () => Promise<CommandResult>, title: string) => {
    setResult(null);
    const r = await fn();
    setResult(r);
    if (r.success) setConfirmAction(null);
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
        const r = await api.sqliteRestore(path);
        setResult(r);
        setConfirmAction(null);
      },
    });
    // Reset input so the same file can be selected again
    e.target.value = '';
  };

  return (
    <div className="view settings-view">
      <h2>Settings</h2>
      {loading ? <div className="loading">Loading...</div> : (
        <div className="settings-sections">
          {/* Backends */}
          <section className="settings-group">
            <h3>Wallpaper Backends</h3>
            {BACKEND_CONFIGS.map((c) => (
              <ConfigRow key={c.key} config={c} value={configs[c.key] ?? ''} saving={saving === c.key} onSet={(v) => handleSet(c.key, v)} />
            ))}
          </section>

          {/* Library & Thumbnails */}
          <section className="settings-group">
            <h3>Library & Thumbnails</h3>
            {LIBRARY_CONFIGS.map((c) => (
              <ConfigRow key={c.key} config={c} value={configs[c.key] ?? ''} saving={saving === c.key} onSet={(v) => handleSet(c.key, v)} />
            ))}
            <div className="config-row">
              <div className="config-info">
                <span className="config-label">Thumbnail cache</span>
                <span className="config-desc">
                  {thumbCache ? `${thumbCache.entries} files, ${thumbCache.size}` : '...'}
                </span>
              </div>
              <button
                className="btn small danger"
                onClick={() => setConfirmAction({
                  title: 'Clear Thumbnail Cache',
                  msg: `Delete all ${thumbCache?.entries ?? 0} cached thumbnails?`,
                  fn: async () => {
                    const r = await api.thumbnailCacheClear();
                    setResult(r);
                    setConfirmAction(null);
                    loadThumbCache();
                  },
                })}
              >
                Clear
              </button>
            </div>
          </section>

          {/* Storage / SQLite */}
          <section className="settings-group">
            <h3>Storage / SQLite</h3>
            <ConfigRow
              config={{ key: 'storage_backend', label: 'Storage backend', type: 'select', options: ['file', 'hybrid', 'sqlite'] }}
              value={configs['storage_backend'] ?? 'file'}
              saving={saving === 'storage_backend'}
              onSet={handleStorageBackendChange}
            />
            <div className="db-actions">
              <button className="btn small" onClick={() => handleDbAction(() => api.sqliteVerify(), 'Verify')}>Verify</button>
              <button className="btn small" onClick={() => setConfirmAction({
                title: 'Migrate to SQLite', msg: 'Create wallpapers.db from flat files?', fn: () => handleDbAction(() => api.migrateToSqlite(), 'Migrate'),
              })}>Migrate</button>
              <button className="btn small" onClick={() => setConfirmAction({
                title: 'Resync SQLite', msg: 'Rebuild wallpapers.db from flat files?', fn: () => handleDbAction(() => api.sqliteResync(), 'Resync'),
              })}>Resync</button>
              <button className="btn small" onClick={() => handleDbAction(() => api.sqliteBackup(), 'Backup')}>Backup</button>
              <button className="btn small" onClick={() => setConfirmAction({
                title: 'Export Flat', msg: 'Export SQLite back to flat files?', fn: () => handleDbAction(() => api.sqliteExportFlat(), 'Export'),
              })}>Export</button>
              <button className="btn small" onClick={handleRestore}>Restore</button>
              <input
                ref={restoreInputRef}
                type="file"
                accept=".db,.db.bak*"
                style={{ display: 'none' }}
                onChange={handleRestoreFileSelected}
              />
            </div>
          </section>

          {/* Result */}
          {result && (
            <div className={`result-banner ${result.success ? 'success' : 'error'}`}>
              {result.success ? <CheckCircle size={14} /> : <AlertTriangle size={14} />}
              <span>{result.success ? 'OK' : result.stderr || 'Failed'}</span>
            </div>
          )}
        </div>
      )}

      {confirmAction && (
        <ConfirmDialog
          title={confirmAction.title}
          message={confirmAction.msg}
          onConfirm={confirmAction.fn}
          onCancel={() => setConfirmAction(null)}
          danger={confirmAction.title.includes('Clear') || confirmAction.title.includes('Export')}
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
  onSet: (v: string) => void;
}) {
  const [edit, setEdit] = useState(value);

  useEffect(() => { setEdit(value); }, [value]);

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
            onChange={(e) => {
              setEdit(e.target.value);
              onSet(e.target.value);
            }}
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
            onBlur={() => { if (edit !== value) onSet(edit); }}
            onKeyDown={(e) => { if (e.key === 'Enter' && edit !== value) onSet(edit); }}
            disabled={saving}
          />
        )}
        {saving && <Loader size={12} className="spin" />}
      </div>
    </div>
  );
}
