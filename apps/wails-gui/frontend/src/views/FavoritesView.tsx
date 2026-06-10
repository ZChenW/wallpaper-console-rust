import { useState, useEffect, useCallback } from 'react';
import { api, WallpaperDTO } from '../api/bridge';
import WallpaperGrid, { ContextAction } from '../components/WallpaperGrid';
import { Shuffle } from 'lucide-react';

interface Props {
  onApply: (path: string) => void;
  applying: boolean;
}

export default function FavoritesView({ onApply, applying }: Props) {
  const [entries, setEntries] = useState<WallpaperDTO[]>([]);
  const [loading, setLoading] = useState(true);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const paths = await api.favoritesList();
      // Build lightweight entries from paths
      const mapped: WallpaperDTO[] = paths.map((p) => ({
        path: p,
        type: inferType(p),
        ext: p.split('.').pop() ?? '',
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
