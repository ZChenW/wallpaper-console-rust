import { classifyBookWheel, type BookWheelInput, type BookWheelSample } from '../wallpaperBookModel.ts';
import { arcLength, curveNormalXY, curvePoint, modulo, resampleCurve, type KnotCurve, type Vec3 } from './knotCurves.ts';

// Reference scene and motion values, kept together for tuning.
export const CAM_Z = 10;
export const CAMERA_FOV = 60;
export const FOG_NEAR = 10;
export const FOG_FAR = 40;
export const FOCUS_DIST = 5.5;
export const Z_GATE = 11;
export const MAX_SCALE = 9;
export const FOCUS_LIFT = 1.5;
export const FOCUS_LOOP_DISTANCE = FOCUS_DIST * 2;
export const SCALE_SECONDS = 0.4;
export const CAMERA_SECONDS = 0.25;
export const SETTLE_IDLE_MS = 140;
export const REDISTRIBUTE_SECONDS = 0.9;
export const REDISTRIBUTE_STAGGER_SECONDS = 0.12;
export const TEXTURE_RADIUS = 60;
export const MIN_DENSITY_COUNT = 60;
export const PATH_UNITS_PER_PICTURE = 0.8;
export const DEPTH_FACTOR = 2.5;
export const CURVE_SAMPLES = 8192;
export const DEFAULT_ASPECT = 16 / 10;
export const AUTO_PICTURES_PER_SECOND = 0.7;
export const DRAG_DEAD_ZONE = 4;
export const LINE_PIXELS = 40;
export const BASE_SIZE_MIN = 0.3;
export const BASE_SIZE_MAX = 0.62;
// Tighter than the reference's 1: with one picture per wallpaper the strand has to stay readable.
export const LATERAL_RANGE = 0.55;
export const DEPTH_RANGE = 0.6;

export const loopDistance = (a: number, b: number) => Math.abs(modulo(a - b + 0.5) - 0.5);
export const nearestLoopTarget = (current: number, t: number) => current + modulo(t - current + 0.5) - 0.5;
export const settleTarget = (target: number, count: number) => count > 0 ? Math.round(target * count) / count : 0;
export const selectedIndex = (t: number, count: number) => count > 0 ? modulo(Math.round(t * count), count) : 0;
export const pathLengthForCount = (count: number) => Math.max(count, MIN_DENSITY_COUNT) * PATH_UNITS_PER_PICTURE;
export const wheelSensitivity = (height: number, count: number) => 1 / (Math.max(1, height) * 4) * MIN_DENSITY_COUNT / Math.max(count, MIN_DENSITY_COUNT);
export const exponentialStep = (current: number, target: number, dt: number, seconds: number) => target + (current - target) * Math.exp(-Math.max(0, dt) / seconds);
export function accumulateKnotWheel(target: number, input: BookWheelInput, height: number, count: number, at: number, previous?: BookWheelSample) {
  const intent = classifyBookWheel(input, at, previous);
  // Classification preserves accelerated trackpad streams. Pixel deltas remain exact;
  // line/page events are converted to CSS pixels before using the reference sensitivity.
  const delta = input.deltaY;
  const pixels = delta * (input.deltaMode === 1 ? LINE_PIXELS : input.deltaMode === 2 ? Math.max(1, height) : 1);
  return { target: target + pixels * wheelSensitivity(height, count), sample: intent.sample };
}
export function seededOffsets(id: number | string) {
  let seed = 2166136261;
  for (const char of String(id)) seed = Math.imul(seed ^ char.charCodeAt(0), 16777619);
  const random = () => {
    seed += 0x6D2B79F5;
    let value = Math.imul(seed ^ seed >>> 15, seed | 1);
    value ^= value + Math.imul(value ^ value >>> 7, value | 61);
    return ((value ^ value >>> 14) >>> 0) / 4294967296;
  };
  return { lateral: (random() * 2 - 1) * LATERAL_RANGE, depth: (random() * 2 - 1) * DEPTH_RANGE,
    size: BASE_SIZE_MIN + random() * (BASE_SIZE_MAX - BASE_SIZE_MIN) };
}
export function scaledKnotPath(curve: KnotCurve, count: number): Vec3[] {
  const points = resampleCurve((t) => {
    const [x, y, z] = curve.components[0](t);
    return [x, y, z * DEPTH_FACTOR];
  }, CURVE_SAMPLES);
  const factor = pathLengthForCount(count) / arcLength(points);
  return points.map(([x, y, z]) => [x * factor, y * factor, z * factor]);
}
export function planePosition(points: readonly Vec3[], index: number, count: number, id: number | string): Vec3 {
  const t = index / Math.max(1, count);
  const p = curvePoint(points, t), normal = curveNormalXY(points, t), offset = seededOffsets(id);
  return [p[0] + normal[0] * offset.lateral, p[1] + normal[1] * offset.lateral, p[2] + offset.depth];
}
export function focusScale(distanceXY: number, depthDistance = CAM_Z, loopUnits = 0): number {
  if (distanceXY >= FOCUS_DIST || depthDistance >= Z_GATE || loopUnits >= FOCUS_LOOP_DISTANCE) return 1;
  return 1 + (1 - Math.max(0, distanceXY) / FOCUS_DIST) ** 3 * (MAX_SCALE - 1);
}
/** Nearest first, unique even when the library has fewer than 121 pictures. */
export function textureWindow(t: number, count: number, radius = TEXTURE_RADIUS): number[] {
  if (count <= 0) return [];
  const centre = selectedIndex(t, count), indices = new Set<number>();
  indices.add(centre);
  for (let d = 1; d <= radius && indices.size < count; d++) {
    indices.add(modulo(centre + d, count));
    indices.add(modulo(centre - d, count));
  }
  return [...indices];
}
export function redistributionProgress(elapsed: number, index: number, count: number, reduced = false) {
  if (reduced) return 1;
  const delay = REDISTRIBUTE_STAGGER_SECONDS * index / Math.max(1, count - 1);
  const t = Math.min(1, Math.max(0, (elapsed - delay) / REDISTRIBUTE_SECONDS));
  return t * t * (3 - 2 * t);
}

/** Points drawn for the hairline that traces the knot through the pictures. */
export const ROPE_POINTS = 1024;
export const ROPE_OPACITY = 0.3;
