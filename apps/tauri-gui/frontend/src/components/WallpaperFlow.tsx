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
  type WheelEvent as ReactWheelEvent,
} from 'react';
import {
  defaultRangeExtractor,
  useVirtualizer,
  type Range,
} from '@tanstack/react-virtual';

import type { LibraryBrowserItemDTO } from '../api/types.ts';
import { recordMetric } from '../perf/metrics.ts';
import { resolveCardPointerInteraction } from '../shell/cardInteraction.ts';
import type { ApplyGesture } from '../shell/shellPreferences.ts';
import { useThumbnailStore } from '../state/ThumbnailStoreContext.tsx';
import { ApplyIndicator } from './ApplyIndicator.tsx';
import ContextMenu from './ContextMenu.tsx';
import FlowIndexDialog from './FlowIndexDialog.tsx';
import FlowIndexRail, { type FlowIndexRailEntry } from './FlowIndexRail.tsx';
import FlowMetadataRail from './FlowMetadataRail.tsx';
import WallpaperPreviewMedia from './WallpaperPreviewMedia.tsx';
import { applyFlowHover } from './flowHoverDom.ts';
import type { FlowMotionKind } from './flowInteractionController.ts';
import { FlowMomentumController } from './flowMomentum.ts';
import {
  libraryEntryApplyDisabledReason,
  resolveLibraryFlowStartupAnchor,
  type LibraryViewModel,
} from './libraryViewModel.ts';
import { displayName } from './wallpaperCardHelpers.ts';
import {
  estimateFlowItemSize,
  flowPageStep,
  flowScrollBehavior,
  flowStateLabels,
  flowStatePresentation,
  localFlowIndexWindow,
  retainFlowActiveIndex,
  resolveFlowKey,
  shouldRequestFlowNextPage,
  visibleMeasuredFlowThumbnailRange,
} from './wallpaperFlowModel.ts';
import { staticPreviewAssetPath } from './wallpaperPreviewMedia.ts';
import { useFlowInteraction } from './useFlowInteraction.ts';

const FLOW_ITEM_GAP = 84;
const FLOW_OVERSCAN = 4;
const FLOW_METRICS_SAMPLE_MS = 500;
const FLOW_RESIZE_REANCHOR_IDLE_MS = 120;
const FLOW_MOMENTUM_CAPTURE_DELAY_MS = 32;
const FLOW_WHEEL_BURST_RESET_MS = 160;
const FLOW_DIRECT_INPUT_IDLE_MS = FLOW_WHEEL_BURST_RESET_MS;
const FLOW_INDEX_ROW_HEIGHT = 31;
const FLOW_INDEX_INITIAL_VIEWPORT_HEIGHT = FLOW_INDEX_ROW_HEIGHT * 15;
const FLOW_UNCLAIMED_NAVIGATION_KEYS = new Set([
  'ArrowDown',
  'ArrowUp',
  'End',
  'Home',
  'PageDown',
  'PageUp',
]);

type FlowKeyEvent = Pick<
  globalThis.KeyboardEvent,
  | 'altKey'
  | 'ctrlKey'
  | 'key'
  | 'metaKey'
  | 'preventDefault'
  | 'shiftKey'
  | 'stopPropagation'
>;

export interface WallpaperFlowProps {
  readonly model: LibraryViewModel;
  readonly applyGesture?: ApplyGesture;
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

interface PendingProgrammaticCommit {
  readonly index: number;
  readonly sequence: number;
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

function updateFlowVisualProximity(
  stream: HTMLElement,
  itemCount: number,
): number | null {
  const streamBounds = stream.getBoundingClientRect();
  const streamCenter = streamBounds.top + streamBounds.height / 2;
  let nearestIndex: number | null = null;
  let nearestDelta = Number.POSITIVE_INFINITY;
  const proximityUpdates: Array<{
    index: number;
    item: HTMLElement;
    proximity: number;
  }> = [];
  for (const item of stream.querySelectorAll<HTMLElement>('.flow-preview-item')) {
    const index = Number(item.dataset.index);
    if (!Number.isInteger(index) || index < 0 || index >= itemCount) continue;
    const bounds = item.getBoundingClientRect();
    const delta = Math.abs(bounds.top + bounds.height / 2 - streamCenter);
    const proximitySpan = Math.max(1, (bounds.height + FLOW_ITEM_GAP) * 1.5);
    const proximity = Math.max(0, 1 - delta / proximitySpan);
    proximityUpdates.push({ index, item, proximity });
    if (delta >= nearestDelta) continue;
    nearestIndex = index;
    nearestDelta = delta;
  }
  for (const { index, item, proximity } of proximityUpdates) {
    item.style.setProperty('--flow-center-proximity', String(proximity));
    item.toggleAttribute('data-flow-visual-focus', index === nearestIndex);
  }
  return nearestIndex;
}

function isRenderedFlowItemCentered(
  stream: HTMLElement | null,
  index: number,
): boolean | null {
  const option = stream?.querySelector<HTMLElement>(`[data-index="${index}"]`);
  const optionBounds = option?.getBoundingClientRect();
  const streamBounds = stream?.getBoundingClientRect();
  if (!optionBounds || !streamBounds) return null;
  const signedDelta = optionBounds.top + optionBounds.height / 2
    - (streamBounds.top + streamBounds.height / 2);
  const tolerance = Math.max(3, Math.min(18, optionBounds.height * 0.035));
  return Math.abs(signedDelta) <= tolerance;
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

function WallpaperFlowReady({
  model,
  applyGesture = 'single',
  initialAnchorWallpaperId = null,
  focusToken = 0,
  onAnchorChange,
}: WallpaperFlowProps) {
  const flowRef = useRef<HTMLElement>(null);
  const streamRef = useRef<HTMLDivElement>(null);
  const { observeVisible, setScrolling, setInteracting } = useThumbnailStore();
  const reducedMotion = useReducedMotion();
  const initialAnchor = resolveLibraryFlowStartupAnchor(
    model.entries,
    initialAnchorWallpaperId,
    model.currentPath,
  );
  const initialIndex = initialAnchor?.index ?? 0;
  const flowInteraction = useFlowInteraction({
    initialAnchor: initialAnchor && model.entries[initialAnchor.index] ? {
      id: model.entries[initialAnchor.index]!.wallpaperId,
      index: initialAnchor.index,
    } : null,
    directStartup: initialAnchorWallpaperId === null,
    currentObservationReady: model.currentObservationReady,
    resetKey: model.resetKey,
    replaceCount: model.replaceCount,
    queryReplacementPending: model.queryReplacementPending && model.entries.length > 0,
  });
  const {
    controller: interactionController,
    snapshot: interactionSnapshot,
    update: updateInteraction,
  } = flowInteraction;
  const [centeredIndex, setCenteredIndex] = useState(initialIndex);
  const [indexRailIndex, setIndexRailIndex] = useState(initialIndex);
  const [indexRailViewportHeight, setIndexRailViewportHeight] = useState(
    FLOW_INDEX_INITIAL_VIEWPORT_HEIGHT,
  );
  const [pendingProgrammaticCommit, setPendingProgrammaticCommit] =
    useState<PendingProgrammaticCommit | null>(null);
  const programmaticCommitSequenceRef = useRef(0);
  const hoveredWallpaperIdRef = useRef<number | null>(null);
  const settled = interactionSnapshot.settled && pendingProgrammaticCommit === null;
  const [indexOpen, setIndexOpen] = useState(false);
  const [pageVisible, setPageVisible] = useState(() => (
    typeof document === 'undefined' || document.visibilityState !== 'hidden'
  ));
  const [windowFocused, setWindowFocused] = useState(() => (
    typeof document === 'undefined' || document.hasFocus()
  ));
  const [dimensions, setDimensions] = useState<FlowDimensions>({ width: 720, height: 600 });
  const dimensionsRef = useRef(dimensions);
  dimensionsRef.current = dimensions;
  const [showReturnToTop, setShowReturnToTop] = useState(false);
  const [contextMenu, setContextMenu] = useState<FlowContextMenuState | null>(null);
  const settleTimerRef = useRef<number | null>(null);
  const scrollEndFallbackTimerRef = useRef<number | null>(null);
  const scrollCenterFrameRef = useRef<number | null>(null);
  const initialCenterFrameRef = useRef<number | null>(null);
  const scrollingRef = useRef(false);
  const directInputActiveRef = useRef(false);
  const directInputDirectionRef = useRef(0);
  const directInputLastEventAtRef = useRef(Number.NEGATIVE_INFINITY);
  const scrollLifecycleObservedRef = useRef(false);
  const settleScrollLifecycleRef = useRef<() => void>(() => {});
  const suppressProgrammaticScrollRef = useRef(false);
  const momentumControllerRef = useRef<FlowMomentumController | null>(null);
  momentumControllerRef.current ??= new FlowMomentumController();
  const lastThumbnailKeyRef = useRef('');
  const pointerInteractionRef = useRef(false);
  const previousDimensionsRef = useRef<FlowDimensions | null>(null);
  const resizeReanchorTimerRef = useRef<number | null>(null);
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
    directDomUpdates: true,
    directDomUpdatesMode: 'transform',
    getScrollElement: () => streamRef.current,
    estimateSize: estimateEntrySize,
    measureElement: (element) => {
      const index = Number(element.getAttribute('data-index'));
      return Number.isInteger(index) ? estimateEntrySize(index) : estimateEntrySize(0);
    },
    getItemKey: (index) => model.entries[index]?.wallpaperId ?? index,
    overscan: FLOW_OVERSCAN,
    paddingStart: centerPaddingStart,
    paddingEnd: centerPaddingEnd,
    rangeExtractor: extractVirtualRange,
    isScrollingResetDelay: 150,
    useScrollendEvent: true,
    onChange: (_instance, isScrolling) => {
      if (suppressProgrammaticScrollRef.current) return;
      if (isScrolling) {
        scrollLifecycleObservedRef.current = true;
        return;
      }
      if (!scrollLifecycleObservedRef.current) return;
      scrollLifecycleObservedRef.current = false;
      settleScrollLifecycleRef.current();
    },
  });

  const clearMotionTimers = useCallback(() => {
    if (settleTimerRef.current !== null) {
      window.clearTimeout(settleTimerRef.current);
      settleTimerRef.current = null;
    }
    if (scrollEndFallbackTimerRef.current !== null) {
      window.clearTimeout(scrollEndFallbackTimerRef.current);
      scrollEndFallbackTimerRef.current = null;
    }
  }, []);

  const clearResizeReanchorTimer = useCallback(() => {
    if (resizeReanchorTimerRef.current !== null) {
      window.clearTimeout(resizeReanchorTimerRef.current);
      resizeReanchorTimerRef.current = null;
    }
  }, []);

  const cancelResizeReanchor = useCallback(() => {
    clearResizeReanchorTimer();
    updateInteraction((controller) => controller.cancelResize());
  }, [clearResizeReanchorTimer, updateInteraction]);

  const cancelInitialCenterFrame = useCallback(() => {
    if (initialCenterFrameRef.current === null) return;
    window.cancelAnimationFrame(initialCenterFrameRef.current);
    initialCenterFrameRef.current = null;
  }, []);

  const markUserInteraction = useCallback(() => {
    updateInteraction((controller) => controller.beginDirectInput());
  }, [updateInteraction]);

  const markCentered = useCallback((index: number) => {
    const entry = model.entries[index];
    if (!entry) return;
    const committed = updateInteraction((controller) => controller.finishProgrammatic({
      id: entry.wallpaperId,
      index,
    }));
    if (!committed) return;
    setCenteredIndex(index);
    setIndexRailIndex(index);
    onAnchorChange?.(entry.wallpaperId);
  }, [model.entries, onAnchorChange, updateInteraction]);

  const finishProgrammaticScroll = useCallback((targetIndex: number) => {
    if (interactionController.snapshot().programmaticTarget?.index !== targetIndex) return;
    setCenteredIndex(targetIndex);
    setIndexRailIndex(targetIndex);
    programmaticCommitSequenceRef.current += 1;
    setPendingProgrammaticCommit({
      index: targetIndex,
      sequence: programmaticCommitSequenceRef.current,
    });
  }, [interactionController]);

  const scheduleProgrammaticSettle = useCallback((
    targetIndex: number,
    waitForScrollQuiescence: boolean,
  ) => {
    let attempts = 0;
    let centeredChecks = 0;
    let lastScrollTop: number | null = null;
    let lastMovementAt = performance.now();
    const checkTarget = () => {
      settleTimerRef.current = null;
      if (interactionController.snapshot().programmaticTarget?.index !== targetIndex) return;
      const stream = streamRef.current;
      const now = performance.now();
      if (stream) {
        if (lastScrollTop !== null && Math.abs(stream.scrollTop - lastScrollTop) >= 0.5) {
          lastMovementAt = now;
        }
        lastScrollTop = stream.scrollTop;
      }
      if (isRenderedFlowItemCentered(stream, targetIndex) === true) {
        centeredChecks += 1;
        const scrollIsQuiet = !waitForScrollQuiescence
          || now - lastMovementAt >= FLOW_MOMENTUM_CAPTURE_DELAY_MS;
        if (centeredChecks >= 2 && scrollIsQuiet) {
          finishProgrammaticScroll(targetIndex);
          return;
        }
        settleTimerRef.current = window.setTimeout(checkTarget, 16);
        return;
      }
      centeredChecks = 0;
      attempts += 1;
      if (attempts >= 40) {
        virtualizer.scrollToIndex(targetIndex, { align: 'center', behavior: 'auto' });
        attempts = 0;
        settleTimerRef.current = window.setTimeout(checkTarget, 50);
        return;
      }
      settleTimerRef.current = window.setTimeout(
        checkTarget,
        waitForScrollQuiescence ? 50 : 16,
      );
    };
    settleTimerRef.current = window.setTimeout(checkTarget, 0);
  }, [finishProgrammaticScroll, interactionController, virtualizer]);

  const centerAtIndex = useCallback((
    index: number,
    direct = false,
    kind: FlowMotionKind = 'smooth',
  ) => {
    const entry = model.entries[index];
    if (!entry) return;
    cancelResizeReanchor();
    cancelInitialCenterFrame();
    clearMotionTimers();
    setPendingProgrammaticCommit(null);
    directInputActiveRef.current = false;
    streamRef.current?.setAttribute('data-direct-scroll', 'true');
    suppressProgrammaticScrollRef.current = true;
    scrollLifecycleObservedRef.current = false;
    updateInteraction((controller) => controller.beginProgrammatic({
      id: entry.wallpaperId,
      index,
    }, kind));
    setIndexRailIndex(index);
    scrollingRef.current = true;
    setScrolling(true);
    const targetRendered = virtualizer.getVirtualItems().some((item) => item.index === index);
    const immediate = direct || reducedMotion || !targetRendered;
    if (immediate) {
      streamRef.current?.setAttribute('data-instant-navigation', 'true');
    } else {
      streamRef.current?.removeAttribute('data-instant-navigation');
    }
    virtualizer.scrollToIndex(index, {
      align: 'center',
      behavior: flowScrollBehavior(reducedMotion, immediate),
    });
    scheduleProgrammaticSettle(index, !immediate);
  }, [
    cancelInitialCenterFrame,
    clearMotionTimers,
    cancelResizeReanchor,
    model.entries,
    reducedMotion,
    scheduleProgrammaticSettle,
    setScrolling,
    updateInteraction,
    virtualizer,
  ]);

  const reanchorAfterResize = useCallback((index: number) => {
    const entry = model.entries[index];
    if (!entry) return;
    cancelInitialCenterFrame();
    clearMotionTimers();
    directInputActiveRef.current = false;
    streamRef.current?.setAttribute('data-direct-scroll', 'true');
    suppressProgrammaticScrollRef.current = true;
    scrollLifecycleObservedRef.current = false;
    updateInteraction((controller) => controller.beginProgrammatic({
      id: entry.wallpaperId,
      index,
    }, 'resize'));
    scrollingRef.current = true;
    setScrolling(true);

    const alignToTarget = () => {
      virtualizer.measure();
      virtualizer.scrollToIndex(index, { align: 'center', behavior: 'auto' });
    };

    window.requestAnimationFrame(() => {
      window.requestAnimationFrame(() => {
        alignToTarget();
        window.requestAnimationFrame(() => {
          alignToTarget();
          finishProgrammaticScroll(index);
        });
      });
    });
  }, [
    cancelInitialCenterFrame,
    clearMotionTimers,
    finishProgrammaticScroll,
    model.entries,
    setScrolling,
    updateInteraction,
    virtualizer,
  ]);

  useEffect(() => {
    if (pendingProgrammaticCommit === null) return undefined;
    const pending = pendingProgrammaticCommit;
    let frame: number | null = null;
    let centeredFrames = 0;
    let commitRenderedAnchor =
      interactionController.snapshot().programmaticTarget?.index === pending.index;

    const clearPending = () => {
      setPendingProgrammaticCommit((current) => (
        current?.sequence === pending.sequence ? null : current
      ));
    };
    const finalizeSettledAnchor = () => {
      directInputActiveRef.current = false;
      streamRef.current?.removeAttribute('data-direct-scroll');
      streamRef.current?.removeAttribute('data-instant-navigation');
      suppressProgrammaticScrollRef.current = false;
      clearPending();
      scrollingRef.current = false;
      setScrolling(false);
      setInteracting(interactionActiveRef.current);
    };
    const verifyAnchor = () => {
      frame = null;
      const snapshot = interactionController.snapshot();
      const expectedIndex = commitRenderedAnchor
        ? snapshot.programmaticTarget?.index
        : snapshot.committedAnchor?.index;
      if (expectedIndex !== pending.index) {
        clearPending();
        return;
      }
      const stream = streamRef.current;
      const isCentered = isRenderedFlowItemCentered(stream, pending.index);
      if (!stream || isCentered === null) {
        centeredFrames = 0;
        virtualizer.scrollToIndex(pending.index, { align: 'center', behavior: 'auto' });
        frame = window.requestAnimationFrame(verifyAnchor);
        return;
      }
      if (!isCentered) {
        centeredFrames = 0;
        virtualizer.scrollToIndex(pending.index, { align: 'center', behavior: 'auto' });
        frame = window.requestAnimationFrame(verifyAnchor);
        return;
      }
      centeredFrames += 1;
      if (centeredFrames < 2) {
        frame = window.requestAnimationFrame(verifyAnchor);
        return;
      }
      if (!commitRenderedAnchor) {
        finalizeSettledAnchor();
        return;
      }
      markCentered(pending.index);
      commitRenderedAnchor = false;
      centeredFrames = 0;
      frame = window.requestAnimationFrame(verifyAnchor);
    };

    frame = window.requestAnimationFrame(verifyAnchor);
    return () => {
      if (frame !== null) window.cancelAnimationFrame(frame);
    };
  }, [
    interactionController,
    markCentered,
    pendingProgrammaticCommit,
    setInteracting,
    setScrolling,
    virtualizer,
  ]);

  const computeCentered = useCallback((): number | null => {
    const snapshot = interactionController.snapshot();
    if (snapshot.resizeAnchorId !== null || snapshot.phase === 'resize') return null;
    const stream = streamRef.current;
    if (!stream) return null;
    const nearestIndex = updateFlowVisualProximity(stream, model.entries.length);
    if (nearestIndex !== null) {
      const entry = model.entries[nearestIndex];
      if (!entry) return null;
      const tracked = updateInteraction((controller) => controller.trackCandidate({
        id: entry.wallpaperId,
        index: nearestIndex,
      }));
      if (tracked || snapshot.programmaticTarget !== null) {
        setIndexRailIndex((current) => current === nearestIndex ? current : nearestIndex);
      }
    }
    return nearestIndex;
  }, [interactionController, model.entries, updateInteraction]);

  const scheduleMovingCenterUpdate = useCallback(() => {
    if (scrollCenterFrameRef.current !== null) return;
    scrollCenterFrameRef.current = window.requestAnimationFrame(() => {
      scrollCenterFrameRef.current = null;
      computeCentered();
    });
  }, [computeCentered]);

  const settleScrollLifecycle = useCallback(() => {
    if (scrollEndFallbackTimerRef.current !== null) {
      window.clearTimeout(scrollEndFallbackTimerRef.current);
      scrollEndFallbackTimerRef.current = null;
    }
    if (!scrollingRef.current) {
      directInputActiveRef.current = false;
      streamRef.current?.removeAttribute('data-direct-scroll');
      return;
    }
    if (pointerInteractionRef.current) {
      scrollEndFallbackTimerRef.current = window.setTimeout(() => {
        scrollEndFallbackTimerRef.current = null;
        settleScrollLifecycleRef.current();
      }, FLOW_MOMENTUM_CAPTURE_DELAY_MS);
      return;
    }
    const directInputIdleFor = performance.now() - directInputLastEventAtRef.current;
    if (directInputIdleFor < FLOW_DIRECT_INPUT_IDLE_MS) {
      scrollEndFallbackTimerRef.current = window.setTimeout(() => {
        scrollEndFallbackTimerRef.current = null;
        settleScrollLifecycleRef.current();
      }, FLOW_DIRECT_INPUT_IDLE_MS - directInputIdleFor);
      return;
    }
    const snapshot = interactionController.snapshot();
    if (snapshot.resizeAnchorId !== null || snapshot.phase === 'resize') return;
    const programmaticTarget = snapshot.programmaticTarget;
    if (programmaticTarget !== null) {
      finishProgrammaticScroll(programmaticTarget.index);
      return;
    }
    const stream = streamRef.current;
    if (!stream) return;
    const targets = model.entries.flatMap((entry, index) => {
      const offset = virtualizer.getOffsetForIndex(index, 'center')?.[0];
      return offset === undefined ? [] : [{ index, offset }];
    });
    const target = momentumControllerRef.current?.capture({
      offset: stream.scrollTop,
      viewportSize: stream.clientHeight,
      targets,
      reducedMotion,
      onTarget: ({ index }) => {
        const entry = model.entries[index];
        if (!entry) return;
        directInputActiveRef.current = false;
        scrollLifecycleObservedRef.current = false;
        suppressProgrammaticScrollRef.current = true;
        updateInteraction((controller) => controller.beginProgrammatic({
          id: entry.wallpaperId,
          index,
        }, 'smooth'));
      },
      onUpdate: (offset) => {
        stream.scrollTop = offset;
        scheduleMovingCenterUpdate();
      },
      onComplete: ({ index }) => finishProgrammaticScroll(index),
    });
    if (target !== null && target !== undefined) {
      return;
    }
    directInputActiveRef.current = false;
    stream.removeAttribute('data-direct-scroll');
  }, [
    finishProgrammaticScroll,
    interactionController,
    model.entries,
    reducedMotion,
    scheduleMovingCenterUpdate,
    updateInteraction,
    virtualizer,
  ]);
  settleScrollLifecycleRef.current = settleScrollLifecycle;

  const cancelProgrammaticScroll = useCallback(() => {
    momentumControllerRef.current?.cancel();
    const stream = streamRef.current;
    stream?.removeAttribute('data-instant-navigation');
    if (suppressProgrammaticScrollRef.current && stream) {
      stream.scrollTo({ top: stream.scrollTop, behavior: 'auto' });
    }
    if (interactionController.snapshot().programmaticTarget !== null) {
      updateInteraction((controller) => controller.cancelProgrammatic());
    }
    setPendingProgrammaticCommit(null);
    suppressProgrammaticScrollRef.current = false;
  }, [interactionController, updateInteraction]);

  const preemptProgrammaticMotion = useCallback(() => {
    cancelInitialCenterFrame();
    clearMotionTimers();
    cancelProgrammaticScroll();
  }, [cancelInitialCenterFrame, cancelProgrammaticScroll, clearMotionTimers]);

  const armDirectInput = useCallback((continueMomentumGesture = false) => {
    const continuingDirectInput = directInputActiveRef.current;
    directInputActiveRef.current = true;
    if (!continuingDirectInput) {
      scrollLifecycleObservedRef.current = false;
    }
    const interactionSnapshot = interactionController.snapshot();
    const committedIndex = interactionSnapshot.committedAnchor?.index;
    const committedOffset = committedIndex === undefined
      ? undefined
      : virtualizer.getOffsetForIndex(committedIndex, 'center')?.[0];
    cancelResizeReanchor();
    preemptProgrammaticMotion();
    if (!continuingDirectInput) {
      const stream = streamRef.current;
      if (stream) {
        momentumControllerRef.current?.begin(
          stream.scrollTop,
          performance.now(),
          committedOffset ?? stream.scrollTop,
          continueMomentumGesture,
        );
      }
    }
    streamRef.current?.setAttribute('data-direct-scroll', 'true');
  }, [
    cancelResizeReanchor,
    interactionController,
    preemptProgrammaticMotion,
    virtualizer,
  ]);

  const scheduleScrollEndFallback = useCallback(() => {
    if (scrollEndFallbackTimerRef.current !== null) {
      window.clearTimeout(scrollEndFallbackTimerRef.current);
    }
    scrollEndFallbackTimerRef.current = window.setTimeout(() => {
      scrollEndFallbackTimerRef.current = null;
      settleScrollLifecycleRef.current();
    }, FLOW_DIRECT_INPUT_IDLE_MS);
  }, []);

  const handleScroll = useCallback(() => {
    const stream = streamRef.current;
    if (!stream) return;
    if (suppressProgrammaticScrollRef.current) {
      setShowReturnToTop(stream.scrollTop > stream.clientHeight);
      return;
    }
    if (!directInputActiveRef.current) {
      setShowReturnToTop(stream.scrollTop > stream.clientHeight);
      return;
    }
    if (!scrollingRef.current) {
      scrollingRef.current = true;
      setScrolling(true);
    }
    const snapshot = interactionController.snapshot();
    if (snapshot.resizeAnchorId !== null || snapshot.phase === 'resize') {
      setShowReturnToTop(stream.scrollTop > stream.clientHeight);
      return;
    }
    markUserInteraction();
    momentumControllerRef.current?.observe(
      stream.scrollTop,
      performance.now(),
      directInputDirectionRef.current,
    );
    cancelInitialCenterFrame();
    if (settleTimerRef.current !== null) {
      window.clearTimeout(settleTimerRef.current);
      settleTimerRef.current = null;
    }
    scrollingRef.current = true;
    setScrolling(true);
    scheduleMovingCenterUpdate();
    scheduleScrollEndFallback();
    setShowReturnToTop(stream.scrollTop > stream.clientHeight);
  }, [
    cancelInitialCenterFrame,
    interactionController,
    markUserInteraction,
    scheduleMovingCenterUpdate,
    scheduleScrollEndFallback,
    setScrolling,
  ]);

  const handleWheel = useCallback((event: ReactWheelEvent<HTMLDivElement>) => {
    const now = performance.now();
    const startsNewGesture =
      now - directInputLastEventAtRef.current > FLOW_WHEEL_BURST_RESET_MS;
    if (startsNewGesture) {
      directInputDirectionRef.current = 0;
    }
    directInputLastEventAtRef.current = now;
    if (event.deltaY !== 0) {
      directInputDirectionRef.current = Math.sign(event.deltaY);
    }
    armDirectInput(!startsNewGesture);
    scheduleScrollEndFallback();
  }, [armDirectInput, scheduleScrollEndFallback]);

  const handlePointerDown = useCallback(() => {
    directInputDirectionRef.current = 0;
    directInputLastEventAtRef.current = performance.now();
    pointerInteractionRef.current = true;
    armDirectInput();
  }, [armDirectInput]);

  const finishPointerInteraction = useCallback(() => {
    if (!pointerInteractionRef.current) return;
    pointerInteractionRef.current = false;
    if (!scrollingRef.current) {
      directInputActiveRef.current = false;
      streamRef.current?.removeAttribute('data-direct-scroll');
      return;
    }
    window.requestAnimationFrame(() => settleScrollLifecycleRef.current());
  }, []);

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
      if (next.width <= 0 || next.height <= 0) return;
      const current = dimensionsRef.current;
      if (
        interactionController.snapshot().phase !== 'unpositioned'
        && (next.width !== current.width || next.height !== current.height)
      ) {
        updateInteraction((controller) => controller.noteResize());
      }
      setDimensions(next);
    };
    update();
    const observer = new ResizeObserver(update);
    observer.observe(stream);
    return () => observer.disconnect();
  }, [interactionController, updateInteraction]);

  useEffect(() => {
    virtualizer.measure();
    const previous = previousDimensionsRef.current;
    previousDimensionsRef.current = dimensions;
    if (
      previous === null
      || (previous.width === dimensions.width && previous.height === dimensions.height)
      || interactionSnapshot.phase === 'unpositioned'
      || model.entries.length === 0
    ) {
      return undefined;
    }
    if (
      scrollingRef.current
      && interactionSnapshot.phase !== 'programmatic'
      && interactionSnapshot.phase !== 'resize'
    ) {
      updateInteraction((controller) => controller.cancelResize());
      return undefined;
    }
    const anchorId = interactionSnapshot.resizeAnchorId
      ?? interactionSnapshot.programmaticTarget?.id
      ?? interactionSnapshot.committedAnchor?.id
      ?? interactionSnapshot.trackingCandidate?.id
      ?? null;
    if (anchorId === null) return undefined;
    const stableIndex = model.entries.findIndex(
      (entry) => entry.wallpaperId === anchorId,
    );
    if (stableIndex < 0) {
      updateInteraction((controller) => controller.cancelResize());
      return undefined;
    }
    if (resizeReanchorTimerRef.current !== null) {
      window.clearTimeout(resizeReanchorTimerRef.current);
    }
    resizeReanchorTimerRef.current = window.setTimeout(() => {
      resizeReanchorTimerRef.current = null;
      reanchorAfterResize(stableIndex);
    }, FLOW_RESIZE_REANCHOR_IDLE_MS);
    return () => {
      if (resizeReanchorTimerRef.current !== null) {
        window.clearTimeout(resizeReanchorTimerRef.current);
        resizeReanchorTimerRef.current = null;
      }
    };
  }, [
    dimensions,
    interactionSnapshot.committedAnchor,
    interactionSnapshot.phase,
    interactionSnapshot.programmaticTarget,
    interactionSnapshot.resizeAnchorId,
    interactionSnapshot.trackingCandidate,
    model.entries,
    reanchorAfterResize,
    updateInteraction,
    virtualizer,
  ]);

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
    setInteracting(interactionActive);
    setScrolling(scrollingRef.current);
  }, [interactionActive, setInteracting, setScrolling]);

  useEffect(() => {
    const currentWallpaperId = model.currentPath === null
      ? null
      : model.entries.find((entry) => entry.path === model.currentPath)?.wallpaperId ?? null;
    const intent = updateInteraction((controller) => controller.observeDataset({
      wallpaperIds: model.entries.map((entry) => entry.wallpaperId),
      currentWallpaperId,
      currentObservationReady: model.currentObservationReady,
      resetKey: model.resetKey,
      replaceCount: model.replaceCount,
    }));
    if (!intent) return;
    if (intent.kind === 'startup') {
      initialCenterFrameRef.current = window.requestAnimationFrame(() => {
        initialCenterFrameRef.current = null;
        centerAtIndex(intent.anchor.index, intent.direct, intent.kind);
      });
      return;
    }
    centerAtIndex(intent.anchor.index, intent.direct, intent.kind);
  }, [
    centerAtIndex,
    model.currentObservationReady,
    model.currentPath,
    model.entries,
    model.replaceCount,
    model.resetKey,
    updateInteraction,
  ]);

  useEffect(() => {
    if (focusToken <= 0) return;
    const frame = window.requestAnimationFrame(() => {
      streamRef.current?.focus({ preventScroll: true });
    });
    return () => window.cancelAnimationFrame(frame);
  }, [focusToken]);

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
    observeVisible(paths, { priority: 'front' });
  }, [centeredIndex, observeVisible, model.entries, virtualizer, virtualizer.range]);

  useEffect(() => {
    const range = virtualizer.range;
    if (!range) return;
    if (!shouldRequestFlowNextPage({
      itemCount: model.entries.length,
      visibleEndIndex: range.endIndex,
      canAutoAppend: model.canAutoAppend,
      loadingMore: model.loadingMore,
      refreshing: model.refreshing,
    })) return;
    const requestKey = [
      model.resetKey,
      model.replaceCount,
      model.entries.length,
    ].join(':');
    if (!updateInteraction((controller) => controller.claimAppend(requestKey))) return;
    void model.onRequestMoreIfNeeded();
  }, [
    model.canAutoAppend,
    model.entries.length,
    model.loadingMore,
    model.onRequestMoreIfNeeded,
    model.replaceCount,
    model.refreshing,
    model.resetKey,
    updateInteraction,
    virtualizer.range?.endIndex,
  ]);

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
      recordMetric('library.flow.centeredIndex', interactionController.activeIndex(centeredIndex));
    };
    sample();
    const timer = window.setInterval(sample, FLOW_METRICS_SAMPLE_MS);
    return () => window.clearInterval(timer);
  }, [centeredIndex, interactionController, virtualizer]);

  useEffect(() => () => {
    clearResizeReanchorTimer();
    cancelInitialCenterFrame();
    if (scrollCenterFrameRef.current !== null) {
      window.cancelAnimationFrame(scrollCenterFrameRef.current);
      scrollCenterFrameRef.current = null;
    }
    clearMotionTimers();
    momentumControllerRef.current?.cancel();
    interactionController.abortAdapterMotion();
    interactionController.releaseAppend();
    streamRef.current?.removeAttribute('data-direct-scroll');
    streamRef.current?.removeAttribute('data-instant-navigation');
    suppressProgrammaticScrollRef.current = false;
    setScrolling(false);
    setInteracting(true);
  }, [
    cancelInitialCenterFrame,
    clearResizeReanchorTimer,
    clearMotionTimers,
    interactionController,
    setScrolling,
    setInteracting,
  ]);

  const centeredEntry = model.entries[centeredIndex] ?? null;
  const indexRailEntry = model.entries[indexRailIndex] ?? centeredEntry;
  const localRange = localFlowIndexWindow({
    centerIndex: indexRailIndex,
    itemCount: model.entries.length,
    railHeight: indexRailViewportHeight,
    rowHeight: FLOW_INDEX_ROW_HEIGHT,
  });
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

  const selectEntry = useCallback((entry: LibraryBrowserItemDTO, immediate = false) => {
    markUserInteraction();
    const index = model.entries.findIndex((candidate) => candidate.wallpaperId === entry.wallpaperId);
    if (index < 0) return;
    centerAtIndex(index, immediate);
    model.onSelect(entry);
    window.requestAnimationFrame(() => streamRef.current?.focus({ preventScroll: true }));
  }, [centerAtIndex, markUserInteraction, model.entries, model.onSelect]);

  const handleEntryClick = useCallback((
    event: MouseEvent<HTMLDivElement>,
    entry: LibraryBrowserItemDTO,
  ) => {
    const interaction = resolveCardPointerInteraction({
      gesture: applyGesture,
      clickCount: event.detail,
      canApply: model.canApplyToDisplay && model.isEntryApplicable(entry),
      fromControl: false,
    });
    if (!interaction.select) return;
    selectEntry(entry);
    if (interaction.apply) model.onApply(entry);
  }, [
    applyGesture,
    model.canApplyToDisplay,
    model.isEntryApplicable,
    model.onApply,
    selectEntry,
  ]);

  const applyEntry = useCallback((entry: LibraryBrowserItemDTO) => {
    if (!model.canApplyToDisplay || !model.isEntryApplicable(entry)) return;
    markUserInteraction();
    preemptProgrammaticMotion();
    model.onSelect(entry);
    model.onApply(entry);
    window.requestAnimationFrame(() => streamRef.current?.focus());
  }, [
    markUserInteraction,
    preemptProgrammaticMotion,
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

  const handleFlowKey = useCallback((event: FlowKeyEvent): boolean => {
    const activeIndex = interactionController.activeIndex(centeredIndex);
    const pageStep = flowPageStep({
      viewportSize: dimensions.height,
      itemSize: estimateEntrySize(activeIndex),
      gap: 0,
    });
    const intent = resolveFlowKey({
      key: event.key,
      currentIndex: activeIndex,
      itemCount: model.entries.length,
      selectedIndex: model.entries.findIndex((entry) => entry.path === model.selectedPath),
      pageStep,
      ctrlKey: event.ctrlKey,
      metaKey: event.metaKey,
      shiftKey: event.shiftKey,
      hasMore: model.canAppend && !model.refreshing,
      loadingMore: model.loadingMore,
      endLoadRequestAllowed: true,
    });
    if (!intent) return false;
    event.preventDefault();
    event.stopPropagation();
    const entry = model.entries[intent.index];
    if (!entry) return true;
    if (intent.type === 'navigate') {
      markUserInteraction();
      centerAtIndex(intent.index, false, 'navigation');
      const appendKey = [
        model.resetKey,
        model.replaceCount,
        model.entries.length,
      ].join(':');
      if (
        intent.requestLoadMore
        && updateInteraction((controller) => controller.claimAppend(appendKey))
      ) {
        void model.onAppendMore();
      }
      return true;
    }
    if (intent.type === 'select') {
      updateInteraction((controller) => controller.noteUserIntent());
      model.onSelect(entry);
      return true;
    }
    if (intent.type === 'apply') {
      if (!model.canApplyToDisplay || !model.isEntryApplicable(entry)) return true;
      updateInteraction((controller) => controller.noteUserIntent());
      model.onSelect(entry);
      model.onApply(entry);
      return true;
    }
    const option = document.getElementById(`flow-option-${entry.wallpaperId}`);
    const rect = option?.getBoundingClientRect();
    openContextMenu(entry, (rect?.left ?? 16) + 12, (rect?.top ?? 16) + 12);
    return true;
  }, [
    centerAtIndex,
    centeredIndex,
    dimensions.height,
    estimateEntrySize,
    interactionController,
    markUserInteraction,
    model,
    openContextMenu,
    updateInteraction,
  ]);

  const handleKeyDown = useCallback((event: KeyboardEvent<HTMLDivElement>) => {
    if (event.target !== event.currentTarget) return;
    handleFlowKey(event);
  }, [handleFlowKey]);

  useEffect(() => {
    if (!interactionActive) return;
    const handleUnclaimedKeyDown = (event: globalThis.KeyboardEvent) => {
      if (event.target !== document.body) return;
      if (!FLOW_UNCLAIMED_NAVIGATION_KEYS.has(event.key)) return;
      if (!handleFlowKey(event)) return;
      streamRef.current?.focus({ preventScroll: true });
    };
    window.addEventListener('keydown', handleUnclaimedKeyDown);
    return () => window.removeEventListener('keydown', handleUnclaimedKeyDown);
  }, [handleFlowKey, interactionActive]);

  const activateIndexEntry = (entry: LibraryBrowserItemDTO) => {
    setIndexOpen(false);
    selectEntry(entry, true);
  };
  const returnToTop = () => {
    centerAtIndex(0, true);
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
        centeredWallpaperId={indexRailEntry?.wallpaperId ?? null}
        entries={localEntries}
        loadedCount={model.entries.length}
        onActivate={selectEntry}
        onHover={handleFlowHover}
        onOpenIndex={() => setIndexOpen(true)}
        onViewportHeightChange={(height) => {
          setIndexRailViewportHeight((current) => current === height ? current : height);
        }}
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
          ref={virtualizer.containerRef}
          style={{ position: 'relative' }}
        >
          {virtualizer.getVirtualItems().map((row) => {
            const entry = model.entries[row.index];
            if (!entry) return null;
            const centered = centeredEntry?.wallpaperId === entry.wallpaperId;
            const selected = model.selectedPath === entry.path;
            const current = model.currentPath === entry.path;
            const applying = model.applying && model.activePath === entry.path;
            const pending = model.pendingPath === entry.path;
            const preloadStaticFallback = Math.abs(row.index - centeredIndex) <= 1;
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
              height: row.size,
              '--flow-media-aspect': String(aspect),
            } as CSSProperties;
            return (
              <div
                {...presentation.attributes}
                aria-current={current ? 'true' : undefined}
                aria-label={`${row.index + 1}. ${displayName(entry)}`}
                aria-posinset={row.index + 1}
                aria-selected={selected}
                aria-setsize={model.totalKnown && model.total !== null ? model.total : undefined}
                className={presentation.className}
                data-aspect={aspectClass(entry)}
                data-index={row.index}
                data-wallpaper-id={entry.wallpaperId}
                data-wallpaper-path={entry.path}
                id={`flow-option-${entry.wallpaperId}`}
                key={row.key}
                onClick={(event) => handleEntryClick(event, entry)}
                onContextMenu={(event: MouseEvent<HTMLDivElement>) => {
                  event.preventDefault();
                  openContextMenu(entry, event.clientX, event.clientY);
                }}
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
                    loading={preloadStaticFallback ? 'eager' : 'lazy'}
                    staticFallback={preloadStaticFallback}
                    stabilizeEntranceDuringMotion
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
                {applying || pending ? (
                  <div aria-hidden="true" className="flow-preview-item__indicator-layer">
                    <ApplyIndicator state={applying ? 'applying' : 'pending'} />
                  </div>
                ) : null}
              </div>
            );
          })}
        </div>
      </div>

      {showReturnToTop ? (
        <button
          aria-label="Return to first wallpaper"
          className="flow-return-to-top"
          data-flow-action="return"
          onClick={(event) => {
            event.stopPropagation();
            returnToTop();
          }}
          title="Return to first wallpaper"
          type="button"
        >
          <span aria-hidden="true">↑</span>
        </button>
      ) : null}

      <FlowMetadataRail
        activeQueueName={activeQueueName}
        allViewed={!model.canAppend && !model.loadingMore}
        applyAvailable={centeredApplicable}
        applyDisabledReason={centeredEntry
          ? libraryEntryApplyDisabledReason(
            model.canApplyToDisplay,
            model.displayApplyDisabledReason,
            centeredEntry,
          )
          : model.displayApplyDisabledReason}
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
        pending={Boolean(centeredEntry && model.pendingPath === centeredEntry.path)}
        pendingQueueName={pendingQueueName}
        selected={Boolean(centeredEntry && model.selectedPath === centeredEntry.path)}
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

function WallpaperFlowImpl(props: WallpaperFlowProps) {
  if (
    props.initialAnchorWallpaperId == null
    && !props.model.currentObservationReady
  ) {
    return (
      <section className="wallpaper-flow wallpaper-flow--preparing">
        <div className="flow-preview-preparing" role="status">
          Preparing Flow preview…
        </div>
      </section>
    );
  }

  return <WallpaperFlowReady {...props} />;
}

export default memo(WallpaperFlowImpl);
