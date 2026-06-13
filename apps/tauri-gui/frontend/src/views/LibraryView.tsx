import { useState, useEffect, useCallback, useMemo, useRef } from 'react';
import { Search, Filter } from 'lucide-react';
import { api, WallpaperDTO } from '../api/bridge';
import { measureAsync, recordMetric } from '../perf/metrics';
import WallpaperGrid, { ContextAction } from '../components/WallpaperGrid';
import { useAppState } from '../state/AppStateContext';
import { invalidateFavoritesCache } from './FavoritesView';

interface Props {
  onApply: (path: string) => void;
  applying: boolean;
  active?: boolean;
}

type FilterType = 'all' | 'image' | 'gif' | 'video' | 'we_scene' | 'we_web' | 'unsupported';
type SortMode = 'name' | 'newest' | 'largest';
const PAGE_SIZE = 120;

export default function LibraryView({ onApply, applying, active = true }: Props) {
  const [entries, setEntries] = useState<WallpaperDTO[]>([]);
  const [loading, setLoading] = useState(true);
  const [search, setSearch] = useState('');
  const [debouncedSearch, setDebouncedSearch] = useState('');
  const [filter, setFilter] = useState<FilterType>('all');
  const [sort, setSort] = useState<SortMode>('newest');
  const [total, setTotal] = useState(0);
  const { libraryVersion, invalidateLibrary } = useAppState();
  const requestSeq = useRef(0);

  useEffect(() => {
    const timer = window.setTimeout(() => setDebouncedSearch(search), 200);
    return () => window.clearTimeout(timer);
  }, [search]);

  const load = useCallback(async (append = false, offset = 0) => {
    const requestId = requestSeq.current + 1;
    requestSeq.current = requestId;
    const isCurrent = () => requestSeq.current === requestId;
    setLoading(true);

    try {
      const page = await measureAsync('library.page.ms', () =>
        api.libraryPage(filter, sort, debouncedSearch, offset, PAGE_SIZE)
      );
      if (!isCurrent()) return;
      recordMetric('library.page.total', page.total);
      setTotal(page.total);
      setEntries((prev) => append ? [...prev, ...(page.items ?? [])] : (page.items ?? []));
    } catch {
      if (!isCurrent()) return;
      setEntries([]);
      setTotal(0);
    } finally {
      if (isCurrent()) {
        setLoading(false);
      }
    }
  }, [debouncedSearch, filter, sort, libraryVersion]);

  useEffect(() => { load(); }, [load]);
  useEffect(() => () => { requestSeq.current += 1; }, []);

  const filtered = useMemo(() => {
    return entries;
  }, [entries]);
  const entryByPath = useMemo(() => new Map(entries.map((entry) => [entry.path, entry])), [entries]);

  const isWeEntry = useCallback((path: string) => {
    const entry = entryByPath.get(path);
    return entry?.type === 'we_scene';
  }, [entryByPath]);

  const isFailedScene = useCallback((path: string) => {
    const entry = entryByPath.get(path);
    return entry?.type === 'we_scene' && entry?.backendStatus === 'failed';
  }, [entryByPath]);

  const contextActions: ContextAction[] = [
    {
      label: 'Apply with linux-wallpaperengine',
      visible: (path: string) => isWeEntry(path) && !isFailedScene(path),
      action: (path: string) => { onApply(path); },
    },
    {
      label: 'Retry backend apply',
      visible: isFailedScene,
      action: async (path: string) => {
        try { await api.weClearBackendError(path); } catch { /* */ }
        onApply(path);
        setTimeout(() => invalidateLibrary(), 500);
      },
    },
    {
      label: 'Apply preview GIF',
      visible: (path: string) => Boolean(entryByPath.get(path)?.previewPath),
      action: (path: string) => {
        const previewPath = entryByPath.get(path)?.previewPath;
        if (previewPath) onApply(previewPath);
      },
    },
    {
      label: 'Add to Favorites',
      action: async (path: string) => { await api.favoriteAdd(path); invalidateFavoritesCache(); },
    },
    {
      label: 'Open Project Folder',
      action: async (path: string) => { await api.revealInFileManager(path); },
    },
    {
      label: 'Copy Workshop ID',
      visible: (path: string) => Boolean(entryByPath.get(path)?.workshopId),
      action: async (path: string) => {
        const workshopId = entryByPath.get(path)?.workshopId;
        if (workshopId) await navigator.clipboard?.writeText(workshopId);
      },
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
            <option value="we_scene">WE Scene</option>
            <option value="we_web">WE Web</option>
            <option value="unsupported">Unsupported</option>
          </select>
          <select value={sort} onChange={(e) => setSort(e.target.value as SortMode)}>
            <option value="newest">Newest</option>
            <option value="largest">Largest</option>
            <option value="name">Name</option>
          </select>
          <span className="library-count">
            {entries.length} / {total}
          </span>
        </div>
      </div>
      {loading ? (
        <div className="loading">Loading library...</div>
      ) : (
        <WallpaperGrid
          entries={filtered}
          onApply={onApply}
          applying={applying}
          emptyText="Library is empty. Add sources or scan Wallpaper Engine."
          contextActions={contextActions}
          active={active}
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
