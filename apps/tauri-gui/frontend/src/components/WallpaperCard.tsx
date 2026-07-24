import { memo, useState, type CSSProperties } from 'react';
import type { LibraryBrowserItemDTO } from '../api/bridge';
import { isApplyAvailable } from '../domain/applyActions';
import { emitFeedback } from '../events/appEvents';
import { wallpaperCardMetrics, type WallpaperCardSize } from '../utils/layout';
import {
  displayName,
  cardHoverLabel,
  editorialActionLabel,
  formatSize,
  metaLine,
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
  shouldStartAnimatedHover,
} from './wallpaperGridHelpers';
import WallpaperPreviewMedia from './WallpaperPreviewMedia.tsx';

interface CardProps {
  entry: LibraryBrowserItemDTO;
  ordinal?: string;
  posInSet?: number;
  setSize?: number;
  applying: boolean;
  onApply: (entry: LibraryBrowserItemDTO) => void;
  onSelect?: (entry: LibraryBrowserItemDTO) => void;
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
  applyDisabledReason?: string | null;
  isScrolling?: () => boolean;
}

const neverScrolling = () => false;

function WallpaperCardImpl({
  entry,
  ordinal,
  posInSet,
  setSize,
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
  applyDisabledReason = null,
  isScrolling = neverScrolling,
}: CardProps) {
  const [hovered, setHovered] = useState(false);
  const reducedMotion = typeof window !== 'undefined'
    && window.matchMedia?.('(prefers-reduced-motion: reduce)').matches === true;
  const animatedPreview = animatedPreviewPath(
    entry,
    hovered,
    isScrolling(),
    reducedMotion,
  );

  const handleClick = (event: React.MouseEvent<HTMLButtonElement>) => {
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
    if (interaction.apply) onApply(entry);

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
        detail: applyDisabledReason
          || entry.applyReason
          || entry.unsupportedReason
          || 'This item cannot be applied as a live wallpaper.',
      });
    }
  };

  const canApply = applyAvailable ?? isApplyAvailable(entry);
  const stateDescription = [
    selected ? 'Selected' : null,
    current ? 'Currently applied' : null,
    applying ? 'Applying wallpaper' : null,
    pending ? 'Pending apply' : null,
  ].filter((label): label is string => label !== null).join('. ');
  const stateDescriptionId = `wallpaper-card-state-${entry.wallpaperId}`;
  const reportUnsupportedApply = () => {
    emitFeedback({
      state: 'warning',
      label: 'Cannot apply',
      detail: applyDisabledReason
        || entry.applyReason
        || entry.unsupportedReason
        || 'This item cannot be applied as a live wallpaper.',
    });
  };

  const handleKeyDown = (event: React.KeyboardEvent<HTMLButtonElement>) => {
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
    if (interaction.apply) onApply(entry);
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
      onMouseEnter={() => setHovered(shouldStartAnimatedHover(isScrolling(), reducedMotion))}
      onMouseLeave={() => setHovered(false)}
      data-pending={pending || undefined}
      data-editorial-action={editorialActionLabel(canApply, applyGesture)}
      data-wallpaper-index={ordinal}
      data-wallpaper-id={entry.wallpaperId}
      data-wallpaper-path={entry.path}
      aria-posinset={posInSet}
      aria-setsize={setSize}
      role="listitem"
    >
      <button
        aria-busy={applying || undefined}
        aria-current={current ? 'true' : undefined}
        aria-describedby={stateDescription ? stateDescriptionId : undefined}
        className="wallpaper-card__primary"
        onClick={handleClick}
        onKeyDown={handleKeyDown}
        title={cardHoverLabel(entry)}
        type="button"
      >
        <div className="wallpaper-thumb">
          <WallpaperPreviewMedia
            entry={entry}
            transientImagePath={animatedPreview}
          />
          {badge && <span className={weBadgeClass(entry)}>{badge}</span>}
        </div>
        <div className="wallpaper-info">
          <span className="wallpaper-name">{displayName(entry)}</span>
          <span className="wallpaper-meta">{metaLine(entry)}</span>
          {ordinal ? (
            <span aria-hidden="true" className="wallpaper-index">{ordinal}</span>
          ) : null}
        </div>
      </button>
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
      >
        {entry.favorite ? '♥' : '♡'}
      </button>
      {stateDescription ? (
        <span className="wallpaper-card__state" id={stateDescriptionId}>
          {stateDescription}
        </span>
      ) : null}
    </div>
  );
}

export const WallpaperCard = memo(WallpaperCardImpl);

export { formatSize };
