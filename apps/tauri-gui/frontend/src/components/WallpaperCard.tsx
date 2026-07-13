import { memo, useSyncExternalStore, type CSSProperties } from 'react';
import { convertFileSrc } from '@tauri-apps/api/core';
import type { WallpaperDTO } from '../api/bridge';
import { isApplyAvailable } from '../domain/applyActions';
import { emitFeedback } from '../events/appEvents';
import { BoundedFileSrcCache } from './fileSrcCache';
import { useThumbnailStore } from '../state/ThumbnailStoreContext';
import { wallpaperCardMetrics, type WallpaperCardSize } from '../utils/layout';
import {
  displayName,
  formatSize,
  metaLine,
  typeIcon,
  weBadge,
  weBadgeClass,
} from './wallpaperCardHelpers';

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
  entry: WallpaperDTO;
  applying: boolean;
  onApply: (path: string) => void;
  onContextMenu: (e: React.MouseEvent, path: string) => void;
  cardSize?: WallpaperCardSize;
}

function WallpaperCardImpl({
  entry,
  applying,
  onApply,
  onContextMenu,
  cardSize = 'medium',
}: CardProps) {
  const store = useThumbnailStore();
  const thumbnail = useSyncExternalStore(
    (cb) => store.subscribe(entry.path, cb),
    () => store.get(entry.path),
  );
  const thumbnailFailure = useSyncExternalStore(
    (cb) => store.subscribe(entry.path, cb),
    () => store.getFailure(entry.path),
  );

  const handleDoubleClick = () => {
    if (!isApplyAvailable(entry)) {
      emitFeedback({
        state: 'warning',
        label: 'Cannot apply',
        detail: 'This item cannot be applied as a live wallpaper.',
      });
      return;
    }
    onApply(entry.path);
  };

  const badge = weBadge(entry);
  const cardStyle = {
    '--wallpaper-thumbnail-height': `${wallpaperCardMetrics(cardSize).thumbnailHeight}px`,
  } as CSSProperties;

  return (
    <div
      className={`wallpaper-card${applying ? ' disabled' : ''}`}
      style={cardStyle}
      onContextMenu={(ev) => onContextMenu(ev, entry.path)}
      onDoubleClick={handleDoubleClick}
      title={entry.path}
    >
      <div className="wallpaper-thumb">
        {entry.previewPath ? (
          <img src={safeFileSrc(entry.previewPath)} alt="" loading="lazy" decoding="async" />
        ) : thumbnail ? (
          <img src={safeFileSrc(thumbnail)} alt="" loading="lazy" decoding="async" />
        ) : (
          <div className="wallpaper-thumb-placeholder" title={thumbnailFailure ? `Preview failed: ${thumbnailFailure}` : undefined}>
            <span className="wallpaper-type-icon">{typeIcon(entry.type)}</span>
            {thumbnailFailure ? <span className="wallpaper-thumb-error">Preview failed</span> : null}
          </div>
        )}
        {badge && <span className={weBadgeClass(entry)}>{badge}</span>}
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
