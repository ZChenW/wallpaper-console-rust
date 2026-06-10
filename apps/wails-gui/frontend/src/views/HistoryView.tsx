import { useState, useEffect, useCallback } from 'react';
import { api, HistoryDTO, WallpaperDTO } from '../api/bridge';
import WallpaperGrid from '../components/WallpaperGrid';
import ConfirmDialog from '../components/ConfirmDialog';
import { Shuffle, Trash2 } from 'lucide-react';

interface Props {
  onApply: (path: string) => void;
  applying: boolean;
}

export default function HistoryView({ onApply, applying }: Props) {
  const [entries, setEntries] = useState<WallpaperDTO[]>([]);
  const [loading, setLoading] = useState(true);
  const [showClear, setShowClear] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const hist: HistoryDTO[] = await api.historyList();
      const mapped: WallpaperDTO[] = hist.map((h) => ({
        path: h.path,
        type: inferType(h.path),
        ext: h.path.split('.').pop() ?? '',
        backend: '',
        size: 0,
        mtime: 0,
        resolution: '',
      }));
      setEntries(mapped);
    } catch {
      setEntries([]);
    }
    setLoading(false);
  }, []);

  useEffect(() => { load(); }, [load]);

  const handleClear = async () => {
    await api.historyClear();
    setShowClear(false);
    load();
  };

  const handleRandom = () => {
    if (entries.length === 0) return;
    const pick = entries[Math.floor(Math.random() * entries.length)];
    onApply(pick.path);
  };

  return (
    <div className="view history-view">
      <div className="view-header">
        <h2>History</h2>
        <div className="view-controls">
          <button className="toolbar-btn" onClick={handleRandom} title="Random from history">
            <Shuffle size={16} />
          </button>
          <button className="toolbar-btn danger" onClick={() => setShowClear(true)} title="Clear history">
            <Trash2 size={16} />
          </button>
        </div>
      </div>
      {loading ? (
        <div className="loading">Loading history...</div>
      ) : (
        <WallpaperGrid
          entries={entries}
          onApply={onApply}
          applying={applying}
          emptyText="No history yet — apply a wallpaper to start"
        />
      )}
      {showClear && (
        <ConfirmDialog
          title="Clear History"
          message="Remove all history entries? This cannot be undone."
          onConfirm={handleClear}
          onCancel={() => setShowClear(false)}
          danger
        />
      )}
    </div>
  );
}

function inferType(path: string): 'image' | 'gif' | 'video' {
  const ext = (path.split('.').pop() ?? '').toLowerCase();
  if (ext === 'mp4' || ext === 'webm' || ext === 'mkv' || ext === 'mov') return 'video';
  if (ext === 'gif') return 'gif';
  return 'image';
}
