import { useState, useEffect, useCallback, useMemo } from 'react';
import { Search, Filter } from 'lucide-react';
import { api, WallpaperDTO } from '../api/bridge';
import WallpaperGrid, { ContextAction } from '../components/WallpaperGrid';

interface Props {
  onApply: (path: string) => void;
  applying: boolean;
}

type FilterType = 'all' | 'image' | 'gif' | 'video';
type SortMode = 'name' | 'newest' | 'largest';

export default function LibraryView({ onApply, applying }: Props) {
  const [entries, setEntries] = useState<WallpaperDTO[]>([]);
  const [loading, setLoading] = useState(true);
  const [search, setSearch] = useState('');
  const [filter, setFilter] = useState<FilterType>('all');
  const [sort, setSort] = useState<SortMode>('newest');
  const [source, setSource] = useState('tsv');
  const [stale, setStale] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const data = await api.libraryList(source);
      setEntries(data);
      setStale(false);
    } catch {
      // Try fallback to the other source
      try {
        const fallback = source === 'sqlite' ? 'tsv' : 'sqlite';
        const data = await api.libraryList(fallback);
        setEntries(data);
      } catch {
        setEntries([]);
      }
    }
    setLoading(false);
  }, [source]);

  useEffect(() => { load(); }, [load]);

  const filtered = useMemo(() => {
    let list = entries;
    if (filter !== 'all') list = list.filter((e) => e.type === filter);
    if (search) {
      const q = search.toLowerCase();
      list = list.filter((e) => e.path.toLowerCase().includes(q));
    }
    switch (sort) {
      case 'newest': list = [...list].sort((a, b) => b.mtime - a.mtime); break;
      case 'largest': list = [...list].sort((a, b) => b.size - a.size); break;
      case 'name': list = [...list].sort((a, b) => a.path.localeCompare(b.path)); break;
    }
    return list;
  }, [entries, filter, search, sort]);

  const contextActions: ContextAction[] = [
    {
      label: 'Add to Favorites',
      action: async (path: string) => { await api.favoriteAdd(path); },
    },
    {
      label: 'Open Containing Folder',
      action: async (path: string) => { await api.revealInFileManager(path); },
    },
  ];

  return (
    <div className="view library-view">
      <div className="view-header">
        <h2>Library</h2>
        <div className="view-controls">
          <div className="search-box">
            <Search size={14} />
            <input
              type="text"
              placeholder="Search by filename..."
              value={search}
              onChange={(e) => setSearch(e.target.value)}
            />
          </div>
          <select value={filter} onChange={(e) => setFilter(e.target.value as FilterType)}>
            <option value="all">All</option>
            <option value="image">Images</option>
            <option value="gif">GIFs</option>
            <option value="video">Videos</option>
          </select>
          <select value={sort} onChange={(e) => setSort(e.target.value as SortMode)}>
            <option value="newest">Newest</option>
            <option value="largest">Largest</option>
            <option value="name">Name</option>
          </select>
          <select value={source} onChange={(e) => setSource(e.target.value)}>
            <option value="tsv">TSV</option>
            <option value="sqlite">SQLite</option>
          </select>
          {stale && <span className="stale-badge">Library stale — rescan</span>}
        </div>
      </div>
      {loading ? (
        <div className="loading">Loading library...</div>
      ) : (
        <WallpaperGrid
          entries={filtered}
          onApply={onApply}
          applying={applying}
          emptyText="Library empty — add sources and rescan"
          contextActions={contextActions}
        />
      )}
    </div>
  );
}
