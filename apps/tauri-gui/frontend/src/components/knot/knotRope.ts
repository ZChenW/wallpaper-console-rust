/**
 * The path behaves like an elastic rope: one node per wallpaper, each held to its place on the knot
 * by a spring and tied to its two neighbours, so pulling one drags the stretch around it along and
 * letting go sends a damped wave both ways round the loop. The simulation only ever stores how far
 * each node is from its place; the knot's own geometry is untouched.
 */
export const ROPE_HOME = 55; // 1/s²: pull of a node towards its place on the knot
export const ROPE_COUPLING = 700; // 1/s²: pull of a node towards its neighbours
export const ROPE_DAMPING = 4.5; // 1/s
export const ROPE_MAX_PULL = 7; // scene units a grabbed node can be dragged from its place
export const ROPE_STEP = 1 / 240; // fixed step, far inside the explicit scheme's stability limit
export const ROPE_MAX_FRAME = 0.05; // a long frame (a hitch, a wake from idle) is not simulated in full
export const ROPE_REST_DISTANCE = 0.002;
export const ROPE_REST_SPEED = 0.01;

export interface Rope {
  readonly count: number;
  /** x, y, z per node: displacement from the node's place on the knot. */
  readonly offsets: Float32Array;
  readonly velocities: Float32Array;
  grabbed: number;
  readonly hold: [number, number, number];
  carry: number;
  active: boolean;
}

export function createRope(count: number): Rope {
  const size = Math.max(0, count) * 3;
  return { count: Math.max(0, count), offsets: new Float32Array(size), velocities: new Float32Array(size),
    grabbed: -1, hold: [0, 0, 0], carry: 0, active: false };
}

/** Take hold of a node where it currently is, so grabbing a swinging rope does not make it jump. */
export function grabRope(rope: Rope, index: number): void {
  if (index < 0 || index >= rope.count) return;
  rope.grabbed = index; rope.active = true;
  rope.hold[0] = rope.offsets[index * 3]; rope.hold[1] = rope.offsets[index * 3 + 1]; rope.hold[2] = rope.offsets[index * 3 + 2];
}

/** Where the held node should be, relative to its place on the knot; clamped to the rope's give. */
export function pullRope(rope: Rope, x: number, y: number, z = 0): void {
  if (rope.grabbed < 0) return;
  const length = Math.hypot(x, y, z), scale = length > ROPE_MAX_PULL ? ROPE_MAX_PULL / length : 1;
  rope.hold[0] = x * scale; rope.hold[1] = y * scale; rope.hold[2] = z * scale;
  rope.active = true;
}

export function releaseRope(rope: Rope): void { rope.grabbed = -1; }

function pin(rope: Rope): void {
  if (rope.grabbed < 0) return;
  const at = rope.grabbed * 3;
  for (let axis = 0; axis < 3; axis++) { rope.offsets[at + axis] = rope.hold[axis]; rope.velocities[at + axis] = 0; }
}

function substep(rope: Rope): void {
  const { count, offsets, velocities } = rope;
  // Velocities first, from the positions of the step before (semi-implicit Euler).
  for (let node = 0; node < count; node++) {
    const at = node * 3, before = (node === 0 ? count - 1 : node - 1) * 3, after = (node === count - 1 ? 0 : node + 1) * 3;
    for (let axis = 0; axis < 3; axis++) {
      const here = offsets[at + axis];
      const neighbours = count > 1 ? offsets[before + axis] + offsets[after + axis] - 2 * here : 0;
      velocities[at + axis] += (ROPE_COUPLING * neighbours - ROPE_HOME * here - ROPE_DAMPING * velocities[at + axis]) * ROPE_STEP;
    }
  }
  for (let i = 0; i < offsets.length; i++) offsets[i] += velocities[i] * ROPE_STEP;
  pin(rope);
}

/**
 * Advance by a frame. Returns whether the rope is still moving (or held); at rest every offset is
 * exactly zero again, so a rope nobody touches costs nothing.
 */
export function stepRope(rope: Rope, dt: number): boolean {
  if (!rope.active) return false;
  pin(rope);
  rope.carry += Math.min(ROPE_MAX_FRAME, Math.max(0, dt));
  while (rope.carry >= ROPE_STEP) { substep(rope); rope.carry -= ROPE_STEP; }
  if (rope.grabbed >= 0) return true;
  let far = 0, fast = 0;
  for (let i = 0; i < rope.offsets.length; i++) {
    far = Math.max(far, Math.abs(rope.offsets[i])); fast = Math.max(fast, Math.abs(rope.velocities[i]));
  }
  if (far < ROPE_REST_DISTANCE && fast < ROPE_REST_SPEED) {
    rope.offsets.fill(0); rope.velocities.fill(0); rope.carry = 0; rope.active = false;
  }
  return rope.active;
}

/** Without motion the rope does not swing: it is simply back in place. */
export function settleRope(rope: Rope): void {
  rope.grabbed = -1; rope.offsets.fill(0); rope.velocities.fill(0); rope.carry = 0; rope.active = false;
}
