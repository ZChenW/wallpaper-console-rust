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
  return instantiateActiveLibraryAdapter(mode, {
    flow: () => (
      <div className="library-viewport" data-library-view="flow">
        <WallpaperFlow
          focusToken={focusToken}
          initialAnchorWallpaperId={initialAnchorWallpaperId}
          model={model}
          onAnchorChange={onAnchorChange}
        />
      </div>
    ),
    grid: () => (
      <div className="library-viewport" data-library-view="grid">
        <WallpaperGrid
          active={model.active}
          activePath={model.activePath}
          applyGesture={applyGesture}
          applying={model.applying}
          buildContextActions={model.buildContextActions}
          cardSize={cardSize}
          currentPath={model.currentPath}
          entries={model.entries}
          favoritePendingPaths={model.favoritePendingPaths}
          focusToken={focusToken}
          hasMore={model.hasMore && !model.automaticAppendPaused && !model.refreshing}
          initialAnchorWallpaperId={initialAnchorWallpaperId}
          isEntryApplicable={model.isEntryApplicable}
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
      </div>
    ),
  });
}

export default memo(LibraryViewportImpl);
