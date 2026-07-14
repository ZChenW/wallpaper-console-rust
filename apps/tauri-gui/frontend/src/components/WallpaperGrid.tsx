import { useState, useRef, useEffect, useMemo, useCallback } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import type { LibraryBrowserItemDTO, WallpaperDTO } from '../api/bridge';
import ContextMenu from './ContextMenu';
import { WallpaperCard } from './WallpaperCard';
import {
  anchoredScrollTopForLayoutChange,
  shouldRequestNextPage,
  shouldResetScroll,
  visibleThumbnailPaths,
} from './wallpaperGridHelpers';
import { useThumbnailStore } from '../state/ThumbnailStoreContext';
import { recordMetric } from '../perf/metrics';
import {
  calculateWallpaperGridLayout,
  GRID_GAP,
  overscanRowsFor,
  wallpaperCardMetrics,
  type GridLayout,
  type WallpaperCardSize,
} from '../utils/layout';
import type { ApplyGesture } from '../shell/shellPreferences';

interface Props {
  entries: LibraryBrowserItemDTO[];
  onApply: (path: string) => void;
  onSelect?: (entry: WallpaperDTO) => void;
  onToggleFavorite: (entry: LibraryBrowserItemDTO) => void;
  applying: boolean;
  emptyText?: string;
  contextActions?: ContextAction[];
  buildContextActions?: (entry: WallpaperDTO) => ContextAction[];
  active?: boolean;
  refreshing?: boolean;
  resetKey?: string;
  cardSize?: WallpaperCardSize;
  applyGesture?: ApplyGesture;
  selectedPath?: string | null;
  pendingPath?: string | null;
  favoritePendingPaths?: ReadonlySet<string>;
  currentPath?: string | null;
  isEntryApplicable?: (entry: WallpaperDTO) => boolean;
  hasMore?: boolean;
  loadingMore?: boolean;
  onLoadMore?: () => void | Promise<void>;
}

export interface ContextAction {
  label: string;
  action: (path: string) => void;
  danger?: boolean;
  visible?: (entry: WallpaperDTO) => boolean;
}

const SCROLL_IDLE_MS = 180;
const METRICS_SAMPLE_MS = 500;

interface WallpaperGridLayout extends GridLayout {
  rowHeight: number;
}

function initialGridLayout(cardSize: WallpaperCardSize): WallpaperGridLayout {
  const metrics = wallpaperCardMetrics(cardSize);
  return {
    colCount: 4,
    columnWidth: metrics.minWidth,
    rowWidth: metrics.minWidth * 4 + GRID_GAP * 3,
    rowHeight: metrics.rowHeight,
  };
}

export default function WallpaperGrid({
  entries,
  onApply,
  onSelect,
  onToggleFavorite,
  applying,
  emptyText = 'No wallpapers found',
  contextActions = [],
  buildContextActions,
  active = true,
  refreshing = false,
  resetKey,
  cardSize = 'medium',
  applyGesture = 'single',
  selectedPath = null,
  pendingPath = null,
  favoritePendingPaths = new Set(),
  currentPath = null,
  isEntryApplicable,
  hasMore = false,
  loadingMore = false,
  onLoadMore,
}: Props) {
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number; path: string } | null>(null);
  const [scrolling, setScrolling] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const { enqueueVisible, setRevealPaused } = useThumbnailStore();

  const prevResetKeyRef = useRef(resetKey);
  const cardMetrics = wallpaperCardMetrics(cardSize);
  const [gridLayout, setGridLayout] = useState<WallpaperGridLayout>(() => initialGridLayout(cardSize));
  const colCount = gridLayout.colCount;
  const isScrollingRef = useRef(false);
  const scrollIdleTimerRef = useRef<number | null>(null);
  const pendingScrollTopRef = useRef<number | null>(null);
  const lastEnqueueKeyRef = useRef('');
  const suppressScrollPauseRef = useRef(false);
  const colCountRef = useRef(colCount);
  const entriesLengthRef = useRef(entries.length);
  colCountRef.current = colCount;
  entriesLengthRef.current = entries.length;

  const overscan = overscanRowsFor(colCount, true);

  const anchorScrollForLayoutChange = useCallback(
    (
      previousColumns: number,
      previousRowHeight: number,
      nextColumns: number,
      nextRowHeight: number,
    ) => {
      const el = containerRef.current;
      if (!el) return;

      pendingScrollTopRef.current = anchoredScrollTopForLayoutChange({
        scrollTop: el.scrollTop,
        previousColumns,
        previousRowHeight,
        nextColumns,
        nextRowHeight,
      });
    },
    [],
  );

  const updateGridLayoutFromWidth = useCallback(
    (w: number) => {
      if (w <= 0) return;
      setGridLayout((prev) => {
        const next = {
          ...calculateWallpaperGridLayout(w, cardSize),
          rowHeight: cardMetrics.rowHeight,
        };
        if (next.colCount !== prev.colCount || next.rowHeight !== prev.rowHeight) {
          anchorScrollForLayoutChange(
            prev.colCount,
            prev.rowHeight,
            next.colCount,
            next.rowHeight,
          );
        }
        if (
          next.colCount === prev.colCount &&
          next.columnWidth === prev.columnWidth &&
          next.rowWidth === prev.rowWidth &&
          next.rowHeight === prev.rowHeight
        ) {
          return prev;
        }
        return next;
      });
    },
    [anchorScrollForLayoutChange, cardMetrics.rowHeight, cardSize],
  );

  const beginScrolling = useCallback(() => {
    if (!isScrollingRef.current) {
      isScrollingRef.current = true;
      setScrolling(true);
      setRevealPaused(true);
    }
    if (scrollIdleTimerRef.current !== null) {
      window.clearTimeout(scrollIdleTimerRef.current);
    }
    scrollIdleTimerRef.current = window.setTimeout(() => {
      scrollIdleTimerRef.current = null;
      isScrollingRef.current = false;
      setScrolling(false);
      setRevealPaused(false);
    }, SCROLL_IDLE_MS);
  }, [setRevealPaused]);

  const remeasure = useCallback(() => {
    const el = containerRef.current;
    if (!el) return;
    const w = el.clientWidth;
    if (w > 0) {
      updateGridLayoutFromWidth(w);
    }
  }, [updateGridLayoutFromWidth]);

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const obs = new ResizeObserver(([entry]) => {
      if (!active) return;
      updateGridLayoutFromWidth(entry.contentRect.width);
    });
    obs.observe(el);
    return () => obs.disconnect();
  }, [active, updateGridLayoutFromWidth]);

  useEffect(() => {
    if (!active) return;
    remeasure();
  }, [active, remeasure]);

  const rowCount = Math.ceil(entries.length / colCount);

  const virtualizer = useVirtualizer({
    count: rowCount,
    getScrollElement: () => containerRef.current,
    estimateSize: () => cardMetrics.rowHeight,
    overscan,
  });

  useEffect(() => {
    virtualizer.measure();
  }, [cardSize, virtualizer]);

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
  }, [colCount, gridLayout.rowHeight, virtualizer]);

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

  const shouldSampleGridMetrics =
    import.meta.env.DEV || localStorage.getItem('wc.debug.metrics') === 'on';

  useEffect(() => {
    if (!active) return;
    if (!shouldSampleGridMetrics) return;

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
    if (!active) return;
    const range = virtualizer.range;
    const paths = visibleThumbnailPaths(entries, colCount, range, 3);

    if (paths.length === 0) return;

    const keyPrefix = range ? `${range.startIndex}:${range.endIndex}` : 'fallback';
    const key = `${keyPrefix}:${colCount}:${paths.join('\0')}`;
    if (lastEnqueueKeyRef.current === key) return;
    lastEnqueueKeyRef.current = key;

    enqueueVisible(paths, { priority: 'front' });
  }, [entries, colCount, virtualizer.range, enqueueVisible, active]);

  useEffect(() => {
    if (!active || !onLoadMore) return;
    if (!shouldRequestNextPage({
      rowCount,
      visibleEndRow: virtualizer.range?.endIndex,
      hasMore,
      loadingMore,
    })) return;
    void onLoadMore();
  }, [active, hasMore, loadingMore, onLoadMore, rowCount, virtualizer.range?.endIndex]);

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

  const handleKeyboardContextMenu = useCallback((path: string, x: number, y: number) => {
    setContextMenu({ x, y, path });
  }, []);

  const entryByPath = useMemo(() => new Map(entries.map((entry) => [entry.path, entry])), [entries]);

  const findEntry = (path: string): LibraryBrowserItemDTO | undefined => entryByPath.get(path);

  const contextEntry = contextMenu ? findEntry(contextMenu.path) : null;

  if (entries.length === 0) {
    return <div className="empty-state">{emptyText}</div>;
  }

  return (
    <div
      className={`wallpaper-grid${refreshing ? ' is-refreshing' : ''}`}
      ref={containerRef}
      onScroll={handleScroll}
      aria-label="Wallpaper library"
      role="listbox"
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
                  onSelect={onSelect}
                  onToggleFavorite={onToggleFavorite}
                  onContextMenu={handleContextMenu}
                  onKeyboardContextMenu={handleKeyboardContextMenu}
                  cardSize={cardSize}
                  applyGesture={applyGesture}
                  selected={selectedPath === e.path}
                  pending={pendingPath === e.path}
                  favoritePending={favoritePendingPaths.has(e.path)}
                  current={currentPath === e.path}
                  applyAvailable={isEntryApplicable?.(e)}
                  scrolling={scrolling}
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
