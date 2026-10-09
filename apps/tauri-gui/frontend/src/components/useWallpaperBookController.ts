/** Book timing and input live here; only spread/window changes reach React. */
import {
  useCallback, useEffect, useLayoutEffect, useRef, useState,
  type KeyboardEvent, type MouseEvent, type PointerEvent,
} from 'react';
import type { LibraryBrowserItemDTO } from '../api/types.ts';
import { useReducedMotion } from '../hooks/useReducedMotion.ts';
import type { ApplyGesture } from '../shell/shellPreferences.ts';
import { useThumbnailStore } from '../state/ThumbnailStoreContext.tsx';
import { libraryEntryApplyAvailable, resolveLibraryFlowStartupAnchor, type LibraryViewModel } from './libraryViewModel.ts';
import {
  accumulateBookWheel, BOOK_APPEND_DISTANCE, BOOK_WHEEL_IDLE_MS, BOOK_WINDOW_RADIUS, BOOK_ZOOM_DURATION_MS,
  bookAppendApproach, bookLastPosition, bookLeafCount, bookLeafTransform, bookPositionForWallpaper, bookSnapTarget,
  bookVisibleWindow, bookZoomTransform, clampBookPosition, openBookWallpapers,
  resolveBookKey, resolveBookPointerInteraction, resolveBookSelectedIndex, resolveBookWheelIntent, type BookRect,
} from './wallpaperBookModel.ts';
import { staticPreviewAssetPath } from './wallpaperPreviewMedia.ts';

export interface WallpaperBookProps {
  readonly model: LibraryViewModel;
  readonly applyGesture?: ApplyGesture;
  readonly initialAnchorWallpaperId?: number | null;
  readonly focusToken?: number;
  readonly returnFocusToken?: number;
  readonly onAnchorChange?: (wallpaperId: number, settled?: boolean) => void;
}

interface BookContextMenu { readonly entry: LibraryBrowserItemDTO; readonly x: number; readonly y: number }
interface BookDrag {
  readonly element: HTMLDivElement;
  readonly pointerId: number;
  readonly startX: number;
  readonly startPosition: number;
  readonly width: number;
  moved: boolean;
  at: number;
  position: number;
  velocity: number;
}

export function useWallpaperBookController(props: WallpaperBookProps) {
  const { model, focusToken = 0, returnFocusToken = 0 } = props;
  const reducedMotion = useReducedMotion();
  const [initial] = useState(() => resolveLibraryFlowStartupAnchor(model.entries, props.initialAnchorWallpaperId, model.currentPath)?.index ?? 0);
  const [spread, setSpread] = useState(() => bookPositionForWallpaper(initial));
  const [selectedIndex, setSelectedIndex] = useState(initial);
  const [settled, setSettled] = useState(true);
  const [zoomIndex, setZoomIndex] = useState<number | null>(null);
  const [zoomMoving, setZoomMoving] = useState(false);
  const [contextMenu, setContextMenu] = useState<BookContextMenu | null>(null);
  const [visible, setVisible] = useState(() => typeof document === 'undefined' || !document.hidden);
  const [focused, setFocused] = useState(() => typeof document === 'undefined' || document.hasFocus());
  const stageRef = useRef<HTMLDivElement>(null);
  const zoomRef = useRef<HTMLDivElement>(null);
  const leavesRef = useRef(new Map<number, HTMLDivElement>());
  const positionRef = useRef(spread);
  const selectedRef = useRef(initial);
  const preferredIndexRef = useRef(initial);
  const spreadRef = useRef(spread);
  const settledRef = useRef(true);
  const frameRef = useRef<number | null>(null);
  const targetRef = useRef<number | null>(null);
  const wheelTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const dragRef = useRef<BookDrag | null>(null);
  const suppressClickRef = useRef(false);
  const resetRef = useRef({ key: model.resetKey, replacement: model.replaceCount });
  const appendClaimRef = useRef('');
  const anchorRef = useRef('');
  const zoomOriginRef = useRef<BookRect | null>(null);
  const zoomAnimationRef = useRef<Animation | null>(null);
  const zoomClosingRef = useRef(false);
  const crossFadeRef = useRef<HTMLElement | null>(null);
  const crossFadeAnimationRef = useRef<Animation | null>(null);
  const latest = useRef({ props, reducedMotion, zoomIndex });
  latest.current = { props, reducedMotion, zoomIndex };
  const { observeVisible, setScrolling, setInteracting } = useThumbnailStore();
  const leafCount = bookLeafCount(model.entries.length);
  const lastPosition = bookLastPosition(model.entries.length);
  const openIndices = openBookWallpapers(spread, model.entries.length);
  const visibleLeaves = bookVisibleWindow(spread, leafCount);
  const selectedEntry = model.entries[zoomIndex ?? selectedIndex] ?? null;
  const interactionActive = model.active && visible && focused && !contextMenu;

  const focusStage = useCallback(() => stageRef.current?.focus({ preventScroll: true }), []);

  const releaseDrag = useCallback(() => {
    const drag = dragRef.current;
    dragRef.current = null;
    if (drag?.element.hasPointerCapture(drag.pointerId)) drag.element.releasePointerCapture(drag.pointerId);
    return drag;
  }, []);

  const cancelZoomAnimation = useCallback(() => {
    const animation = zoomAnimationRef.current;
    zoomAnimationRef.current = null;
    if (animation) {
      animation.onfinish = null;
      animation.cancel();
    }
  }, []);

  const cancelCrossFade = useCallback(() => {
    const animation = crossFadeAnimationRef.current;
    crossFadeAnimationRef.current = null;
    if (animation) {
      animation.onfinish = null;
      animation.cancel();
    }
    crossFadeRef.current?.remove();
    crossFadeRef.current = null;
  }, []);

  // Update the ref synchronously so successive native inputs cannot see stale zoom state.
  const changeZoomIndex = useCallback((index: number | null) => {
    latest.current.zoomIndex = index;
    setZoomIndex(index);
  }, []);

  const resetZoom = useCallback(() => {
    cancelZoomAnimation();
    zoomOriginRef.current = null;
    zoomClosingRef.current = false;
    changeZoomIndex(null);
    setZoomMoving(false);
  }, [cancelZoomAnimation, changeZoomIndex]);

  const publishAnchor = useCallback((index: number, isSettled: boolean) => {
    const { props: current } = latest.current;
    const entry = current.model.entries[index];
    if (!entry) return;
    const key = `${entry.wallpaperId}:${isSettled}`;
    if (anchorRef.current === key) return;
    anchorRef.current = key;
    current.onAnchorChange?.(entry.wallpaperId, isSettled);
  }, []);

  const paint = useCallback(() => {
    const { reducedMotion: reduce } = latest.current;
    for (const [leaf, element] of leavesRef.current) {
      const state = bookLeafTransform(leaf, reduce ? spreadRef.current : positionRef.current, reduce);
      element.style.transform = state.transform;
      element.style.opacity = String(state.opacity);
      element.style.pointerEvents = state.opacity > 0 ? '' : 'none';
      element.querySelectorAll<HTMLElement>('.book-leaf__shade').forEach((shade) => { shade.style.opacity = String(state.shade); });
      element.querySelectorAll<HTMLElement>('.book-leaf__highlight').forEach((highlight) => { highlight.style.opacity = String(state.highlight); });
    }
  }, []);

  const updatePosition = useCallback((position: number, isSettled: boolean) => {
    const current = latest.current.props.model;
    const next = clampBookPosition(position, bookLastPosition(current.entries.length));
    positionRef.current = next;
    const nextSpread = Math.round(next);
    if (spreadRef.current !== nextSpread) {
      if (latest.current.reducedMotion) {
        cancelCrossFade();
        const previous = stageRef.current?.querySelector('.wallpaper-book__spread')?.cloneNode(true) as HTMLElement | undefined;
        if (previous) {
          previous.setAttribute('aria-hidden', 'true');
          previous.classList.add('wallpaper-book__outgoing-spread');
          previous.querySelectorAll('[id]').forEach((element) => element.removeAttribute('id'));
          stageRef.current?.append(previous);
          crossFadeRef.current = previous;
          const fade = previous.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 140 });
          crossFadeAnimationRef.current = fade;
          fade.onfinish = cancelCrossFade;
        }
      }
      spreadRef.current = nextSpread;
      setSpread(nextSpread);
    }
    const index = resolveBookSelectedIndex(nextSpread, current.entries.length, preferredIndexRef.current, selectedRef.current);
    if (selectedRef.current !== index) {
      selectedRef.current = index;
      setSelectedIndex(index);
    }
    if (settledRef.current !== isSettled) {
      settledRef.current = isSettled;
      setSettled(isSettled);
      setScrolling(!isSettled);
    }
    publishAnchor(index, isSettled);
    if (isSettled && current.entries[index]) current.onSelect(current.entries[index]);
    paint();
  }, [cancelCrossFade, paint, publishAnchor, setScrolling]);

  const cancelMotion = useCallback(() => {
    if (frameRef.current !== null) cancelAnimationFrame(frameRef.current);
    frameRef.current = null;
    targetRef.current = null;
    if (wheelTimerRef.current !== null) clearTimeout(wheelTimerRef.current);
    wheelTimerRef.current = null;
  }, []);

  const moveTo = useCallback((target: number, instant = false, velocity = 0) => {
    cancelMotion();
    const count = bookLastPosition(latest.current.props.model.entries.length);
    const destination = Math.round(clampBookPosition(target, count));
    // Far navigation replaces the bounded window without traversing the entire result.
    if (instant || latest.current.reducedMotion || Math.abs(destination - positionRef.current) > BOOK_WINDOW_RADIUS || Math.abs(destination - positionRef.current) < 0.001) {
      updatePosition(destination, true);
      return;
    }
    updatePosition(positionRef.current, false);
    targetRef.current = destination;
    let lastAt = performance.now();
    let speed = Math.min(18, Math.max(-18, velocity));
    const startedAt = lastAt;
    const tick = (at: number) => {
      const elapsed = Math.min(32, at - lastAt) / 1000;
      lastAt = at;
      // Semi-implicit spring in leaf units, interruptible by every new input.
      speed += ((destination - positionRef.current) * 190 - speed * 26) * elapsed;
      const position = positionRef.current + speed * elapsed;
      if ((Math.abs(destination - position) < 0.002 && Math.abs(speed) < 0.025) || at - startedAt > 1200) {
        frameRef.current = null;
        targetRef.current = null;
        updatePosition(destination, true);
        return;
      }
      updatePosition(position, false);
      frameRef.current = requestAnimationFrame(tick);
    };
    frameRef.current = requestAnimationFrame(tick);
  }, [cancelMotion, updatePosition]);

  const selectIndex = useCallback((index: number) => {
    const entry = latest.current.props.model.entries[index];
    if (!entry || !openBookWallpapers(positionRef.current, latest.current.props.model.entries.length).includes(index)) return;
    selectedRef.current = index;
    preferredIndexRef.current = index;
    setSelectedIndex(index);
    latest.current.props.model.onSelect(entry);
    publishAnchor(index, settledRef.current);
  }, [publishAnchor]);

  const closeZoom = useCallback(() => {
    if (latest.current.zoomIndex === null || zoomClosingRef.current) return;
    focusStage();
    const element = zoomRef.current;
    const option = stageRef.current?.querySelector<HTMLElement>(`#book-option-${latest.current.props.model.entries[selectedRef.current]?.wallpaperId}`);
    const origin = option?.getBoundingClientRect();
    cancelZoomAnimation();
    const finish = () => {
      zoomClosingRef.current = false;
      zoomAnimationRef.current = null;
      changeZoomIndex(null);
      setZoomMoving(false);
      stageRef.current?.focus({ preventScroll: true });
    };
    if (!element || !origin || latest.current.reducedMotion) { finish(); return; }
    zoomClosingRef.current = true;
    setZoomMoving(true);
    const animation = element.animate([
      { transform: 'none' }, { transform: bookZoomTransform(origin, element.getBoundingClientRect()) },
    ], { duration: BOOK_ZOOM_DURATION_MS, easing: 'cubic-bezier(0.22, 1, 0.36, 1)', fill: 'forwards' });
    zoomAnimationRef.current = animation;
    animation.onfinish = finish;
  }, [cancelZoomAnimation, changeZoomIndex, focusStage]);

  const toggleZoom = useCallback(() => {
    if (latest.current.zoomIndex !== null) { closeZoom(); return; }
    if (!settledRef.current) return;
    const index = selectedRef.current;
    const entry = latest.current.props.model.entries[index];
    if (!entry) return;
    zoomOriginRef.current = stageRef.current?.querySelector<HTMLElement>(`#book-option-${entry.wallpaperId}`)?.getBoundingClientRect() ?? null;
    changeZoomIndex(index);
    setZoomMoving(!latest.current.reducedMotion);
    stageRef.current?.focus({ preventScroll: true });
  }, [changeZoomIndex, closeZoom]);

  useLayoutEffect(() => {
    paint();
  }, [paint, spread, model.entries, reducedMotion]);

  useLayoutEffect(() => {
    const element = zoomRef.current;
    const origin = zoomOriginRef.current;
    if (zoomIndex === null) return;
    zoomOriginRef.current = null;
    if (!element || !origin || reducedMotion) {
      setZoomMoving(false);
      return;
    }
    const animation = element.animate([
      { transform: bookZoomTransform(origin, element.getBoundingClientRect()) }, { transform: 'none' },
    ], { duration: BOOK_ZOOM_DURATION_MS, easing: 'cubic-bezier(0.22, 1, 0.36, 1)' });
    zoomAnimationRef.current = animation;
    animation.onfinish = () => {
      zoomAnimationRef.current = null;
      setZoomMoving(false);
    };
    return cancelZoomAnimation;
  }, [cancelZoomAnimation, zoomIndex, reducedMotion]);

  useLayoutEffect(() => {
    const previous = resetRef.current;
    resetRef.current = { key: model.resetKey, replacement: model.replaceCount };
    if (previous.key !== model.resetKey || previous.replacement !== model.replaceCount) {
      releaseDrag();
      resetZoom();
      cancelCrossFade();
      appendClaimRef.current = '';
      anchorRef.current = '';
      setContextMenu(null);
      selectedRef.current = 0;
      preferredIndexRef.current = 0;
      setSelectedIndex(0);
      moveTo(0, true);
      cancelCrossFade();
    } else if (model.entries.length > 0) {
      updatePosition(positionRef.current, settledRef.current);
    }
  }, [cancelCrossFade, model.entries, model.resetKey, model.replaceCount, moveTo, releaseDrag, resetZoom, updatePosition]);

  useEffect(() => {
    if (focusToken <= 0 && returnFocusToken <= 0) return;
    const frame = requestAnimationFrame(() => stageRef.current?.focus({ preventScroll: true }));
    return () => cancelAnimationFrame(frame);
  }, [focusToken, returnFocusToken]);

  useEffect(() => {
    const stopInteraction = () => {
      releaseDrag();
      resetZoom();
      moveTo(bookSnapTarget(positionRef.current, 0, bookLastPosition(latest.current.props.model.entries.length)), true);
      cancelCrossFade();
    };
    const visibility = () => {
      setVisible(!document.hidden);
      if (document.hidden) stopInteraction();
    };
    const focus = () => setFocused(true);
    const blur = () => {
      setFocused(false);
      stopInteraction();
    };
    document.addEventListener('visibilitychange', visibility);
    window.addEventListener('focus', focus);
    window.addEventListener('blur', blur);
    return () => {
      document.removeEventListener('visibilitychange', visibility);
      window.removeEventListener('focus', focus);
      window.removeEventListener('blur', blur);
    };
  }, [cancelCrossFade, moveTo, releaseDrag, resetZoom]);

  useEffect(() => {
    setInteracting(interactionActive);
  }, [interactionActive, setInteracting]);

  useEffect(() => {
    if (!interactionActive) {
      releaseDrag();
      cancelZoomAnimation();
      zoomOriginRef.current = null;
      if (zoomClosingRef.current) changeZoomIndex(null);
      zoomClosingRef.current = false;
      setZoomMoving(false);
      moveTo(bookSnapTarget(positionRef.current, 0, lastPosition), true);
      cancelCrossFade();
    }
  }, [cancelCrossFade, cancelZoomAnimation, changeZoomIndex, interactionActive, lastPosition, moveTo, releaseDrag]);

  useEffect(() => {
    if (!interactionActive) return;
    const nearby = model.entries.slice(Math.max(0, spread * 2 - 3), spread * 2 + 4);
    observeVisible(nearby.map(staticPreviewAssetPath), { priority: 'front' });
    const rest = bookVisibleWindow(spread, leafCount).flatMap((leaf) => model.entries.slice(leaf * 2, leaf * 2 + 2));
    observeVisible(rest.map(staticPreviewAssetPath), { priority: 'back' });
  }, [interactionActive, leafCount, model.entries, observeVisible, spread]);

  useEffect(() => {
    const key = `${model.resetKey}:${model.replaceCount}:${model.entries.length}`;
    const approach = bookAppendApproach(appendClaimRef.current, key, lastPosition - spread <= BOOK_APPEND_DISTANCE,
      interactionActive && model.canAutoAppend && !model.loadingMore && !model.refreshing);
    appendClaimRef.current = approach.claim;
    if (approach.request) void model.onRequestMoreIfNeeded();
  }, [interactionActive, lastPosition, spread, model]);

  const handleWheel = useCallback((event: globalThis.WheelEvent) => {
    event.preventDefault();
    if (!latest.current.props.model.active || document.hidden) return;
    const intent = resolveBookWheelIntent(event, dragRef.current !== null, latest.current.zoomIndex !== null);
    if (intent === 'zoom-in') {
      releaseDrag();
      moveTo(bookSnapTarget(positionRef.current, 0, bookLastPosition(latest.current.props.model.entries.length)), true);
      toggleZoom();
      return;
    }
    if (intent === 'zoom-out') { closeZoom(); return; }
    if (intent !== 'turn') return;
    cancelMotion();
    const count = bookLastPosition(latest.current.props.model.entries.length);
    const position = accumulateBookWheel(positionRef.current, event, count);
    updatePosition(position, false);
    wheelTimerRef.current = setTimeout(() => moveTo(bookSnapTarget(positionRef.current, 0, count)), BOOK_WHEEL_IDLE_MS);
  }, [cancelMotion, closeZoom, moveTo, releaseDrag, toggleZoom, updatePosition]);

  // Native non-passive wheel listener also prevents browser zoom on trackpad pinch.
  useEffect(() => {
    const stage = stageRef.current;
    stage?.addEventListener('wheel', handleWheel, { passive: false });
    return () => stage?.removeEventListener('wheel', handleWheel);
  }, [handleWheel]);

  useEffect(() => () => {
    cancelMotion();
    releaseDrag();
    cancelZoomAnimation();
    cancelCrossFade();
    setScrolling(false);
    setInteracting(true);
  }, [cancelCrossFade, cancelMotion, cancelZoomAnimation, releaseDrag, setScrolling, setInteracting]);

  const handlePointerDown = (event: PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0 || !model.active || zoomIndex !== null || !event.isPrimary) return;
    cancelMotion();
    releaseDrag();
    suppressClickRef.current = false;
    stageRef.current?.focus({ preventScroll: true });
    const page = stageRef.current?.querySelector<HTMLElement>('.book-leaf');
    dragRef.current = {
      element: event.currentTarget,
      pointerId: event.pointerId, startX: event.clientX, startPosition: positionRef.current,
      width: Math.max(1, page?.offsetWidth ?? 300), moved: false,
      at: performance.now(), position: positionRef.current, velocity: 0,
    };
  };

  const handlePointerMove = (event: PointerEvent<HTMLDivElement>) => {
    const drag = dragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    const distance = event.clientX - drag.startX;
    if (!drag.moved && Math.abs(distance) < 5) return;
    if (!drag.moved) event.currentTarget.setPointerCapture(event.pointerId);
    drag.moved = true;
    suppressClickRef.current = true;
    const position = clampBookPosition(drag.startPosition - distance / drag.width, lastPosition);
    const now = performance.now();
    drag.velocity = (position - drag.position) / Math.max(0.008, (now - drag.at) / 1000);
    drag.position = position;
    drag.at = now;
    updatePosition(position, false);
  };

  const finishPointer = (event: PointerEvent<HTMLDivElement>) => {
    const drag = dragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    releaseDrag();
    if (!drag.moved && settledRef.current) return;
    const velocity = event.type === 'pointerup' ? drag.velocity * Math.exp(-(performance.now() - drag.at) / 160) : 0;
    moveTo(bookSnapTarget(positionRef.current, velocity, lastPosition), false, velocity);
  };

  const applySelected = () => {
    if (!selectedEntry || !model.active || !settledRef.current || zoomMoving) return;
    if (!libraryEntryApplyAvailable(model.canApplyToDisplay, model.isEntryApplicable, selectedEntry)) return;
    model.onSelect(selectedEntry);
    model.onApply(selectedEntry);
    stageRef.current?.focus({ preventScroll: true });
  };

  const handlePageClick = (event: MouseEvent<HTMLDivElement>, index: number) => {
    if (!model.active || zoomIndex !== null) return;
    const entry = model.entries[index];
    if (!entry) return;
    const intent = resolveBookPointerInteraction({
      gesture: props.applyGesture ?? 'single', clickCount: event.detail,
      canApply: libraryEntryApplyAvailable(model.canApplyToDisplay, model.isEntryApplicable, entry), fromControl: false,
      dragged: suppressClickRef.current, open: openIndices.includes(index), settled: settledRef.current,
    });
    if (intent.turn) {
      preferredIndexRef.current = index;
      moveTo(bookPositionForWallpaper(index));
      return;
    }
    if (intent.select) selectIndex(index);
    if (intent.apply) model.onApply(entry);
  };

  const openContextMenu = (index: number, x: number, y: number) => {
    const entry = model.entries[index];
    if (!entry || !model.active) return;
    releaseDrag();
    preferredIndexRef.current = index;
    if (zoomIndex === null) moveTo(bookPositionForWallpaper(index), true);
    else selectIndex(index);
    stageRef.current?.focus({ preventScroll: true });
    setContextMenu({ entry, x, y });
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.target !== event.currentTarget || !interactionActive) return;
    if (event.altKey || event.metaKey || event.ctrlKey) return;
    const intent = resolveBookKey(event.key, event.shiftKey);
    if (!intent) return;
    event.preventDefault();
    event.stopPropagation();
    if (intent === 'next' || intent === 'previous') {
      const direction = intent === 'next' ? 1 : -1;
      if (zoomIndex !== null) {
        const index = Math.min(model.entries.length - 1, Math.max(0, zoomIndex + direction));
        cancelZoomAnimation();
        zoomClosingRef.current = false;
        zoomOriginRef.current = null;
        setZoomMoving(false);
        preferredIndexRef.current = index;
        moveTo(bookPositionForWallpaper(index), true);
        selectIndex(index);
        changeZoomIndex(index);
      } else moveTo((targetRef.current ?? Math.round(positionRef.current)) + direction);
    } else if (intent === 'first' || intent === 'last') {
      if (zoomIndex !== null) {
        const index = intent === 'first' ? 0 : model.entries.length - 1;
        cancelZoomAnimation();
        zoomClosingRef.current = false;
        zoomOriginRef.current = null;
        setZoomMoving(false);
        preferredIndexRef.current = index;
        moveTo(bookPositionForWallpaper(index), true);
        selectIndex(index);
        changeZoomIndex(index);
      } else {
        preferredIndexRef.current = intent === 'first' ? 0 : model.entries.length - 1;
        moveTo(intent === 'first' ? 0 : lastPosition);
      }
      if (intent === 'last' && model.canAppend && !model.loadingMore) void model.onAppendMore();
    } else if (intent === 'select-left' || intent === 'select-right') {
      if (settledRef.current && zoomIndex === null && openIndices.length > 0) selectIndex(intent === 'select-left' ? openIndices[0] : openIndices.at(-1)!);
    } else if (intent === 'apply') applySelected();
    else if (intent === 'zoom') toggleZoom();
    else if (intent === 'unzoom') closeZoom();
    else if (intent === 'context') {
      const rect = (zoomRef.current ?? stageRef.current)?.getBoundingClientRect();
      openContextMenu(zoomIndex ?? selectedIndex, (rect?.left ?? 0) + 16, (rect?.top ?? 0) + 16);
    }
  };

  return {
    elements: { stageRef, zoomRef, leavesRef },
    snapshot: { spread, selectedIndex, selectedEntry, settled, zoomIndex, zoomMoving, contextMenu, reducedMotion, interactionActive, openIndices, visibleLeaves },
    actions: {
      handlePointerDown, handlePointerMove, finishPointer, handleKeyDown, handlePageClick,
      openContextMenu, applySelected, toggleZoom, closeZoom, focusStage,
      closeContextMenu: () => { setContextMenu(null); focusStage(); },
    },
  };
}
