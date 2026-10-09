/** The prototype's only three.js boundary. Importing this module does not load three. */
import type * as Three from 'three';
import { bandFrame, curvePoint, curveTangent, frontMostIndex, resampleCurve, slotCentres, TREFOIL, type Vec3 } from './knotCurves.ts';
import { KNOT_CAMERA_FOV, KNOT_CARD_ASPECT, KNOT_CARD_LENGTH, KNOT_FLOAT_WIDTH, KNOT_SLOT_COUNT } from './knotModel.ts';

let three: typeof Three;
export async function loadKnotRenderer() {
  three = await import('three');
  return KnotRenderer;
}
const CURVE_SAMPLES = 1408;
const CARD_SEGMENTS = 8;
const TEXTURE_CACHE_LIMIT = 80;
const ROPE_RADIUS = 0.013;
const CAMERA_DISTANCE = 12;
const FLOAT_DEPTH = 3.5;
const CAMERA_PADDING = 1.12;

export interface KnotRenderSlot { readonly slot: number; readonly relative: number; readonly key: string | null; readonly entryIndex: number | null }
export interface KnotRenderState {
  readonly slots: readonly KnotRenderSlot[];
  readonly fractionalFlow: number;
  readonly fly: number;
  readonly flyingEntry: number;
  readonly flyingKey: string | null;
  readonly originalKey?: string | null;
  readonly yaw: number;
  readonly pitch: number;
}
interface CachedTexture { texture: Three.Texture; image: HTMLImageElement; ready: boolean; failed: boolean }
interface Card { geometry: Three.BufferGeometry; front: Three.Mesh<Three.BufferGeometry, Three.MeshLambertMaterial>; back: Three.Mesh<Three.BufferGeometry, Three.MeshLambertMaterial> }

export class KnotRenderer {
  onInvalidate: (() => void) | null = null;
  private readonly renderer: Three.WebGLRenderer;
  private readonly scene = new three.Scene();
  private readonly root = new three.Group();
  private readonly camera = new three.PerspectiveCamera(KNOT_CAMERA_FOV, 1, 0.1, 100);
  private readonly raycaster = new three.Raycaster();
  private readonly points = resampleCurve(TREFOIL.components[0], CURVE_SAMPLES);
  private readonly selectedSlot = frontMostIndex(slotCentres(this.points, KNOT_SLOT_COUNT), [0, 0, 1]);
  private readonly cards: Card[] = [];
  private readonly textures = new Map<string, CachedTexture>();
  private readonly pictures = new Map<string, string>();
  private readonly backColor = new three.Color();
  private readonly colorCanvas = document.createElement('canvas');
  private readonly rope: Three.Mesh<Three.TubeGeometry, Three.MeshLambertMaterial>;
  private readonly floating: Three.Mesh<Three.BufferGeometry, Three.MeshBasicMaterial>;
  private disposed = false;
  private lost = false;
  private width = 1;
  private state: KnotRenderState | null = null;
  private readonly onLost = (event: Event) => { event.preventDefault(); this.lost = true; };
  private readonly onRestored = () => {
    this.lost = false;
    for (const { texture, ready } of this.textures.values()) if (ready) texture.needsUpdate = true;
    this.onInvalidate?.();
  };

  constructor(private readonly canvas: HTMLCanvasElement) {
    this.renderer = new three.WebGLRenderer({ canvas, alpha: true, antialias: true, powerPreference: 'low-power' });
    this.renderer.setClearColor(0, 0);
    this.camera.position.z = CAMERA_DISTANCE;
    this.scene.add(this.root, new three.AmbientLight(0xffffff, 2));
    const light = new three.DirectionalLight(0xffffff, 2.2);
    light.position.set(0, 4, 8);
    this.scene.add(light);
    const points = this.points;
    class RopeCurve extends three.Curve<Three.Vector3> {
      constructor() { super(); }
      getPoint(t: number, target = new three.Vector3()) { return target.set(...curvePoint(points, t * points.length)); }
    }
    this.rope = new three.Mesh(new three.TubeGeometry(new RopeCurve(), 704, ROPE_RADIUS, 6, true), new three.MeshLambertMaterial());
    this.root.add(this.rope);
    for (let slot = 0; slot < KNOT_SLOT_COUNT; slot++) {
      const geometry = this.cardGeometry();
      const front = new three.Mesh(geometry, new three.MeshLambertMaterial({ side: three.FrontSide }));
      const back = new three.Mesh(geometry, new three.MeshLambertMaterial({ side: three.BackSide }));
      front.userData.slot = slot;
      back.userData.slot = slot;
      this.cards.push({ geometry, front, back });
      this.root.add(front, back);
    }
    this.floating = new three.Mesh(this.cardGeometry(), new three.MeshBasicMaterial({ side: three.DoubleSide }));
    this.scene.add(this.floating);
    canvas.addEventListener('webglcontextlost', this.onLost);
    canvas.addEventListener('webglcontextrestored', this.onRestored);
  }

  private cardGeometry() {
    const geometry = new three.BufferGeometry();
    geometry.setAttribute('position', new three.BufferAttribute(new Float32Array((CARD_SEGMENTS + 1) * 6), 3).setUsage(three.DynamicDrawUsage));
    const uv = new Float32Array((CARD_SEGMENTS + 1) * 4);
    const indices: number[] = [];
    for (let i = 0; i <= CARD_SEGMENTS; i++) {
      uv.set([i / CARD_SEGMENTS, 0, i / CARD_SEGMENTS, 1], i * 4);
      if (i < CARD_SEGMENTS) indices.push(i * 2, i * 2 + 2, i * 2 + 1, i * 2 + 1, i * 2 + 2, i * 2 + 3);
    }
    geometry.setAttribute('uv', new three.BufferAttribute(uv, 2));
    geometry.setIndex(indices);
    return geometry;
  }

  setSize(width: number, height: number, dpr: number) {
    this.width = Math.max(1, width);
    this.renderer.setPixelRatio(Math.min(2, Math.max(1, dpr)));
    this.renderer.setSize(this.width, Math.max(1, height), false);
    this.camera.aspect = this.width / Math.max(1, height);
    // Fit the three lobes on narrow stages too, without cropping the ribbon.
    this.camera.position.z = Math.max(CAMERA_DISTANCE, CAMERA_PADDING * 3.5 / (Math.tan(KNOT_CAMERA_FOV * Math.PI / 360) * this.camera.aspect));
    this.camera.updateProjectionMatrix();
  }
  setTheme(theme: { rope: string; cardBack: string; background?: string }) {
    this.rope.material.color.copy(this.cssColor(theme.rope));
    this.backColor.copy(this.cssColor(theme.cardBack));
    for (const card of this.cards) {
      card.back.material.color.copy(this.backColor);
      if (!card.front.material.map) card.front.material.color.copy(this.backColor);
    }
    this.renderer.setClearColor(theme.background ? this.cssColor(theme.background) : 0, theme.background ? 1 : 0);
  }
  private cssColor(css: string) {
    // three's Color parser doesn't support color-mix, color(srgb), or OKLCH.
    // A 1px canvas resolves all CSS colors that this webview understands to sRGB.
    this.colorCanvas.width = this.colorCanvas.height = 1;
    const context = this.colorCanvas.getContext('2d', { willReadFrequently: true });
    if (!context) return new three.Color().setStyle(css);
    context.fillStyle = css;
    context.fillRect(0, 0, 1, 1);
    const [r, g, b] = context.getImageData(0, 0, 1, 1).data;
    return new three.Color().setRGB(r / 255, g / 255, b / 255, three.SRGBColorSpace);
  }

  setPicture(key: string, url: string | null) {
    if (this.disposed) return;
    if (url === null) this.pictures.delete(key);
    else {
      this.pictures.set(key, url);
      this.loadTexture(url);
    }
    this.evictTextures();
  }
  private loadTexture(url: string) {
    const existing = this.textures.get(url);
    if (existing) { this.textures.delete(url); this.textures.set(url, existing); return; }
    const image = new Image();
    // Must precede src: Tauri's asset protocol is a separate origin in WebKitGTK.
    image.crossOrigin = 'anonymous';
    const texture = new three.Texture(image);
    texture.colorSpace = three.SRGBColorSpace;
    const cached: CachedTexture = { texture, image, ready: false, failed: false };
    this.textures.set(url, cached);
    image.onload = () => {
      void image.decode().then(() => {
        if (this.disposed || this.textures.get(url) !== cached) return;
        cached.ready = true;
        texture.needsUpdate = true;
        this.onInvalidate?.();
      }, () => { cached.failed = true; });
    };
    image.onerror = () => { cached.failed = true; };
    image.src = url;
  }
  private evictTextures() {
    const used = new Set(this.pictures.values());
    for (const [url, cached] of this.textures) {
      if (this.textures.size <= TEXTURE_CACHE_LIMIT) break;
      if (used.has(url)) continue;
      cached.image.onload = cached.image.onerror = null;
      cached.image.removeAttribute('src');
      cached.texture.dispose();
      this.textures.delete(url);
    }
  }
  private texture(key: string | null | undefined) {
    const url = key ? this.pictures.get(key) : null;
    const cached = url ? this.textures.get(url) : null;
    if (url && cached) { this.textures.delete(url); this.textures.set(url, cached); }
    return cached?.ready ? cached.texture : null;
  }
  private bandPositions(relative: number, fractional: number, view: Vec3): Float32Array {
    const result = new Float32Array((CARD_SEGMENTS + 1) * 6);
    const stride = this.points.length / KNOT_SLOT_COUNT;
    const centre = (this.selectedSlot + relative - fractional) * stride;
    const length = stride * KNOT_CARD_LENGTH;
    const chord = new three.Vector3(...curvePoint(this.points, centre - length / 2)).distanceTo(new three.Vector3(...curvePoint(this.points, centre + length / 2)));
    for (let i = 0; i <= CARD_SEGMENTS; i++) {
      const position = centre + (i / CARD_SEGMENTS - 0.5) * length;
      const point = curvePoint(this.points, position);
      const { width, normal } = bandFrame(curveTangent(this.points, position), view);
      for (let side = 0; side < 2; side++) {
        result.set(point.map((n, axis) => n + width[axis] * (side - 0.5) * chord / KNOT_CARD_ASPECT + normal[axis] * ROPE_RADIUS * 1.5), i * 6 + side * 3);
      }
    }
    return result;
  }
  private updateGeometry(geometry: Three.BufferGeometry, positions: Float32Array) {
    (geometry.getAttribute('position').array as Float32Array).set(positions);
    geometry.getAttribute('position').needsUpdate = true;
    geometry.computeVertexNormals();
    geometry.computeBoundingSphere();
  }
  render(state: KnotRenderState) {
    this.state = state;
    if (this.disposed || this.lost || document.hidden) return;
    this.root.rotation.set(state.pitch, state.yaw, 0, 'YXZ');
    this.root.updateMatrixWorld(true);
    const direction = new three.Vector3(0, 0, 1).applyQuaternion(this.root.quaternion.clone().invert());
    const view: Vec3 = [direction.x, direction.y, direction.z];
    let flyingSlot: KnotRenderSlot | undefined;
    for (const slot of state.slots) {
      const card = this.cards[slot.slot];
      const flying = slot.entryIndex === state.flyingEntry && state.fly > 0;
      if (flying) flyingSlot = slot;
      card.front.visible = card.back.visible = slot.key !== null && !flying;
      if (!card.front.visible) continue;
      this.updateGeometry(card.geometry, this.bandPositions(slot.relative, state.fractionalFlow, view));
      const texture = this.texture(slot.key);
      if (card.front.material.map !== texture) {
        card.front.material.map = texture;
        card.front.material.needsUpdate = true;
      }
      if (texture) card.front.material.color.set(0xffffff);
      else card.front.material.color.copy(this.backColor);
    }
    this.floating.visible = Boolean(flyingSlot && state.fly > 0);
    if (flyingSlot) {
      this.floating.userData.slot = flyingSlot.slot;
      const texture = this.texture(state.originalKey) ?? this.texture(state.flyingKey);
      if (this.floating.material.map !== texture) {
        this.floating.material.map = texture;
        this.floating.material.needsUpdate = true;
      }
      if (texture) this.floating.material.color.set(0xffffff);
      else this.floating.material.color.copy(this.backColor);
      const positions = this.bandPositions(flyingSlot.relative, state.fractionalFlow, view);
      const image = texture?.image as HTMLImageElement | undefined;
      const aspect = image?.naturalWidth && image.naturalHeight ? image.naturalWidth / image.naturalHeight : KNOT_CARD_ASPECT;
      // Size the picture against the view's height (the knot is fitted to it), not its width: on a
      // wide stage a width-based size covered the whole knot.
      const viewHeight = 2 * (this.camera.position.z - FLOAT_DEPTH) * Math.tan(KNOT_CAMERA_FOV * Math.PI / 360);
      const width = viewHeight * Math.min(this.camera.aspect, KNOT_CARD_ASPECT) * KNOT_FLOAT_WIDTH;
      const height = Math.min(width / aspect, viewHeight * KNOT_FLOAT_WIDTH);
      const halfHeightPixels = height / (2 * (this.camera.position.z - FLOAT_DEPTH) * Math.tan(KNOT_CAMERA_FOV * Math.PI / 360)) * this.canvas.clientHeight / 2;
      this.canvas.parentElement?.style.setProperty('--knot-picture-half-height', `${halfHeightPixels}px`);
      const fittedWidth = height * aspect;
      for (let i = 0; i <= CARD_SEGMENTS; i++) for (let side = 0; side < 2; side++) {
        const index = i * 6 + side * 3;
        const source = new three.Vector3().fromArray(positions, index).applyMatrix4(this.root.matrixWorld);
        const destination = new three.Vector3((i / CARD_SEGMENTS - 0.5) * fittedWidth, (side - 0.5) * height, FLOAT_DEPTH);
        source.lerp(destination, state.fly).toArray(positions, index);
      }
      this.updateGeometry(this.floating.geometry, positions);
    }
    this.renderer.render(this.scene, this.camera);
  }
  pick(x: number, y: number): number | null {
    if (this.lost || !this.state) return null;
    const rect = this.canvas.getBoundingClientRect();
    this.raycaster.setFromCamera(new three.Vector2(x / rect.width * 2 - 1, 1 - y / rect.height * 2), this.camera);
    this.scene.updateMatrixWorld(true);
    const objects: Three.Object3D[] = this.cards.flatMap((card) => card.front.visible ? [card.front, card.back] : []);
    if (this.floating.visible) objects.push(this.floating);
    const hit = this.raycaster.intersectObjects(objects, false)[0];
    if (!hit) return null;
    return hit.object.userData.slot as number;
  }
  dispose() {
    if (this.disposed) return;
    this.disposed = true;
    this.onInvalidate = null;
    this.canvas.removeEventListener('webglcontextlost', this.onLost);
    this.canvas.removeEventListener('webglcontextrestored', this.onRestored);
    for (const card of this.cards) { card.geometry.dispose(); card.front.material.dispose(); card.back.material.dispose(); }
    this.rope.geometry.dispose(); this.rope.material.dispose();
    this.floating.geometry.dispose(); this.floating.material.dispose();
    for (const cached of this.textures.values()) {
      cached.image.onload = cached.image.onerror = null;
      cached.image.removeAttribute('src');
      cached.texture.dispose();
    }
    this.textures.clear(); this.pictures.clear();
    this.renderer.dispose();
  }
}
