import { useState, useEffect, useCallback, useMemo, useRef } from 'react';
import { Search, Filter, X } from 'lucide-react';
import { api, WallpaperDTO, CommandResult } from '../api/bridge';
import { measureAsync, recordMetric } from '../perf/metrics';
import WallpaperGrid, { ContextAction } from '../components/WallpaperGrid';
import OpenLocationDialog from '../components/OpenLocationDialog';
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
  const [selectedPaths, setSelectedPaths] = useState<Set<string>>(new Set());
  const [openLocDialog, setOpenLocDialog] = useState<{ path: string; dir: string } | null>(null);

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

  const entryByPath = useMemo(() => new Map(entries.map((entry) => [entry.path, entry])), [entries]);

  const isWeEntry = useCallback((entry: WallpaperDTO) => entry.type === 'we_scene', []);
  const isFailedScene = useCallback((entry: WallpaperDTO) => entry.type === 'we_scene' && entry.backendStatus === 'failed', []);
  const hasProjectFolder = useCallback((entry: WallpaperDTO) => {
    return entry.type === 'we_scene' || entry.type === 'we_web' || entry.type === 'unsupported' || Boolean(entry.workshopId);
  }, []);

  const handleOpenProjectFolder = useCallback(async (entryPath: string) => {
    const entry = entryByPath.get(entryPath);
    if (!entry) return;
    const mode = await api.configGet('open_project_location_mode');
    if (!mode || mode === 'ask') {
      setOpenLocDialog({ path: entryPath, dir: entryPath });
    } else {
      await api.openProjectLocation(entryPath, mode);
    }
  }, [entryByPath]);

  const handleOpenLocSelect = useCallback(async (mode: 'files' | 'terminal') => {
    if (!openLocDialog) return;
    await api.configSet('open_project_location_mode', mode);
    await api.openProjectLocation(openLocDialog.dir, mode);
    setOpenLocDialog(null);
  }, [openLocDialog]);

  const handleBatchAddFavorites = useCallback(async () => {
    if (selectedPaths.size === 0) return;
    const paths = [...selectedPaths];
    let success = 0;
    let fail = 0;
    for (let i = 0; i < paths.length; i += 4) {
      const batch = paths.slice(i, i + 4);
      const results = await Promise.allSettled(batch.map((p) => api.favoriteAdd(p)));
      for (const r of results) {
        if (r.status === 'fulfilled' && r.value.success) success++;
        else fail++;
      }
    }
    invalidateFavoritesCache();
    setSelectedPaths(new Set());
    if (fail === 0) {
      window.dispatchEvent(new CustomEvent('wc-feedback', { detail: { state: 'success', label: 'Batch add', detail: `Added ${success} to favorites.` } }));
    } else {
      window.dispatchEvent(new CustomEvent('wc-feedback', { detail: { state: 'warning', label: 'Batch add', detail: `Added ${success} to favorites. ${fail} failed.` } }));
    }
  }, [selectedPaths]);

  const contextActions: ContextAction[] = useMemo(() => [
    {
      label: 'Apply with linux-wallpaperengine',
      visible: (entry: WallpaperDTO) => isWeEntry(entry) && !isFailedScene(entry),
      action: (path: string) => { onApply(path); },
    },
    {
      label: 'Retry backend apply',
      visible: (entry: WallpaperDTO) => isFailedScene(entry),
      action: async (path: string) => {
        try { await api.weClearBackendError(path); } catch { /* */ }
        onApply(path);
        setTimeout(() => invalidateLibrary(), 500);
      },
    },
    {
      label: 'Apply preview GIF',
      visible: (entry: WallpaperDTO) => Boolean(entry.previewPath) && entry.type !== 'we_web' && entry.type !== 'unsupported',
      action: (path: string) => {
        const entry = entryByPath.get(path);
        const previewPath = entry?.previewPath;
        if (previewPath) onApply(previewPath);
      },
    },
    {
      label: 'Add to Favorites',
      action: async (path: string) => {
        const r = await api.favoriteAdd(path);
        if (!r.success) throw new Error(r.stderr || 'Add to Favorites failed');
        invalidateFavoritesCache();
      },
    },
    {
      label: 'Open Project Folder',
      visible: (entry: WallpaperDTO) => hasProjectFolder(entry),
      action: handleOpenProjectFolder,
    },
    {
      label: 'Copy Workshop ID',
      visible: (entry: WallpaperDTO) => Boolean(entry.workshopId),
      action: async (path: string) => {
        const entry = entryByPath.get(path);
        const workshopId = entry?.workshopId;
        if (workshopId) await navigator.clipboard?.writeText(workshopId);
      },
    },
  ], [onApply, invalidateLibrary, entryByPath, isWeEntry, isFailedScene, hasProjectFolder, handleOpenProjectFolder]);

  return (
    <div className="view library-view">
      <div className="view-header">
        <h2>Library</h2>
        <div className="view-controls">
          {selectedPaths.size > 0 && (
            <>
              <span className="selection-count">{selectedPaths.size} selected</span>
              <button className="btn small" onClick={handleBatchAddFavorites}>
                Add to Favorites
              </button>
              <button className="btn small" onClick={() => setSelectedPaths(new Set())}>
                <X size={14} /> Clear
              </button>
            </>
          )}
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
          entries={entries}
          onApply={onApply}
          applying={applying}
          emptyText="Library is empty. Add sources or scan Wallpaper Engine."
          contextActions={contextActions}
          active={active}
          selectedPaths={selectedPaths}
          onSelectionChange={setSelectedPaths}
        />
      )}
      {!loading && entries.length < total && (
        <div className="load-more">
          <button onClick={() => load(true, entries.length)}>
            Load more ({total - entries.length} remaining)
          </button>
        </div>
      )}

      {openLocDialog && (
        <OpenLocationDialog
          path={openLocDialog.path}
          onSelect={handleOpenLocSelect}
          onClose={() => setOpenLocDialog(null)}
        />
      )}
    </div>
  );
}
