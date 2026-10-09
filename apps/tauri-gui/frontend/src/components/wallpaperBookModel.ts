import { isContextMenuKey } from '../shell/keyboardInteraction.ts';
import { resolveCardPointerInteraction, type CardPointerInteractionInput } from '../shell/cardInteraction.ts';
import type { WallpaperDTO } from '../api/types.ts';
import { staticFallbackAssetPath, staticPreviewAssetPath } from './wallpaperPreviewMedia.ts';

/** Book prefers a full-colour video frame; a failed extraction restores the project preview. */
export function bookStaticSource(entry: WallpaperDTO, videoFrameFailed = false) {
  return entry.type === 'video' && !videoFrameFailed
    ? { thumbnailPath: entry.path, fallbackPath: null }
    : { thumbnailPath: staticPreviewAssetPath(entry), fallbackPath: staticFallbackAssetPath(entry, true) };
}

export type BookTravelDirection = -1 | 0 | 1;

/** Turning needs arrived pictures immediately; only a live pointer drag pauses reveals. */
export function bookRevealPaused(dragging: boolean): boolean { return dragging; }

export const BOOK_FAN_ANGLE = 14;
// Deeper pile leaves flatten away from the viewer, behind the open page.
export const BOOK_FAN_ANGLE_STEP = -3;
export const BOOK_FAN_DEPTH = 24;
export const BOOK_FAN_OUTWARD = 40;
export const BOOK_FAN_SCALE_STEP = 0;
export const BOOK_VISIBLE_PILE_LEAVES = 6;
export const BOOK_WINDOW_RADIUS = 10;
export const BOOK_TURN_LIFT = 24;
export const BOOK_WHEEL_PIXELS_PER_LEAF = 160;
export const BOOK_WHEEL_IDLE_MS = 160;
export const BOOK_ZOOM_DURATION_MS = 320;
export const BOOK_APPEND_DISTANCE = 6;

/** The fan was tuned on a 480px page (the former 60rem spread). */
export function bookPageScale(pageWidth: number): number {
  return Number.isFinite(pageWidth) ? Math.min(2.2, Math.max(0.6, pageWidth / 480)) : 1;
}

export function resolveBookEscape(contextMenuOpen: boolean, zoomed: boolean, immersive: boolean) {
  return contextMenuOpen ? 'context' : zoomed ? 'zoom' : immersive ? 'immersive' : null;
}

export function shouldEndBookImmersive(mode: string, active: boolean, queryReset: boolean): boolean {
  return mode !== 'book' || !active || queryReset;
}

export type BookFace = 'front' | 'back';
export const bookLeafCount = (wallpaperCount: number) => Math.ceil(Math.max(0, wallpaperCount) / 2);
/** An odd final leaf has a blank back; keep its front open, including a one-page book. */
export const bookLastPosition = (wallpaperCount: number) => Math.floor(Math.max(0, wallpaperCount) / 2);
export const clampBookPosition = (position: number, leafCount: number) =>
  Math.min(leafCount, Math.max(0, Number.isFinite(position) ? position : 0));

export interface BookPositionSegment {
  readonly from: number;
  readonly to: number;
  /** Zero means use the normal interruptible spring. */
  readonly durationMs: number;
}

/** Skip the middle at matching half-turn poses; never render the skipped leaves. */
export function planBookMove(from: number, to: number, leafCount: number): readonly BookPositionSegment[] {
  const start = clampBookPosition(from, leafCount);
  const destination = Math.round(clampBookPosition(to, leafCount));
  if (Math.abs(destination - start) <= BOOK_WINDOW_RADIUS) {
    return [{ from: start, to: destination, durationMs: 0 }];
  }
  const direction = Math.sign(destination - start);
  const departure = direction > 0 ? Math.floor(start) + 2.5 : Math.ceil(start) - 2.5;
  const arrival = destination - direction * 2.5;
  return [
    { from: start, to: departure, durationMs: 260 },
    { from: arrival, to: destination, durationMs: 260 },
  ];
}

export function resolveBookContextMenu(open: boolean, zoomed: boolean): 'open' | 'turn' {
  return zoomed || open ? 'open' : 'turn';
}

export function wallpaperBookAddress(index: number): { leaf: number; face: BookFace } {
  return { leaf: Math.floor(index / 2), face: index % 2 === 0 ? 'front' : 'back' };
}

export function bookWallpaperIndex(leaf: number, face: BookFace, count: number): number | null {
  const index = leaf * 2 + (face === 'back' ? 1 : 0);
  return index >= 0 && index < count ? index : null;
}

/** Show an even page on the right, an odd page on the left. */
export function bookPositionForWallpaper(index: number): number {
  const { leaf, face } = wallpaperBookAddress(index);
  return leaf + (face === 'back' ? 1 : 0);
}

export function openBookWallpapers(position: number, count: number): readonly number[] {
  const spread = Math.round(clampBookPosition(position, bookLastPosition(count)));
  return [spread * 2 - 1, spread * 2].filter((index) => index >= 0 && index < count);
}

export function resolveBookSelectedIndex(position: number, count: number, preferred: number, selected: number): number {
  const open = openBookWallpapers(position, count);
  return open.includes(preferred) ? preferred : open.includes(selected) ? selected : open.at(-1) ?? -1;
}

export function bookAppendApproach(previous: string, key: string, nearEnd: boolean, allowed: boolean) {
  if (!nearEnd) return { claim: '', request: false };
  if (!allowed || previous === key) return { claim: previous, request: false };
  return { claim: key, request: true };
}

export function resolveBookWheelIntent(
  input: { readonly ctrlKey: boolean; readonly deltaY: number },
  dragging: boolean,
  zoomed: boolean,
): 'zoom-in' | 'zoom-out' | 'turn' | 'none' {
  if (input.ctrlKey) {
    if (input.deltaY < 0 && !zoomed) return 'zoom-in';
    if (input.deltaY > 0 && zoomed) return 'zoom-out';
    return 'none';
  }
  return dragging || zoomed ? 'none' : 'turn';
}

export function bookVisibleWindow(position: number, leafCount: number): readonly number[] {
  const center = clampBookPosition(position, leafCount);
  const start = Math.max(0, Math.floor(center) - BOOK_WINDOW_RADIUS);
  const end = Math.min(leafCount - 1, Math.ceil(center) + BOOK_WINDOW_RADIUS);
  const leaves: number[] = [];
  for (let leaf = start; leaf <= end; leaf += 1) {
    const depth = leaf < center ? Math.max(0, center - leaf - 1) : leaf - center;
    if (depth < BOOK_VISIBLE_PILE_LEAVES) leaves.push(leaf);
  }
  return leaves;
}

export function bookLeafTransform(leaf: number, position: number, reducedMotion = false, scale = 1) {
  const progress = Math.min(1, Math.max(0, position - leaf));
  const turned = progress === 1;
  const depth = progress > 0 && progress < 1 ? 0
    : turned ? Math.max(0, position - leaf - 1) : Math.max(0, leaf - position);
  const visible = depth < BOOK_VISIBLE_PILE_LEAVES;
  const eased = progress * progress * (3 - 2 * progress);
  const fanAngle = BOOK_FAN_ANGLE + BOOK_FAN_ANGLE_STEP * depth;
  const angle = progress > 0 && progress < 1
    ? -BOOK_FAN_ANGLE - (180 - BOOK_FAN_ANGLE * 2) * eased
    : turned ? -180 + fanAngle : -fanAngle;
  const lift = Math.sin(Math.PI * progress);
  const x = (turned ? -1 : 1) * BOOK_FAN_OUTWARD * depth * scale;
  const z = (-BOOK_FAN_DEPTH * depth + BOOK_TURN_LIFT * lift) * scale;
  return {
    progress,
    depth,
    angle,
    opacity: visible && (!reducedMotion || depth === 0) ? 1 : 0,
    shade: lift * 0.28,
    highlight: lift * 0.2,
    transform: reducedMotion
      ? `translateX(${turned ? '-100%' : '0'})`
      : `translate3d(${x}px, ${-lift * 8 * scale}px, ${z}px) rotateY(${angle}deg) scale(${1 - depth * BOOK_FAN_SCALE_STEP})`,
  };
}

/** Momentum in leaves/second; limit a fling to two leaves past the hand. */
export function bookSnapTarget(position: number, velocity: number, leafCount: number): number {
  const projection = Math.min(2, Math.max(-2, velocity * 0.115));
  return Math.round(clampBookPosition(position + projection, leafCount));
}

export interface BookWheelInput {
  readonly deltaX: number;
  readonly deltaY: number;
  readonly deltaMode: number;
}

export interface BookWheelSample { readonly magnitude: number; readonly at: number; readonly continuousUntil: number }

/** Canonical notches work on the first event; a recent irregular/small stream
 * stays continuous even when a trackpad's acceleration reaches mouse sizes. */
export function classifyBookWheel(input: BookWheelInput, at: number, previous?: BookWheelSample) {
  const delta = Math.abs(input.deltaX) > Math.abs(input.deltaY) ? input.deltaX : input.deltaY;
  const magnitude = Math.abs(delta);
  const recent = previous !== undefined && at - previous.at < 180;
  const regular = recent && Math.abs(magnitude - previous.magnitude) <= Math.max(1, magnitude * 0.025);
  const canonical = [53, 60, 100, 120].some((notch) => Math.abs(magnitude - notch) <= 2);
  const continuous = recent && previous.continuousUntil > at;
  const diagonal = Math.min(Math.abs(input.deltaX), Math.abs(input.deltaY)) > Math.max(2, magnitude * 0.15);
  const discrete = magnitude > 0 && (input.deltaMode !== 0
    || (!continuous && !diagonal && magnitude >= 50 && (canonical || regular)));
  return {
    delta, discrete,
    sample: { magnitude, at, continuousUntil: discrete || magnitude === 0 || (!recent && magnitude >= 50 && !diagonal) ? 0 : at + 180 },
  };
}

export function accumulateBookWheel(position: number, input: BookWheelInput, leafCount: number, discrete = classifyBookWheel(input, 0).discrete) {
  const delta = Math.abs(input.deltaX) > Math.abs(input.deltaY) ? input.deltaX : input.deltaY;
  return clampBookPosition(position + (discrete ? Math.sign(delta) : delta / BOOK_WHEEL_PIXELS_PER_LEAF), leafCount);
}

/** Bound queued notches to three leaves beyond the hand, including reversal. */
export function bookWheelTarget(position: number, target: number | null, delta: number, last: number) {
  const next = (target ?? Math.round(position)) + Math.sign(delta);
  return Math.round(clampBookPosition(Math.min(Math.floor(position) + 3, Math.max(Math.ceil(position) - 3, next)), last));
}

export function bookSpringStep(position: number, velocity: number, target: number, seconds: number) {
  const stiffness = 190 * (1 + Math.min(3, Math.abs(target - position)) * 0.5);
  const damping = 2 * Math.sqrt(stiffness);
  const dt = Math.min(0.032, Math.max(0, seconds));
  const speed = velocity + ((target - position) * stiffness - velocity * damping) * dt;
  return { position: position + speed * dt, velocity: speed };
}

export interface BookDragSample { readonly position: number; readonly at: number }
/** The last 80ms, including a stationary release, determines momentum. */
export function bookDragVelocity(samples: readonly BookDragSample[], at: number): number {
  const recent = samples.filter((sample) => at - sample.at <= 80);
  const first = recent[0];
  const last = recent.at(-1);
  return first && last && at > first.at ? (last.position - first.position) / ((at - first.at) / 1000) : 0;
}

export type BookKeyIntent = 'next' | 'previous' | 'first' | 'last' | 'select-left'
  | 'select-right' | 'apply' | 'zoom' | 'unzoom' | 'context' | 'immersive';

export function resolveBookKey(key: string, shiftKey = false): BookKeyIntent | null {
  if (isContextMenuKey(key, shiftKey)) return 'context';
  switch (key) {
    case 'ArrowRight': case 'PageDown': return 'next';
    case 'ArrowLeft': case 'PageUp': return 'previous';
    case 'Home': return 'first';
    case 'End': return 'last';
    case 'ArrowUp': return 'select-left';
    case 'ArrowDown': return 'select-right';
    case 'Enter': return 'apply';
    case ' ': case 'z': case 'Z': return 'zoom';
    case 'Escape': return 'unzoom';
    case 'f': case 'F': return 'immersive';
    default: return null;
  }
}

export function resolveBookPointerInteraction(input: CardPointerInteractionInput & {
  readonly dragged: boolean;
  readonly open: boolean;
  readonly settled: boolean;
}): { readonly select: boolean; readonly apply: boolean; readonly turn: boolean } {
  if (input.dragged || input.fromControl) return { select: false, apply: false, turn: false };
  if (!input.open) return { select: true, apply: false, turn: true };
  if (!input.settled) return { select: false, apply: false, turn: false };
  return { ...resolveCardPointerInteraction(input), turn: false };
}

export interface BookRect { readonly left: number; readonly top: number; readonly width: number; readonly height: number }

export const BOOK_PREVIEW_AHEAD_LEAVES = 24;
export const BOOK_MOVING_AHEAD_LEAVES = 40;
export const BOOK_MOVING_BEHIND_LEAVES = 8;

/**
 * Wallpaper indices whose previews are worth having, nearest the open spread first: the pages on
 * screen, then pages the reader is about to turn to, so they are ready before they arrive.
 */
export function bookPreviewOrder(position: number, count: number, direction: BookTravelDirection = 0): number[] {
  const spread = Math.round(clampBookPosition(position, bookLeafCount(count)));
  const centre = spread * 2 - 0.5;
  const left = direction < 0 ? BOOK_MOVING_AHEAD_LEAVES : direction > 0 ? BOOK_MOVING_BEHIND_LEAVES : BOOK_PREVIEW_AHEAD_LEAVES;
  const right = direction > 0 ? BOOK_MOVING_AHEAD_LEAVES : direction < 0 ? BOOK_MOVING_BEHIND_LEAVES : BOOK_PREVIEW_AHEAD_LEAVES;
  const first = Math.max(0, (spread - left) * 2);
  const last = Math.min(count - 1, (spread + right) * 2 - 1);
  const indices: number[] = [];
  for (let index = first; index <= last; index += 1) indices.push(index);
  return indices.sort((a, b) => Math.abs(a - centre) - Math.abs(b - centre) || (direction > 0 ? b - a : a - b));
}
