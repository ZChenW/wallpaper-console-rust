import { useEffect, useRef, useState } from 'react';
import { useReducedMotion } from '../../hooks/useReducedMotion.ts';
import { useThumbnailStore } from '../../state/ThumbnailStoreContext.tsx';
import { resolveLibraryFlowStartupAnchor, type LibraryViewModel } from '../libraryViewModel.ts';
import { safeFileSrc } from '../safeFileSrc.ts';
import { staticPreviewAssetPath } from '../wallpaperPreviewMedia.ts';
import { bookSpringStep, type BookWheelSample } from '../wallpaperBookModel.ts';
import { loadKnotRenderer, type KnotRenderer, type KnotRenderState } from './knotRenderer.ts';
import {
  accumulateKnotWheel, clampOffset, knotFlyOut, knotFlowDuration, knotPreviewOrder, knotSlots, knotSnapTarget, knotTimeline,
  KNOT_IDLE_SECONDS, KNOT_ORBIT_SENSITIVITY, KNOT_PITCH_LIMIT, KNOT_RETURN_SECONDS,
} from './knotModel.ts';

export interface WallpaperKnotProps {
  readonly model: LibraryViewModel;
  readonly initialAnchorWallpaperId?: number | null;
  readonly focusToken?: number;
  readonly returnFocusToken?: number;
  readonly onAnchorChange?: (wallpaperId: number, settled?: boolean) => void;
}
interface KnotEngine {
  sync: () => void;
  setOriginal: (key: string | null, url: string | null) => void;
}
const ORBIT_FRICTION = 7;
const DRAG_THRESHOLD = 4;
const APPEND_DISTANCE = 10;
const INPUT_KEYS = new Set(['ArrowLeft', 'ArrowRight', 'Home', 'End', 'r', 'R']);

/** Imperative animation/input ownership; React sees only selection and settle changes. */
export function useWallpaperKnotController(props: WallpaperKnotProps) {
  const reducedMotion = useReducedMotion();
  const store = useThumbnailStore();
  const stageRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const engineRef = useRef<KnotEngine | null>(null);
  const latest = useRef({ props, reducedMotion });
  latest.current = { props, reducedMotion };
  const [selection, setSelection] = useState(() => ({
    index: resolveLibraryFlowStartupAnchor(props.model.entries, props.initialAnchorWallpaperId, props.model.currentPath)?.index ?? 0,
    settled: true,
  }));
  const [status, setStatus] = useState<'loading' | 'ready' | 'failed'>('loading');

  useEffect(() => {
    const stage = stageRef.current;
    const canvas = canvasRef.current;
    if (!stage || !canvas) return;
    let disposed = false;
    let renderer: KnotRenderer | null = null;
    let frame: number | null = null;
    const initial = resolveLibraryFlowStartupAnchor(latest.current.props.model.entries, latest.current.props.initialAnchorWallpaperId, latest.current.props.model.currentPath)?.index ?? 0;
    let offset = initial;
    let target = initial;
    let velocity = 0;
    let selected = initial;
    let flyingEntry = initial;
    let fly = 0;
    let phase: 'return' | 'flow' | 'out' = 'out';
    let phaseAt = performance.now();
    let flowDuration = knotFlowDuration(0);
    let returnFly = 0;
    let lastInput = 0;
    let lastFrame = phaseAt;
    let yaw = 0;
    let pitch = 0;
    let yawVelocity = 0;
    let pitchVelocity = 0;
    let wheelSample: BookWheelSample | undefined;
    let renderState: KnotRenderState | null = null;
    let originalKey: string | null = null;
    let observationKey = '';
    let appendClaim = '';
    let unsubscriptions: (() => void)[] = [];
    let pictureKeys = new Set<string>();
    let reset = latest.current.props.model.replaceCount;
    let focusToken = latest.current.props.focusToken;
    let returnFocusToken = latest.current.props.returnFocusToken;
    let pausedAt: number | null = document.hidden || !latest.current.props.model.active ? performance.now() : null;
    let drag: { id: number; x: number; y: number; startX: number; startY: number; at: number; moved: boolean; slot: number | null } | null = null;
    const canRun = () => !disposed && !document.hidden && latest.current.props.model.active;
    store.setScrolling(false);
    store.setInteracting(canRun());
    const interactive = () => renderer !== null && canRun() && !latest.current.props.model.queryReplacementPending;
    const focus = () => stage.focus({ preventScroll: true });
    const cancelFrame = () => { if (frame !== null) cancelAnimationFrame(frame); frame = null; };
    const invalidate = () => { if (canRun() && frame === null) frame = requestAnimationFrame(tick); };

    const report = (settled: boolean) => {
      const { model, onAnchorChange } = latest.current.props;
      const entry = model.entries[selected];
      setSelection((previous) => previous.index === selected && previous.settled === settled ? previous : { index: selected, settled });
      if (entry) {
        onAnchorChange?.(entry.wallpaperId, settled);
        if (settled) model.onSelect(entry);
      }
    };
    const requestMore = () => {
      const { model } = latest.current.props;
      if (model.entries.length - 1 - target > APPEND_DISTANCE) { appendClaim = ''; return; }
      const claim = `${model.replaceCount}:${model.entries.length}`;
      if (model.canAutoAppend && !model.refreshing && !model.loadingMore && claim !== appendClaim) {
        appendClaim = claim;
        void model.onRequestMoreIfNeeded();
      }
    };
    const observePictures = () => {
      if (!renderer || !canRun()) return;
      const { model } = latest.current.props;
      const slots = knotSlots(offset, model.entries.length);
      const signature = `${model.replaceCount}:${model.entries.length}:${Math.floor(offset)}:${Math.sign(target - offset)}`;
      if (signature === observationKey) return;
      observationKey = signature;
      for (const unsubscribe of unsubscriptions) unsubscribe();
      unsubscriptions = [];
      const paths = knotPreviewOrder(offset, model.entries.length, target - offset).map((i) => staticPreviewAssetPath(model.entries[i]));
      for (const slot of slots) if (slot.entryIndex !== null) paths.push(staticPreviewAssetPath(model.entries[slot.entryIndex]));
      store.observeVisible([...new Set(paths)], { priority: 'front' });
      const nextKeys = new Set<string>();
      for (const slot of slots) {
        if (slot.entryIndex === null) continue;
        const entry = model.entries[slot.entryIndex];
        const path = staticPreviewAssetPath(entry);
        nextKeys.add(entry.path);
        const update = () => {
          const thumbnail = store.get(path);
          renderer?.setPicture(entry.path, thumbnail ? safeFileSrc(thumbnail) : null);
          invalidate();
        };
        unsubscriptions.push(store.subscribe(path, update));
        update();
      }
      for (const key of pictureKeys) if (!nextKeys.has(key)) renderer.setPicture(key, null);
      pictureKeys = nextKeys;
    };
    const draw = () => {
      const { model } = latest.current.props;
      renderState = {
        slots: knotSlots(offset, model.entries.length).map((slot) => ({ ...slot, key: slot.entryIndex === null ? null : model.entries[slot.entryIndex].path })),
        fractionalFlow: offset - Math.floor(offset), fly, flyingEntry,
        flyingKey: model.entries[flyingEntry]?.path ?? null,
        originalKey, yaw, pitch,
      };
      renderer?.render(renderState);
    };
    function tick(now: number) {
      frame = null;
      if (!canRun()) return;
      const dt = Math.min(0.032, Math.max(0, (now - lastFrame) / 1000));
      lastFrame = now;
      const reduced = latest.current.reducedMotion;
      if (reduced) {
        yawVelocity = pitchVelocity = 0;
        offset = target = knotSnapTarget(target, 0, latest.current.props.model.entries.length);
        velocity = 0;
        if (phase !== 'out' || selected !== target) {
          selected = flyingEntry = target;
          phase = 'out';
          report(true);
        }
        fly = 1;
      } else if (phase === 'return') {
        fly = returnFly * knotTimeline((now - phaseAt) / 1000, target - offset).fly;
        if (now - phaseAt >= KNOT_RETURN_SECONDS * 1000) { fly = 0; phase = 'flow'; phaseAt = now; }
      } else if (phase === 'flow') {
        const next = bookSpringStep(offset, velocity, target, dt * 0.65 / flowDuration);
        offset = clampOffset(next.position, latest.current.props.model.entries.length);
        velocity = next.velocity;
        const idle = now - lastInput >= KNOT_IDLE_SECONDS * 1000;
        if (idle && now - phaseAt >= flowDuration * 1000) {
          target = knotSnapTarget(target, 0, latest.current.props.model.entries.length);
          // Continuous wheels can stop between slots; give that last snap its own spring.
          if (Math.abs(offset - target) < 0.015 && Math.abs(velocity) < 0.15) {
            offset = target; velocity = 0;
            selected = flyingEntry = target;
            phase = 'out'; phaseAt = now;
            report(true);
          }
        }
      } else fly = knotFlyOut((now - phaseAt) / 1000);
      if (!drag && !reduced) {
        yaw += yawVelocity * dt;
        pitch = Math.min(KNOT_PITCH_LIMIT, Math.max(-KNOT_PITCH_LIMIT, pitch + pitchVelocity * dt));
        yawVelocity *= Math.exp(-ORBIT_FRICTION * dt);
        pitchVelocity *= Math.exp(-ORBIT_FRICTION * dt);
        if (Math.abs(yawVelocity) < 0.001) yawVelocity = 0;
        if (Math.abs(pitchVelocity) < 0.001 || Math.abs(pitch) >= KNOT_PITCH_LIMIT) pitchVelocity = 0;
      }
      observePictures();
      requestMore();
      draw();
      if (phase !== 'out' || fly < 1 || (!drag && (yawVelocity || pitchVelocity))) invalidate();
    }
    const moveTo = (position: number) => {
      if (!interactive()) return;
      if (phase === 'out' && clampOffset(position, latest.current.props.model.entries.length) === offset) return;
      target = clampOffset(position, latest.current.props.model.entries.length);
      lastInput = performance.now();
      flowDuration = knotFlowDuration(target - offset);
      if (phase === 'out') {
        phase = latest.current.reducedMotion ? 'flow' : fly > 0 ? 'return' : 'flow';
        returnFly = fly;
        phaseAt = lastInput;
      } else if (phase === 'flow') phaseAt = lastInput;
      selected = Math.round(target);
      report(false);
      store.setInteracting(true);
      invalidate();
    };
    const resetOrbit = () => { yaw = pitch = yawVelocity = pitchVelocity = 0; invalidate(); };
    const keydown = (event: KeyboardEvent) => {
      if (event.target !== stage || !interactive() || event.altKey || event.ctrlKey || event.metaKey) return;
      if (!INPUT_KEYS.has(event.key)) return;
      event.preventDefault();
      if (event.key === 'r' || event.key === 'R') resetOrbit();
      else if (event.key === 'Home') moveTo(0);
      else if (event.key === 'End') moveTo(latest.current.props.model.entries.length - 1);
      else moveTo(Math.round(target) + (event.key === 'ArrowRight' ? 1 : -1));
    };
    const wheel = (event: WheelEvent) => {
      if (!interactive() || event.ctrlKey) return;
      event.preventDefault();
      const next = accumulateKnotWheel(target, event, latest.current.props.model.entries.length, performance.now(), wheelSample);
      wheelSample = next.sample;
      if (next.offset !== target) moveTo(next.offset);
    };
    const hit = (x: number, y: number) => { const rect = canvas.getBoundingClientRect(); return renderer?.pick(x - rect.left, y - rect.top) ?? null; };
    const pointerdown = (event: PointerEvent) => {
      if (!interactive() || event.button !== 0 || drag) return;
      focus();
      stage.setPointerCapture(event.pointerId);
      yawVelocity = pitchVelocity = 0;
      drag = { id: event.pointerId, x: event.clientX, y: event.clientY, startX: event.clientX, startY: event.clientY, at: performance.now(), moved: false, slot: hit(event.clientX, event.clientY) };
    };
    const pointermove = (event: PointerEvent) => {
      if (!drag || drag.id !== event.pointerId) return;
      const now = performance.now();
      const dx = event.clientX - drag.x;
      const dy = event.clientY - drag.y;
      drag.moved ||= Math.hypot(event.clientX - drag.startX, event.clientY - drag.startY) > DRAG_THRESHOLD;
      if (drag.moved) {
        const dt = Math.max(0.008, (now - drag.at) / 1000);
        yaw += dx * KNOT_ORBIT_SENSITIVITY;
        pitch = Math.min(KNOT_PITCH_LIMIT, Math.max(-KNOT_PITCH_LIMIT, pitch + dy * KNOT_ORBIT_SENSITIVITY));
        yawVelocity = latest.current.reducedMotion ? 0 : dx * KNOT_ORBIT_SENSITIVITY / dt;
        pitchVelocity = latest.current.reducedMotion ? 0 : dy * KNOT_ORBIT_SENSITIVITY / dt;
        invalidate();
      }
      drag.x = event.clientX; drag.y = event.clientY; drag.at = now;
    };
    const finishPointer = (event: PointerEvent) => {
      if (!drag || event.pointerId !== drag.id) return;
      const completed = drag;
      drag = null;
      if (stage.hasPointerCapture(completed.id)) stage.releasePointerCapture(completed.id);
      if (event.type === 'pointerup' && !completed.moved && completed.slot !== null) {
        const entry = renderState?.slots.find((slot) => slot.slot === completed.slot)?.entryIndex;
        if (entry != null) moveTo(entry);
      }
      if (event.type !== 'pointerup' || performance.now() - completed.at > 80) yawVelocity = pitchVelocity = 0;
      invalidate();
    };
    const doubleclick = (event: MouseEvent) => { if (interactive() && hit(event.clientX, event.clientY) === null) resetOrbit(); };
    const adopt = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.target !== document.body || !interactive() || !INPUT_KEYS.has(event.key)) return;
      focus();
      stage.dispatchEvent(new KeyboardEvent('keydown', { key: event.key, altKey: event.altKey, ctrlKey: event.ctrlKey, metaKey: event.metaKey, bubbles: true, cancelable: true }));
      if (!event.altKey && !event.ctrlKey && !event.metaKey) event.preventDefault();
    };
    const syncPause = () => {
      if (!canRun()) {
        if (pausedAt === null) pausedAt = performance.now();
        cancelFrame();
        yawVelocity = pitchVelocity = 0;
        const captured = drag; drag = null;
        if (captured && stage.hasPointerCapture(captured.id)) stage.releasePointerCapture(captured.id);
        store.setInteracting(false);
      } else {
        store.setInteracting(true);
        if (pausedAt !== null) {
          const pause = performance.now() - pausedAt;
          phaseAt += pause; lastInput += pause;
          lastFrame = performance.now(); pausedAt = null;
        }
        invalidate();
      }
    };
    const resize = () => { const rect = stage.getBoundingClientRect(); renderer?.setSize(rect.width, rect.height, window.devicePixelRatio); invalidate(); };
    const theme = () => {
      const style = getComputedStyle(stage);
      // Resolve token-backed CSS colors to concrete RGB (including color-mix/OKLCH themes).
      renderer?.setTheme({ rope: style.borderTopColor, cardBack: style.backgroundColor });
      invalidate();
    };
    const resizeObserver = new ResizeObserver(resize);
    resizeObserver.observe(stage);
    const themeObserver = new MutationObserver(theme);
    themeObserver.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme', 'style', 'class', 'data-color-scheme'] });
    stage.addEventListener('keydown', keydown);
    stage.addEventListener('wheel', wheel, { passive: false });
    stage.addEventListener('pointerdown', pointerdown);
    stage.addEventListener('pointermove', pointermove);
    stage.addEventListener('pointerup', finishPointer);
    stage.addEventListener('pointercancel', finishPointer);
    stage.addEventListener('lostpointercapture', finishPointer);
    stage.addEventListener('dblclick', doubleclick);
    document.addEventListener('keydown', adopt);
    document.addEventListener('visibilitychange', syncPause);
    window.addEventListener('resize', resize);
    engineRef.current = {
      sync: () => {
        const next = latest.current.props;
        syncPause();
        if (next.model.queryReplacementPending) return;
        if (reset !== next.model.replaceCount) {
          reset = next.model.replaceCount;
          offset = target = selected = flyingEntry = 0;
          velocity = fly = 0; phase = 'out'; phaseAt = performance.now();
          lastFrame = phaseAt;
          if (pausedAt !== null) pausedAt = phaseAt;
          observationKey = ''; originalKey = null; appendClaim = '';
          report(true);
        }
        if (focusToken !== next.focusToken || returnFocusToken !== next.returnFocusToken) {
          focusToken = next.focusToken; returnFocusToken = next.returnFocusToken;
          if (next.model.active) focus();
        }
        observationKey = '';
        syncPause();
      },
      setOriginal: (key, url) => {
        if (originalKey && originalKey !== key) renderer?.setPicture(originalKey, null);
        originalKey = key;
        if (key) renderer?.setPicture(key, url);
        invalidate();
      },
    };
    void loadKnotRenderer().then((Renderer) => {
      if (disposed) return;
      try {
        renderer = new Renderer(canvas);
        renderer.onInvalidate = invalidate;
        resize(); theme();
        setStatus('ready');
        phaseAt = lastFrame = performance.now();
        if (pausedAt !== null) pausedAt = phaseAt;
        report(true);
        if (latest.current.props.model.active && (document.activeElement == null || document.activeElement === document.body || (latest.current.props.focusToken ?? 0) > 0)) focus();
        invalidate();
      } catch { renderer?.dispose(); renderer = null; setStatus('failed'); }
    }, () => { if (!disposed) setStatus('failed'); });
    return () => {
      disposed = true;
      cancelFrame();
      const captured = drag; drag = null;
      if (captured && stage.hasPointerCapture(captured.id)) stage.releasePointerCapture(captured.id);
      engineRef.current = null;
      for (const unsubscribe of unsubscriptions) unsubscribe();
      store.setInteracting(false);
      resizeObserver.disconnect(); themeObserver.disconnect();
      stage.removeEventListener('keydown', keydown);
      stage.removeEventListener('wheel', wheel);
      stage.removeEventListener('pointerdown', pointerdown);
      stage.removeEventListener('pointermove', pointermove);
      stage.removeEventListener('pointerup', finishPointer);
      stage.removeEventListener('pointercancel', finishPointer);
      stage.removeEventListener('lostpointercapture', finishPointer);
      stage.removeEventListener('dblclick', doubleclick);
      document.removeEventListener('keydown', adopt);
      document.removeEventListener('visibilitychange', syncPause);
      window.removeEventListener('resize', resize);
      renderer?.dispose();
    };
  }, [store]);

  useEffect(() => { engineRef.current?.sync(); }, [props.model.entries, props.model.active, props.model.replaceCount, props.model.queryReplacementPending, props.model.loadingMore, props.model.canAutoAppend, props.focusToken, props.returnFocusToken, reducedMotion]);
  return { stageRef, canvasRef, engineRef, selection, status, selectedEntry: props.model.entries[selection.index] ?? null };
}
