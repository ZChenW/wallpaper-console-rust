import { useState, useEffect, useCallback } from 'react';
import { Search, Filter, X } from 'lucide-react';
import { api, WallpaperDTO, ApplyRequestDTO } from '../api/bridge';
import { measureAsync, recordMetric } from '../perf/metrics';
import WallpaperGrid from '../components/WallpaperGrid';
import OpenLocationDialog from '../components/OpenLocationDialog';
import { useLibraryEntryActions } from '../hooks/useLibraryEntryActions';
import { usePagedWallpapers, type WallpaperPageDTO } from '../hooks/usePagedWallpapers';
import { useAppState } from '../state/AppStateContext';
import { invalidateFavoritesCache } from './FavoritesView';
import { emitFeedback } from '../events/appEvents';

interface Props {
  onApply: (path: string) => void;
  onApplyAction: (request: ApplyRequestDTO) => void;
  applying: boolean;
  active?: boolean;
}

type FilterType = 'all' | 'image' | 'gif' | 'video' | 'we_scene' | 'we_web' | 'unsupported';
type SortMode = 'name' | 'newest' | 'largest';
const PAGE_SIZE = 120;

export default function LibraryView({ onApply, onApplyAction, applying, active = true }: Props) {
  const [search, setSearch] = useState('');
  const [debouncedSearch, setDebouncedSearch] = useState('');
  const [filter, setFilter] = useState<FilterType>('all');
  const [sort, setSort] = useState<SortMode>('newest');
  const { libraryVersion, invalidateLibrary } = useAppState();
  const [selectedPaths, setSelectedPaths] = useState<Set<string>>(new Set());
  const [openLocDialog, setOpenLocDialog] = useState<{ path: string } | null>(null);

  useEffect(() => {
    const timer = window.setTimeout(() => setDebouncedSearch(search), 200);
    return () => window.clearTimeout(timer);
  }, [search]);

  const loadPage = useCallback((offset: number, limit: number) => {
    return measureAsync('library.page.ms', () =>
      api.libraryPage(filter, sort, debouncedSearch, offset, limit)
    );
  }, [debouncedSearch, filter, sort, libraryVersion]);

  const handlePage = useCallback((page: WallpaperPageDTO) => {
    recordMetric('library.page.total', page.total);
  }, []);

  const {
    entries,
    total,
    loading,
    loadMore,
    entryByPath,
  } = usePagedWallpapers({
    pageSize: PAGE_SIZE,
    loadPage,
    onPage: handlePage,
  });

  const handleOpenProjectFolder = useCallback(async (entryPath: string) => {
    const entry = entryByPath.get(entryPath);
    if (!entry) return;
    const mode = await api.configGet('open_project_location_mode');
    if (!mode || mode === 'ask') {
      setOpenLocDialog({ path: entryPath });
    } else {
      const r = await api.openProjectLocation(entryPath, mode);
      if (!r.success) {
        emitFeedback({ state: 'error', label: 'Open location', detail: r.stderr || r.error?.message || 'Open location failed' });
      }
    }
  }, [entryByPath]);

  const handleOpenLocSelect = useCallback(async (mode: 'file_manager' | 'terminal') => {
    if (!openLocDialog) return;
    await api.configSet('open_project_location_mode', mode);
    const r = await api.openProjectLocation(openLocDialog.path, mode);
    setOpenLocDialog(null);
    if (!r.success) {
      emitFeedback({ state: 'error', label: 'Open location', detail: r.stderr || r.error?.message || 'Open location failed' });
    }
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
      emitFeedback({ state: 'success', label: 'Batch add', detail: `Added ${success} to favorites.` });
    } else {
      emitFeedback({ state: 'warning', label: 'Batch add', detail: `Added ${success} to favorites. ${fail} failed.` });
    }
  }, [selectedPaths]);

  const { buildContextActions: buildBaseActions } = useLibraryEntryActions({
    onApplyAction,
    invalidate: () => invalidateLibrary(),
    openFolder: handleOpenProjectFolder,
    findEntry: (path) => entryByPath.get(path),
  });

  const buildContextActions = useCallback((entry: WallpaperDTO) => {
    const actions = buildBaseActions(entry);
    actions.push({
      label: 'Add to Favorites',
      action: async (path: string) => {
        const r = await api.favoriteAdd(path);
        if (!r.success) throw new Error(r.stderr || 'Add to Favorites failed');
        invalidateFavoritesCache();
      },
    });
    return actions;
  }, [buildBaseActions]);

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
          buildContextActions={buildContextActions}
          active={active}
          selectedPaths={selectedPaths}
          onSelectionChange={setSelectedPaths}
        />
      )}
      {!loading && entries.length < total && (
        <div className="load-more">
          <button onClick={() => void loadMore()}>
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
