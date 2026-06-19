import { useState, useEffect, useCallback, useRef } from 'react';
import { Search } from 'lucide-react';
import { api, WallpaperDTO, ApplyRequestDTO } from '../api/bridge';
import { measureAsync, recordMetric } from '../perf/metrics';
import WallpaperGrid from '../components/WallpaperGrid';
import OpenLocationDialog from '../components/OpenLocationDialog';
import { useLibraryEntryActions } from '../hooks/useLibraryEntryActions';
import { usePagedWallpapers, type WallpaperPageDTO } from '../hooks/usePagedWallpapers';
import { useAppState } from '../state/AppStateContext';
import { invalidateFavoritesCache } from './FavoritesView';
import { emitFeedback } from '../events/appEvents';
import { resolveLibraryDisplay } from './libraryDisplay';

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
  const { libraryVersion, invalidateLibrary, scanProgress } = useAppState();
  const [openLocDialog, setOpenLocDialog] = useState<{ path: string } | null>(null);
  const mountTimeRef = useRef<number | null>(null);
  const firstPageRecordedRef = useRef(false);
  const firstContentRecordedRef = useRef(false);
  const prevDisplayRef = useRef<string>('loading');

  if (mountTimeRef.current === null) {
    mountTimeRef.current = performance.now();
  }

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
    if (!firstPageRecordedRef.current) {
      firstPageRecordedRef.current = true;
      if (mountTimeRef.current !== null) {
        recordMetric('library.firstPage.ms', performance.now() - mountTimeRef.current);
      }
    }
  }, []);

  const {
    entries,
    total,
    initialLoading,
    refreshing,
    hasLoadedOnce,
    loadMore,
    entryByPath,
  } = usePagedWallpapers({
    pageSize: PAGE_SIZE,
    loadPage,
    onPage: handlePage,
  });

  const display = resolveLibraryDisplay({
    initialLoading,
    hasLoadedOnce,
    total,
    entryCount: entries.length,
    scanRunning: scanProgress?.running ?? false,
  });

  useEffect(() => {
    if (!firstContentRecordedRef.current && entries.length > 0) {
      firstContentRecordedRef.current = true;
      if (mountTimeRef.current !== null) {
        recordMetric('library.firstContent.ms', performance.now() - mountTimeRef.current);
      }
    }
  }, [entries.length]);

  useEffect(() => {
    if (prevDisplayRef.current === 'empty' && display === 'grid') {
      recordMetric('library.emptyFlash.count', 1);
    }
    prevDisplayRef.current = display;
  }, [display]);

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
      {display === 'grid' ? (
        <>
          <WallpaperGrid
            entries={entries}
            onApply={onApply}
            applying={applying}
            emptyText="Library is empty. Add sources or scan Wallpaper Engine."
            buildContextActions={buildContextActions}
            active={active}
            refreshing={refreshing}
            resetKey={`${filter}|${sort}|${debouncedSearch}`}
          />
          {!refreshing && entries.length < total && (
            <div className="load-more">
              <button onClick={() => void loadMore()}>
                Load more ({total - entries.length} remaining)
              </button>
            </div>
          )}
        </>
      ) : display === 'empty' ? (
        <div className="empty-state">Library is empty. Add sources or scan Wallpaper Engine.</div>
      ) : display === 'indexing' ? (
        <div className="loading">Indexing library…</div>
      ) : (
        <div className="loading">Loading library...</div>
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
