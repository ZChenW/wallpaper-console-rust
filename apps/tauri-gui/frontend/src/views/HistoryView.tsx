import { useState, useCallback } from 'react';
import { api, WallpaperDTO, ApplyRequestDTO } from '../api/bridge';
import { isApplyAvailable } from '../domain/applyActions';
import WallpaperGrid from '../components/WallpaperGrid';
import ConfirmDialog from '../components/ConfirmDialog';
import { Shuffle, Trash2 } from 'lucide-react';
import { useLibraryEntryActions } from '../hooks/useLibraryEntryActions';
import { usePagedWallpapers } from '../hooks/usePagedWallpapers';
import { APP_EVENTS, emitFeedback, emitHistoryInvalidated } from '../events/appEvents';

interface Props {
  onApply: (path: string) => void;
  onApplyAction: (request: ApplyRequestDTO) => void;
  applying: boolean;
  active?: boolean;
}

const PAGE_SIZE = 120;

export function invalidateHistoryCache() {
  emitHistoryInvalidated();
}

export default function HistoryView({ onApply, onApplyAction, applying, active = true }: Props) {
  const [showClear, setShowClear] = useState(false);

  const loadPage = useCallback((offset: number, limit: number) => {
    return api.historyPage(offset, limit);
  }, []);

  const {
    entries,
    total,
    loading,
    reload,
    loadMore,
    entryByPath,
    replaceCount,
  } = usePagedWallpapers({
    pageSize: PAGE_SIZE,
    loadPage,
    refreshEvent: APP_EVENTS.historyInvalidated,
  });

  const handleClear = async () => {
    await api.historyClear();
    invalidateHistoryCache();
    setShowClear(false);
  };

  const { buildContextActions } = useLibraryEntryActions({
    onApplyAction,
    invalidate: () => { void reload(); },
    openFolder: async (path: string) => { await api.revealInFileManager(path); },
    findEntry: (path) => entryByPath.get(path),
  });

  const handleRandom = () => {
    if (entries.length === 0) return;
    const applicable = entries.filter(e => isApplyAvailable(e));
    if (applicable.length === 0) {
      emitFeedback({ state: 'warning', label: 'No applicable', detail: 'No items in history can be applied as a live wallpaper.' });
      return;
    }
    const pick = applicable[Math.floor(Math.random() * applicable.length)];
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
          buildContextActions={buildContextActions}
          active={active}
          resetKey={String(replaceCount)}
        />
      )}
      {!loading && entries.length < total && (
        <div className="load-more">
          <button onClick={() => void loadMore()}>
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
