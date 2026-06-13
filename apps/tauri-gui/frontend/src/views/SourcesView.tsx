import { useState, useEffect, useCallback } from 'react';
import { api, SourceDTO } from '../api/bridge';
import { CommandFeedback, commandErrorFeedback, commandSuccessFeedback } from '../api/feedback';
import ConfirmDialog from '../components/ConfirmDialog';
import { useAppState } from '../state/AppStateContext';
import { FolderPlus, Trash2, CheckCircle, XCircle, MonitorPlay, Loader } from 'lucide-react';

interface Props {
  onRefresh: () => void;
  onFeedback: (fb: CommandFeedback) => void;
}

export default function SourcesView({ onRefresh, onFeedback }: Props) {
  const [sources, setSources] = useState<SourceDTO[]>([]);
  const [loading, setLoading] = useState(true);
  const [adding, setAdding] = useState(false);
  const [scanning, setScanning] = useState(false);
  const { beginScanPolling, finishScanPolling, invalidateLibrary } = useAppState();
  const [removingMissing, setRemovingMissing] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      setSources(await api.sourcesList());
    } catch (e) {
      onFeedback({ state: 'error', label: 'Failed to load sources', detail: String(e) });
      setSources([]);
    }
    setLoading(false);
  }, [onFeedback]);

  useEffect(() => { load(); }, [load]);

  const handleAdd = async () => {
    setAdding(true);
    try {
      const dir = await api.browseDirectory();
      if (!dir) {
        onFeedback({ state: 'error', label: 'No directory selected', detail: 'Install zenity, kdialog, or yad for directory picker.' });
        return;
      }
      const r = await api.sourceAdd(dir);
      if (r.success) {
        onFeedback(commandSuccessFeedback('Source add', r));
        await load();
        onRefresh();
      } else {
        onFeedback(commandErrorFeedback('Add source', r));
      }
    } catch (e) {
      onFeedback(commandErrorFeedback('Add source', e));
    } finally {
      setAdding(false);
    }
  };

  const handleRemove = async (path: string) => {
    try {
      const r = await api.sourceRemove(path);
      if (r.success) {
        onFeedback(commandSuccessFeedback('Source remove', r));
        await load();
        onRefresh();
      } else {
        onFeedback(commandErrorFeedback('Remove source', r));
      }
    } catch (e) {
      onFeedback(commandErrorFeedback('Remove source', e));
    }
  };

  const handleScanWorkshop = async () => {
    setScanning(true);
    beginScanPolling();
    onFeedback({ state: 'running', label: 'Scanning Steam Workshop' });
    try {
      const r = await api.scanSteamWorkshop();
      if (r.success) {
        onFeedback(commandSuccessFeedback('Steam Workshop scan', r));
        invalidateLibrary();
        await load();
        onRefresh();
      } else {
        onFeedback(commandErrorFeedback('Scan', r));
      }
    } catch (e) {
      onFeedback(commandErrorFeedback('Scan', e));
    } finally {
      finishScanPolling(1000);
      setScanning(false);
    }
  };

  const handleRemoveMissing = async () => {
    setRemovingMissing(true);
    onFeedback({ state: 'running', label: 'Removing missing sources' });
    try {
      const r = await api.removeMissingSources();
      if (r.success) {
        onFeedback(commandSuccessFeedback('Remove missing sources', r));
        await load();
        onRefresh();
      } else {
        onFeedback(commandErrorFeedback('Remove missing', r));
      }
    } catch (e) {
      onFeedback(commandErrorFeedback('Remove missing', e));
    } finally {
      setRemovingMissing(false);
    }
  };

  return (
    <div className="view sources-view">
      <div className="view-header">
        <h2>Sources</h2>
        <div className="view-controls">
          <button className="toolbar-btn" onClick={handleAdd} disabled={adding} title="Add source">
            {adding ? <Loader size={16} className="spin" /> : <FolderPlus size={16} />}
          </button>
          <button className="toolbar-btn we-scan-btn" onClick={handleScanWorkshop} disabled={scanning} title="Scan Wallpaper Engine">
            {scanning ? <Loader size={16} className="spin" /> : <MonitorPlay size={16} />}
          </button>
          <button className="toolbar-btn" onClick={handleRemoveMissing} disabled={removingMissing} title="Remove missing sources">
            {removingMissing ? <Loader size={16} className="spin" /> : <Trash2 size={16} />}
          </button>
        </div>
      </div>

      {loading ? (
        <div className="loading">Loading sources...</div>
      ) : sources.length === 0 ? (
        <div className="empty-state">No sources configured — add a directory to start</div>
      ) : (
        <div className="sources-list">
          {sources.map((s) => (
            <SourceItem key={s.path} source={s} onRemove={handleRemove} />
          ))}
        </div>
      )}
    </div>
  );
}

function SourceItem({ source, onRemove }: { source: SourceDTO; onRemove: (p: string) => Promise<void> }) {
  const [confirm, setConfirm] = useState(false);
  const [removing, setRemoving] = useState(false);

  const handleConfirm = async () => {
    setRemoving(true);
    try {
      await onRemove(source.path);
    } finally {
      setRemoving(false);
      setConfirm(false);
    }
  };

  return (
    <div className="source-item">
      <span className="source-status">
        {source.exists ? <CheckCircle size={14} className="text-green" /> : <XCircle size={14} className="text-red" />}
      </span>
      <div className="source-info">
        <span className="source-label">{source.label}</span>
        <span className="source-path" title={source.path}>{source.path}</span>
      </div>
      <button className="source-remove" onClick={() => setConfirm(true)} disabled={removing} title="Remove source">
        {removing ? <Loader size={14} className="spin" /> : <Trash2 size={14} />}
      </button>
      {confirm && (
        <ConfirmDialog
          title="Remove Source"
          message={`Remove this source?\n\n${source.path}`}
          onConfirm={handleConfirm}
          onCancel={() => setConfirm(false)}
          danger
          confirming={removing}
          confirmLabel="Removing..."
        />
      )}
    </div>
  );
}
