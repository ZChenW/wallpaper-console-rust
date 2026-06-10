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
const PAGE_SIZE = 120;

export default function LibraryView({ onApply, applying }: Props) {
  const [entries, setEntries] = useState<WallpaperDTO[]>([]);
  const [loading, setLoading] = useState(true);
  const [search, setSearch] = useState('');
  const [debouncedSearch, setDebouncedSearch] = useState('');
  const [filter, setFilter] = useState<FilterType>('all');
  const [sort, setSort] = useState<SortMode>('newest');
  const [source, setSource] = useState('tsv');
  const [total, setTotal] = useState(0);
  const [stale, setStale] = useState(false);

  useEffect(() => {
    const timer = window.setTimeout(() => setDebouncedSearch(search), 200);
    return () => window.clearTimeout(timer);
  }, [search]);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const [configuredSource, storageBackend] = await Promise.all([
          api.configGet('gui_library_source'),
          api.configGet('storage_backend'),
        ]);
        if (cancelled) return;
        if (configuredSource === 'sqlite' || configuredSource === 'tsv') {
          setSource(configuredSource);
        } else if (storageBackend === 'sqlite') {
          setSource('sqlite');
        }
      } catch {
        // Keep the default source.
      }
    })();
    return () => { cancelled = true; };
  }, []);

  const load = useCallback(async (append = false, offset = 0) => {
    setLoading(true);
    try {
      const page = await api.libraryPage(source, filter, sort, debouncedSearch, offset, PAGE_SIZE);
      setTotal(page.total);
      setEntries((prev) => append ? [...prev, ...(page.items ?? [])] : (page.items ?? []));
      setStale(false);
    } catch {
      // Try fallback to the other source
      try {
        const fallback = source === 'sqlite' ? 'tsv' : 'sqlite';
        const page = await api.libraryPage(fallback, filter, sort, debouncedSearch, 0, PAGE_SIZE);
        setTotal(page.total);
        setEntries(page.items ?? []);
      } catch {
        setEntries([]);
        setTotal(0);
      }
    }
    setLoading(false);
  }, [debouncedSearch, filter, sort, source]);

  useEffect(() => { load(); }, [load]);

  const filtered = useMemo(() => {
    return entries;
  }, [entries]);

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
          <span className="library-count">
            {entries.length} / {total}
          </span>
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
      {!loading && entries.length < total && (
        <div className="load-more">
          <button onClick={() => load(true, entries.length)}>
            Load more ({total - entries.length} remaining)
          </button>
        </div>
      )}
    </div>
  );
}
