import { useState, useRef, useEffect, useMemo, useCallback } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { WallpaperDTO } from '../api/bridge';
import ContextMenu from './ContextMenu';
import { WallpaperCard } from './WallpaperCard';
import { shouldResetScroll } from './wallpaperGridHelpers';
import { useThumbnailStore } from '../state/ThumbnailStoreContext';
import { recordMetric } from '../perf/metrics';
import { COL_MIN_WIDTH, calculateGridLayout, GRID_GAP, overscanRowsFor, type GridLayout } from '../utils/layout';

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
const SCROLL_IDLE_MS = 180;
const METRICS_SAMPLE_MS = 500;
const INITIAL_GRID_LAYOUT: GridLayout = {
  colCount: 4,
  columnWidth: COL_MIN_WIDTH,
  rowWidth: COL_MIN_WIDTH * 4 + GRID_GAP * 3,
};

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
  const { enqueueVisible, setRevealPaused } = useThumbnailStore();

  const prevResetKeyRef = useRef(resetKey);
  const [gridLayout, setGridLayout] = useState<GridLayout>(INITIAL_GRID_LAYOUT);
  const colCount = gridLayout.colCount;
  const [thumbnailPaused, setThumbnailPaused] = useState(false);
  const isScrollingRef = useRef(false);
  const scrollIdleTimerRef = useRef<number | null>(null);
  const pendingScrollTopRef = useRef<number | null>(null);
  const lastEnqueueKeyRef = useRef('');
  const suppressScrollPauseRef = useRef(false);
  const colCountRef = useRef(colCount);
  const entriesLengthRef = useRef(entries.length);
  colCountRef.current = colCount;
  entriesLengthRef.current = entries.length;

  const overscan = overscanRowsFor(colCount, thumbnailPaused);

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
      if (w <= 0) return;
      setGridLayout((prev) => {
        const next = calculateGridLayout(w);
        if (next.colCount !== prev.colCount) {
          anchorScrollForColumnChange(prev.colCount, next.colCount);
        }
        if (
          next.colCount === prev.colCount &&
          next.columnWidth === prev.columnWidth &&
          next.rowWidth === prev.rowWidth
        ) {
          return prev;
        }
        return next;
      });
    },
    [anchorScrollForColumnChange],
  );

  const beginScrolling = useCallback(() => {
    if (!isScrollingRef.current) {
      isScrollingRef.current = true;
      setThumbnailPaused(true);
      setRevealPaused(true);
    }
    if (scrollIdleTimerRef.current !== null) {
      window.clearTimeout(scrollIdleTimerRef.current);
    }
    scrollIdleTimerRef.current = window.setTimeout(() => {
      scrollIdleTimerRef.current = null;
      isScrollingRef.current = false;
      setThumbnailPaused(false);
      setRevealPaused(false);
    }, SCROLL_IDLE_MS);
  }, [setRevealPaused]);

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

    suppressScrollPauseRef.current = true;
    requestAnimationFrame(() => {
      el.scrollTop = Math.min(nextTop, Math.max(0, el.scrollHeight - el.clientHeight));
      virtualizer.measure();
      requestAnimationFrame(() => {
        suppressScrollPauseRef.current = false;
      });
    });
  }, [colCount, virtualizer]);

  useEffect(() => {
    if (shouldResetScroll(prevResetKeyRef.current, resetKey)) {
      prevResetKeyRef.current = resetKey;
      if (active) {
        suppressScrollPauseRef.current = true;
        virtualizer.scrollToIndex(0);
        requestAnimationFrame(() => {
          suppressScrollPauseRef.current = false;
        });
      }
    }
  }, [resetKey, active, virtualizer]);

  useEffect(() => {
    if (!active) return;

    const sampleGridMetrics = () => {
      const cols = colCountRef.current;
      const entryCount = entriesLengthRef.current;
      const virtualRows = virtualizer.getVirtualItems();
      const renderedCards = virtualRows.reduce((sum, row) => {
        const start = row.index * cols;
        if (start >= entryCount) return sum;
        return sum + Math.min(cols, entryCount - start);
      }, 0);
      recordMetric('library.grid.colCount', cols);
      recordMetric('library.grid.renderedCards', renderedCards);
    };

    sampleGridMetrics();
    const id = window.setInterval(sampleGridMetrics, METRICS_SAMPLE_MS);
    return () => window.clearInterval(id);
  }, [active, virtualizer]);

  useEffect(() => {
    if (!active || thumbnailPaused) return;
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
  }, [entries, colCount, virtualizer.range, enqueueVisible, active, thumbnailPaused]);

  useEffect(() => () => {
    if (scrollIdleTimerRef.current !== null) {
      window.clearTimeout(scrollIdleTimerRef.current);
    }
    setRevealPaused(false);
  }, [setRevealPaused]);

  const handleScroll = useCallback(() => {
    if (suppressScrollPauseRef.current) return;
    beginScrolling();
  }, [beginScrolling]);

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
                width: `${gridLayout.rowWidth}px`,
                height: virtualRow.size,
                transform: `translateY(${virtualRow.start}px)`,
                display: 'grid',
                gridTemplateColumns: `repeat(${colCount}, ${gridLayout.columnWidth}px)`,
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
