import {
  memo,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
  type MouseEvent,
} from 'react';
import {
  defaultRangeExtractor,
  useVirtualizer,
  type Range,
} from '@tanstack/react-virtual';

import type { LibraryBrowserItemDTO } from '../api/types.ts';
import { recordMetric } from '../perf/metrics.ts';
import { useThumbnailStore } from '../state/ThumbnailStoreContext.tsx';
import ContextMenu from './ContextMenu.tsx';
import FlowIndexDialog from './FlowIndexDialog.tsx';
import FlowIndexRail, { type FlowIndexRailEntry } from './FlowIndexRail.tsx';
import FlowMetadataRail from './FlowMetadataRail.tsx';
import WallpaperPreviewMedia from './WallpaperPreviewMedia.tsx';
import { applyFlowHover } from './flowHoverDom.ts';
import {
  resolveLibraryFlowStartupAnchor,
  resolveLibraryQueryResetAnchor,
  type LibraryViewModel,
} from './libraryViewModel.ts';
import { displayName } from './wallpaperCardHelpers.ts';
import {
  FLOW_SCROLL_IDLE_MS,
  FLOW_SNAP_DURATION_MS,
  estimateFlowItemSize,
  flowPageStep,
  flowScrollBehavior,
  flowStateLabels,
  flowStatePresentation,
  localFlowIndexWindow,
  nearestFlowCenterIndex,
  retainFlowActiveIndex,
  resolveFlowKey,
  shouldRequestFlowNextPage,
  visibleMeasuredFlowThumbnailRange,
} from './wallpaperFlowModel.ts';
import { staticPreviewAssetPath } from './wallpaperPreviewMedia.ts';

const FLOW_ITEM_GAP = 84;
const FLOW_OVERSCAN = 4;
const FLOW_METRICS_SAMPLE_MS = 500;

export interface WallpaperFlowProps {
  readonly model: LibraryViewModel;
  readonly initialAnchorWallpaperId?: number | null;
  readonly focusToken?: number;
  readonly onAnchorChange?: (wallpaperId: number) => void;
}

interface FlowDimensions {
  readonly width: number;
  readonly height: number;
}

interface FlowContextMenuState {
  readonly entry: LibraryBrowserItemDTO;
  readonly x: number;
  readonly y: number;
}

function resolutionAspectRatio(resolution: string): number {
  const match = /^(\d+)\s*[x×]\s*(\d+)$/i.exec(resolution.trim());
  if (!match) return 16 / 9;
  const width = Number(match[1]);
  const height = Number(match[2]);
  return width > 0 && height > 0 ? width / height : 16 / 9;
}

function aspectClass(entry: LibraryBrowserItemDTO): 'landscape' | 'square' | 'portrait' {
  const aspect = resolutionAspectRatio(entry.resolution);
  if (aspect > 1.16) return 'landscape';
  if (aspect < 0.86) return 'portrait';
  return 'square';
}

function entryLabelForPath(
  entries: readonly LibraryBrowserItemDTO[],
  path: string | null,
): string | null {
  if (!path) return null;
  const entry = entries.find((candidate) => candidate.path === path);
  if (entry) return displayName(entry);
  return path.split(/[\\/]/).filter(Boolean).at(-1) ?? path;
}

function useReducedMotion(): boolean {
  const [reduced, setReduced] = useState(() => (
    typeof window !== 'undefined'
      && window.matchMedia?.('(prefers-reduced-motion: reduce)').matches === true
  ));
  useEffect(() => {
    const query = window.matchMedia?.('(prefers-reduced-motion: reduce)');
    if (!query) return undefined;
    const update = () => setReduced(query.matches);
    query.addEventListener?.('change', update);
    return () => query.removeEventListener?.('change', update);
  }, []);
  return reduced;
}

function WallpaperFlowImpl({
  model,
  initialAnchorWallpaperId = null,
  focusToken = 0,
  onAnchorChange,
}: WallpaperFlowProps) {
  const flowRef = useRef<HTMLElement>(null);
  const streamRef = useRef<HTMLDivElement>(null);
  const { enqueueVisible, setRevealPaused } = useThumbnailStore();
  const reducedMotion = useReducedMotion();
  const initialAnchor = resolveLibraryFlowStartupAnchor(
    model.entries,
    initialAnchorWallpaperId,
    model.currentPath,
  );
  const initialIndex = initialAnchor?.index ?? 0;
  const [centeredIndex, setCenteredIndex] = useState(initialIndex);
  const centeredIndexRef = useRef(initialIndex);
  const centeredIdRef = useRef(model.entries[initialIndex]?.wallpaperId ?? null);
  const hoveredWallpaperIdRef = useRef<number | null>(null);
  const [settled, setSettled] = useState(true);
  const [indexOpen, setIndexOpen] = useState(false);
  const [pageVisible, setPageVisible] = useState(() => (
    typeof document === 'undefined' || document.visibilityState !== 'hidden'
  ));
  const [windowFocused, setWindowFocused] = useState(() => (
    typeof document === 'undefined' || document.hasFocus()
  ));
  const [dimensions, setDimensions] = useState<FlowDimensions>({ width: 720, height: 600 });
  const [showReturnToTop, setShowReturnToTop] = useState(false);
  const [contextMenu, setContextMenu] = useState<FlowContextMenuState | null>(null);
  const scrollIdleTimerRef = useRef<number | null>(null);
  const settleTimerRef = useRef<number | null>(null);
  const initialCenterFrameRef = useRef<number | null>(null);
  const centerFrameRef = useRef<number | null>(null);
  const scrollingRef = useRef(false);
  const programmaticScrollRef = useRef(false);
  const programmaticTargetIndexRef = useRef<number | null>(null);
  const initializedRef = useRef(false);
  const previousResetKeyRef = useRef(model.resetKey);
  const previousReplaceCountRef = useRef(model.replaceCount);
  const pendingQueryResetRef = useRef<{
    readonly resetKey: string;
    readonly replaceCount: number;
  } | null>(model.queryReplacementPending && model.entries.length > 0 ? {
    resetKey: model.resetKey,
    replaceCount: model.replaceCount,
  } : null);
  const directStartupRef = useRef(initialAnchorWallpaperId === null);
  const startupAnchorResolvedRef = useRef(
    initialAnchorWallpaperId !== null || model.currentObservationReady,
  );
  const userInteractedRef = useRef(false);
  const lastThumbnailKeyRef = useRef('');
  const endLoadRequestedRef = useRef(false);
  const pointerInteractionRef = useRef(false);
  const interactionActive = model.active && pageVisible && windowFocused && !indexOpen;
  const interactionActiveRef = useRef(interactionActive);
  interactionActiveRef.current = interactionActive;

  const handleFlowHover = useCallback((wallpaperId: number | null) => {
    hoveredWallpaperIdRef.current = wallpaperId;
    applyFlowHover(flowRef.current, wallpaperId);
  }, []);

  const estimateEntrySize = useCallback((index: number) => {
    const entry = model.entries[index];
    const aspect = entry ? resolutionAspectRatio(entry.resolution) : 16 / 9;
    const widthFactor = aspect > 1.16 ? 0.9 : aspect < 0.86 ? 0.62 : 0.74;
    const previewHeight = estimateFlowItemSize({
      availableWidth: Math.max(180, dimensions.width * widthFactor),
      aspectRatio: aspect,
      minHeight: 150,
      maxHeight: Math.max(180, dimensions.height * 0.55),
    });
    return previewHeight + FLOW_ITEM_GAP;
  }, [dimensions.height, dimensions.width, model.entries]);
  const firstSize = estimateEntrySize(0);
  const finalSize = estimateEntrySize(Math.max(0, model.entries.length - 1));
  const centerPaddingStart = Math.max(0, dimensions.height / 2 - firstSize / 2);
  const centerPaddingEnd = Math.max(0, dimensions.height / 2 - finalSize / 2);
  const extractVirtualRange = useCallback((range: Range) => retainFlowActiveIndex(
    defaultRangeExtractor(range),
    centeredIndex,
    model.entries.length,
  ), [centeredIndex, model.entries.length]);

  const virtualizer = useVirtualizer({
    count: model.entries.length,
    getScrollElement: () => streamRef.current,
    estimateSize: estimateEntrySize,
    getItemKey: (index) => model.entries[index]?.wallpaperId ?? index,
    overscan: FLOW_OVERSCAN,
    paddingStart: centerPaddingStart,
    paddingEnd: centerPaddingEnd,
    rangeExtractor: extractVirtualRange,
  });

  const clearMotionTimers = useCallback(() => {
    if (scrollIdleTimerRef.current !== null) {
      window.clearTimeout(scrollIdleTimerRef.current);
      scrollIdleTimerRef.current = null;
    }
    if (settleTimerRef.current !== null) {
      window.clearTimeout(settleTimerRef.current);
      settleTimerRef.current = null;
    }
  }, []);

  const cancelInitialCenterFrame = useCallback(() => {
    if (initialCenterFrameRef.current === null) return;
    window.cancelAnimationFrame(initialCenterFrameRef.current);
    initialCenterFrameRef.current = null;
  }, []);

  const cancelCenterCalculationFrame = useCallback(() => {
    if (centerFrameRef.current === null) return;
    window.cancelAnimationFrame(centerFrameRef.current);
    centerFrameRef.current = null;
  }, []);

  const markUserInteraction = useCallback(() => {
    userInteractedRef.current = true;
    startupAnchorResolvedRef.current = true;
  }, []);

  const markCentered = useCallback((index: number) => {
    const entry = model.entries[index];
    if (!entry) return;
    if (
      centeredIdRef.current === entry.wallpaperId
      && centeredIndexRef.current === index
    ) return;
    centeredIdRef.current = entry.wallpaperId;
    centeredIndexRef.current = index;
    setCenteredIndex(index);
    onAnchorChange?.(entry.wallpaperId);
  }, [model.entries, onAnchorChange]);

  const finishProgrammaticScroll = useCallback((targetIndex: number) => {
    if (programmaticTargetIndexRef.current !== targetIndex) return;
    markCentered(targetIndex);
    programmaticTargetIndexRef.current = null;
    programmaticScrollRef.current = false;
    scrollingRef.current = false;
    setSettled(true);
    setRevealPaused(!interactionActiveRef.current);
  }, [markCentered, setRevealPaused]);

  const scheduleProgrammaticSettle = useCallback((
    targetIndex: number,
    immediate: boolean,
  ) => {
    let attempts = 0;
    const checkTarget = () => {
      settleTimerRef.current = null;
      if (programmaticTargetIndexRef.current !== targetIndex) return;
      const stream = streamRef.current;
      const option = stream?.querySelector<HTMLElement>(`[data-index="${targetIndex}"]`);
      const optionBounds = option?.getBoundingClientRect();
      const streamBounds = stream?.getBoundingClientRect();
      const centerDelta = optionBounds && streamBounds
        ? Math.abs(
          optionBounds.top + optionBounds.height / 2
          - (streamBounds.top + streamBounds.height / 2),
        )
        : Number.POSITIVE_INFINITY;
      const tolerance = optionBounds
        ? Math.max(3, Math.min(18, optionBounds.height * 0.035))
        : 3;
      if (reducedMotion || centerDelta <= tolerance) {
        finishProgrammaticScroll(targetIndex);
        return;
      }
      attempts += 1;
      if (attempts >= 20) {
        virtualizer.scrollToIndex(targetIndex, { align: 'center', behavior: 'auto' });
        window.requestAnimationFrame(() => finishProgrammaticScroll(targetIndex));
        return;
      }
      settleTimerRef.current = window.setTimeout(checkTarget, 50);
    };
    settleTimerRef.current = window.setTimeout(
      checkTarget,
      immediate ? 0 : FLOW_SNAP_DURATION_MS,
    );
  }, [finishProgrammaticScroll, reducedMotion, virtualizer]);

  const centerAtIndex = useCallback((index: number, direct = false) => {
    const entry = model.entries[index];
    if (!entry) return;
    cancelInitialCenterFrame();
    cancelCenterCalculationFrame();
    clearMotionTimers();
    programmaticScrollRef.current = true;
    programmaticTargetIndexRef.current = index;
    scrollingRef.current = true;
    setSettled(false);
    setRevealPaused(true);
    const targetRendered = virtualizer.getVirtualItems().some((item) => item.index === index);
    const immediate = direct || reducedMotion || !targetRendered;
    virtualizer.scrollToIndex(index, {
      align: 'center',
      behavior: flowScrollBehavior(reducedMotion, immediate),
    });
    scheduleProgrammaticSettle(index, immediate);
  }, [
    cancelInitialCenterFrame,
    cancelCenterCalculationFrame,
    clearMotionTimers,
    model.entries,
    reducedMotion,
    scheduleProgrammaticSettle,
    setRevealPaused,
    virtualizer,
  ]);

  const computeCentered = useCallback(() => {
    centerFrameRef.current = null;
    const stream = streamRef.current;
    if (!stream) return;
    const index = nearestFlowCenterIndex({
      items: virtualizer.getVirtualItems().map((item) => ({
        id: model.entries[item.index]?.wallpaperId ?? item.index,
        index: item.index,
        start: item.start,
        size: item.size,
      })),
      viewportStart: stream.scrollTop,
      viewportSize: stream.clientHeight,
      previousCenteredId: centeredIdRef.current,
    });
    if (index !== null) markCentered(index);
  }, [markCentered, model.entries, virtualizer]);

  const scheduleCenterCalculation = useCallback(() => {
    if (centerFrameRef.current !== null) return;
    centerFrameRef.current = window.requestAnimationFrame(computeCentered);
  }, [computeCentered]);

  const scheduleIdleSnap = useCallback(() => {
    if (scrollIdleTimerRef.current !== null) window.clearTimeout(scrollIdleTimerRef.current);
    scrollIdleTimerRef.current = window.setTimeout(() => {
      scrollIdleTimerRef.current = null;
      centerAtIndex(centeredIndexRef.current);
    }, FLOW_SCROLL_IDLE_MS);
  }, [centerAtIndex]);

  const cancelProgrammaticScroll = useCallback(() => {
    const stream = streamRef.current;
    if (programmaticTargetIndexRef.current !== null && stream) {
      stream.scrollTo({ top: stream.scrollTop, behavior: 'auto' });
    }
    programmaticTargetIndexRef.current = null;
    programmaticScrollRef.current = false;
  }, []);

  const beginInteraction = useCallback(() => {
    markUserInteraction();
    cancelInitialCenterFrame();
    cancelCenterCalculationFrame();
    clearMotionTimers();
    cancelProgrammaticScroll();
    scrollingRef.current = true;
    setSettled(false);
    setRevealPaused(true);
  }, [
    cancelInitialCenterFrame,
    cancelCenterCalculationFrame,
    cancelProgrammaticScroll,
    clearMotionTimers,
    markUserInteraction,
    setRevealPaused,
  ]);

  const handleScroll = useCallback(() => {
    const stream = streamRef.current;
    if (!stream) return;
    if (programmaticScrollRef.current) {
      scheduleCenterCalculation();
      setShowReturnToTop(stream.scrollTop > stream.clientHeight);
      return;
    }
    markUserInteraction();
    cancelInitialCenterFrame();
    if (scrollIdleTimerRef.current !== null) {
      window.clearTimeout(scrollIdleTimerRef.current);
      scrollIdleTimerRef.current = null;
    }
    if (settleTimerRef.current !== null) {
      window.clearTimeout(settleTimerRef.current);
      settleTimerRef.current = null;
    }
    scrollingRef.current = true;
    setSettled(false);
    setRevealPaused(true);
    scheduleCenterCalculation();
    scheduleIdleSnap();
    setShowReturnToTop(stream.scrollTop > stream.clientHeight);
  }, [
    cancelInitialCenterFrame,
    markUserInteraction,
    scheduleCenterCalculation,
    scheduleIdleSnap,
    setRevealPaused,
  ]);

  const handleWheel = useCallback(() => {
    beginInteraction();
    scheduleIdleSnap();
  }, [beginInteraction, scheduleIdleSnap]);

  const handlePointerDown = useCallback(() => {
    pointerInteractionRef.current = true;
    beginInteraction();
  }, [beginInteraction]);

  const finishPointerInteraction = useCallback(() => {
    if (!pointerInteractionRef.current) return;
    pointerInteractionRef.current = false;
    scheduleIdleSnap();
  }, [scheduleIdleSnap]);

  useEffect(() => {
    window.addEventListener('pointerup', finishPointerInteraction);
    window.addEventListener('pointercancel', finishPointerInteraction);
    return () => {
      window.removeEventListener('pointerup', finishPointerInteraction);
      window.removeEventListener('pointercancel', finishPointerInteraction);
    };
  }, [finishPointerInteraction]);

  useEffect(() => {
    const stream = streamRef.current;
    if (!stream) return undefined;
    const update = () => {
      const next = { width: stream.clientWidth, height: stream.clientHeight };
      if (next.width > 0 && next.height > 0) setDimensions(next);
    };
    update();
    const observer = new ResizeObserver(update);
    observer.observe(stream);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    virtualizer.measure();
  }, [dimensions.height, dimensions.width, virtualizer]);

  useEffect(() => {
    const update = () => setPageVisible(document.visibilityState !== 'hidden');
    document.addEventListener('visibilitychange', update);
    return () => document.removeEventListener('visibilitychange', update);
  }, []);

  useEffect(() => {
    const focus = () => setWindowFocused(true);
    const blur = () => {
      setWindowFocused(false);
      finishPointerInteraction();
    };
    window.addEventListener('focus', focus);
    window.addEventListener('blur', blur);
    return () => {
      window.removeEventListener('focus', focus);
      window.removeEventListener('blur', blur);
    };
  }, [finishPointerInteraction]);

  useEffect(() => {
    setRevealPaused(!interactionActive || scrollingRef.current);
  }, [interactionActive, setRevealPaused]);

  useEffect(() => {
    const resetChanged = previousResetKeyRef.current !== model.resetKey;
    const replacementChanged = previousReplaceCountRef.current !== model.replaceCount;
    previousResetKeyRef.current = model.resetKey;
    previousReplaceCountRef.current = model.replaceCount;

    if (resetChanged) {
      startupAnchorResolvedRef.current = true;
      if (replacementChanged) {
        pendingQueryResetRef.current = null;
        const resetAnchor = resolveLibraryQueryResetAnchor(model.entries);
        if (resetAnchor) centerAtIndex(resetAnchor.index);
      } else {
        pendingQueryResetRef.current = {
          resetKey: model.resetKey,
          replaceCount: model.replaceCount,
        };
      }
      return;
    }

    const pendingReset = pendingQueryResetRef.current;
    if (
      pendingReset?.resetKey === model.resetKey
      && pendingReset.replaceCount !== model.replaceCount
    ) {
      pendingQueryResetRef.current = null;
      const resetAnchor = resolveLibraryQueryResetAnchor(model.entries);
      if (resetAnchor) centerAtIndex(resetAnchor.index);
      return;
    }

    if (!model.entries.length) return;
    if (!initializedRef.current) {
      initializedRef.current = true;
      initialCenterFrameRef.current = window.requestAnimationFrame(() => {
        initialCenterFrameRef.current = null;
        centerAtIndex(initialIndex);
      });
      return;
    }
    if (
      directStartupRef.current
      && !startupAnchorResolvedRef.current
      && model.currentObservationReady
    ) {
      startupAnchorResolvedRef.current = true;
      const currentIndex = model.currentPath === null
        ? -1
        : model.entries.findIndex((entry) => entry.path === model.currentPath);
      if (!userInteractedRef.current && currentIndex >= 0) {
        centerAtIndex(currentIndex);
        return;
      }
    }
    const stableIndex = model.entries.findIndex((entry) => entry.wallpaperId === centeredIdRef.current);
    if (stableIndex >= 0) {
      if (stableIndex !== centeredIndexRef.current) centerAtIndex(stableIndex);
      return;
    }
    centerAtIndex(Math.min(centeredIndexRef.current, model.entries.length - 1));
  }, [
    centerAtIndex,
    initialIndex,
    model.currentObservationReady,
    model.currentPath,
    model.entries,
    model.replaceCount,
    model.resetKey,
  ]);

  useEffect(() => {
    if (focusToken <= 0) return;
    window.requestAnimationFrame(() => streamRef.current?.focus());
  }, [focusToken]);

  useEffect(() => {
    const entry = model.entries[centeredIndex];
    if (entry) onAnchorChange?.(entry.wallpaperId);
  }, [centeredIndex, model.entries, onAnchorChange]);

  useEffect(() => {
    const stream = streamRef.current;
    const range = stream
      ? visibleMeasuredFlowThumbnailRange({
        items: virtualizer.getVirtualItems().map((item) => ({
          id: model.entries[item.index]?.wallpaperId ?? item.index,
          index: item.index,
          start: item.start,
          size: item.size,
        })),
        itemCount: model.entries.length,
        viewportStart: stream.scrollTop,
        viewportSize: stream.clientHeight,
        overscan: 2,
      })
      : null;
    if (!range) return;
    const paths = model.entries
      .slice(range.startIndex, range.endIndex + 1)
      .map(staticPreviewAssetPath);
    const key = `${range.startIndex}:${range.endIndex}:${paths.join('\0')}`;
    if (key === lastThumbnailKeyRef.current) return;
    lastThumbnailKeyRef.current = key;
    enqueueVisible(paths, { priority: 'front' });
  }, [centeredIndex, enqueueVisible, model.entries, virtualizer, virtualizer.range]);

  useEffect(() => {
    const range = virtualizer.range;
    if (!range || model.automaticAppendPaused) return;
    if (!shouldRequestFlowNextPage({
      itemCount: model.entries.length,
      visibleEndIndex: range.endIndex,
      hasMore: model.hasMore,
      loadingMore: model.loadingMore,
      refreshing: model.refreshing,
      automaticAppendPaused: model.automaticAppendPaused,
    })) return;
    void model.onLoadMore();
  }, [
    model.automaticAppendPaused,
    model.entries.length,
    model.hasMore,
    model.loadingMore,
    model.onLoadMore,
    model.refreshing,
    virtualizer.range?.endIndex,
  ]);

  useEffect(() => {
    endLoadRequestedRef.current = false;
  }, [model.entries.length, model.hasMore]);

  useEffect(() => {
    const hoveredWallpaperId = hoveredWallpaperIdRef.current;
    if (hoveredWallpaperId !== null
      && !model.entries.some((entry) => entry.wallpaperId === hoveredWallpaperId)) {
      handleFlowHover(null);
      return;
    }
    applyFlowHover(flowRef.current, hoveredWallpaperId);
  }, [
    handleFlowHover,
    model.entries,
    virtualizer.range?.endIndex,
    virtualizer.range?.startIndex,
  ]);

  useEffect(() => {
    const shouldSample = import.meta.env.DEV || localStorage.getItem('wc.debug.metrics') === 'on';
    if (!shouldSample) return undefined;
    const sample = () => {
      recordMetric('library.flow.renderedItems', virtualizer.getVirtualItems().length);
      recordMetric('library.flow.centeredIndex', centeredIndexRef.current);
    };
    sample();
    const timer = window.setInterval(sample, FLOW_METRICS_SAMPLE_MS);
    return () => window.clearInterval(timer);
  }, [virtualizer]);

  useEffect(() => () => {
    initializedRef.current = false;
    cancelInitialCenterFrame();
    cancelCenterCalculationFrame();
    clearMotionTimers();
    programmaticTargetIndexRef.current = null;
    setRevealPaused(false);
  }, [
    cancelCenterCalculationFrame,
    cancelInitialCenterFrame,
    clearMotionTimers,
    setRevealPaused,
  ]);

  const centeredEntry = model.entries[centeredIndex] ?? null;
  const localRange = localFlowIndexWindow(centeredIndex, model.entries.length);
  const localEntries = useMemo<FlowIndexRailEntry[]>(() => {
    if (!localRange) return [];
    return model.entries
      .slice(localRange.startIndex, localRange.endIndex + 1)
      .map((entry, offset) => ({
        entry,
        index: localRange.startIndex + offset,
        selected: model.selectedPath === entry.path,
        current: model.currentPath === entry.path,
        favorite: entry.favorite,
      }));
  }, [localRange?.endIndex, localRange?.startIndex, model.currentPath, model.entries, model.selectedPath]);

  const selectEntry = useCallback((entry: LibraryBrowserItemDTO) => {
    markUserInteraction();
    const index = model.entries.findIndex((candidate) => candidate.wallpaperId === entry.wallpaperId);
    if (index < 0) return;
    centerAtIndex(index, true);
    model.onSelect(entry);
    window.requestAnimationFrame(() => streamRef.current?.focus());
  }, [centerAtIndex, markUserInteraction, model.entries, model.onSelect]);

  const applyEntry = useCallback((entry: LibraryBrowserItemDTO) => {
    if (!model.canApplyToDisplay || !model.isEntryApplicable(entry)) return;
    markUserInteraction();
    model.onSelect(entry);
    model.onApply(entry);
    window.requestAnimationFrame(() => streamRef.current?.focus());
  }, [
    markUserInteraction,
    model.canApplyToDisplay,
    model.isEntryApplicable,
    model.onApply,
    model.onSelect,
  ]);

  const openContextMenu = useCallback((entry: LibraryBrowserItemDTO, x: number, y: number) => {
    markUserInteraction();
    const index = model.entries.findIndex((candidate) => candidate.wallpaperId === entry.wallpaperId);
    if (index >= 0) centerAtIndex(index);
    streamRef.current?.focus({ preventScroll: true });
    setContextMenu({ entry, x, y });
  }, [centerAtIndex, markUserInteraction, model.entries]);

  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.target !== event.currentTarget) return;
    const activeIndex = centeredIndexRef.current;
    const pageStep = flowPageStep({
      viewportSize: dimensions.height,
      itemSize: estimateEntrySize(activeIndex),
      gap: 0,
    });
    const intent = resolveFlowKey({
      key: event.key,
      currentIndex: activeIndex,
      itemCount: model.entries.length,
      pageStep,
      ctrlKey: event.ctrlKey,
      metaKey: event.metaKey,
      shiftKey: event.shiftKey,
      hasMore: model.hasMore && !model.refreshing,
      loadingMore: model.loadingMore,
      endLoadRequestAllowed: !endLoadRequestedRef.current,
    });
    if (!intent) return;
    cancelCenterCalculationFrame();
    markUserInteraction();
    event.preventDefault();
    event.stopPropagation();
    const entry = model.entries[intent.index];
    if (!entry) return;
    if (intent.type === 'navigate') {
      centerAtIndex(intent.index);
      if (intent.requestLoadMore && !endLoadRequestedRef.current) {
        endLoadRequestedRef.current = true;
        void model.onLoadMore();
      }
      return;
    }
    if (intent.type === 'select') {
      model.onSelect(entry);
      return;
    }
    if (intent.type === 'apply') {
      applyEntry(entry);
      return;
    }
    const option = document.getElementById(`flow-option-${entry.wallpaperId}`);
    const rect = option?.getBoundingClientRect();
    openContextMenu(entry, (rect?.left ?? 16) + 12, (rect?.top ?? 16) + 12);
  };

  const activateIndexEntry = (entry: LibraryBrowserItemDTO) => {
    setIndexOpen(false);
    selectEntry(entry);
  };
  const returnToTop = () => {
    centerAtIndex(0);
    setShowReturnToTop(false);
    window.requestAnimationFrame(() => streamRef.current?.focus());
  };
  const activeQueueName = model.activePath && model.activePath !== centeredEntry?.path
    ? entryLabelForPath(model.entries, model.activePath)
    : null;
  const pendingQueueName = model.pendingPath && model.pendingPath !== centeredEntry?.path
    ? entryLabelForPath(model.entries, model.pendingPath)
    : null;
  const centeredApplicable = centeredEntry !== null
    && model.canApplyToDisplay
    && model.isEntryApplicable(centeredEntry);

  if (model.entries.length === 0) {
    return <div className="empty-state">No wallpapers found</div>;
  }

  return (
    <section
      aria-label="Flow wallpaper browser"
      className={`wallpaper-flow${model.refreshing ? ' is-refreshing' : ''}`}
      data-active={interactionActive || undefined}
      data-scrolling={!settled || undefined}
      ref={flowRef}
    >
      <FlowIndexRail
        centeredWallpaperId={centeredEntry?.wallpaperId ?? null}
        entries={localEntries}
        loadedCount={model.entries.length}
        onActivate={selectEntry}
        onHover={handleFlowHover}
        onOpenIndex={() => setIndexOpen(true)}
        total={model.total}
        totalKnown={model.totalKnown}
      />

      <div
        aria-activedescendant={centeredEntry ? `flow-option-${centeredEntry.wallpaperId}` : undefined}
        aria-label="Wallpaper Flow"
        className="flow-preview-stream"
        onKeyDown={handleKeyDown}
        onPointerDown={handlePointerDown}
        onPointerCancel={finishPointerInteraction}
        onPointerUp={finishPointerInteraction}
        onScroll={handleScroll}
        onWheel={handleWheel}
        ref={streamRef}
        role="listbox"
        tabIndex={0}
      >
        <div
          className="flow-preview-stream__virtual"
          style={{ height: virtualizer.getTotalSize(), position: 'relative' }}
        >
          {virtualizer.getVirtualItems().map((row) => {
            const entry = model.entries[row.index];
            if (!entry) return null;
            const centered = centeredEntry?.wallpaperId === entry.wallpaperId;
            const selected = model.selectedPath === entry.path;
            const current = model.currentPath === entry.path;
            const applying = model.applying && model.activePath === entry.path;
            const pending = model.pendingPath === entry.path;
            const presentation = flowStatePresentation('flow-preview-item', {
              active: interactionActive,
              settled,
              centered,
              hovered: false,
              selected,
              current,
              applying,
              pending,
              favorite: entry.favorite,
            });
            const aspect = resolutionAspectRatio(entry.resolution);
            const style = {
              position: 'absolute',
              insetInline: 0,
              top: 0,
              minHeight: row.size,
              transform: `translateY(${row.start}px)`,
              '--flow-media-aspect': String(aspect),
            } as CSSProperties;
            return (
              <div
                {...presentation.attributes}
                aria-current={current ? 'true' : undefined}
                aria-label={`${row.index + 1}. ${displayName(entry)}`}
                aria-selected={selected}
                className={presentation.className}
                data-aspect={aspectClass(entry)}
                data-index={row.index}
                data-wallpaper-id={entry.wallpaperId}
                data-wallpaper-path={entry.path}
                id={`flow-option-${entry.wallpaperId}`}
                key={row.key}
                onClick={() => selectEntry(entry)}
                onContextMenu={(event: MouseEvent<HTMLDivElement>) => {
                  event.preventDefault();
                  openContextMenu(entry, event.clientX, event.clientY);
                }}
                onDoubleClick={() => applyEntry(entry)}
                onPointerEnter={() => handleFlowHover(entry.wallpaperId)}
                onPointerLeave={() => handleFlowHover(null)}
                ref={virtualizer.measureElement}
                role="option"
                style={style}
              >
                <div className="flow-preview-item__media">
                  <WallpaperPreviewMedia
                    alt=""
                    eligibility={{
                      active: interactionActive && !contextMenu,
                      centered,
                      selected,
                      settled,
                      reducedMotion,
                    }}
                    entry={entry}
                    loading={centered ? 'eager' : 'lazy'}
                  />
                  <span aria-hidden="true" className="flow-preview-item__ordinal">
                    {String(row.index + 1).padStart(2, '0')}
                  </span>
                  <span className="flow-preview-item__states">
                    {flowStateLabels({
                      selected,
                      current,
                      applying,
                      pending,
                      favorite: entry.favorite,
                    }).join(' · ')}
                  </span>
                </div>
              </div>
            );
          })}
        </div>
      </div>

      <FlowMetadataRail
        activeQueueName={activeQueueName}
        allViewed={!model.hasMore && !model.loadingMore}
        applyAvailable={centeredApplicable}
        applyDisabledReason={centeredEntry?.applyReason || centeredEntry?.unsupportedReason || (
          !model.canApplyToDisplay ? 'The selected display is unavailable.' : null
        )}
        applying={Boolean(centeredEntry && model.applying && model.activePath === centeredEntry.path)}
        centeredEntry={centeredEntry}
        centeredIndex={centeredIndex}
        current={Boolean(centeredEntry && model.currentPath === centeredEntry.path)}
        favorite={centeredEntry?.favorite ?? false}
        favoritePending={Boolean(centeredEntry && model.favoritePendingPaths.has(centeredEntry.path))}
        loadedCount={model.entries.length}
        onApply={applyEntry}
        onDetails={(entry) => model.onDetails(
          entry,
          document.activeElement instanceof HTMLElement ? document.activeElement : null,
        )}
        onFavorite={(entry) => { void model.onToggleFavorite(entry); }}
        onReturnToTop={returnToTop}
        pending={Boolean(centeredEntry && model.pendingPath === centeredEntry.path)}
        pendingQueueName={pendingQueueName}
        selected={Boolean(centeredEntry && model.selectedPath === centeredEntry.path)}
        showReturnToTop={showReturnToTop}
        total={model.totalKnown ? model.total : null}
        totalKnown={model.totalKnown}
      />

      <FlowIndexDialog
        centeredWallpaperId={centeredEntry?.wallpaperId ?? null}
        currentPath={model.currentPath}
        entries={model.entries}
        onActivate={activateIndexEntry}
        onClose={() => {
          setIndexOpen(false);
          window.requestAnimationFrame(() => streamRef.current?.focus());
        }}
        open={indexOpen}
        selectedPath={model.selectedPath}
        total={model.total}
        totalKnown={model.totalKnown}
      />

      {contextMenu ? (
        <ContextMenu
          actions={model.buildContextActions(contextMenu.entry)}
          onClose={() => setContextMenu(null)}
          path={contextMenu.entry.path}
          x={contextMenu.x}
          y={contextMenu.y}
        />
      ) : null}
    </section>
  );
}

export default memo(WallpaperFlowImpl);
