/** Prototype geometry, independent of WebGL. Closed samples omit the duplicated endpoint. */
export type Vec3 = readonly [number, number, number];
export interface KnotCurve {
  readonly id: string;
  readonly label: string;
  readonly components: readonly ((t: number) => Vec3)[];
}
export const TREFOIL: KnotCurve = {
  id: 'trefoil', label: 'Trefoil',
  components: [(t) => {
    const a = t * Math.PI * 2;
    return [Math.sin(a) + 2 * Math.sin(2 * a), Math.cos(a) - 2 * Math.cos(2 * a), -Math.sin(3 * a)];
  }],
};
export const add = (a: Vec3, b: Vec3): Vec3 => [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
export const scale = (a: Vec3, n: number): Vec3 => [a[0] * n, a[1] * n, a[2] * n];
export const dot = (a: Vec3, b: Vec3) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
export const cross = (a: Vec3, b: Vec3): Vec3 => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
export const unit = (a: Vec3): Vec3 => scale(a, 1 / (Math.hypot(...a) || 1));
export const lerp = (a: Vec3, b: Vec3, t: number): Vec3 => add(scale(a, 1 - t), scale(b, t));
export const modulo = (n: number, length: number) => ((n % length) + length) % length;

export function resampleCurve(curve: (t: number) => Vec3, count: number, resolution = Math.max(4096, count * 32)): Vec3[] {
  if (count < 3) throw new RangeError('A closed curve needs at least three samples');
  const dense = Array.from({ length: resolution + 1 }, (_, i) => curve(i / resolution));
  const lengths = [0];
  for (let i = 1; i <= resolution; i++) lengths.push(lengths[i - 1] + Math.hypot(...add(dense[i], scale(dense[i - 1], -1))));
  const result: Vec3[] = [];
  let cursor = 1;
  for (let i = 0; i < count; i++) {
    const distance = lengths[resolution] * i / count;
    while (cursor < resolution && lengths[cursor] < distance) cursor++;
    const span = lengths[cursor] - lengths[cursor - 1];
    result.push(lerp(dense[cursor - 1], dense[cursor], span ? (distance - lengths[cursor - 1]) / span : 0));
  }
  return result;
}
export function curvePoint(points: readonly Vec3[], position: number): Vec3 {
  const index = modulo(position, points.length);
  return lerp(points[Math.floor(index)], points[(Math.floor(index) + 1) % points.length], index % 1);
}
export function curveTangent(points: readonly Vec3[], position: number): Vec3 {
  return unit(add(curvePoint(points, position + 0.5), scale(curvePoint(points, position - 0.5), -1)));
}
export function bandFrame(tangent: Vec3, viewDirection: Vec3) {
  const along = unit(tangent);
  let normal = add(viewDirection, scale(along, -dot(viewDirection, along)));
  if (Math.hypot(...normal) < 1e-7) {
    const fallback: Vec3 = Math.abs(along[0]) < 0.9 ? [1, 0, 0] : [0, 1, 0];
    normal = add(fallback, scale(along, -dot(fallback, along)));
  }
  normal = unit(normal);
  return { tangent: along, normal, width: unit(cross(normal, along)) };
}
export function slotCentres(points: readonly Vec3[], count: number, phase = 0): Vec3[] {
  return Array.from({ length: count }, (_, i) => curvePoint(points, (i + phase) * points.length / count));
}
export function frontMostIndex(points: readonly Vec3[], viewDirection: Vec3): number {
  let best = 0;
  for (let i = 1; i < points.length; i++) if (dot(points[i], viewDirection) > dot(points[best], viewDirection)) best = i;
  return best;
}
