/** Book timing and input live here; only spread/window changes reach React. */
import {
  useCallback, useEffect, useLayoutEffect, useRef, useState,
  type KeyboardEvent, type MouseEvent, type PointerEvent,
} from 'react';
import { flushSync } from 'react-dom';
import type { LibraryBrowserItemDTO } from '../api/types.ts';
import { useReducedMotion } from '../hooks/useReducedMotion.ts';
import type { ApplyGesture } from '../shell/shellPreferences.ts';
import { useThumbnailStore } from '../state/ThumbnailStoreContext.tsx';
import { libraryEntryApplyAvailable, resolveLibraryFlowStartupAnchor, type LibraryViewModel } from './libraryViewModel.ts';
import {
  accumulateBookWheel, classifyBookWheel, bookWheelTarget, bookSpringStep, bookDragVelocity,
  type BookWheelSample, type BookDragSample, BOOK_APPEND_DISTANCE, BOOK_WHEEL_IDLE_MS,
  bookAppendApproach, bookLastPosition, bookLeafCount, bookLeafTransform, bookPositionForWallpaper, bookSnapTarget,
  bookPreviewOrder, bookVisibleWindow, bookZoomTransform, clampBookPosition, openBookWallpapers,
  planBookMove, resolveBookContextMenu, resolveBookKey, resolveBookPointerInteraction,
  resolveBookSelectedIndex, resolveBookWheelIntent, wallpaperBookAddress, type BookZoomOrigin,
} from './wallpaperBookModel.ts';
import { api } from '../api/bridge.ts';
import { safeFileSrc } from './safeFileSrc.ts';
import { bookZoomTimeline, BOOK_ZOOM_REST, bookZoomReveal, bookZoomSwapTimeline } from './wallpaperBookZoom.ts';
import { captureBookVideoStill, decodeBookStill } from './wallpaperBookZoomMedia.ts';
import { staticFallbackAssetPath, staticPreviewAssetPath } from './wallpaperPreviewMedia.ts';

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
  position: number;
  samples: BookDragSample[];
}

export function useWallpaperBookController(props: WallpaperBookProps) {
  const { model, focusToken = 0, returnFocusToken = 0 } = props;
  const reducedMotion = useReducedMotion();
  const [initial] = useState(() => resolveLibraryFlowStartupAnchor(model.entries, props.initialAnchorWallpaperId, model.currentPath)?.index ?? 0);
  const [spread, setSpread] = useState(() => bookPositionForWallpaper(initial));
  const [leafKeyOffset, setLeafKeyOffset] = useState(0);
  const [selectedIndex, setSelectedIndex] = useState(initial);
  const [settled, setSettled] = useState(true);
  const [zoomIndex, setZoomIndex] = useState<number | null>(null);
  const [zoomMoving, setZoomMoving] = useState(false);
  const [zoomStillSrc, setZoomStillSrc] = useState<string | null>(null);
  const [contextMenu, setContextMenu] = useState<BookContextMenu | null>(null);
  const [visible, setVisible] = useState(() => typeof document === 'undefined' || !document.hidden);
  const [focused, setFocused] = useState(() => typeof document === 'undefined' || document.hasFocus());
  const stageRef = useRef<HTMLDivElement>(null);
  const zoomMediaRef = useRef<HTMLDivElement>(null);
  const zoomStillRef = useRef<HTMLImageElement>(null);
  const zoomDecorationRef = useRef<HTMLDivElement>(null);
  const zoomPendingNavigationRef = useRef<(() => void) | null>(null);
  const zoomPoseRef = useRef<string | null>(null);
  const zoomMovingRef = useRef(false);
  const zoomReadyRef = useRef(false);
  const zoomNavigationRef = useRef(0);
  const zoomNavigationTargetRef = useRef<number | null>(null);
  const zoomResumePageVideoRef = useRef<(() => void) | null>(null);
  const zoomFadeRef = useRef<Animation | null>(null);
  const zoomSwapAnimationsRef = useRef<Animation[]>([]);
  const zoomOutgoingRef = useRef<HTMLElement | null>(null);
  const zoomSwapRef = useRef<{ outgoing: HTMLElement; direction: number } | null>(null);
  const zoomDecodeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const zoomRef = useRef<HTMLDivElement>(null);
  const leavesRef = useRef(new Map<number, HTMLDivElement>());
  const [visibleLeaves, setVisibleLeaves] = useState(() => bookVisibleWindow(spread, bookLeafCount(model.entries.length)));
  const windowRef = useRef(visibleLeaves);
  const wheelSampleRef = useRef<BookWheelSample | undefined>(undefined);
  const dragFrameRef = useRef<number | null>(null);
  const positionRef = useRef(spread);
  const selectedRef = useRef(initial);
  const preferredIndexRef = useRef(initial);
  const spreadRef = useRef(spread);
  const settledRef = useRef(true);
  const frameRef = useRef<number | null>(null);
  const riffleRef = useRef(false);
  const targetRef = useRef<number | null>(null);
  const wheelTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const dragRef = useRef<BookDrag | null>(null);
  const suppressClickRef = useRef(false);
  const resetRef = useRef({ key: model.resetKey, replacement: model.replaceCount });
  const appendClaimRef = useRef('');
  const anchorRef = useRef('');
  const restoredAnchorRef = useRef(props.initialAnchorWallpaperId);
  const pendingMenuRef = useRef<{ wallpaperId: number; x: number; y: number } | null>(null);
  const zoomOriginRef = useRef<BookZoomOrigin | null>(null);
  const zoomAnimationsRef = useRef<Animation[]>([]);
  const zoomClosingRef = useRef(false);
  const crossFadeRef = useRef<HTMLElement | null>(null);
  const crossFadeAnimationRef = useRef<Animation | null>(null);
  const latest = useRef({ props, reducedMotion, zoomIndex });
  latest.current = { props, reducedMotion, zoomIndex };
  const { observeVisible, setScrolling, setInteracting, get: getThumbnail } = useThumbnailStore();
  const leafCount = bookLeafCount(model.entries.length);
  const lastPosition = bookLastPosition(model.entries.length);
  const openIndices = openBookWallpapers(spread, model.entries.length);
  const selectedEntry = model.entries[zoomIndex ?? selectedIndex] ?? null;
  const viewActive = model.active && visible;
  const interactionActive = viewActive && focused && !contextMenu;

  const focusStage = useCallback(() => stageRef.current?.focus({ preventScroll: true }), []);

  const releaseDrag = useCallback(() => {
    const drag = dragRef.current;
    dragRef.current = null;
    if (dragFrameRef.current !== null) cancelAnimationFrame(dragFrameRef.current);
    dragFrameRef.current = null;
    stageRef.current?.removeAttribute('data-dragging');
    if (drag?.element.hasPointerCapture(drag.pointerId)) drag.element.releasePointerCapture(drag.pointerId);
    return drag;
  }, []);

  const cancelZoomFade = useCallback(() => {
    if (zoomFadeRef.current) {
      zoomFadeRef.current.onfinish = null;
      zoomFadeRef.current.cancel();
      zoomFadeRef.current = null;
    }
  }, []);

  const cancelZoomSwap = useCallback(() => {
    for (const animation of zoomSwapAnimationsRef.current) {
      animation.onfinish = null;
      animation.cancel();
    }
    zoomSwapAnimationsRef.current = [];
    zoomOutgoingRef.current?.remove();
    zoomOutgoingRef.current = null;
  }, []);

  const cancelZoomDecodeTimer = useCallback(() => {
    if (zoomDecodeTimerRef.current !== null) clearTimeout(zoomDecodeTimerRef.current);
    zoomDecodeTimerRef.current = null;
  }, []);

  const cancelZoomAnimation = useCallback(() => {
    cancelZoomSwap();
    cancelZoomFade();
    for (const animation of zoomAnimationsRef.current) {
      animation.onfinish = null;
      animation.cancel();
    }
    zoomAnimationsRef.current = [];
  }, [cancelZoomFade, cancelZoomSwap]);

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
    cancelZoomDecodeTimer();
    cancelZoomAnimation();
    zoomResumePageVideoRef.current?.();
    zoomResumePageVideoRef.current = null;
    zoomOriginRef.current = null;
    zoomSwapRef.current = null;
    zoomPoseRef.current = null;
    zoomMovingRef.current = false;
    zoomReadyRef.current = false;
    zoomNavigationRef.current += 1;
    zoomPendingNavigationRef.current = null;
    zoomNavigationTargetRef.current = null;
    zoomClosingRef.current = false;
    changeZoomIndex(null);
    setZoomStillSrc(null);
    setZoomMoving(false);
  }, [cancelZoomAnimation, cancelZoomDecodeTimer, changeZoomIndex]);

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
      element.style.willChange = !settledRef.current && Math.abs(leaf - positionRef.current) <= 1.5 ? 'transform' : '';
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
    const window = bookVisibleWindow(next, bookLeafCount(current.entries.length));
    if (window.length !== windowRef.current.length || window.some((leaf, i) => leaf !== windowRef.current[i])) {
      windowRef.current = window;
      setVisibleLeaves(window);
    }
    // Selection, open spread and media identity only change at rest. A frame
    // publishes React state only when the rendered window needs different leaves.
    const nextSpread = isSettled ? Math.round(next) : spreadRef.current;
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
    const settlingChanged = settledRef.current !== isSettled;
    if (settlingChanged) {
      settledRef.current = isSettled;
      setSettled(isSettled);
      setScrolling(!isSettled);
    }
    if (isSettled || settlingChanged) publishAnchor(index, isSettled);
    if (isSettled && current.entries[index]) current.onSelect(current.entries[index]);
    const pendingMenu = pendingMenuRef.current;
    if (isSettled && pendingMenu) {
      pendingMenuRef.current = null;
      const entry = current.entries[index];
      if (entry?.wallpaperId === pendingMenu.wallpaperId) {
        setContextMenu({ entry, x: pendingMenu.x, y: pendingMenu.y });
      }
    }
    paint();
  }, [cancelCrossFade, paint, publishAnchor, setScrolling]);

  const cancelMotion = useCallback(() => {
    pendingMenuRef.current = null;
    if (frameRef.current !== null) cancelAnimationFrame(frameRef.current);
    frameRef.current = null;
    targetRef.current = null;
    riffleRef.current = false;
    if (wheelTimerRef.current !== null) clearTimeout(wheelTimerRef.current);
    wheelTimerRef.current = null;
  }, []);

  const moveTo = useCallback((target: number, instant = false, velocity = 0) => {
    cancelMotion();
    const count = bookLastPosition(latest.current.props.model.entries.length);
    const destination = Math.round(clampBookPosition(target, count));
    if (instant || latest.current.reducedMotion || Math.abs(destination - positionRef.current) < 0.001) {
      updatePosition(destination, true);
      return;
    }
    updatePosition(positionRef.current, false);
    targetRef.current = destination;
    const segments = planBookMove(positionRef.current, destination, count);
    if (segments.length > 1) {
      riffleRef.current = true;
      const startedAt = performance.now();
      const first = segments[0];
      const last = segments[1];
      let swapped = false;
      const tick = (at: number) => {
        const elapsed = Math.max(0, at - startedAt);
        if (elapsed >= first.durationMs && !swapped) {
          // Render the exact midpoint before transplanting: the keys retain the
          // moving sheet and its media, with the same pose on both sides of the skip.
          flushSync(() => updatePosition(first.to, false));
          flushSync(() => {
            setLeafKeyOffset((offset) => offset + last.from - first.to);
            updatePosition(last.from, false);
          });
          swapped = true;
        }
        const segment = swapped ? last : first;
        const progress = Math.min(1, (elapsed - (swapped ? first.durationMs : 0)) / segment.durationMs);
        // Fast, steady departure; a gentle arrival with zero final velocity.
        const eased = swapped ? 1 - (1 - progress) ** 1.5 : progress;
        const finished = swapped && progress === 1;
        updatePosition(segment.from + (segment.to - segment.from) * eased, finished);
        if (finished) {
          frameRef.current = null;
          targetRef.current = null;
          riffleRef.current = false;
        } else frameRef.current = requestAnimationFrame(tick);
      };
      frameRef.current = requestAnimationFrame(tick);
      return;
    }
    let lastAt = performance.now();
    let speed = Math.min(18, Math.max(-18, velocity));
    const tick = (at: number) => {
      const elapsed = Math.min(32, Math.max(0, at - lastAt)) / 1000;
      lastAt = at;
      // Wheel notches extend this live target without restarting the timeline.
      const destination = targetRef.current ?? Math.round(positionRef.current);
      const step = bookSpringStep(positionRef.current, speed, destination, elapsed);
      speed = step.velocity;
      const position = step.position;
      if (Math.abs(destination - position) < 0.002 && Math.abs(speed) < 0.025) {
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

  const mayUpdateZoomStill = useCallback(() => latest.current.zoomIndex !== null
    && !zoomMovingRef.current && !zoomClosingRef.current, []);

  const revealZoomLive = useCallback(() => {
    zoomReadyRef.current = true;
    if (!mayUpdateZoomStill() || !zoomStillRef.current) return;
    const still = zoomStillRef.current;
    const opacity = Number.parseFloat(getComputedStyle(still).opacity);
    cancelZoomFade();
    const reveal = bookZoomReveal(opacity, latest.current.reducedMotion);
    const fade = still.animate(reveal.frames, { duration: reveal.duration, easing: 'linear', fill: 'both' });
    zoomFadeRef.current = fade;
  }, [cancelZoomFade, mayUpdateZoomStill]);

  const setZoomDirection = useCallback((closing: boolean, initial = false) => {
    const page = zoomRef.current;
    const stage = stageRef.current;
    const pose = zoomPoseRef.current;
    const scene = stage?.querySelector<HTMLElement>('.wallpaper-book__scene');
    const backdrop = stage?.querySelector<HTMLElement>('.wallpaper-book__zoom-backdrop');
    if (!page || !stage || !scene || !backdrop || !pose) return;
    // Sample BEFORE canceling any filled animation, including the still reveal.
    const sample = initial ? undefined : {
      transform: getComputedStyle(page).transform,
      sceneOpacity: Number.parseFloat(getComputedStyle(scene).opacity),
      backdropOpacity: Number.parseFloat(getComputedStyle(backdrop).opacity),
      stillOpacity: zoomStillRef.current ? Number.parseFloat(getComputedStyle(zoomStillRef.current).opacity) : 1,
    };
    zoomClosingRef.current = closing;
    zoomMovingRef.current = true;
    zoomNavigationRef.current += 1;
    zoomPendingNavigationRef.current = null;
    zoomNavigationTargetRef.current = latest.current.zoomIndex;
    cancelZoomDecodeTimer();
    cancelZoomAnimation();
    setZoomMoving(true);
    const timeline = bookZoomTimeline(closing ? 'close' : 'open', pose, sample);
    const timing = { duration: timeline.duration, easing: 'linear', fill: 'both' as const };
    const animations = [
      page.animate(timeline.page, timing),
      scene.animate(timeline.scene, timing),
      backdrop.animate(timeline.backdrop, timing),
    ];
    if (zoomStillRef.current) animations.push(zoomStillRef.current.animate(timeline.still, timing));
    zoomAnimationsRef.current = animations;
    animations[0].onfinish = () => {
      if (zoomClosingRef.current) {
        // The real picture uses the same decoded source as the landing still.
        const entry = latest.current.props.model.entries[latest.current.zoomIndex ?? -1];
        const realImage = entry ? stage.querySelector<HTMLImageElement>(`#book-option-${entry.wallpaperId} img[data-preview-loaded="true"]`) : null;
        if (realImage && zoomStillRef.current?.src) realImage.src = zoomStillRef.current.src;
        flushSync(() => resetZoom());
        focusStage();
      } else {
        zoomMovingRef.current = false;
        setZoomMoving(false);
        const navigate = zoomPendingNavigationRef.current;
        zoomPendingNavigationRef.current = null;
        if (navigate) navigate();
        else if (zoomReadyRef.current) revealZoomLive();
      }
    };
  }, [cancelZoomAnimation, cancelZoomDecodeTimer, focusStage, resetZoom, revealZoomLive]);

  const closeZoom = useCallback(() => {
    if (latest.current.zoomIndex === null) {
      if (zoomOriginRef.current) resetZoom(); // Cancel a pending video-frame decode.
      return;
    }
    if (zoomClosingRef.current) return;
    focusStage();
    if (latest.current.reducedMotion || !zoomPoseRef.current) {
      resetZoom();
      return;
    }
    // No pre-fade, timer or decode: the new flight starts in this input frame.
    setZoomDirection(true);
  }, [focusStage, resetZoom, setZoomDirection]);

  const toggleZoom = useCallback(() => {
    pendingMenuRef.current = null;
    if (latest.current.zoomIndex !== null) {
      if (zoomClosingRef.current) setZoomDirection(false);
      else closeZoom();
      return;
    }
    if (zoomOriginRef.current) { resetZoom(); return; }
    if (!settledRef.current) return;
    const index = selectedRef.current;
    const entry = latest.current.props.model.entries[index];
    const stage = stageRef.current;
    if (!entry || !stage) return;
    const option = stage.querySelector<HTMLElement>(`#book-option-${entry.wallpaperId}`);
    const spread = stage.querySelector<HTMLElement>('.wallpaper-book__spread');
    const rect = stage.getBoundingClientRect();
    zoomOriginRef.current = option && spread ? {
      spineX: rect.left + (Number.parseFloat(getComputedStyle(spread).left) || spread.offsetLeft),
      spineY: rect.top + (Number.parseFloat(getComputedStyle(spread).top) || spread.offsetTop),
      width: Number.parseFloat(getComputedStyle(option).width) || option.offsetWidth,
      face: wallpaperBookAddress(index).face,
      paperMargin: Number.parseFloat(getComputedStyle(option).paddingLeft) || 0,
    } : null;
    const image = option?.querySelector<HTMLImageElement>('img[data-preview-loaded="true"]');
    const launch = (source: string | null) => {
      setZoomStillSrc(source);
      zoomMovingRef.current = !latest.current.reducedMotion;
      zoomNavigationTargetRef.current = index;
      zoomReadyRef.current = false;
      changeZoomIndex(index);
      setZoomMoving(zoomMovingRef.current);
      focusStage();
    };
    const video = option?.querySelector<HTMLVideoElement>('video[data-enhanced-preview="video"]');
    const frame = video && typeof video.pause === 'function' ? captureBookVideoStill(video) : null;
    if (frame) {
      zoomResumePageVideoRef.current = frame.resume;
      const request = ++zoomNavigationRef.current;
      const current = () => request === zoomNavigationRef.current && latest.current.zoomIndex === null
        && latest.current.props.model.active && zoomOriginRef.current !== null;
      void decodeBookStill(frame.source, (decoded) => launch(decoded.src), current).then(() => {
        if (current()) resetZoom(); // Decode failed: leave the real page usable.
      });
    } else launch(image?.currentSrc || image?.src || video?.poster || null);
  }, [changeZoomIndex, closeZoom, focusStage, resetZoom, setZoomDirection]);

  useLayoutEffect(() => {
    paint();
  }, [paint, spread, visibleLeaves, model.entries, reducedMotion]);

  useLayoutEffect(() => {
    const element = zoomRef.current;
    const opening = zoomOriginRef.current !== null;
    let origin = zoomOriginRef.current;
    const stage = stageRef.current;
    if (zoomIndex === null) return;
    zoomOriginRef.current = null;
    if (!origin && stage) {
      const entry = latest.current.props.model.entries[zoomIndex];
      const option = entry ? stage.querySelector<HTMLElement>(`#book-option-${entry.wallpaperId}`) : null;
      const spread = stage.querySelector<HTMLElement>('.wallpaper-book__spread');
      const rect = stage.getBoundingClientRect();
      if (option && spread) origin = {
        spineX: rect.left + (Number.parseFloat(getComputedStyle(spread).left) || spread.offsetLeft), spineY: rect.top + (Number.parseFloat(getComputedStyle(spread).top) || spread.offsetTop),
        width: Number.parseFloat(getComputedStyle(option).width) || option.offsetWidth, face: wallpaperBookAddress(zoomIndex).face,
        paperMargin: Number.parseFloat(getComputedStyle(option).paddingLeft) || 0,
      };
    }
    if (!element || !origin || !stage) {
      zoomMovingRef.current = false;
      setZoomMoving(false);
      return;
    }
    // getBoundingClientRect must see the untransformed destination on navigation.
    element.style.transform = BOOK_ZOOM_REST;
    const pose = bookZoomTransform(origin, element.getBoundingClientRect());
    zoomPoseRef.current = pose.transform;
    const entry = latest.current.props.model.entries[zoomIndex];
    const option = entry ? stage.querySelector<HTMLElement>(`#book-option-${entry.wallpaperId}`) : null;
    const realPrint = option?.querySelector<HTMLElement>('.book-leaf__print');
    const radius = (target: HTMLElement | null | undefined, fallback: string) => {
      if (!target) return fallback;
      const css = getComputedStyle(target);
      const values = [css.borderTopLeftRadius, css.borderTopRightRadius, css.borderBottomRightRadius, css.borderBottomLeftRadius];
      return values.every((value) => value?.endsWith('px'))
        ? values.map((value) => `${Number.parseFloat(value) / pose.scale}px`).join(' ') : fallback;
    };
    element.style.borderRadius = radius(option, pose.paperBorderRadius);
    const decoration = zoomDecorationRef.current;
    if (decoration) {
      // Original page coordinates keep its marker, shadows and labels exactly
      // aligned at both endpoints, without changing those styles or any media.
      decoration.style.width = `${origin.width}px`;
      decoration.style.height = `${origin.width * 10 / 16}px`;
      decoration.style.transform = `scale(${1 / pose.scale})`;
      decoration.style.padding = `${origin.paperMargin * 10 / 16}px ${origin.paperMargin}px`;
      const decorationPrint = decoration.querySelector<HTMLElement>('.book-leaf__print');
      if (decorationPrint) decorationPrint.style.borderRadius = realPrint ? getComputedStyle(realPrint).borderRadius : '4px';
      if (option) {
        const css = getComputedStyle(option);
        decoration.style.borderRadius = css.borderRadius;
        decoration.style.boxShadow = css.boxShadow;
        decoration.style.outline = css.outline;
      }
    }
    const print = element.querySelector<HTMLElement>('.wallpaper-book__zoom-print');
    if (print) {
      print.style.inset = pose.paperInset;
      print.style.borderRadius = radius(realPrint, pose.paperRadius);
    }
    if (opening && !reducedMotion) setZoomDirection(false, true);
    else if (zoomSwapRef.current && !reducedMotion) {
      const { outgoing, direction } = zoomSwapRef.current;
      zoomSwapRef.current = null;
      const incoming = element.querySelector<HTMLElement>('.wallpaper-book__zoom-picture');
      if (incoming && print) {
        print.append(outgoing);
        zoomOutgoingRef.current = outgoing;
        const swap = bookZoomSwapTimeline(direction);
        const timing = { duration: swap.duration, easing: swap.easing, fill: 'both' as const };
        const animations = [outgoing.animate(swap.outgoing, timing), incoming.animate(swap.incoming, timing)];
        zoomSwapAnimationsRef.current = animations;
        zoomMovingRef.current = true;
        setZoomMoving(true);
        animations[1].onfinish = () => {
          cancelZoomSwap();
          zoomMovingRef.current = false;
          setZoomMoving(false);
          if (zoomReadyRef.current) revealZoomLive();
        };
      } else {
        zoomMovingRef.current = false;
        setZoomMoving(false);
      }
    } else {
      zoomSwapRef.current = null;
      zoomMovingRef.current = false;
      setZoomMoving(false);
    }
    return cancelZoomAnimation;
  }, [cancelZoomAnimation, cancelZoomSwap, revealZoomLive, setZoomDirection, zoomIndex, reducedMotion]);

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
      pendingMenuRef.current = null;
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
    const anchor = props.initialAnchorWallpaperId;
    if (anchor == null || restoredAnchorRef.current === anchor) return;
    // The shell echoes our anchor reports back as the initial anchor prop.
    // A report from the current moving selection must not restart its motion.
    if (model.entries[selectedRef.current]?.wallpaperId === anchor) {
      restoredAnchorRef.current = anchor;
      return;
    }
    const index = model.entries.findIndex((entry) => entry.wallpaperId === anchor);
    if (index < 0) return;
    restoredAnchorRef.current = anchor;
    releaseDrag();
    resetZoom();
    preferredIndexRef.current = index;
    moveTo(bookPositionForWallpaper(index));
  }, [model.entries, props.initialAnchorWallpaperId, moveTo, releaseDrag, resetZoom]);

  useEffect(() => {
    if (focusToken <= 0 && returnFocusToken <= 0) return;
    const frame = requestAnimationFrame(() => stageRef.current?.focus({ preventScroll: true }));
    return () => cancelAnimationFrame(frame);
  }, [focusToken, returnFocusToken]);

  // The book is the page's main control: when nothing else holds focus (a fresh window, or a
  // closed dialog that left focus on the body) keys go to it without a click first.
  useEffect(() => {
    if (!model.active) return undefined;
    const unfocused = () => document.body != null
      && (document.activeElement == null || document.activeElement === document.body);
    if (unfocused()) stageRef.current?.focus({ preventScroll: true });
    const adopt = (event: globalThis.KeyboardEvent) => {
      const stage = stageRef.current;
      if (!stage || event.defaultPrevented || event.target !== document.body || !unfocused()) return;
      if (resolveBookKey(event.key, event.shiftKey) === null) return;
      stage.focus({ preventScroll: true });
      event.preventDefault();
      stage.dispatchEvent(new KeyboardEvent('keydown', {
        key: event.key, code: event.code, shiftKey: event.shiftKey, bubbles: true, cancelable: true,
      }));
    };
    document.addEventListener('keydown', adopt);
    return () => document.removeEventListener('keydown', adopt);
  }, [model.active]);

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
    const blur = () => setFocused(false);
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
    setInteracting(viewActive);
  }, [viewActive, setInteracting]);

  useEffect(() => {
    if (!viewActive) {
      releaseDrag();
      resetZoom();
      moveTo(bookSnapTarget(positionRef.current, 0, lastPosition), true);
      cancelCrossFade();
    }
  }, [cancelCrossFade, resetZoom, viewActive, lastPosition, moveTo, releaseDrag]);

  useEffect(() => {
    // Pages are on screen whether or not the stage has keyboard focus or a menu is open, so
    // previews must not wait for either. The store keeps only the latest call: send one list,
    // open spread first.
    if (!model.active || !visible) return;
    observeVisible(
      bookPreviewOrder(spread, model.entries.length).map((index) => staticPreviewAssetPath(model.entries[index])),
      { priority: 'front' },
    );
  }, [leafCount, model.active, model.entries, observeVisible, spread, visible]);

  useEffect(() => {
    const key = `${model.resetKey}:${model.replaceCount}:${model.entries.length}`;
    const approach = bookAppendApproach(appendClaimRef.current, key, lastPosition - spread <= BOOK_APPEND_DISTANCE,
      model.active && visible && model.canAutoAppend && !model.loadingMore && !model.refreshing);
    appendClaimRef.current = approach.claim;
    if (approach.request) void model.onRequestMoreIfNeeded();
  }, [lastPosition, spread, model, visible]);

  const handleWheel = useCallback((event: globalThis.WheelEvent) => {
    pendingMenuRef.current = null;
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
    const count = bookLastPosition(latest.current.props.model.entries.length);
    const wheel = classifyBookWheel(event, performance.now(), wheelSampleRef.current);
    wheelSampleRef.current = wheel.sample;
    if (!wheel.delta) return;
    if (wheel.discrete) {
      const target = bookWheelTarget(positionRef.current, riffleRef.current ? null : targetRef.current, wheel.delta, count);
      if (wheelTimerRef.current !== null) clearTimeout(wheelTimerRef.current);
      wheelTimerRef.current = null;
      if (!riffleRef.current && targetRef.current !== null && frameRef.current !== null) targetRef.current = target;
      else moveTo(target);
      return;
    }
    cancelMotion();
    const position = accumulateBookWheel(positionRef.current, event, count, false);
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
    zoomNavigationRef.current += 1;
    zoomPendingNavigationRef.current = null;
    zoomMovingRef.current = true;
    cancelZoomDecodeTimer();
    cancelZoomAnimation();
    zoomResumePageVideoRef.current?.();
    zoomResumePageVideoRef.current = null;
    cancelCrossFade();
    setScrolling(false);
    setInteracting(true);
  }, [cancelCrossFade, cancelMotion, cancelZoomAnimation, cancelZoomDecodeTimer, releaseDrag, setScrolling, setInteracting]);

  const handlePointerDown = (event: PointerEvent<HTMLDivElement>) => {
    pendingMenuRef.current = null;
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
      position: positionRef.current, samples: [{ at: performance.now(), position: positionRef.current }],
    };
  };

  const sampleDrag = (drag: BookDrag, clientX: number, at: number) => {
    const position = clampBookPosition(drag.startPosition - (clientX - drag.startX) / drag.width, lastPosition);
    drag.position = position;
    drag.samples = [...drag.samples.filter((sample) => at - sample.at <= 80), { position, at }];
  };

  const handlePointerMove = (event: PointerEvent<HTMLDivElement>) => {
    const drag = dragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    const distance = event.clientX - drag.startX;
    if (!drag.moved && Math.abs(distance) < 4) return;
    if (!drag.moved) {
      event.currentTarget.setPointerCapture(event.pointerId);
      stageRef.current?.setAttribute('data-dragging', 'true');
    }
    drag.moved = true;
    suppressClickRef.current = true;
    const now = performance.now();
    const native = event.nativeEvent;
    // Coalesced samples improve velocity only; paint the newest dispatched position.
    for (const sample of native?.getCoalescedEvents?.() ?? []) {
      sampleDrag(drag, sample.clientX, Math.min(now, sample.timeStamp));
    }
    sampleDrag(drag, event.clientX, now);
    if (dragFrameRef.current === null) {
      dragFrameRef.current = requestAnimationFrame(() => {
        dragFrameRef.current = null;
        if (dragRef.current === drag) updatePosition(drag.position, false);
      });
    }
  };

  const finishPointer = (event: PointerEvent<HTMLDivElement>) => {
    const drag = dragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    if (drag.moved && event.type === 'pointerup') sampleDrag(drag, event.clientX, performance.now());
    releaseDrag();
    if (!drag.moved && settledRef.current) {
      const page = (event.target as HTMLElement | undefined)?.closest?.<HTMLElement>('[data-book-index]');
      const index = page ? Number(page.dataset.bookIndex) : -1;
      if (event.type === 'pointerup' && index >= 0 && !openIndices.includes(index)) {
        suppressClickRef.current = true;
        preferredIndexRef.current = index;
        moveTo(bookPositionForWallpaper(index));
      }
      return;
    }
    if (drag.moved) updatePosition(drag.position, false);
    const velocity = event.type === 'pointerup' ? bookDragVelocity(drag.samples, performance.now()) : 0;
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
    pendingMenuRef.current = null;
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
    const entry = latest.current.props.model.entries[index];
    if (!entry || !model.active) return;
    releaseDrag();
    pendingMenuRef.current = null;
    preferredIndexRef.current = index;
    stageRef.current?.focus({ preventScroll: true });
    const decision = resolveBookContextMenu(
      openBookWallpapers(positionRef.current, model.entries.length).includes(index),
      latest.current.zoomIndex !== null,
    );
    if (decision === 'open') {
      selectIndex(index);
      setContextMenu({ entry, x, y });
    } else {
      moveTo(bookPositionForWallpaper(index));
      if (settledRef.current) setContextMenu({ entry, x, y });
      else pendingMenuRef.current = { wallpaperId: entry.wallpaperId, x, y };
    }
  };

  const navigateZoom = (index: number) => {
    const entry = latest.current.props.model.entries[index];
    if (!entry || zoomClosingRef.current) return;
    // Finish the incoming picture immediately before accepting another input.
    if (zoomSwapAnimationsRef.current.length) {
      cancelZoomSwap();
      zoomMovingRef.current = false;
      setZoomMoving(false);
    }
    cancelZoomDecodeTimer();
    zoomPendingNavigationRef.current = null;
    zoomNavigationTargetRef.current = index;
    if (index === latest.current.zoomIndex) {
      zoomNavigationRef.current += 1;
      zoomPendingNavigationRef.current = null;
      return;
    }
    const asset = staticPreviewAssetPath(entry);
    const thumbnail = getThumbnail(asset);
    const fallback = staticFallbackAssetPath(entry, true);
    const request = ++zoomNavigationRef.current;
    const current = () => request === zoomNavigationRef.current && latest.current.zoomIndex !== null && !zoomClosingRef.current;
    let accepted = false;
    const accept = (source: string | null) => {
      if (!current() || accepted) return;
      accepted = true;
      cancelZoomDecodeTimer();
      const commit = () => {
        if (!current() || zoomMovingRef.current) return;
        const direction = index - (latest.current.zoomIndex ?? index);
        const picture = zoomRef.current?.querySelector<HTMLElement>('.wallpaper-book__zoom-picture');
        const outgoing = !latest.current.reducedMotion && picture ? picture.cloneNode(true) as HTMLElement : null;
        if (outgoing && picture) {
          outgoing.className = 'wallpaper-book__zoom-outgoing';
          outgoing.setAttribute('aria-hidden', 'true');
          const oldStill = picture.querySelector<HTMLElement>('.wallpaper-book__zoom-still');
          const clonedStill = outgoing.querySelector<HTMLElement>('.wallpaper-book__zoom-still');
          if (oldStill && clonedStill) clonedStill.style.opacity = getComputedStyle(oldStill).opacity;
          const video = picture.querySelector<HTMLVideoElement>('video');
          const clonedVideo = outgoing.querySelector<HTMLVideoElement>('video');
          if (video && clonedVideo) {
            const frame = captureBookVideoStill(video);
            clonedVideo.removeAttribute('autoplay');
            clonedVideo.removeAttribute('src');
            if (frame) { clonedVideo.poster = frame.source; frame.resume(); }
          }
        }
        cancelZoomAnimation();
        zoomResumePageVideoRef.current?.();
        zoomResumePageVideoRef.current = null;
        zoomReadyRef.current = false;
        zoomPoseRef.current = null;
        zoomSwapRef.current = outgoing ? { outgoing, direction } : null;
        zoomMovingRef.current = Boolean(outgoing);
        preferredIndexRef.current = index;
        // Source, hidden real face, spread and landing pose change together.
        flushSync(() => {
          moveTo(bookPositionForWallpaper(index), true);
          selectIndex(index);
          setZoomStillSrc(source);
          changeZoomIndex(index);
          setZoomMoving(Boolean(outgoing));
        });
      };
      if (zoomMovingRef.current) zoomPendingNavigationRef.current = commit;
      else commit();
    };
    // Bound authorization + decoding together; a slow original never stalls input.
    zoomDecodeTimerRef.current = setTimeout(() => {
      zoomDecodeTimerRef.current = null;
      const page = stageRef.current?.querySelector<HTMLElement>(`#book-option-${entry.wallpaperId}`);
      const image = page?.querySelector<HTMLImageElement>('img[data-preview-loaded="true"]');
      const paintedSource = image?.currentSrc || image?.src;
      const cachedThumbnail = getThumbnail(asset);
      accept(paintedSource || (cachedThumbnail ? safeFileSrc(cachedThumbnail) : null));
    }, 120);
    const source = thumbnail ? Promise.resolve(thumbnail) : fallback
      ? api.previewAssetAuthorize(fallback, entry.path)
      : api.thumbnailFor(asset).then((thumbnail) => thumbnail.path);
    void source.then((authorized) => {
      if (!current() || accepted) return;
      return decodeBookStill(safeFileSrc(authorized), (image) => accept(image.src), () => current() && !accepted);
    }).catch(() => { /* The bounded thumbnail fallback still runs. */ });
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    pendingMenuRef.current = null;
    if (event.target !== event.currentTarget || !interactionActive) return;
    if (event.altKey || event.metaKey || event.ctrlKey) return;
    const intent = resolveBookKey(event.key, event.shiftKey);
    if (!intent) return;
    event.preventDefault();
    event.stopPropagation();
    if (intent === 'next' || intent === 'previous') {
      const direction = intent === 'next' ? 1 : -1;
      if (zoomIndex !== null) {
        const index = Math.min(model.entries.length - 1, Math.max(0, (zoomNavigationTargetRef.current ?? zoomIndex) + direction));
        navigateZoom(index);
      } else moveTo((targetRef.current ?? Math.round(positionRef.current)) + direction);
    } else if (intent === 'first' || intent === 'last') {
      if (zoomIndex !== null) {
        const index = intent === 'first' ? 0 : model.entries.length - 1;
        navigateZoom(index);
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
    elements: { stageRef, zoomRef, zoomMediaRef, zoomStillRef, zoomDecorationRef, leavesRef },
    snapshot: { spread, leafKeyOffset, selectedIndex, selectedEntry, settled, zoomIndex, zoomMoving, zoomStillSrc, contextMenu, reducedMotion, interactionActive, openIndices, visibleLeaves },
    actions: {
      handlePointerDown, handlePointerMove, finishPointer, handleKeyDown, handlePageClick,
      openContextMenu, applySelected, toggleZoom, closeZoom, focusStage, revealZoomLive, mayUpdateZoomStill,
      cancelPendingMenu: () => { pendingMenuRef.current = null; },
      closeContextMenu: () => { setContextMenu(null); focusStage(); },
    },
  };
}
