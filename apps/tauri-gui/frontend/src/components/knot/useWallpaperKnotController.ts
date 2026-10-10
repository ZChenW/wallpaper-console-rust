import { useEffect, useRef, useState } from 'react';
import { useReducedMotion } from '../../hooks/useReducedMotion.ts';
import { useThumbnailStore } from '../../state/ThumbnailStoreContext.tsx';
import { libraryEntryApplyAvailable, resolveLibraryFlowStartupAnchor, type LibraryViewModel } from '../libraryViewModel.ts';
import { safeFileSrc } from '../safeFileSrc.ts';
import { bookStaticSource } from '../wallpaperBookModel.ts';
import { largePreviewKey, staticPreviewAssetPath } from '../wallpaperPreviewMedia.ts';
import { api } from '../../api/bridge.ts';
import type { LibraryBrowserItemDTO } from '../../api/types.ts';
import type { ApplyGesture, LibraryViewMode } from '../../shell/shellPreferences.ts';
import { isContextMenuKey } from '../../shell/keyboardInteraction.ts';
import type { BookWheelSample } from '../wallpaperBookModel.ts';
import { KNOT_CURVES, modulo } from './knotCurves.ts';
import { loadKnotRenderer, type KnotRenderer } from './knotRenderer.ts';
import { KnotClipPlayer } from './knotClip.ts';
import { createRope, grabRope, pullRope, releaseRope, settleRope, stepRope } from './knotRope.ts';
import {
  AUTO_PICTURES_PER_SECOND, CAMERA_REST_PICTURES, CAMERA_REST_SPEED, DRAG_DEAD_ZONE, SETTLE_IDLE_MS,
  accumulateKnotWheel, knotPointerInteraction, replacementIndex, springStep, nearestLoopTarget, selectedIndex,
  settleTarget, textureWindow, wheelSensitivity,
} from './knotModel.ts';

export interface WallpaperKnotProps {
  readonly model: LibraryViewModel;
  readonly applyGesture: ApplyGesture;
  readonly viewMode?: LibraryViewMode;
  readonly onViewModeChange?: (mode: LibraryViewMode) => void;
  readonly initialAnchorWallpaperId?: number | null;
  readonly focusToken?: number;
  readonly returnFocusToken?: number;
  readonly onAnchorChange?: (wallpaperId: number, settled?: boolean) => void;
}
interface KnotEngine { sync: () => void; switchKnot: (index: number) => void; applySelected: () => void }
const APPEND_DISTANCE = 10;
const CAMERA_SETTLE_PICTURES = 0.03;
// The knot last used is kept across launches. It is a taste, not a setting worth a preferences entry.
const KNOT_STORAGE_KEY = 'wc.knot.curve';
function storedKnotIndex() {
  try {
    const index = KNOT_CURVES.findIndex((curve) => curve.id === localStorage.getItem(KNOT_STORAGE_KEY));
    return index >= 0 ? index : 0;
  } catch { return 0; }
}
function storeKnotIndex(index: number) {
  try { localStorage.setItem(KNOT_STORAGE_KEY, KNOT_CURVES[index].id); } catch { /* Restricted storage: the session still remembers. */ }
}
const INPUT_KEYS = new Set(['ArrowLeft', 'ArrowRight', 'PageDown', 'PageUp', 'Home', 'End', 'a', 'A', '1', '2', '3', '4', 'Enter', 'Escape', 'ContextMenu', 'F10']);

/** RAF, store subscriptions and input stay imperative; React only sees semantic changes. */
export function useWallpaperKnotController(props: WallpaperKnotProps) {
  const reducedMotion = useReducedMotion();
  const store = useThumbnailStore();
  const stageRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const videoRef = useRef<HTMLVideoElement>(null);
  const engineRef = useRef<KnotEngine | null>(null);
  const latest = useRef({ props, reducedMotion });
  latest.current = { props, reducedMotion };
  const [selection, setSelection] = useState(() => ({
    index: resolveLibraryFlowStartupAnchor(props.model.entries, props.initialAnchorWallpaperId, props.model.currentPath)?.index ?? 0,
    settled: true,
  }));
  const [status, setStatus] = useState<'loading' | 'ready' | 'failed'>('loading');
  const [knotIndex, setKnotIndex] = useState(storedKnotIndex);
  const initialKnot = useRef(knotIndex);
  const [contextMenu, setContextMenu] = useState<{ entry: LibraryBrowserItemDTO; x: number; y: number } | null>(null);

  useEffect(() => {
    const stage = stageRef.current, canvas = canvasRef.current;
    if (!stage || !canvas) return;
    let disposed = false, renderer: KnotRenderer | null = null, frame: number | null = null;
    let idleTimer: ReturnType<typeof setTimeout> | null = null;
    const startup = () => {
      const { model, initialAnchorWallpaperId } = latest.current.props;
      const anchor = resolveLibraryFlowStartupAnchor(model.entries, initialAnchorWallpaperId, model.currentPath);
      const entry = anchor ? model.entries[anchor.index] : undefined;
      // "Found" means the session anchor or the current wallpaper, not the first-result fallback.
      return { index: anchor?.index ?? 0, found: Boolean(entry && (entry.wallpaperId === initialAnchorWallpaperId || entry.path === model.currentPath)) };
    };
    const first = startup(), initial = first.index;
    // Until the user travels, a current wallpaper that becomes known late still claims the camera.
    let startupPending = !first.found, userMoved = false;
    let queryKey = latest.current.props.model.resetKey;
    let count = latest.current.props.model.entries.length;
    let targetT = initial / Math.max(1, count), cameraT = targetT, cameraV = 0;
    let curveIndex = initialKnot.current, auto = false, inputPending = false;
    let lastFrame = performance.now();
    let wheelSample: BookWheelSample | undefined;
    let reportedId: number | undefined, reportedSettled: boolean | undefined;
    let observationKey = '', appendClaim = '';
    let previewObservation: string | null = null;
    let previewUnsubscribe: (() => void) | null = null;
    const subscriptions = new Map<string, () => void>();
    let entries = latest.current.props.model.entries;
    let reset = latest.current.props.model.replaceCount;
    let focusToken = latest.current.props.focusToken, returnFocusToken = latest.current.props.returnFocusToken;
    let cameraSettled = true;
    let pendingMenu: { id: number; x: number; y: number } | null = null;
    let suppressClick = false;
    let pausedAt: number | null = null;
    // A drag that starts on a piece pulls the rope there; one that starts on empty space travels.
    let drag: { id: number; x: number; y: number; startX: number; startY: number; moved: boolean; index: number | null;
      pull: { perPixel: number; x: number; y: number; z: number } | null } | null = null;
    let rope = createRope(count);
    const video = videoRef.current;
    const clip = video ? new KnotClipPlayer(video, { load: (path) => api.previewClip(path) }) : null;
    /** The clip plays on a video wallpaper the camera rests on; it rides along when the rope is pulled. */
    const syncClip = (quiet: boolean) => {
      if (!clip) return;
      const index = selectedIndex(targetT, count);
      const entry = quiet && !document.hidden ? latest.current.props.model.entries[index] : undefined;
      clip.want(entry && entry.type === 'video' && renderer?.isAssembled(index)
        ? { key: String(entry.wallpaperId), path: entry.path, place: () => renderer?.pictureRect(index) ?? null } : null);
    };
    const canRun = () => !disposed && !document.hidden && latest.current.props.model.active;
    const interactive = () => renderer?.available && canRun() && !latest.current.props.model.queryReplacementPending;
    const focus = () => stage.focus({ preventScroll: true });
    const fromControl = (event: Event) => event.target instanceof Element && Boolean(event.target.closest('button, [role="menu"], .library-view-switch, .wallpaper-knot__caption'));
    const cancelMenu = () => { pendingMenu = null; setContextMenu(null); };
    const applySelected = () => {
      const { model } = latest.current.props;
      const entry = model.entries[selectedIndex(cameraT, count)];
      if (interactive() && cameraSettled && entry && libraryEntryApplyAvailable(model.canApplyToDisplay, model.isEntryApplicable, entry)) {
        model.onSelect(entry); model.onApply(entry);
      }
      focus();
    };
    const cancelFrame = () => { if (frame !== null) cancelAnimationFrame(frame); frame = null; };
    const invalidate = () => {
      if (canRun() && renderer?.available && frame === null) {
        // Idle time is not animation time. Once awake, use actual frame intervals.
        lastFrame = performance.now();
        frame = requestAnimationFrame(tick);
      }
    };
    const clearIdle = () => { if (idleTimer !== null) clearTimeout(idleTimer); idleTimer = null; };
    const pictureSource = (entry: LibraryBrowserItemDTO) => bookStaticSource(entry, Boolean(store.getFailure(entry.path))).thumbnailPath;
    const clearPreview = () => {
      previewUnsubscribe?.(); previewUnsubscribe = null;
      previewObservation = null;
      renderer?.setPreview(null, null);
    };
    const report = (settled: boolean) => {
      const { model, onAnchorChange } = latest.current.props;
      const index = selectedIndex(cameraT, count), entry = model.entries[index];
      cameraSettled = settled;
      if (settled && pendingMenu && entry?.wallpaperId === pendingMenu.id) {
        const menu = pendingMenu; pendingMenu = null;
        setContextMenu({ entry, x: menu.x, y: menu.y });
      }
      if (!entry || (reportedId === entry.wallpaperId && reportedSettled === settled)) return;
      reportedId = entry.wallpaperId; reportedSettled = settled;
      setSelection((previous) => previous.index === index && previous.settled === settled ? previous : { index, settled });
      onAnchorChange?.(entry.wallpaperId, settled);
      if (settled) model.onSelect(entry);
    };
    const requestMore = () => {
      const { model } = latest.current.props;
      if (count - 1 - selectedIndex(cameraT, count) > APPEND_DISTANCE) { appendClaim = ''; return; }
      const claim = `${model.replaceCount}:${count}`;
      if (model.canAutoAppend && !model.refreshing && !model.loadingMore && claim !== appendClaim) {
        appendClaim = claim;
        void model.onRequestMoreIfNeeded();
      }
    };
    const observePictures = (previewIndex: number | null) => {
      if (!renderer || !canRun()) return;
      const { model } = latest.current.props;
      const indices = textureWindow(cameraT, count);
      const signature = `${model.replaceCount}:${count}:${selectedIndex(cameraT, count)}`;
      if (observationKey !== signature) {
        observationKey = signature;
        const wanted = new Set(indices.map((index) => String(model.entries[index].wallpaperId)));
        // Release exits before allocating arrivals, so crossing the seam stays within budget.
        for (const [key, unsubscribe] of subscriptions) if (!wanted.has(key)) {
          unsubscribe(); subscriptions.delete(key); renderer.setPicture(key, null);
        }
        for (const index of indices) {
          const entry = model.entries[index], key = String(entry.wallpaperId);
          if (subscriptions.has(key)) continue;
          // One source for both sizes (see pictureSource): the video's own frame, with the bundled
          // preview only as a fallback if that frame cannot be made.
          const path = pictureSource(entry), fallback = staticPreviewAssetPath(entry);
          const update = () => {
            const thumbnail = store.get(path) ?? (store.getFailure(path) ? store.get(fallback) : undefined);
            renderer?.setPicture(key, thumbnail ? safeFileSrc(thumbnail) : null);
          };
          const unsubscribePath = store.subscribe(path, update);
          const unsubscribeFallback = fallback === path ? null : store.subscribe(fallback, update);
          subscriptions.set(key, () => { unsubscribePath(); unsubscribeFallback?.(); });
          update();
        }
      }
      const entry = previewIndex === null ? null : model.entries[previewIndex] ?? null;
      // The large preview must be the same picture as the small one, only sharper. Asking for the
      // large size of a different file (the video, when the small one came from its bundled square
      // preview) made the assembled picture change content and shape a moment after landing.
      const key = entry ? largePreviewKey(pictureSource(entry)) : null;
      if (previewObservation !== key) {
        clearPreview();
        previewObservation = key;
        if (entry && key) {
          const pictureKey = String(entry.wallpaperId);
          const update = () => {
            const thumbnail = store.get(key);
            renderer?.setPreview(pictureKey, thumbnail ? safeFileSrc(thumbnail) : null);
          };
          previewUnsubscribe = store.subscribe(key, update);
          update();
        }
      }
      // Replacing pending work keeps a long scroll from creating a stale request backlog.
      // Only refresh the queue when the window or large-preview request changes.
      const paths = indices.map((index) => pictureSource(model.entries[index]));
      const queueKey = `${signature}:${key ?? ''}`;
      if (queueKey !== queuedObservation) {
        queuedObservation = queueKey;
        store.observeVisible(key ? [key, ...paths] : paths, { priority: 'front' });
      }
    };
    let queuedObservation = '';
    const armSettle = () => {
      clearIdle();
      if (!canRun() || auto || drag) return;
      idleTimer = setTimeout(() => {
        idleTimer = null;
        if (!canRun() || auto || drag) return;
        inputPending = false;
        targetT = settleTarget(targetT, count);
        invalidate();
      }, SETTLE_IDLE_MS);
    };
    const stopAuto = () => { auto = false; };
    const moveTo = (t: number, settleImmediately = false) => {
      if (!interactive()) return;
      cancelMenu();
      stopAuto();
      userMoved = true;
      targetT = t;
      inputPending = !settleImmediately;
      clearPreview();
      clearIdle();
      if (!settleImmediately) armSettle();
      // Reduced motion changes the camera before reporting, never starts a travel tween.
      if (latest.current.reducedMotion) { cameraT = targetT; cameraV = 0; }
      report(false);
      invalidate();
    };
    function tick(now: number) {
      frame = null;
      if (!canRun() || !renderer?.available) return;
      const dt = Math.max(0, (now - lastFrame) / 1000);
      lastFrame = now;
      const reduced = latest.current.reducedMotion;
      if (reduced) stopAuto();
      if (auto) targetT += AUTO_PICTURES_PER_SECOND * dt / Math.max(1, count);
      if (reduced) { cameraT = targetT; cameraV = 0; }
      else ({ position: cameraT, velocity: cameraV } = springStep(cameraT, cameraV, targetT, dt));
      const away = Math.abs(cameraT - targetT) * count, speed = Math.abs(cameraV) * count;
      // Rest is declared only when the remaining move is far below a pixel, so ending it is not a jump.
      const cameraMoving = away > CAMERA_REST_PICTURES || speed > CAMERA_REST_SPEED;
      if (!cameraMoving) { cameraT = targetT; cameraV = 0; }
      const swinging = reduced ? (settleRope(rope), false) : stepRope(rope, dt);
      renderer.setOffsets(swinging ? rope.offsets : null);
      syncClip(!auto && !inputPending && !cameraMoving && !reduced);
      const result = renderer.render(cameraT, now, dt, reduced, speed);
      // After drawing, so the clip sits on where the picture has just been put.
      clip?.reposition();
      // Selection/status follow the camera as soon as it has visibly arrived.
      const resting = !auto && !drag && !inputPending;
      report(resting && away <= CAMERA_SETTLE_PICTURES);
      // The sharp picture waits for the camera to stop: decoding and uploading it costs two long
      // frames, which are invisible at rest and a stutter in the middle of a move.
      observePictures(resting && !cameraMoving ? selectedIndex(targetT, count) : null);
      requestMore();
      if ((auto || cameraMoving || result.moving || swinging) && frame === null) frame = requestAnimationFrame(tick);
    }
    const switchKnot = (index: number) => {
      if (!interactive() || index === curveIndex || !KNOT_CURVES[index]) return;
      cancelMenu();
      curveIndex = index; setKnotIndex(index); storeKnotIndex(index);
      clearPreview(); report(false);
      renderer?.setLayout(entries.map((entry) => ({ key: String(entry.wallpaperId), id: entry.wallpaperId })), KNOT_CURVES[index], performance.now(), latest.current.reducedMotion);
      invalidate();
    };
    const toggleAuto = () => {
      if (!interactive() || latest.current.reducedMotion) return;
      cancelMenu();
      auto = !auto; userMoved = true;
      clearIdle(); inputPending = false;
      if (!auto) targetT = settleTarget(targetT, count);
      clearPreview(); report(false); invalidate();
    };
    const keydown = (event: KeyboardEvent) => {
      if (event.target !== stage || !interactive()) return;
      // Tab/other keys also abandon a queued menu instead of stealing focus on arrival.
      if (pendingMenu) cancelMenu();
      if (event.key === 'Escape') { event.preventDefault(); cancelMenu(); return; }
      if (event.altKey || event.ctrlKey || event.metaKey || !INPUT_KEYS.has(event.key)) return;
      if (event.key === 'F10' && !event.shiftKey) return;
      event.preventDefault();
      if (event.key === 'Enter') { if (!event.repeat) applySelected(); }
      else if (isContextMenuKey(event.key, event.shiftKey)) {
        const rect = stage.getBoundingClientRect();
        openMenu(selectedIndex(cameraT, count), rect.left + rect.width / 2, rect.top + rect.height / 2);
      } else if (/^[1-4]$/.test(event.key)) switchKnot(Number(event.key) - 1);
      else if (event.key.toLowerCase() === 'a') { if (!event.repeat) toggleAuto(); }
      else if (event.key === 'Home') moveTo(nearestLoopTarget(cameraT, 0), true);
      else if (event.key === 'End') {
        moveTo(nearestLoopTarget(cameraT, (count - 1) / count), true);
        if (latest.current.props.model.appendNeedsRetry) void latest.current.props.model.onAppendMore();
      } else moveTo((Math.round(targetT * count) + (event.key === 'ArrowRight' || event.key === 'PageDown' ? 1 : -1)) / count, true);
    };
    const wheel = (event: WheelEvent) => {
      if (!interactive() || event.ctrlKey || fromControl(event)) return;
      event.preventDefault();
      const next = accumulateKnotWheel(targetT, event, stage.clientHeight, count, performance.now(), wheelSample);
      wheelSample = next.sample;
      if (next.target !== targetT) moveTo(next.target);
    };
    const hitDetail = (x: number, y: number) => {
      const rect = canvas.getBoundingClientRect(); return renderer?.pickDetail(x - rect.left, y - rect.top) ?? null;
    };
    const hit = (x: number, y: number) => hitDetail(x, y)?.index ?? null;
    const pointerdown = (event: PointerEvent) => {
      if (!interactive() || event.button !== 0 || drag || fromControl(event)) return;
      cancelMenu(); suppressClick = false;
      focus(); stopAuto(); clearIdle();
      stage.setPointerCapture(event.pointerId);
      const detail = hitDetail(event.clientX, event.clientY);
      const perPixel = detail && !latest.current.reducedMotion ? renderer?.unitsPerPixel(detail.depth) ?? 0 : 0;
      drag = { id: event.pointerId, x: event.clientX, y: event.clientY, startX: event.clientX, startY: event.clientY, moved: false,
        index: detail?.index ?? null, pull: perPixel > 0 ? { perPixel, x: 0, y: 0, z: 0 } : null };
    };
    const pointermove = (event: PointerEvent) => {
      if (!drag || drag.id !== event.pointerId) return;
      const moved = Math.hypot(event.clientX - drag.startX, event.clientY - drag.startY) > DRAG_DEAD_ZONE;
      if (!drag.moved && !moved) return;
      if (drag.pull && drag.index !== null) {
        if (!drag.moved) {
          drag.moved = true; cancelMenu(); clearPreview();
          grabRope(rope, drag.index);
          [drag.pull.x, drag.pull.y, drag.pull.z] = rope.hold;
          report(false);
        }
        // Screen y grows downwards, the scene's upwards.
        pullRope(rope, drag.pull.x + (event.clientX - drag.startX) * drag.pull.perPixel,
          drag.pull.y - (event.clientY - drag.startY) * drag.pull.perPixel, drag.pull.z);
        invalidate();
        return;
      }
      const dx = event.clientX - (drag.moved ? drag.x : drag.startX);
      const dy = event.clientY - (drag.moved ? drag.y : drag.startY);
      drag.moved = true; drag.x = event.clientX; drag.y = event.clientY;
      moveTo(targetT + (dx + dy) * wheelSensitivity(stage.clientHeight, count));
    };
    const finishPointer = (event: PointerEvent) => {
      if (!drag || event.pointerId !== drag.id) return;
      const completed = drag; drag = null;
      if (stage.hasPointerCapture(completed.id)) stage.releasePointerCapture(completed.id);
      suppressClick = completed.moved || event.type !== 'pointerup';
      if (completed.pull && completed.moved) { releaseRope(rope); invalidate(); return; }
      if (suppressClick || completed.index === null) { inputPending = true; armSettle(); invalidate(); }
    };
    const click = (event: MouseEvent) => {
      if (!interactive() || fromControl(event) || suppressClick) return;
      const index = hit(event.clientX, event.clientY);
      if (index === null) return;
      const { model, applyGesture } = latest.current.props;
      const entry = model.entries[index];
      const intent = knotPointerInteraction({ gesture: applyGesture, clickCount: event.detail,
        canApply: libraryEntryApplyAvailable(model.canApplyToDisplay, model.isEntryApplicable, entry), fromControl: false },
      cameraSettled && index === selectedIndex(cameraT, count) && Boolean(renderer?.isAssembled(index)));
      if (intent.travel) moveTo(nearestLoopTarget(cameraT, index / count), true);
      if (intent.select) model.onSelect(entry);
      if (intent.apply) model.onApply(entry);
    };
    const openMenu = (index: number, x: number, y: number) => {
      if (!interactive() || !entries[index]) return;
      focus();
      const entry = entries[index];
      if (cameraSettled && index === selectedIndex(cameraT, count)) {
        stopAuto(); latest.current.props.model.onSelect(entry);
        setContextMenu({ entry, x, y });
      } else {
        moveTo(nearestLoopTarget(cameraT, index / count), true);
        pendingMenu = { id: entry.wallpaperId, x, y };
        invalidate();
      }
    };
    const contextmenu = (event: MouseEvent) => {
      if (fromControl(event)) return;
      event.preventDefault();
      const index = hit(event.clientX, event.clientY);
      if (index !== null) openMenu(index, event.clientX, event.clientY);
    };
    const adopt = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.target !== document.body || !interactive() || !INPUT_KEYS.has(event.key) || event.altKey || event.ctrlKey || event.metaKey) return;
      if (event.key === 'F10' && !event.shiftKey) return;
      focus();
      stage.dispatchEvent(new KeyboardEvent('keydown', { key: event.key, repeat: event.repeat, shiftKey: event.shiftKey, bubbles: true, cancelable: true }));
      event.preventDefault();
    };
    const syncPause = () => {
      if (!canRun()) {
        if (pausedAt === null) pausedAt = performance.now();
        cancelFrame(); clearIdle(); cancelMenu(); clip?.halt();
        const captured = drag; drag = null;
        if (captured && stage.hasPointerCapture(captured.id)) stage.releasePointerCapture(captured.id);
        releaseRope(rope);
        store.setInteracting(false);
      } else {
        store.setInteracting(true);
        if (pausedAt !== null) {
          renderer?.resumeAfter(performance.now() - pausedAt);
          lastFrame = performance.now(); pausedAt = null;
          if (inputPending) armSettle();
        }
        invalidate();
      }
    };
    const resize = () => {
      const rect = stage.getBoundingClientRect(); renderer?.setSize(rect.width, rect.height, window.devicePixelRatio); clip?.reposition(); invalidate();
    };
    const theme = () => {
      const style = getComputedStyle(stage);
      renderer?.setTheme(style.backgroundColor, style.borderTopColor);
      invalidate();
    };
    const resizeObserver = new ResizeObserver(resize); resizeObserver.observe(stage);
    const themeObserver = new MutationObserver(theme);
    themeObserver.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme', 'style', 'class', 'data-color-scheme'] });
    const colorScheme = window.matchMedia('(prefers-color-scheme: dark)');
    colorScheme.addEventListener('change', theme);
    stage.addEventListener('click', click);
    stage.addEventListener('contextmenu', contextmenu);
    stage.addEventListener('keydown', keydown);
    stage.addEventListener('wheel', wheel, { passive: false });
    stage.addEventListener('pointerdown', pointerdown);
    stage.addEventListener('pointermove', pointermove);
    stage.addEventListener('pointerup', finishPointer);
    stage.addEventListener('pointercancel', finishPointer);
    stage.addEventListener('lostpointercapture', finishPointer);
    document.addEventListener('keydown', adopt);
    document.addEventListener('visibilitychange', syncPause);
    window.addEventListener('resize', resize);
    store.setScrolling(false); syncPause();
    engineRef.current = {
      switchKnot, applySelected,
      sync: () => {
        const next = latest.current.props;
        syncPause();
        if (next.model.queryReplacementPending) { clearPreview(); return; }
        if (reset !== next.model.replaceCount || entries !== next.model.entries) {
          cancelMenu();
          const replacing = reset !== next.model.replaceCount;
          reset = next.model.replaceCount;
          const nextCount = next.model.entries.length;
          if (replacing) {
            // A refresh of the same result must not throw the camera back to the first picture.
            const queryChanged = queryKey !== next.model.resetKey;
            queryKey = next.model.resetKey;
            const fallback = startup();
            // Nothing has been chosen yet, by the user or by startup: let startup choose now.
            const undecided = startupPending && !userMoved;
            const index = replacementIndex(next.model.entries.map((entry) => entry.wallpaperId),
              undecided ? undefined : entries[selectedIndex(targetT, count)]?.wallpaperId, queryChanged, fallback.index);
            if (queryChanged) { startupPending = false; userMoved = false; } else if (fallback.found) startupPending = false;
            targetT = cameraT = index / Math.max(1, nextCount); cameraV = 0;
            stopAuto(); inputPending = false; clearIdle();
            reportedId = undefined; reportedSettled = undefined; appendClaim = '';
          } else if (count && nextCount) {
            // Appending retains picture-space progress, including the current loop turn.
            targetT = Math.floor(targetT) + modulo(targetT) * count / nextCount;
            cameraT = Math.floor(cameraT) + modulo(cameraT) * count / nextCount;
            cameraV *= count / nextCount;
          }
          entries = next.model.entries; count = nextCount;
          // Nodes are per wallpaper: a different list is a different rope.
          if (drag?.pull) drag.pull = null;
          rope = createRope(count);
          for (const [key, unsubscribe] of subscriptions) { unsubscribe(); renderer?.setPicture(key, null); }
          subscriptions.clear(); clearPreview(); observationKey = queuedObservation = '';
          renderer?.setLayout(entries.map((entry) => ({ key: String(entry.wallpaperId), id: entry.wallpaperId })), KNOT_CURVES[curveIndex], performance.now(), latest.current.reducedMotion);
        }
        if (startupPending && !userMoved && count > 0) {
          const late = startup();
          if (late.found) {
            startupPending = false;
            targetT = cameraT = late.index / count; cameraV = 0;
            reportedId = undefined; reportedSettled = undefined;
            clearPreview(); invalidate();
          }
        }
        if (latest.current.reducedMotion) { stopAuto(); cameraT = targetT; cameraV = 0; }
        if (focusToken !== next.focusToken || returnFocusToken !== next.returnFocusToken) {
          focusToken = next.focusToken; returnFocusToken = next.returnFocusToken;
          if (next.model.active) focus();
        }
        invalidate();
      },
    };
    void loadKnotRenderer().then((Renderer) => {
      if (disposed) return;
      try {
        renderer = new Renderer(canvas); renderer.onInvalidate = invalidate;
        renderer.setLayout(entries.map((entry) => ({ key: String(entry.wallpaperId), id: entry.wallpaperId })), KNOT_CURVES[curveIndex], performance.now(), latest.current.reducedMotion);
        resize(); theme(); setStatus('ready');
        lastFrame = performance.now(); report(true);
        if (latest.current.props.model.active && (document.activeElement == null || document.activeElement === document.body || (latest.current.props.focusToken ?? 0) > 0)) focus();
        invalidate();
      } catch { renderer?.dispose(); renderer = null; setStatus('failed'); }
    }, () => { if (!disposed) setStatus('failed'); });
    return () => {
      disposed = true; cancelFrame(); clearIdle(); clip?.dispose();
      const captured = drag; drag = null;
      if (captured && stage.hasPointerCapture(captured.id)) stage.releasePointerCapture(captured.id);
      engineRef.current = null;
      for (const unsubscribe of subscriptions.values()) unsubscribe();
      previewUnsubscribe?.();
      store.setInteracting(false);
      resizeObserver.disconnect(); themeObserver.disconnect(); colorScheme.removeEventListener('change', theme);
      stage.removeEventListener('click', click); stage.removeEventListener('contextmenu', contextmenu);
      stage.removeEventListener('keydown', keydown); stage.removeEventListener('wheel', wheel);
      stage.removeEventListener('pointerdown', pointerdown); stage.removeEventListener('pointermove', pointermove);
      stage.removeEventListener('pointerup', finishPointer); stage.removeEventListener('pointercancel', finishPointer);
      stage.removeEventListener('lostpointercapture', finishPointer);
      document.removeEventListener('keydown', adopt); document.removeEventListener('visibilitychange', syncPause);
      window.removeEventListener('resize', resize);
      renderer?.dispose();
    };
  }, [store]);
  useEffect(() => { engineRef.current?.sync(); }, [props.model.entries, props.model.active, props.model.replaceCount, props.model.currentPath, props.model.resetKey, props.model.queryReplacementPending, props.model.loadingMore, props.model.canAutoAppend, props.focusToken, props.returnFocusToken, reducedMotion]);
  return { stageRef, canvasRef, videoRef, selection, selectedEntry: props.model.entries[selection.index], status,
    knotIndex, reducedMotion, contextMenu,
    closeContextMenu: () => setContextMenu(null),
    focusStage: () => stageRef.current?.focus({ preventScroll: true }),
    applySelected: () => engineRef.current?.applySelected(),
    switchKnot: (index: number) => engineRef.current?.switchKnot(index),
  };
}
