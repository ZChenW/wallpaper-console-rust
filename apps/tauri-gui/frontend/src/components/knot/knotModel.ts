import { resolveCardPointerInteraction, type CardPointerInteractionInput } from '../../shell/cardInteraction.ts';
import { classifyBookWheel, type BookWheelInput, type BookWheelSample } from '../wallpaperBookModel.ts';
import { arcLength, curveNormalXY, curvePoint, modulo, resampleCurve, type KnotCurve, type Vec3 } from './knotCurves.ts';

// Reference scene and motion values, kept together for tuning.
export const CAM_Z = 9.5;
export const CAMERA_FOV = 60;
export const NARROW_CAMERA_FOV = 76;
export const NARROW_VIEW_ASPECT = 1.2;
export const FOG_NEAR = 9;
export const FOG_FAR = 34;
export const LIFT = 2.6;
export const ASSEMBLE_RADIUS = 0.62;
export const ASSEMBLE_SECONDS = 0.16;
export const ASSEMBLE_EPSILON = 0.0001;
export const CAMERA_SECONDS = 0.25;
export const SETTLE_IDLE_MS = 140;
export const REDISTRIBUTE_SECONDS = 0.9;
export const REDISTRIBUTE_STAGGER_SECONDS = 0.12;
export const TEXTURE_RADIUS = 60;
export const MIN_DENSITY_COUNT = 24;
export const PATH_UNITS_PER_PICTURE = 2.3;
export const DEPTH_FACTOR = 2.4;
export const CURVE_SAMPLES = 8192;
export const DEFAULT_ASPECT = 16 / 10;
export const AUTO_PICTURES_PER_SECOND = 0.7;
export const DRAG_DEAD_ZONE = 4;
export const WHEEL_REFERENCE_COUNT = 60;
export const LINE_PIXELS = 40;
export const TILE_COLUMNS = 4;
export const TILE_ROWS = 3;
export const TILE_COUNT = TILE_COLUMNS * TILE_ROWS;
export const TILE_SIZE_MIN = 0.2;
export const TILE_SIZE_MAX = 0.42;
export const ALONG_RANGE = 0.45;
export const LATERAL_RANGE = 1.05;
export const DEPTH_RANGE = 0.75;
export const SCATTER_GAP = 0.04;
export const PICTURE_VIEW_HEIGHT = 0.46;
export const PICTURE_VIEW_WIDTH = 0.40;

export const loopDistance = (a: number, b: number) => Math.abs(modulo(a - b + 0.5) - 0.5);
export const nearestLoopTarget = (current: number, t: number) => current + modulo(t - current + 0.5) - 0.5;
export const settleTarget = (target: number, count: number) => count > 0 ? Math.round(target * count) / count : 0;
export const selectedIndex = (t: number, count: number) => count > 0 ? modulo(Math.round(t * count), count) : 0;
export const pathLengthForCount = (count: number) => Math.max(count, MIN_DENSITY_COUNT) * PATH_UNITS_PER_PICTURE;
export const wheelSensitivity = (height: number, count: number) => 1 / (Math.max(1, height) * 4) * WHEEL_REFERENCE_COUNT / Math.max(count, WHEEL_REFERENCE_COUNT);
export const exponentialStep = (current: number, target: number, dt: number, seconds: number) => target + (current - target) * Math.exp(-Math.max(0, dt) / seconds);
export function accumulateKnotWheel(target: number, input: BookWheelInput, height: number, count: number, at: number, previous?: BookWheelSample) {
  const intent = classifyBookWheel(input, at, previous);
  // Classification preserves accelerated trackpad streams. Pixel deltas remain exact;
  // line/page events are converted to CSS pixels before using the reference sensitivity.
  const delta = input.deltaY;
  const pixels = delta * (input.deltaMode === 1 ? LINE_PIXELS : input.deltaMode === 2 ? Math.max(1, height) : 1);
  return { target: target + pixels * wheelSensitivity(height, count), sample: intent.sample };
}
export function seededOffsets(id: number | string, tile: number) {
  let seed = 2166136261;
  for (const char of `${id}:${tile}`) seed = Math.imul(seed ^ char.charCodeAt(0), 16777619);
  const random = () => {
    seed += 0x6D2B79F5;
    let value = Math.imul(seed ^ seed >>> 15, seed | 1);
    value ^= value + Math.imul(value ^ value >>> 7, value | 61);
    return ((value ^ value >>> 14) >>> 0) / 4294967296;
  };
  return { along: (random() * 2 - 1) * ALONG_RANGE, lateral: (random() * 2 - 1) * LATERAL_RANGE, depth: (random() * 2 - 1) * DEPTH_RANGE,
    size: TILE_SIZE_MIN + random() * (TILE_SIZE_MAX - TILE_SIZE_MIN) };
}
export function scaledKnotPath(curve: KnotCurve, count: number): Vec3[] {
  const points = resampleCurve((t) => {
    const [x, y, z] = curve.components[0](t);
    return [x, y, z * DEPTH_FACTOR];
  }, CURVE_SAMPLES);
  const factor = pathLengthForCount(count) / arcLength(points);
  return points.map(([x, y, z]) => [x * factor, y * factor, z * factor]);
}
export const smoothstep = (value: number) => {
  const t = Math.max(0, Math.min(1, value));
  return t * t * (3 - 2 * t);
};
export const pictureAspect = (aspect: number) => Math.max(0.6, Math.min(2.4, Number.isFinite(aspect) && aspect > 0 ? aspect : DEFAULT_ASPECT));
export const cameraFov = (viewAspect: number) => viewAspect < NARROW_VIEW_ASPECT ? NARROW_CAMERA_FOV : CAMERA_FOV;
export const pictureLoopDistance = (a: number, b: number, count: number) => loopDistance(a, b) * count;
export const assembleWant = (distancePictures: number) => smoothstep(1 - Math.max(0, distancePictures) / ASSEMBLE_RADIUS);
export function stepAssemble(amount: number, want: number, dt: number, reduced = false) {
  const next = reduced ? want : exponentialStep(amount, want, dt, ASSEMBLE_SECONDS);
  return Math.abs(next - want) < ASSEMBLE_EPSILON ? want : next;
}
/** Dirty geometry includes one-time texture aspect/viewport changes; idle scatter never writes. */
export function needsVertexUpdate(previous: number, next: number, redistributing: boolean, dirty = false) {
  return dirty || redistributing || previous !== next;
}
export interface TilePose { readonly centre: Vec3; readonly width: number; readonly height: number }
export interface PictureSize { readonly width: number; readonly height: number }
/** Fitting is evaluated at the lifted depth, not the distant scattered plane. */
export function assembledSize(aspect: number, viewAspect: number): PictureSize {
  const ratio = pictureAspect(aspect);
  const viewHeight = 2 * (CAM_Z - LIFT) * Math.tan(cameraFov(viewAspect) * Math.PI / 360);
  const height = Math.min(viewHeight * PICTURE_VIEW_HEIGHT, viewHeight * Math.max(0.001, viewAspect) * PICTURE_VIEW_WIDTH / ratio);
  return { width: height * ratio, height };
}
/** Row zero is the top of the image; three's Texture has flipY enabled. */
export function tileUVCell(tile: number) {
  const col = tile % TILE_COLUMNS, row = Math.floor(tile / TILE_COLUMNS);
  return { u0: col / TILE_COLUMNS, u1: (col + 1) / TILE_COLUMNS,
    v0: 1 - (row + 1) / TILE_ROWS, v1: 1 - row / TILE_ROWS };
}
export function scatteredTileCentre(points: readonly Vec3[], index: number, count: number, id: number | string, tile: number): Vec3 {
  const offset = seededOffsets(id, tile);
  const t = (index + offset.along) / Math.max(1, count);
  const p = curvePoint(points, t), normal = curveNormalXY(points, t);
  return [p[0] + normal[0] * offset.lateral, p[1] + normal[1] * offset.lateral, p[2] + offset.depth];
}
export function scatteredTileSize(aspect: number, id: number | string, tile: number): PictureSize {
  const ratio = pictureAspect(aspect) * TILE_ROWS / TILE_COLUMNS;
  const size = seededOffsets(id, tile).size;
  return { width: ratio >= 1 ? size : size * ratio, height: ratio >= 1 ? size / ratio : size };
}
export function assembledCell(centre: Vec3, size: PictureSize, tile: number): TilePose {
  const width = size.width / TILE_COLUMNS, height = size.height / TILE_ROWS;
  return { centre: [centre[0] + (tile % TILE_COLUMNS + 0.5 - TILE_COLUMNS / 2) * width,
    centre[1] + (TILE_ROWS / 2 - Math.floor(tile / TILE_COLUMNS) - 0.5) * height, centre[2] + LIFT], width, height };
}
/** Explicit shared edges avoid float rounding gaps between adjacent quads. */
export function assembledCellBounds(centre: Vec3, size: PictureSize, tile: number) {
  const col = tile % TILE_COLUMNS, row = Math.floor(tile / TILE_COLUMNS);
  return { left: centre[0] + (col / TILE_COLUMNS - 0.5) * size.width,
    right: centre[0] + ((col + 1) / TILE_COLUMNS - 0.5) * size.width,
    top: centre[1] + (0.5 - row / TILE_ROWS) * size.height,
    bottom: centre[1] + (0.5 - (row + 1) / TILE_ROWS) * size.height, z: centre[2] + LIFT };
}
/** A fragment only navigates; an assembled picture follows Grid's exact gesture. */
export function knotPointerInteraction(input: CardPointerInteractionInput, assembled: boolean) {
  if (input.fromControl) return { travel: false, select: false, apply: false };
  if (!assembled) return { travel: true, select: false, apply: false };
  return { travel: false, ...resolveCardPointerInteraction(input) };
}
export const tileGap = (amount: number) => SCATTER_GAP * (1 - smoothstep(amount));
export function tilePose(scattered: Vec3, scatteredSize: PictureSize, assembled: TilePose, amount: number): TilePose {
  if (amount >= 1) return assembled;
  const t = smoothstep(amount), gap = tileGap(amount);
  return { centre: [scattered[0] + (assembled.centre[0] - scattered[0]) * t,
    scattered[1] + (assembled.centre[1] - scattered[1]) * t,
    scattered[2] + (assembled.centre[2] - scattered[2]) * t],
  width: (scatteredSize.width / (1 - SCATTER_GAP) * (1 - t) + assembled.width * t) * (1 - gap),
  height: (scatteredSize.height / (1 - SCATTER_GAP) * (1 - t) + assembled.height * t) * (1 - gap) };
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
