import { useCallback } from 'react';
import { api, WallpaperDTO, ApplyRequestDTO } from '../api/bridge';
import { isApplyAvailable } from '../domain/applyActions';
import WallpaperGrid from '../components/WallpaperGrid';
import { Shuffle } from 'lucide-react';
import { useLibraryEntryActions } from '../hooks/useLibraryEntryActions';
import { usePagedWallpapers } from '../hooks/usePagedWallpapers';
import { APP_EVENTS, emitFavoritesInvalidated, emitFeedback } from '../events/appEvents';

interface Props {
  onApply: (path: string) => void;
  onApplyAction: (request: ApplyRequestDTO) => void;
  applying: boolean;
  active?: boolean;
}

const PAGE_SIZE = 120;

export function invalidateFavoritesCache() {
  emitFavoritesInvalidated();
}

export default function FavoritesView({ onApply, onApplyAction, applying, active = true }: Props) {
  const loadPage = useCallback((offset: number, limit: number) => {
    return api.favoritesPage(offset, limit);
  }, []);

  const {
    entries,
    total,
    loading,
    reload,
    loadMore,
    entryByPath,
  } = usePagedWallpapers({
    pageSize: PAGE_SIZE,
    loadPage,
    refreshEvent: APP_EVENTS.favoritesInvalidated,
  });

  const { buildContextActions: buildBaseActions } = useLibraryEntryActions({
    onApplyAction,
    invalidate: () => { void reload(); },
    openFolder: async (path: string) => { await api.revealInFileManager(path); },
    findEntry: (path) => entryByPath.get(path),
  });

  const buildContextActions = useCallback((entry: WallpaperDTO) => {
    const actions = buildBaseActions(entry);
    actions.push({
      label: 'Remove from Favorites',
      action: async (path: string) => {
        await api.favoriteRemove(path);
        await reload();
      },
      danger: true,
    });
    return actions;
  }, [buildBaseActions, reload]);

  const handleRandom = () => {
    if (entries.length === 0) return;
    const applicable = entries.filter(e => isApplyAvailable(e));
    if (applicable.length === 0) {
      emitFeedback({ state: 'warning', label: 'No applicable', detail: 'No items in favorites can be applied as a live wallpaper.' });
      return;
    }
    const pick = applicable[Math.floor(Math.random() * applicable.length)];
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
          buildContextActions={buildContextActions}
          active={active}
        />
      )}
      {!loading && entries.length < total && (
        <div className="load-more">
          <button onClick={() => void loadMore()}>
            Load more ({total - entries.length} remaining)
          </button>
        </div>
      )}
    </div>
  );
}
