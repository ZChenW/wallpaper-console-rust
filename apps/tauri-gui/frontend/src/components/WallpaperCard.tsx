import { memo, useCallback, useState, useSyncExternalStore, type CSSProperties } from 'react';
import { convertFileSrc } from '@tauri-apps/api/core';
import type { LibraryBrowserItemDTO, WallpaperDTO } from '../api/bridge';
import { isApplyAvailable } from '../domain/applyActions';
import { emitFeedback } from '../events/appEvents';
import { BoundedFileSrcCache } from './fileSrcCache';
import { useThumbnailStore } from '../state/ThumbnailStoreContext';
import { wallpaperCardMetrics, type WallpaperCardSize } from '../utils/layout';
import {
  displayName,
  cardHoverLabel,
  formatSize,
  metaLine,
  typeIcon,
  weBadge,
  weBadgeClass,
} from './wallpaperCardHelpers';
import {
  cardInteractionClassName,
  resolveCardKeyboardInteraction,
  resolveCardPointerInteraction,
} from '../shell/cardInteraction';
import type { ApplyGesture } from '../shell/shellPreferences';
import {
  animatedPreviewPath,
  previewAssetPath,
  shouldStartAnimatedHover,
} from './wallpaperGridHelpers';

const fileSrcCache = new BoundedFileSrcCache((path: string): string => {
  try {
    return convertFileSrc(path);
  } catch {
    return path;
  }
});

export function safeFileSrc(path: string): string {
  return fileSrcCache.get(path);
}

interface CardProps {
  entry: LibraryBrowserItemDTO;
  applying: boolean;
  onApply: (path: string) => void;
  onSelect?: (entry: WallpaperDTO) => void;
  onToggleFavorite: (entry: LibraryBrowserItemDTO) => void;
  onContextMenu: (e: React.MouseEvent, path: string) => void;
  onKeyboardContextMenu?: (path: string, x: number, y: number) => void;
  cardSize?: WallpaperCardSize;
  applyGesture?: ApplyGesture;
  selected?: boolean;
  pending?: boolean;
  favoritePending?: boolean;
  current?: boolean;
  applyAvailable?: boolean;
  isScrolling?: () => boolean;
}

const neverScrolling = () => false;

function WallpaperCardImpl({
  entry,
  applying,
  onApply,
  onSelect,
  onToggleFavorite,
  onContextMenu,
  onKeyboardContextMenu,
  cardSize = 'medium',
  applyGesture = 'single',
  selected = false,
  pending = false,
  favoritePending = false,
  current = false,
  applyAvailable,
  isScrolling = neverScrolling,
}: CardProps) {
  const store = useThumbnailStore();
  const [hovered, setHovered] = useState(false);
  const thumbnailPath = previewAssetPath(entry);
  const subscribeThumbnail = useCallback(
    (callback: () => void) => store.subscribe(thumbnailPath, callback),
    [store, thumbnailPath],
  );
  const getThumbnail = useCallback(
    () => store.get(thumbnailPath),
    [store, thumbnailPath],
  );
  const getThumbnailFailure = useCallback(
    () => store.getFailure(thumbnailPath),
    [store, thumbnailPath],
  );
  const thumbnail = useSyncExternalStore(
    subscribeThumbnail,
    getThumbnail,
  );
  const thumbnailFailure = useSyncExternalStore(
    subscribeThumbnail,
    getThumbnailFailure,
  );
  const animatedPreview = animatedPreviewPath(entry, hovered, isScrolling());

  const handleClick = (event: React.MouseEvent<HTMLDivElement>) => {
    const target = event.target;
    const control = target instanceof Element
      ? target.closest('button, a, input, select, textarea, [role="menuitem"], [data-card-control]')
      : null;
    const fromControl = control !== null
      && control !== event.currentTarget
      && event.currentTarget.contains(control);
    const canApply = applyAvailable ?? isApplyAvailable(entry);
    const interaction = resolveCardPointerInteraction({
      gesture: applyGesture,
      clickCount: event.detail,
      canApply,
      fromControl,
    });
    if (interaction.select) onSelect?.(entry);
    if (interaction.apply) onApply(entry.path);

    const attemptedUnsupportedApply = !canApply
      && interaction.select
      && (
        (applyGesture === 'single' && event.detail === 1)
        || (applyGesture === 'double' && event.detail === 2)
      );
    if (attemptedUnsupportedApply) {
      emitFeedback({
        state: 'warning',
        label: 'Cannot apply',
        detail: entry.applyReason || entry.unsupportedReason || 'This item cannot be applied as a live wallpaper.',
      });
    }
  };

  const canApply = applyAvailable ?? isApplyAvailable(entry);
  const reportUnsupportedApply = () => {
    emitFeedback({
      state: 'warning',
      label: 'Cannot apply',
      detail: entry.applyReason || entry.unsupportedReason || 'This item cannot be applied as a live wallpaper.',
    });
  };

  const handleKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    const interaction = resolveCardKeyboardInteraction({
      key: event.key,
      shiftKey: event.shiftKey,
      canApply,
    });
    if (!interaction.select && !interaction.apply && !interaction.contextMenu) return;
    event.preventDefault();
    if (interaction.contextMenu) {
      const rect = event.currentTarget.getBoundingClientRect();
      onKeyboardContextMenu?.(entry.path, rect.left + 12, rect.top + 12);
      return;
    }
    if (interaction.select) onSelect?.(entry);
    if (interaction.apply) onApply(entry.path);
    else if (interaction.select && !canApply) reportUnsupportedApply();
  };

  const badge = weBadge(entry);
  const cardStyle = {
    '--wallpaper-thumbnail-height': `${wallpaperCardMetrics(cardSize).thumbnailHeight}px`,
  } as CSSProperties;

  return (
    <div
      className={`${cardInteractionClassName({ selected, pending, current })}${applying ? ' applying' : ''}`}
      style={cardStyle}
      onContextMenu={(ev) => onContextMenu(ev, entry.path)}
      onClick={handleClick}
      onKeyDown={handleKeyDown}
      onMouseEnter={() => setHovered(shouldStartAnimatedHover(isScrolling()))}
      onMouseLeave={() => setHovered(false)}
      aria-current={current ? 'true' : undefined}
      aria-selected={selected}
      data-pending={pending || undefined}
      data-wallpaper-path={entry.path}
      role="option"
      tabIndex={0}
      title={cardHoverLabel(entry)}
    >
      <div className="wallpaper-thumb">
        {animatedPreview || thumbnail ? (
          <img
            src={safeFileSrc(animatedPreview ?? thumbnail ?? '')}
            alt=""
            loading="lazy"
            decoding="async"
          />
        ) : (
          <div className="wallpaper-thumb-placeholder" title={thumbnailFailure ? `Preview failed: ${thumbnailFailure}` : undefined}>
            <span className="wallpaper-type-icon">{typeIcon(entry.type)}</span>
            {thumbnailFailure ? <span className="wallpaper-thumb-error">Preview failed</span> : null}
          </div>
        )}
        {badge && <span className={weBadgeClass(entry)}>{badge}</span>}
        <button
          aria-label={entry.favorite ? 'Remove favorite' : 'Add favorite'}
          className={`wallpaper-favorite-button${entry.favorite ? ' is-favorite' : ''}`}
          data-card-control
          disabled={favoritePending}
          type="button"
          onClick={(event) => {
            event.stopPropagation();
            onToggleFavorite(entry);
          }}
          onKeyDown={(event) => event.stopPropagation()}
        >
          {entry.favorite ? '♥' : '♡'}
        </button>
      </div>
      <div className="wallpaper-info">
        <span className="wallpaper-name">{displayName(entry)}</span>
        <span className="wallpaper-meta">{metaLine(entry)}</span>
      </div>
    </div>
  );
}

export const WallpaperCard = memo(WallpaperCardImpl);

export { formatSize };
