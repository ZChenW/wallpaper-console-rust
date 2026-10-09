/** Closed knot diagrams in XY, with crossing height along Z. No WebGL dependency. */
export type Vec3 = readonly [number, number, number];
export interface KnotCurve {
  readonly id: string;
  readonly label: string;
  /** Kept plural for future links; the gallery currently implements one component. */
  readonly components: readonly ((t: number) => Vec3)[];
}
const TAU = 2 * Math.PI;
export const TREFOIL: KnotCurve = {
  id: 'trefoil', label: 'Trefoil', components: [(t) => {
    const a = t * TAU;
    return [Math.sin(a) + 2 * Math.sin(2 * a), Math.cos(a) - 2 * Math.cos(2 * a), -Math.sin(3 * a)];
  }],
};
export const FIGURE_EIGHT: KnotCurve = {
  id: 'figure-eight', label: 'Figure-eight', components: [(t) => {
    const a = t * TAU;
    return [(2 + Math.cos(2 * a)) * Math.cos(3 * a), (2 + Math.cos(2 * a)) * Math.sin(3 * a), Math.sin(4 * a)];
  }],
};
const torus = (p: number, q: number) => (t: number): Vec3 => {
  const a = t * TAU;
  return [(2 + Math.cos(q * a)) * Math.cos(p * a), (2 + Math.cos(q * a)) * Math.sin(p * a), Math.sin(q * a)];
};
export const CINQUEFOIL: KnotCurve = { id: 'cinquefoil', label: 'Cinquefoil', components: [torus(2, 5)] };
export const TORUS_3_4: KnotCurve = { id: 'torus-3-4', label: 'Torus (3,4)', components: [torus(3, 4)] };
export const LISSAJOUS: KnotCurve = {
  id: 'lissajous', label: 'Lissajous', components: [(t) => {
    const a = t * TAU;
    return [Math.cos(2 * a + 0.3), Math.cos(3 * a + 1.2), Math.cos(5 * a + 0.7)];
  }],
};
export const KNOT_CURVES = [TREFOIL, FIGURE_EIGHT, CINQUEFOIL, TORUS_3_4, LISSAJOUS] as const;
export const modulo = (n: number, length = 1) => ((n % length) + length) % length;
export const lerp = (a: Vec3, b: Vec3, t: number): Vec3 => [
  a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t,
];
export const distance = (a: Vec3, b: Vec3) => Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]);
export function arcLength(points: readonly Vec3[]): number {
  return points.reduce((sum, point, i) => sum + distance(point, points[(i + 1) % points.length]), 0);
}
/** Samples exclude the duplicate endpoint. The closing edge participates in the arc table. */
export function resampleCurve(curve: (t: number) => Vec3, count: number, resolution = Math.max(8192, count * 8)): Vec3[] {
  if (!Number.isInteger(count) || count < 3) throw new RangeError('A closed curve needs at least three samples');
  const dense = Array.from({ length: resolution + 1 }, (_, i) => curve(i / resolution));
  const lengths = [0];
  for (let i = 1; i <= resolution; i++) lengths.push(lengths[i - 1] + distance(dense[i], dense[i - 1]));
  const result: Vec3[] = [];
  let cursor = 1;
  for (let i = 0; i < count; i++) {
    const target = lengths[resolution] * i / count;
    while (cursor < resolution && lengths[cursor] < target) cursor++;
    const span = lengths[cursor] - lengths[cursor - 1];
    result.push(lerp(dense[cursor - 1], dense[cursor], span ? (target - lengths[cursor - 1]) / span : 0));
  }
  return result;
}
/** Fractional turns, including unbounded/negative navigation. */
export function curvePoint(points: readonly Vec3[], t: number): Vec3 {
  const index = modulo(t) * points.length;
  const base = Math.floor(index);
  return lerp(points[base], points[(base + 1) % points.length], index - base);
}
export function curveNormalXY(points: readonly Vec3[], t: number): Vec3 {
  const before = curvePoint(points, t - 1 / points.length);
  const after = curvePoint(points, t + 1 / points.length);
  const dx = after[0] - before[0], dy = after[1] - before[1];
  const length = Math.hypot(dx, dy) || 1;
  return [-dy / length, dx / length, 0];
}
