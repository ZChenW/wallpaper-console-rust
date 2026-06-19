import { useState, useRef, useEffect, useMemo, useCallback } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { WallpaperDTO } from '../api/bridge';
import ContextMenu from './ContextMenu';
import { WallpaperCard } from './WallpaperCard';
import { shouldResetScroll } from './wallpaperGridHelpers';
import { useThumbnailStore } from '../state/ThumbnailStoreContext';
import { calculateColumnCount, GRID_GAP } from '../utils/layout';

interface Props {
  entries: WallpaperDTO[];
  onApply: (path: string) => void;
  applying: boolean;
  emptyText?: string;
  contextActions?: ContextAction[];
  buildContextActions?: (entry: WallpaperDTO) => ContextAction[];
  active?: boolean;
  refreshing?: boolean;
  resetKey?: string;
}

export interface ContextAction {
  label: string;
  action: (path: string) => void;
  danger?: boolean;
  visible?: (entry: WallpaperDTO) => boolean;
}

const CARD_HEIGHT = 188;
const OVERSCAN = 2;

export default function WallpaperGrid({
  entries,
  onApply,
  applying,
  emptyText = 'No wallpapers found',
  contextActions = [],
  buildContextActions,
  active = true,
  refreshing = false,
  resetKey,
}: Props) {
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number; path: string } | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const { thumbs: thumbCache, enqueue } = useThumbnailStore();

  const prevResetKeyRef = useRef(resetKey);
  const [colCount, setColCount] = useState(4);

  const remeasure = useCallback(() => {
    const el = containerRef.current;
    if (!el) return;
    const w = el.clientWidth;
    if (w > 0) {
      setColCount(calculateColumnCount(w));
    }
  }, []);

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const obs = new ResizeObserver(([entry]) => {
      if (!active) return;
      const w = entry.contentRect.width;
      setColCount(calculateColumnCount(w));
    });
    obs.observe(el);
    return () => obs.disconnect();
  }, [active]);

  useEffect(() => {
    if (!active) return;
    remeasure();
  }, [active, remeasure]);

  const rowCount = Math.ceil(entries.length / colCount);

  const virtualizer = useVirtualizer({
    count: rowCount,
    getScrollElement: () => containerRef.current,
    estimateSize: () => CARD_HEIGHT,
    overscan: OVERSCAN,
  });

  useEffect(() => {
    if (shouldResetScroll(prevResetKeyRef.current, resetKey)) {
      prevResetKeyRef.current = resetKey;
      if (active) {
        virtualizer.scrollToIndex(0);
      }
    }
  }, [resetKey, active, virtualizer]);

  useEffect(() => {
    if (!active) return;
    const range = virtualizer.range;
    if (!range) return;
    const startIdx = range.startIndex * colCount;
    const endIdx = Math.min((range.endIndex + 1) * colCount, entries.length);
    enqueue(
      entries
        .slice(startIdx, endIdx)
        .filter((e) => !e.previewPath)
        .map((e) => e.path),
      { priority: 'front' },
    );
  }, [entries, colCount, virtualizer.range, enqueue, active]);

  const handleContextMenu = useCallback((e: React.MouseEvent, path: string) => {
    e.preventDefault();
    setContextMenu({ x: e.clientX, y: e.clientY, path });
  }, []);

  const entryByPath = useMemo(() => new Map(entries.map((entry) => [entry.path, entry])), [entries]);

  const findEntry = (path: string): WallpaperDTO | undefined => entryByPath.get(path);

  const contextEntry = contextMenu ? findEntry(contextMenu.path) : null;

  if (entries.length === 0) {
    return <div className="empty-state">{emptyText}</div>;
  }

  return (
    <div className={`wallpaper-grid${refreshing ? ' is-refreshing' : ''}`} ref={containerRef}>
      <div style={{ height: virtualizer.getTotalSize(), position: 'relative' }}>
        {virtualizer.getVirtualItems().map((virtualRow) => {
          const start = virtualRow.index * colCount;
          const rowEntries = entries.slice(start, start + colCount);
          return (
            <div
              key={virtualRow.key}
              style={{
                position: 'absolute',
                top: 0,
                left: 0,
                width: '100%',
                height: virtualRow.size,
                transform: `translateY(${virtualRow.start}px)`,
                display: 'grid',
                gridTemplateColumns: `repeat(${colCount}, minmax(0, 1fr))`,
                gap: `${GRID_GAP}px`,
              }}
            >
              {rowEntries.map((e) => (
                <WallpaperCard
                  key={e.path}
                  entry={e}
                  thumbnail={thumbCache[e.path]}
                  applying={applying}
                  onApply={onApply}
                  onContextMenu={handleContextMenu}
                />
              ))}
            </div>
          );
        })}
      </div>

      {contextMenu && contextEntry && (
        <ContextMenu
          x={contextMenu.x}
          y={contextMenu.y}
          path={contextMenu.path}
          actions={buildContextActions
            ? buildContextActions(contextEntry)
            : contextActions.filter((action) => !action.visible || action.visible(contextEntry))}
          onClose={() => setContextMenu(null)}
        />
      )}
    </div>
  );
}
