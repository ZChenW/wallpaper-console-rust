import { memo } from 'react';

import type { ApplyGesture, LibraryViewMode } from '../shell/shellPreferences.ts';
import type { WallpaperCardSize } from '../utils/layout.ts';
import WallpaperFlow from './WallpaperFlow.tsx';
import WallpaperGrid from './WallpaperGrid.tsx';
import {
  instantiateActiveLibraryAdapter,
  type LibraryViewModel,
} from './libraryViewModel.ts';

export interface LibraryViewportProps {
  readonly mode: LibraryViewMode;
  readonly model: LibraryViewModel;
  readonly cardSize: WallpaperCardSize;
  readonly applyGesture: ApplyGesture;
  readonly initialAnchorWallpaperId: number | null;
  readonly focusToken: number;
  readonly onAnchorChange: (wallpaperId: number) => void;
}

function LibraryViewportImpl({
  mode,
  model,
  cardSize,
  applyGesture,
  initialAnchorWallpaperId,
  focusToken,
  onAnchorChange,
}: LibraryViewportProps) {
  const adapter = instantiateActiveLibraryAdapter(mode, {
    flow: () => (
      <WallpaperFlow
        focusToken={focusToken}
        initialAnchorWallpaperId={initialAnchorWallpaperId}
        model={model}
        onAnchorChange={onAnchorChange}
      />
    ),
    grid: () => (
      <WallpaperGrid
        active={model.active}
        activePath={model.activePath}
        applyGesture={applyGesture}
        applying={model.applying}
        buildContextActions={model.buildContextActions}
        cardSize={cardSize}
        currentPath={model.currentPath}
        entries={model.entries}
        setSize={model.totalKnown && model.total !== null ? model.total : undefined}
        favoritePendingPaths={model.favoritePendingPaths}
        focusToken={focusToken}
        hasMore={model.hasMore && !model.automaticAppendPaused && !model.refreshing}
        initialAnchorWallpaperId={initialAnchorWallpaperId}
        isEntryApplicable={model.isEntryApplicable}
        canApplyToDisplay={model.canApplyToDisplay}
        displayApplyDisabledReason={model.displayApplyDisabledReason}
        loadingMore={model.loadingMore}
        onAnchorChange={onAnchorChange}
        onApply={model.onApply}
        onLoadMore={model.onLoadMore}
        onSelect={model.onSelect}
        onToggleFavorite={model.onToggleFavorite}
        pendingPath={model.pendingPath}
        refreshing={model.refreshing}
        resetKey={model.resetKey}
        selectedPath={model.selectedPath}
      />
    ),
  });

  return (
    <div
      aria-busy={model.queryReplacementPending || undefined}
      className="library-viewport"
      data-library-view={mode}
      data-query-updating={model.queryReplacementPending || undefined}
    >
      {model.queryReplacementPending ? (
        <div className="library-viewport__updating" role="status">
          Updating library results…
        </div>
      ) : null}
      {adapter}
    </div>
  );
}

export default memo(LibraryViewportImpl);
