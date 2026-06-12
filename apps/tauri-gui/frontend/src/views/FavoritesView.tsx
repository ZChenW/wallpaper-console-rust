import { useState, useEffect, useCallback, useRef } from 'react';
import { api, WallpaperDTO } from '../api/bridge';
import WallpaperGrid, { ContextAction } from '../components/WallpaperGrid';
import { Shuffle } from 'lucide-react';

interface Props {
  onApply: (path: string) => void;
  applying: boolean;
  active?: boolean;
}

const PAGE_SIZE = 120;

export function invalidateFavoritesCache() {
  window.dispatchEvent(new CustomEvent('favorites-cache-invalidated'));
}

export default function FavoritesView({ onApply, applying, active = true }: Props) {
  const [entries, setEntries] = useState<WallpaperDTO[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(true);
  const requestSeq = useRef(0);

  const load = useCallback(async (offset = 0, append = false) => {
    const seq = ++requestSeq.current;
    setLoading(true);
    try {
      const page = await api.favoritesPage(offset, PAGE_SIZE);
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
    const handler = () => { load(); };
    window.addEventListener('favorites-cache-invalidated', handler);
    return () => window.removeEventListener('favorites-cache-invalidated', handler);
  }, [load]);

  const contextActions: ContextAction[] = [
    {
      label: 'Remove from Favorites',
      action: async (path: string) => {
        await api.favoriteRemove(path);
        load();
      },
      danger: true,
    },
    {
      label: 'Open Containing Folder',
      action: async (path: string) => { await api.revealInFileManager(path); },
    },
  ];

  const handleRandom = async () => {
    if (entries.length === 0) return;
    const pick = entries[Math.floor(Math.random() * entries.length)];
    onApply(pick.path);
  };

  return (
    <div className="view favorites-view">
      <div className="view-header">
        <h2>Favorites</h2>
        <div className="view-controls">
          <button className="toolbar-btn" onClick={handleRandom} title="Random favorite">
            <Shuffle size={16} />
          </button>
        </div>
      </div>
      {loading ? (
        <div className="loading">Loading favorites...</div>
      ) : (
        <WallpaperGrid
          entries={entries}
          onApply={onApply}
          applying={applying}
          emptyText="No favorites yet — right-click a wallpaper in Library to add"
          contextActions={contextActions}
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
    </div>
  );
}