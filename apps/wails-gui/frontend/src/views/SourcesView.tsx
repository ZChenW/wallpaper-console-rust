import { useState, useEffect, useCallback } from 'react';
import { api, SourceDTO } from '../api/bridge';
import ConfirmDialog from '../components/ConfirmDialog';
import { FolderPlus, Trash2, CheckCircle, XCircle, Scan, Plus } from 'lucide-react';

interface Props {
  onRefresh: () => void;
}

export default function SourcesView({ onRefresh }: Props) {
  const [sources, setSources] = useState<SourceDTO[]>([]);
  const [loading, setLoading] = useState(true);
  const [adding, setAdding] = useState(false);
  const [scanning, setScanning] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      setSources(await api.sourcesList());
    } catch {
      setSources([]);
    }
    setLoading(false);
  }, []);

  useEffect(() => { load(); }, [load]);

  const handleAdd = async () => {
    setAdding(true);
    try {
      const dir = await api.browseDirectory();
      if (dir) {
        await api.sourceAdd(dir);
        await load();
        onRefresh();
      }
    } catch { /* cancelled */ }
    setAdding(false);
  };

  const handleRemove = async (path: string) => {
    await api.sourceRemove(path);
    await load();
    onRefresh();
  };

  const handleScanWorkshop = async () => {
    setScanning(true);
    await api.scanSteamWorkshop();
    await load();
    onRefresh();
    setScanning(false);
  };

  const handleRemoveMissing = async () => {
    await api.removeMissingSources();
    await load();
    onRefresh();
  };

  // Group sources: Wallpaper Engine vs Other
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
  return (
    <div className="source-item">
      <span className="source-status">
        {source.exists ? <CheckCircle size={14} className="text-green" /> : <XCircle size={14} className="text-red" />}
      </span>
      <div className="source-info">
        <span className="source-label">{source.label}</span>
        <span className="source-path" title={source.path}>{source.path}</span>
      </div>
      <button className="source-remove" onClick={() => onRemove(source.path)} title="Remove source">
        <Trash2 size={14} />
      </button>
    </div>
  );
}
