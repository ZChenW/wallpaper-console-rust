import { isContextMenuKey } from '../shell/keyboardInteraction.ts';
import { resolveCardPointerInteraction, type CardPointerInteractionInput } from '../shell/cardInteraction.ts';

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
export const BOOK_ZOOM_DURATION_MS = 280;
export const BOOK_APPEND_DISTANCE = 6;

export type BookFace = 'front' | 'back';
export const bookLeafCount = (wallpaperCount: number) => Math.ceil(Math.max(0, wallpaperCount) / 2);
/** An odd final leaf has a blank back; keep its front open, including a one-page book. */
export const bookLastPosition = (wallpaperCount: number) => Math.floor(Math.max(0, wallpaperCount) / 2);
export const clampBookPosition = (position: number, leafCount: number) =>
  Math.min(leafCount, Math.max(0, Number.isFinite(position) ? position : 0));

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

export function bookLeafTransform(leaf: number, position: number, reducedMotion = false) {
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
  const x = (turned ? -1 : 1) * BOOK_FAN_OUTWARD * depth;
  const z = -BOOK_FAN_DEPTH * depth + BOOK_TURN_LIFT * lift;
  return {
    progress,
    depth,
    angle,
    opacity: visible && (!reducedMotion || depth === 0) ? 1 : 0,
    shade: lift * 0.28,
    highlight: lift * 0.2,
    transform: reducedMotion
      ? `translateX(${turned ? '-100%' : '0'})`
      : `translate3d(${x}px, ${-lift * 8}px, ${z}px) rotateY(${angle}deg) scale(${1 - depth * BOOK_FAN_SCALE_STEP})`,
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

/** Line/page-mode mouse notches are discrete; pixel trackpads remain continuous. */
export function accumulateBookWheel(position: number, input: BookWheelInput, leafCount: number) {
  const delta = Math.abs(input.deltaX) > Math.abs(input.deltaY) ? input.deltaX : input.deltaY;
  const discrete = input.deltaMode !== 0 || (Number.isInteger(delta) && Math.abs(delta) >= 80);
  const travel = discrete ? Math.sign(delta) : delta / BOOK_WHEEL_PIXELS_PER_LEAF;
  return clampBookPosition(position + travel, leafCount);
}

export type BookKeyIntent = 'next' | 'previous' | 'first' | 'last' | 'select-left'
  | 'select-right' | 'apply' | 'zoom' | 'unzoom' | 'context';

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

export function bookZoomTransform(origin: BookRect, destination: BookRect): string {
  return `translate(${origin.left - destination.left}px, ${origin.top - destination.top}px) scale(${origin.width / Math.max(1, destination.width)}, ${origin.height / Math.max(1, destination.height)})`;
}
