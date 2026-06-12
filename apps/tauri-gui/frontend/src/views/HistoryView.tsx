import { useState, useEffect, useCallback, useRef } from 'react';
import { api, WallpaperDTO } from '../api/bridge';
import WallpaperGrid from '../components/WallpaperGrid';
import ConfirmDialog from '../components/ConfirmDialog';
import { Shuffle, Trash2 } from 'lucide-react';

interface Props {
  onApply: (path: string) => void;
  applying: boolean;
  active?: boolean;
}

const PAGE_SIZE = 120;

export function invalidateHistoryCache() {
  window.dispatchEvent(new CustomEvent('history-cache-invalidated'));
}

export default function HistoryView({ onApply, applying, active = true }: Props) {
  const [entries, setEntries] = useState<WallpaperDTO[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(true);
  const [showClear, setShowClear] = useState(false);
  const requestSeq = useRef(0);

  const load = useCallback(async (offset = 0, append = false) => {
    const seq = ++requestSeq.current;
    setLoading(true);
    try {
      const page = await api.historyPage(offset, PAGE_SIZE);
      if (requestSeq.current !== seq) return;
      setTotal(page.total);
      if (append) {
        setEntries(prev => [...prev, ...(page.items ?? [])]);
      } else {
        setEntries(page.items ?? []);
      }
    } catch {
      if (requestSeq.current !== seq) return;
      if (!append) {
        setEntries([]);
        setTotal(0);
      }
    }
    setLoading(false);
  }, []);

  useEffect(() => { load(); }, [load]);

  useEffect(() => {
    const handler = () => {
      load();
    };
    window.addEventListener('history-cache-invalidated', handler);
    return () => window.removeEventListener('history-cache-invalidated', handler);
  }, [load]);

  const handleClear = async () => {
    await api.historyClear();
    invalidateHistoryCache();
    setShowClear(false);
  };

  const handleRandom = () => {
    if (entries.length === 0) return;
    const pick = entries[Math.floor(Math.random() * entries.length)];
    onApply(pick.path);
  };

  return (
    <div className="view history-view">
      <div className="view-header">
        <h2>History <span className="statusbar-badge" style={{fontSize: 10}}>Newest first</span></h2>
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
          active={active}
        />
      )}
      {!loading && entries.length < total && (
        <div className="load-more">
          <button onClick={() => load(entries.length, true)}>
            Load more ({total - entries.length} remaining)
          </button>
        </div>
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