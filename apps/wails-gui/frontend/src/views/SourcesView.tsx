import { useState, useEffect, useCallback } from 'react';
import { api, SourceDTO, CommandResult } from '../api/bridge';
import ConfirmDialog from '../components/ConfirmDialog';
import { FolderPlus, Trash2, CheckCircle, XCircle, Scan, AlertTriangle } from 'lucide-react';

interface Props {
  onRefresh: () => void;
}

export default function SourcesView({ onRefresh }: Props) {
  const [sources, setSources] = useState<SourceDTO[]>([]);
  const [loading, setLoading] = useState(true);
  const [adding, setAdding] = useState(false);
  const [scanning, setScanning] = useState(false);
  const [result, setResult] = useState<CommandResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      setSources(await api.sourcesList());
      setError(null);
    } catch (e) {
      setError(String(e));
      setSources([]);
    }
    setLoading(false);
  }, []);

  useEffect(() => { load(); }, [load]);

  const clearResult = () => { setResult(null); setError(null); };

  const handleAdd = async () => {
    clearResult();
    setAdding(true);
    try {
      const dir = await api.browseDirectory();
      if (!dir) {
        setError('No directory selected, or no directory picker available.\nInstall zenity, kdialog, or yad.');
        return;
      }
      const r = await api.sourceAdd(dir);
      if (!r.success) {
        setResult(r);
        return;
      }
      await load();
      onRefresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setAdding(false);
    }
  };

  const handleRemove = async (path: string) => {
    clearResult();
    const r = await api.sourceRemove(path);
    if (!r.success) {
      setResult(r);
      return;
    }
    await load();
    onRefresh();
  };

  const handleScanWorkshop = async () => {
    clearResult();
    setScanning(true);
    try {
      const r = await api.scanSteamWorkshop();
      if (!r.success) {
        setResult(r);
        return;
      }
      await load();
      onRefresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setScanning(false);
    }
  };

  const handleRemoveMissing = async () => {
    clearResult();
    const r = await api.removeMissingSources();
    if (!r.success) {
      setResult(r);
      return;
    }
    await load();
    onRefresh();
  };

  const weSources = sources.filter((s) => s.isWE);
  const otherSources = sources.filter((s) => !s.isWE);

  return (
    <div className="view sources-view">
      <div className="view-header">
        <h2>Sources</h2>
        <div className="view-controls">
          <button className="toolbar-btn" onClick={handleAdd} disabled={adding} title="Add source">
            <FolderPlus size={16} />
          </button>
          <button className="toolbar-btn" onClick={handleScanWorkshop} disabled={scanning} title="Scan Wallpaper Engine">
            <Scan size={16} />
          </button>
          <button className="toolbar-btn" onClick={handleRemoveMissing} title="Remove missing sources">
            <Trash2 size={16} />
          </button>
        </div>
      </div>

      {(result || error) && (
        <div className={`result-banner ${result?.success === false || error ? 'error' : 'success'}`}>
          <AlertTriangle size={14} />
          <span>{error || result?.stderr || result?.stdout || 'Operation failed'}</span>
          <button className="btn small" onClick={clearResult} style={{marginLeft:'auto'}}>Dismiss</button>
        </div>
      )}

      {loading ? (
        <div className="loading">Loading sources...</div>
      ) : sources.length === 0 ? (
        <div className="empty-state">No sources configured — add a directory to start</div>
      ) : (
        <div className="sources-list">
          {weSources.length > 0 && (
            <details className="source-group" open>
              <summary className="source-group-header">
                Wallpaper Engine ({weSources.length})
              </summary>
              {weSources.map((s) => (
                <SourceItem key={s.path} source={s} onRemove={handleRemove} />
              ))}
            </details>
          )}
          {otherSources.length > 0 && (
            <details className="source-group" open>
              <summary className="source-group-header">
                Other Sources ({otherSources.length})
              </summary>
              {otherSources.map((s) => (
                <SourceItem key={s.path} source={s} onRemove={handleRemove} />
              ))}
            </details>
          )}
        </div>
      )}
    </div>
  );
}

function SourceItem({ source, onRemove }: { source: SourceDTO; onRemove: (p: string) => void }) {
  const [confirm, setConfirm] = useState(false);

  return (
    <div className="source-item">
      <span className="source-status">
        {source.exists ? <CheckCircle size={14} className="text-green" /> : <XCircle size={14} className="text-red" />}
      </span>
      <div className="source-info">
        <span className="source-label">{source.label}</span>
        <span className="source-path" title={source.path}>{source.path}</span>
      </div>
      <button className="source-remove" onClick={() => setConfirm(true)} title="Remove source">
        <Trash2 size={14} />
      </button>
      {confirm && (
        <ConfirmDialog
          title="Remove Source"
          message={`Remove this source?\n\n${source.path}`}
          onConfirm={() => { onRemove(source.path); setConfirm(false); }}
          onCancel={() => setConfirm(false)}
          danger
        />
      )}
    </div>
  );
}
