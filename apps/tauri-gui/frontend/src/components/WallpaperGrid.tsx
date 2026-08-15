import { memo, useState, useRef, useEffect, useMemo, useCallback } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { SearchX } from 'lucide-react';
import type { LibraryBrowserItemDTO } from '../api/bridge';
import { emitFeedback } from '../events/appEvents.ts';
import { recordMetric } from '../perf/metrics';
import type { ApplyGesture } from '../shell/shellPreferences';
import { useThumbnailStore } from '../state/ThumbnailStoreContext';
import {
  calculateWallpaperGridLayout,
  GRID_GAP,
  overscanRowsFor,
  wallpaperCardMetrics,
  wallpaperGridCardGeometry,
  type GridLayout,
  type WallpaperCardSize,
} from '../utils/layout';
import ContextMenu from './ContextMenu';
import LibraryState from './LibraryState.tsx';
import { WallpaperCard } from './WallpaperCard';
import {
  libraryEntryApplyAvailable,
  libraryEntryApplyDisabledReason,
  type ContextAction,
} from './libraryViewModel.ts';
import {
  anchoredScrollTopForLayoutChange,
  captureStableViewportAnchor,
  resolveGridKey,
  restoreStableViewportAnchor,
  shouldApplyFocusToken,
  shouldRequestNextPage,
  shouldResetScroll,
  visibleThumbnailPaths,
  wallpaperIdNearestGridViewportCenter,
  wallpaperApplyFlags,
  wallpaperOrdinal,
} from './wallpaperGridHelpers';

interface Props {
  entries: readonly LibraryBrowserItemDTO[];
  setSize?: number;
  onApply: (entry: LibraryBrowserItemDTO) => void;
  onSelect?: (entry: LibraryBrowserItemDTO) => void;
  onToggleFavorite: (entry: LibraryBrowserItemDTO) => void;
  applying: boolean;
  emptyText?: string;
  contextActions?: ContextAction[];
  buildContextActions?: (entry: LibraryBrowserItemDTO) => ContextAction[];
  active?: boolean;
  refreshing?: boolean;
  resetKey?: string;
  cardSize?: WallpaperCardSize;
  applyGesture?: ApplyGesture;
  selectedPath?: string | null;
  activePath?: string | null;
  pendingPath?: string | null;
  favoritePendingPaths?: ReadonlySet<string>;
  currentPath?: string | null;
  canApplyToDisplay?: boolean;
  displayApplyDisabledReason?: string | null;
  isEntryApplicable?: (entry: LibraryBrowserItemDTO) => boolean;
  hasMore?: boolean;
  loadingMore?: boolean;
  onLoadMore?: () => void | Promise<void>;
  initialAnchorWallpaperId?: number | null;
  focusToken?: number;
  onAnchorChange?: (wallpaperId: number) => void;
}

const SCROLL_IDLE_MS = 180;
const METRICS_SAMPLE_MS = 500;

interface WallpaperGridLayout extends GridLayout {
  rowHeight: number;
  thumbnailHeight: number;
}

function initialGridLayout(cardSize: WallpaperCardSize): WallpaperGridLayout {
  const metrics = wallpaperCardMetrics(cardSize);
  const geometry = wallpaperGridCardGeometry(metrics.minWidth, cardSize);
  return {
    colCount: 4,
    columnWidth: metrics.minWidth,
    rowWidth: metrics.minWidth * 4 + GRID_GAP * 3,
    rowHeight: geometry.rowHeight,
    thumbnailHeight: geometry.thumbnailHeight,
  };
}

function WallpaperGridImpl({
  entries,
  setSize,
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
  activePath = null,
  pendingPath = null,
  favoritePendingPaths = new Set(),
  currentPath = null,
  canApplyToDisplay = true,
  displayApplyDisabledReason = null,
  isEntryApplicable,
  hasMore = false,
  loadingMore = false,
  onLoadMore,
  initialAnchorWallpaperId = null,
  focusToken = 0,
  onAnchorChange,
}: Props) {
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number; path: string } | null>(null);
  const [activeIndex, setActiveIndex] = useState(0);
  const containerRef = useRef<HTMLDivElement>(null);
  const { observeVisible, setScrolling, setInteracting } = useThumbnailStore();

  const prevResetKeyRef = useRef(resetKey);
  const [gridLayout, setGridLayout] = useState<WallpaperGridLayout>(() => initialGridLayout(cardSize));
  const colCount = gridLayout.colCount;
  const isScrollingRef = useRef(false);
  const activeRef = useRef(active);
  const scrollIdleTimerRef = useRef<number | null>(null);
  const pendingScrollTopRef = useRef<number | null>(null);
  const lastEnqueueKeyRef = useRef('');
  const suppressScrollPauseRef = useRef(false);
  const viewportAnchorRef = useRef<ReturnType<typeof captureStableViewportAnchor>>(null);
  const previousEntriesRef = useRef(entries);
  const previousEntriesResetKeyRef = useRef(resetKey);
  const colCountRef = useRef(colCount);
  const entriesLengthRef = useRef(entries.length);
  const initialAnchorAppliedRef = useRef(false);
  const lastHandledFocusTokenRef = useRef(0);
  const entriesRef = useRef(entries);
  const activeIndexRef = useRef(activeIndex);
  entriesRef.current = entries;
  activeIndexRef.current = activeIndex;
  colCountRef.current = colCount;
  entriesLengthRef.current = entries.length;
  activeRef.current = active;

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
        const layout = calculateWallpaperGridLayout(w, cardSize);
        const geometry = wallpaperGridCardGeometry(layout.columnWidth, cardSize);
        const next = {
          ...layout,
          ...geometry,
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
          next.rowHeight === prev.rowHeight &&
          next.thumbnailHeight === prev.thumbnailHeight
        ) {
          return prev;
        }
        return next;
      });
    },
    [anchorScrollForLayoutChange, cardSize],
  );

  const beginScrolling = useCallback(() => {
    if (!isScrollingRef.current) {
      isScrollingRef.current = true;
      setScrolling(true);
    }
    if (scrollIdleTimerRef.current !== null) {
      window.clearTimeout(scrollIdleTimerRef.current);
    }
    scrollIdleTimerRef.current = window.setTimeout(() => {
      scrollIdleTimerRef.current = null;
      isScrollingRef.current = false;
      setScrolling(false);
    }, SCROLL_IDLE_MS);
  }, [setScrolling]);

  useEffect(() => {
    setInteracting(active);
  }, [active, setInteracting]);

  const remeasure = useCallback(() => {
    const el = containerRef.current;
    if (!el) return;
    const w = el.clientWidth;
    if (w > 0) {
      updateGridLayoutFromWidth(w);
    }
  }, [updateGridLayoutFromWidth]);

  const publishViewportCenter = useCallback(() => {
    const el = containerRef.current;
    if (!el) return;
    const wallpaperId = wallpaperIdNearestGridViewportCenter({
      entries,
      columns: colCount,
      rowHeight: gridLayout.rowHeight,
      scrollTop: el.scrollTop,
      viewportHeight: el.clientHeight,
    });
    if (wallpaperId !== null) onAnchorChange?.(wallpaperId);
  }, [colCount, entries, gridLayout.rowHeight, onAnchorChange]);

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

  useEffect(() => {
    if (!active || entries.length === 0) return undefined;
    const frame = window.requestAnimationFrame(publishViewportCenter);
    return () => window.cancelAnimationFrame(frame);
  }, [active, entries, publishViewportCenter]);

  const rowCount = Math.ceil(entries.length / colCount);

  const virtualizer = useVirtualizer({
    count: rowCount,
    getScrollElement: () => containerRef.current,
    estimateSize: () => gridLayout.rowHeight,
    overscan,
  });

  useEffect(() => {
    virtualizer.measure();
  }, [gridLayout.rowHeight, virtualizer]);

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
      setActiveIndex(0);
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
    if (!active || initialAnchorAppliedRef.current || entries.length === 0) return;
    initialAnchorAppliedRef.current = true;
    const anchorIndex = initialAnchorWallpaperId === null
      ? 0
      : entries.findIndex((entry) => entry.wallpaperId === initialAnchorWallpaperId);
    const index = anchorIndex >= 0 ? anchorIndex : 0;
    setActiveIndex(index);
    suppressScrollPauseRef.current = true;
    virtualizer.scrollToIndex(Math.floor(index / colCount), { align: 'start' });
    requestAnimationFrame(() => {
      suppressScrollPauseRef.current = false;
      publishViewportCenter();
    });
  }, [active, colCount, entries, initialAnchorWallpaperId, publishViewportCenter, virtualizer]);

  useEffect(() => {
    if (!active || !shouldApplyFocusToken(lastHandledFocusTokenRef.current, focusToken)) return;
    lastHandledFocusTokenRef.current = focusToken;
    const currentEntries = entriesRef.current;
    if (currentEntries.length === 0) return;
    const anchorIndex = initialAnchorWallpaperId === null
      ? 0
      : currentEntries.findIndex((entry) => entry.wallpaperId === initialAnchorWallpaperId);
    const entry = currentEntries[anchorIndex >= 0 ? anchorIndex : 0];
    if (!entry) return;
    setActiveIndex(anchorIndex >= 0 ? anchorIndex : 0);
    virtualizer.scrollToIndex(Math.floor((anchorIndex >= 0 ? anchorIndex : 0) / colCount), {
      align: 'start',
    });
    requestAnimationFrame(() => requestAnimationFrame(() => {
      containerRef.current
        ?.querySelector<HTMLButtonElement>(
          `[data-wallpaper-id="${entry.wallpaperId}"] .wallpaper-card__primary`,
        )
        ?.focus();
    }));
  }, [active, colCount, focusToken, initialAnchorWallpaperId, virtualizer]);

  useEffect(() => {
    setActiveIndex((current) => (
      entries.length === 0 ? 0 : Math.min(current, entries.length - 1)
    ));
  }, [entries.length]);

  useEffect(() => {
    const previous = previousEntriesRef.current;
    previousEntriesRef.current = entries;
    const criteriaChanged = shouldResetScroll(previousEntriesResetKeyRef.current, resetKey);
    previousEntriesResetKeyRef.current = resetKey;
    if (previous === entries || criteriaChanged) return;
    const nextTop = restoreStableViewportAnchor(
      entries,
      viewportAnchorRef.current,
      colCount,
      gridLayout.rowHeight,
    );
    const el = containerRef.current;
    if (nextTop === null || !el) return;
    suppressScrollPauseRef.current = true;
    el.scrollTop = nextTop;
    requestAnimationFrame(() => {
      suppressScrollPauseRef.current = false;
    });
  }, [colCount, entries, gridLayout.rowHeight, resetKey]);

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

    observeVisible(paths, { priority: 'front' });
  }, [entries, colCount, virtualizer.range, observeVisible, active]);

  useEffect(() => {
    if (!active || !onLoadMore) return;
    if (!shouldRequestNextPage({
      rowCount,
      visibleEndRow: virtualizer.range?.endIndex,
      canAutoAppend: hasMore,
      loadingMore,
    })) return;
    void onLoadMore();
  }, [active, hasMore, loadingMore, onLoadMore, rowCount, virtualizer.range?.endIndex]);

  useEffect(() => () => {
    if (scrollIdleTimerRef.current !== null) {
      window.clearTimeout(scrollIdleTimerRef.current);
    }
    setScrolling(false);
    setInteracting(true);
  }, [setScrolling, setInteracting]);

  const handleScroll = useCallback(() => {
    if (suppressScrollPauseRef.current) return;
    const el = containerRef.current;
    if (el) {
      viewportAnchorRef.current = captureStableViewportAnchor(
        entries,
        colCount,
        gridLayout.rowHeight,
        el.scrollTop,
      );
      publishViewportCenter();
    }
    beginScrolling();
  }, [beginScrolling, colCount, entries, gridLayout.rowHeight, publishViewportCenter]);

  const isScrolling = useCallback(() => isScrollingRef.current, []);

  const handleContextMenu = useCallback((e: React.MouseEvent, path: string) => {
    e.preventDefault();
    setContextMenu({ x: e.clientX, y: e.clientY, path });
  }, []);

  const handleKeyboardContextMenu = useCallback((path: string, x: number, y: number) => {
    setContextMenu({ x, y, path });
  }, []);

  const focusGridIndex = useCallback((index: number) => {
    const entry = entriesRef.current[index];
    if (!entry) return;
    setActiveIndex(index);
    virtualizer.scrollToIndex(Math.floor(index / Math.max(1, colCountRef.current)), {
      align: 'auto',
    });
    requestAnimationFrame(() => requestAnimationFrame(() => {
      document.getElementById(`wallpaper-grid-option-${entry.wallpaperId}`)?.focus({
        preventScroll: true,
      });
    }));
  }, [virtualizer]);

  const handleGridKeyDown = useCallback((event: React.KeyboardEvent<HTMLDivElement>) => {
    const target = event.target;
    if (
      target !== event.currentTarget
      && (
        !(target instanceof HTMLElement)
        || target.closest('.wallpaper-card__primary') === null
      )
    ) {
      return;
    }
    const el = containerRef.current;
    const currentEntries = entriesRef.current;
    const intent = resolveGridKey({
      key: event.key,
      shiftKey: event.shiftKey,
      currentIndex: activeIndexRef.current,
      colCount: colCountRef.current,
      itemCount: currentEntries.length,
      pageRows: el
        ? Math.max(1, Math.floor(el.clientHeight / Math.max(1, gridLayout.rowHeight)))
        : 1,
      hasMore,
      loadingMore,
    });
    if (!intent) return;
    event.preventDefault();
    event.stopPropagation();

    const entry = currentEntries[intent.index];
    if (!entry) return;
    if (intent.type === 'navigate') {
      focusGridIndex(intent.index);
      if (intent.requestLoadMore) void onLoadMore?.();
      return;
    }
    if (intent.type === 'context') {
      focusGridIndex(intent.index);
      requestAnimationFrame(() => {
        const option = document.getElementById(`wallpaper-grid-option-${entry.wallpaperId}`);
        const rect = option?.getBoundingClientRect();
        handleKeyboardContextMenu(
          entry.path,
          (rect?.left ?? 16) + 12,
          (rect?.top ?? 16) + 12,
        );
      });
      return;
    }

    const canApply = libraryEntryApplyAvailable(
      canApplyToDisplay,
      isEntryApplicable ?? (() => true),
      entry,
    );
    onSelect?.(entry);
    if (canApply) {
      onApply(entry);
      return;
    }
    emitFeedback({
      state: 'warning',
      label: 'Cannot apply',
      detail: libraryEntryApplyDisabledReason(
        canApplyToDisplay,
        displayApplyDisabledReason,
        entry,
      ) ?? 'This item cannot be applied as a live wallpaper.',
    });
  }, [
    canApplyToDisplay,
    displayApplyDisabledReason,
    focusGridIndex,
    gridLayout.rowHeight,
    handleKeyboardContextMenu,
    hasMore,
    isEntryApplicable,
    loadingMore,
    onApply,
    onLoadMore,
    onSelect,
  ]);

  const entryByPath = useMemo(() => new Map(entries.map((entry) => [entry.path, entry])), [entries]);

  const findEntry = (path: string): LibraryBrowserItemDTO | undefined => entryByPath.get(path);

  const contextEntry = contextMenu ? findEntry(contextMenu.path) : null;

  if (entries.length === 0) {
    return (
      <LibraryState
        description="Try clearing the active filters or changing your search."
        icon={<SearchX aria-hidden="true" size={28} />}
        role="status"
        title={emptyText}
      />
    );
  }

  // Roving tabindex: real focus moves between card buttons, so the container
  // must not also be a tab stop or claim aria-activedescendant.
  return (
    <div
      className={`wallpaper-grid${refreshing ? ' is-refreshing' : ''}`}
      ref={containerRef}
      onKeyDown={handleGridKeyDown}
      onScroll={handleScroll}
      aria-label="Wallpaper library"
      aria-colcount={colCount}
      aria-rowcount={setSize === undefined ? -1 : Math.ceil(setSize / colCount)}
      role="grid"
    >
      <div style={{ height: virtualizer.getTotalSize(), position: 'relative' }}>
        {virtualizer.getVirtualItems().map((virtualRow) => {
          const start = virtualRow.index * colCount;
          const rowEntries = entries.slice(start, start + colCount);
          return (
            <div
              key={virtualRow.key}
              aria-rowindex={virtualRow.index + 1}
              role="row"
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
              {rowEntries.map((e, offset) => {
                const entryIndex = start + offset;
                const activity = wallpaperApplyFlags(
                  e.path,
                  applying,
                  activePath,
                  pendingPath,
                );
                return (
                  <WallpaperCard
                    key={e.path}
                    id={`wallpaper-grid-option-${e.wallpaperId}`}
                    entry={e}
                    posInSet={start + offset + 1}
                    columnIndex={offset + 1}
                    ordinal={wallpaperOrdinal(start + offset)}
                    applying={activity.applying}
                    onApply={onApply}
                    onSelect={onSelect}
                    onToggleFavorite={onToggleFavorite}
                    onContextMenu={handleContextMenu}
                    onKeyboardContextMenu={handleKeyboardContextMenu}
                    cardSize={cardSize}
                    applyGesture={applyGesture}
                    selected={selectedPath === e.path}
                    pending={activity.pending}
                    favoritePending={favoritePendingPaths.has(e.path)}
                    current={currentPath === e.path}
                    applyAvailable={libraryEntryApplyAvailable(
                      canApplyToDisplay,
                      isEntryApplicable ?? (() => true),
                      e,
                    )}
                    applyDisabledReason={libraryEntryApplyDisabledReason(
                      canApplyToDisplay,
                      displayApplyDisabledReason,
                      e,
                    )}
                    isScrolling={isScrolling}
                    thumbnailHeight={gridLayout.thumbnailHeight}
                    onFocus={() => setActiveIndex(entryIndex)}
                    tabIndex={active && entryIndex === activeIndex ? 0 : -1}
                  />
                );
              })}
            </div>
          );
        })}
      </div>

      {loadingMore ? (
        <div className="wallpaper-grid__loading" role="status">
          Loading more…
        </div>
      ) : null}

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

export default memo(WallpaperGridImpl);
