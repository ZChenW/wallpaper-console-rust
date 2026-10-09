import { accumulateBookWheel, classifyBookWheel, type BookWheelInput, type BookWheelSample } from '../wallpaperBookModel.ts';

export const KNOT_SLOT_COUNT = 44;
export const KNOT_CARD_LENGTH = 0.9;
export const KNOT_CARD_ASPECT = 16 / 10;
export const KNOT_CAMERA_FOV = 36;
export const KNOT_FLOAT_WIDTH = 0.36;
export const KNOT_RETURN_SECONDS = 0.36;
export const KNOT_OUT_SECONDS = 0.46;
export const KNOT_IDLE_SECONDS = 0.18;
export const KNOT_ORBIT_SENSITIVITY = 0.35 * Math.PI / 180;
export const KNOT_PITCH_LIMIT = 70 * Math.PI / 180;
export const clampOffset = (offset: number, length: number) => Math.min(Math.max(0, length - 1), Math.max(0, offset));

export interface KnotSlot { readonly slot: number; readonly entryIndex: number | null; readonly relative: number }
/** A bounded window around the selected slot; blanks form one run around the remote seam.
 * Fractional travel is applied to geometry, not to entry identity. No library wraparound. */
export function knotSlots(offset: number, length: number, count = KNOT_SLOT_COUNT): KnotSlot[] {
  const base = Math.floor(clampOffset(offset, length));
  return Array.from({ length: count }, (_, slot) => {
    const relative = slot < Math.ceil(count / 2) ? slot : slot - count;
    const index = base + relative;
    return { slot, relative, entryIndex: index >= 0 && index < length ? index : null };
  });
}
export function knotSnapTarget(position: number, velocity: number, length: number): number {
  return Math.round(clampOffset(position + Math.min(2, Math.max(-2, velocity * 0.115)), length));
}
export function accumulateKnotWheel(offset: number, input: BookWheelInput, length: number, at: number, previous?: BookWheelSample) {
  const intent = classifyBookWheel(input, at, previous);
  return { offset: accumulateBookWheel(offset, input, Math.max(0, length - 1), intent.discrete), sample: intent.sample };
}
export const knotFlowDuration = (slots: number) => Math.min(1.35, 0.5 + Math.abs(slots) * 0.045);
const smooth = (n: number) => { const t = Math.min(1, Math.max(0, n)); return t * t * (3 - 2 * t); };
/** Time since a navigation starts: return the outgoing image, flow, then reveal the new one.
 * Input bursts hold the outgoing phase at zero until the controller declares a settle. */
export function knotTimeline(seconds: number, distance: number, reducedMotion = false) {
  if (reducedMotion) return { phase: 'out' as const, fly: 1, flow: 1 };
  const flowEnd = KNOT_RETURN_SECONDS + knotFlowDuration(distance);
  if (seconds < KNOT_RETURN_SECONDS) return { phase: 'return' as const, fly: 1 - smooth(seconds / KNOT_RETURN_SECONDS), flow: 0 };
  if (seconds < flowEnd) return { phase: 'flow' as const, fly: 0, flow: smooth((seconds - KNOT_RETURN_SECONDS) / knotFlowDuration(distance)) };
  return { phase: 'out' as const, fly: smooth((seconds - flowEnd) / KNOT_OUT_SECONDS), flow: 1 };
}
export function knotFlyOut(secondsSinceSettle: number, reducedMotion = false): number {
  return reducedMotion ? 1 : smooth(secondsSinceSettle / KNOT_OUT_SECONDS);
}
export function knotPreviewOrder(offset: number, length: number, direction: number, count = KNOT_SLOT_COUNT): number[] {
  const centre = Math.round(clampOffset(offset, length));
  const result: number[] = [];
  const forward = direction < 0 ? -1 : 1;
  for (let distance = 0; distance <= Math.ceil(count / 2); distance++) {
    for (const index of distance === 0 ? [centre] : [centre + distance * forward, centre - distance * forward]) {
      if (index >= 0 && index < length && result.length < count) result.push(index);
    }
  }
  return result;
}
