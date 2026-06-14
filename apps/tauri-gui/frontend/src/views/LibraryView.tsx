import { useState, useEffect, useCallback, useMemo, useRef } from 'react';
import { Search, Filter, X } from 'lucide-react';
import { api, WallpaperDTO } from '../api/bridge';
import { normalizeApplyActions } from '../domain/applyActions';
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
  const [openLocDialog, setOpenLocDialog] = useState<{ path: string } | null>(null);

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

  const handleOpenProjectFolder = useCallback(async (entryPath: string) => {
    const entry = entryByPath.get(entryPath);
    if (!entry) return;
    const mode = await api.configGet('open_project_location_mode');
    if (!mode || mode === 'ask') {
      setOpenLocDialog({ path: entryPath });
    } else {
      const r = await api.openProjectLocation(entryPath, mode);
      if (!r.success) {
        window.dispatchEvent(new CustomEvent('wc-feedback', {
          detail: { state: 'error', label: 'Open location', detail: r.stderr || r.error?.message || 'Open location failed' },
        }));
      }
    }
  }, [entryByPath]);

  const handleOpenLocSelect = useCallback(async (mode: 'file_manager' | 'terminal') => {
    if (!openLocDialog) return;
    await api.configSet('open_project_location_mode', mode);
    const r = await api.openProjectLocation(openLocDialog.path, mode);
    setOpenLocDialog(null);
    if (!r.success) {
      window.dispatchEvent(new CustomEvent('wc-feedback', {
        detail: { state: 'error', label: 'Open location', detail: r.stderr || r.error?.message || 'Open location failed' },
      }));
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
      window.dispatchEvent(new CustomEvent('wc-feedback', { detail: { state: 'success', label: 'Batch add', detail: `Added ${success} to favorites.` } }));
    } else {
      window.dispatchEvent(new CustomEvent('wc-feedback', { detail: { state: 'warning', label: 'Batch add', detail: `Added ${success} to favorites. ${fail} failed.` } }));
    }
  }, [selectedPaths]);

  const buildContextActions = useCallback((entry: WallpaperDTO): ContextAction[] => {
    const actions: ContextAction[] = [];

    const normalized = normalizeApplyActions(entry);
    for (const a of normalized) {
      if (!a.enabled) continue;

      switch (a.kind) {
        case 'apply':
          actions.push({
            label: a.label,
            action: (path: string) => { onApply(path); },
          });
          break;
        case 'retry_backend_apply':
          actions.push({
            label: a.label,
            action: async (path: string) => {
              let clearOk = true;
              try { await api.weClearBackendError(path); } catch {
                clearOk = false;
                window.dispatchEvent(new CustomEvent('wc-feedback', {
                  detail: { state: 'error', label: 'Clear backend error', detail: 'Failed to clear backend error before retry.' },
                }));
              }
              onApply(path);
              if (clearOk) setTimeout(() => invalidateLibrary(), 500);
            },
          });
          break;
        case 'apply_preview':
          if (entry.previewPath) {
            actions.push({
              label: a.label,
              action: (path: string) => {
                const e = entryByPath.get(path);
                if (e?.previewPath) onApply(e.previewPath);
              },
            });
          }
          break;
        case 'open_folder':
          actions.push({
            label: a.label,
            action: handleOpenProjectFolder,
          });
          break;
        case 'copy_workshop_id':
          if (entry.workshopId) {
            actions.push({
              label: a.label,
              action: async (path: string) => {
                const e = entryByPath.get(path);
                if (e?.workshopId) {
                  try {
                    await navigator.clipboard?.writeText(e.workshopId);
                  } catch {
                    window.dispatchEvent(new CustomEvent('wc-feedback', {
                      detail: { state: 'error', label: 'Copy Workshop ID', detail: 'Clipboard write failed' },
                    }));
                  }
                }
              },
            });
          }
          break;
      }
    }

    actions.push({
      label: 'Add to Favorites',
      action: async (path: string) => {
        const r = await api.favoriteAdd(path);
        if (!r.success) throw new Error(r.stderr || 'Add to Favorites failed');
        invalidateFavoritesCache();
      },
    });

    return actions;
  }, [onApply, invalidateLibrary, entryByPath, handleOpenProjectFolder]);

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
