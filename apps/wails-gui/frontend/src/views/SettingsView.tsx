import { useState, useEffect, useCallback } from 'react';
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

  // Load all configs
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
    const r = await api.configSet(key, value);
    setResult(r);
    setConfigs((prev) => ({ ...prev, [key]: value }));
    setSaving(null);
  };

  const handleDbAction = async (fn: () => Promise<CommandResult>, title: string) => {
    setResult(null);
    const r = await fn();
    setResult(r);
    if (r.success) setConfirmAction(null);
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
            {/* Thumbnail cache */}
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
              onSet={(v) => {
                setConfirmAction({
                  title: 'Switch Storage Backend',
                  msg: `Change storage backend to "${v}"? This may require a verify or migrate first.`,
                  fn: async () => {
                    await handleSet('storage_backend', v);
                    setConfirmAction(null);
                  },
                });
              }}
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
