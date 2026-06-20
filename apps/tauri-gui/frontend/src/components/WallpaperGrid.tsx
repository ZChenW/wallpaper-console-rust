import { useState, useRef, useEffect, useMemo, useCallback } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { WallpaperDTO } from '../api/bridge';
import ContextMenu from './ContextMenu';
import { WallpaperCard } from './WallpaperCard';
import { shouldResetScroll } from './wallpaperGridHelpers';
import { useThumbnailStore } from '../state/ThumbnailStoreContext';
import { calculateColumnCount, GRID_GAP, overscanRowsFor } from '../utils/layout';

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
const SCROLL_VELOCITY_THRESHOLD = 50;

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
  const { enqueueVisible } = useThumbnailStore();

  const prevResetKeyRef = useRef(resetKey);
  const [colCount, setColCount] = useState(4);
  const [fastScroll, setFastScroll] = useState(false);
  const pendingScrollTopRef = useRef<number | null>(null);
  const lastEnqueueKeyRef = useRef('');
  const lastScrollTop = useRef(0);
  const lastScrollTime = useRef(0);

  const overscan = overscanRowsFor(colCount, fastScroll);

  const anchorScrollForColumnChange = useCallback((oldCols: number, newCols: number) => {
    const el = containerRef.current;
    if (!el || oldCols === newCols) return;

    const firstVisibleRow = Math.floor(el.scrollTop / CARD_HEIGHT);
    const firstVisibleItem = firstVisibleRow * oldCols;
    const nextRow = Math.floor(firstVisibleItem / newCols);
    pendingScrollTopRef.current = nextRow * CARD_HEIGHT;
  }, []);

  const updateColCountFromWidth = useCallback(
    (w: number) => {
      setColCount((prev) => {
        const next = calculateColumnCount(w);
        if (next !== prev) {
          anchorScrollForColumnChange(prev, next);
        }
        return next;
      });
    },
    [anchorScrollForColumnChange],
  );

  const remeasure = useCallback(() => {
    const el = containerRef.current;
    if (!el) return;
    const w = el.clientWidth;
    if (w > 0) {
      updateColCountFromWidth(w);
    }
  }, [updateColCountFromWidth]);

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const obs = new ResizeObserver(([entry]) => {
      if (!active) return;
      updateColCountFromWidth(entry.contentRect.width);
    });
    obs.observe(el);
    return () => obs.disconnect();
  }, [active, updateColCountFromWidth]);

  useEffect(() => {
    if (!active) return;
    remeasure();
  }, [active, remeasure]);

  const rowCount = Math.ceil(entries.length / colCount);

  const virtualizer = useVirtualizer({
    count: rowCount,
    getScrollElement: () => containerRef.current,
    estimateSize: () => CARD_HEIGHT,
    overscan,
  });

  useEffect(() => {
    const nextTop = pendingScrollTopRef.current;
    if (nextTop === null) return;
    pendingScrollTopRef.current = null;

    const el = containerRef.current;
    if (!el) return;

    requestAnimationFrame(() => {
      el.scrollTop = Math.min(nextTop, Math.max(0, el.scrollHeight - el.clientHeight));
      virtualizer.measure();
    });
  }, [colCount, virtualizer]);

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
    const paths = entries
      .slice(startIdx, endIdx)
      .filter((e) => !e.previewPath)
      .map((e) => e.path);

    if (paths.length === 0) return;

    const key = `${range.startIndex}:${range.endIndex}:${colCount}:${paths.join('\0')}`;
    if (lastEnqueueKeyRef.current === key) return;
    lastEnqueueKeyRef.current = key;

    enqueueVisible(paths, { priority: 'front' });
  }, [entries, colCount, virtualizer.range, enqueueVisible, active]);

  const handleScroll = useCallback(() => {
    const el = containerRef.current;
    if (!el) return;
    const now = performance.now();
    const delta = Math.abs(el.scrollTop - lastScrollTop.current);
    const dt = now - lastScrollTime.current;
    lastScrollTop.current = el.scrollTop;
    lastScrollTime.current = now;
    if (dt > 0) {
      const velocity = delta / dt;
      const fast = velocity > SCROLL_VELOCITY_THRESHOLD;
      setFastScroll((prev) => (prev !== fast ? fast : prev));
    }
  }, []);

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
    <div
      className={`wallpaper-grid${refreshing ? ' is-refreshing' : ''}`}
      ref={containerRef}
      onScroll={handleScroll}
    >
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
