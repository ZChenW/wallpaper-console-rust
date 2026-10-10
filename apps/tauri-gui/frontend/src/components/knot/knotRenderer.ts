/** The only three boundary. Merely importing this module does not load WebGL code. */
import type * as Three from 'three';
import { curvePoint, lerp, type KnotCurve, type Vec3 } from './knotCurves.ts';
import {
  CAM_Z, CAMERA_FOV, DEFAULT_ASPECT, FOG_FAR, FOG_NEAR, TILE_COUNT,
  REDISTRIBUTE_SECONDS, REDISTRIBUTE_STAGGER_SECONDS,
  ASSEMBLED_AMOUNT, assembleWant, assembledCell, assembledCellBounds, assembledSize, cameraFov, needsVertexUpdate, pictureAspect,
  pictureLoopDistance, redistributionProgress, scaledKnotPath, scatteredTileCentre,
  scatteredTileSize, selectedIndex, stepAssemble, tileAmount, tilePose, tileUVCell,
} from './knotModel.ts';

let three: typeof Three;
export async function loadKnotRenderer() {
  three = await import('three');
  return KnotRenderer;
}
export interface KnotRenderEntry { readonly key: string; readonly id: number }
interface Picture {
  readonly entry: KnotRenderEntry;
  readonly mesh: Three.Mesh<Three.BufferGeometry, Three.MeshBasicMaterial>;
  from: Vec3;
  to: Vec3;
  position: Vec3;
  fromTiles: readonly Vec3[];
  toTiles: readonly Vec3[];
  scattered: readonly Vec3[];
  amount: number;
  aspect: number;
  dirty: boolean;
}
interface CachedTexture { readonly texture: Three.Texture; readonly image: HTMLImageElement; ready: boolean }

export class KnotRenderer {
  onInvalidate: (() => void) | null = null;
  private readonly renderer: Three.WebGLRenderer;
  private readonly scene = new three.Scene();
  private readonly camera = new three.PerspectiveCamera(CAMERA_FOV, 1, 0.1, 20000);
  private readonly raycaster = new three.Raycaster();
  private readonly tint = new three.Color();
  private readonly colorCanvas = document.createElement('canvas');
  private cards: Picture[] = [];
  private readonly textures = new Map<string, CachedTexture>();
  private readonly pictures = new Map<string, string>();
  private readonly requestedPictures = new Map<string, string>();
  private previewKey: string | null = null;
  private readonly previewKeys = new Set<string>();
  private path: readonly Vec3[] = [];
  private fromPath: readonly Vec3[] = [];
  private layoutAt = 0;
  private transitioning = false;
  private reduced = false;
  private disposed = false;
  private lost = false;
  get available() { return !this.disposed && !this.lost; }
  private readonly onLost = (event: Event) => { event.preventDefault(); this.lost = true; };
  private readonly onRestored = () => {
    this.lost = false;
    for (const cached of this.textures.values()) if (cached.ready) cached.texture.needsUpdate = true;
    this.onInvalidate?.();
  };

  constructor(private readonly canvas: HTMLCanvasElement) {
    this.renderer = new three.WebGLRenderer({ canvas, alpha: true, antialias: true, powerPreference: 'low-power' });
    this.renderer.setClearColor(0, 0);
    this.scene.fog = new three.Fog(0, FOG_NEAR, FOG_FAR);
    // Default quaternion is identity: the camera always looks down -Z.
    canvas.addEventListener('webglcontextlost', this.onLost);
    canvas.addEventListener('webglcontextrestored', this.onRestored);
  }
  setSize(width: number, height: number, dpr: number) {
    this.renderer.setPixelRatio(Math.min(2, Math.max(1, dpr)));
    this.renderer.setSize(Math.max(1, width), Math.max(1, height), false);
    this.camera.aspect = Math.max(1, width) / Math.max(1, height);
    this.camera.fov = cameraFov(this.camera.aspect);
    this.camera.updateProjectionMatrix();
    for (const card of this.cards) card.dirty = true;
  }
  setTheme(background: string, muted: string) {
    (this.scene.fog as Three.Fog).color.copy(this.cssColor(background));
    this.tint.copy(this.cssColor(muted));
    for (const card of this.cards) if (!card.mesh.material.map) card.mesh.material.color.copy(this.tint);
  }
  private cssColor(css: string) {
    // Resolve CSS color-mix/OKLCH via WebKit's own sRGB parser, not three's parser.
    this.colorCanvas.width = this.colorCanvas.height = 1;
    const context = this.colorCanvas.getContext('2d', { willReadFrequently: true });
    if (!context) return new three.Color().setStyle(css);
    context.fillStyle = css;
    context.fillRect(0, 0, 1, 1);
    const [r, g, b] = context.getImageData(0, 0, 1, 1).data;
    return new three.Color().setRGB(r / 255, g / 255, b / 255, three.SRGBColorSpace);
  }
  private currentPath(now: number): readonly Vec3[] {
    if (!this.transitioning) return this.path;
    const progress = redistributionProgress((now - this.layoutAt) / 1000, 0, 1, this.reduced);
    return this.path.map((p, i) => lerp(this.fromPath[i], p, progress));
  }
  setLayout(entries: readonly KnotRenderEntry[], curve: KnotCurve, now: number, reduced: boolean) {
    const oldPath = this.currentPath(now);
    const nextPath = scaledKnotPath(curve, entries.length);
    const existing = new Map(this.cards.map((card) => [card.entry.key, card]));
    this.cards = entries.map((entry, index) => {
      const to = curvePoint(nextPath, index / Math.max(1, entries.length));
      const toTiles = Array.from({ length: TILE_COUNT }, (_, tile) => scatteredTileCentre(nextPath, index, entries.length, entry.id, tile));
      const retained = existing.get(entry.key);
      if (retained) {
        existing.delete(entry.key);
        // Capture the sampled scatter and centre on interrupted redistribution.
        retained.from = retained.position;
        retained.fromTiles = retained.scattered;
        retained.to = to;
        retained.toTiles = toTiles;
        retained.dirty = true;
        return retained;
      }
      const geometry = new three.BufferGeometry();
      geometry.setAttribute('position', new three.BufferAttribute(new Float32Array(TILE_COUNT * 4 * 3), 3).setUsage(three.DynamicDrawUsage));
      const uvs: number[] = [], indices: number[] = [];
      for (let tile = 0; tile < TILE_COUNT; tile++) {
        const { u0, u1, v0, v1 } = tileUVCell(tile), base = tile * 4;
        uvs.push(u0, v0, u1, v0, u1, v1, u0, v1);
        indices.push(base, base + 1, base + 2, base, base + 2, base + 3);
      }
      geometry.setAttribute('uv', new three.Float32BufferAttribute(uvs, 2));
      geometry.setIndex(indices);
      const mesh = new three.Mesh(geometry, new three.MeshBasicMaterial({ side: three.DoubleSide, color: this.tint }));
      mesh.userData.index = index;
      this.scene.add(mesh);
      return { entry, mesh, from: to, to, position: to, fromTiles: toTiles, toTiles, scattered: toTiles,
        amount: 0, aspect: DEFAULT_ASPECT, dirty: true };
    });
    for (const card of existing.values()) {
      this.scene.remove(card.mesh); card.mesh.geometry.dispose(); card.mesh.material.dispose(); this.setPicture(card.entry.key, null);
    }
    this.fromPath = oldPath.length ? oldPath : nextPath;
    this.path = nextPath;
    this.layoutAt = now;
    this.reduced = reduced;
    this.transitioning = oldPath.length > 0 && !reduced;
    this.onInvalidate?.();
  }
  /** Freeze redistribution time while the controller is hidden/inactive. */
  resumeAfter(milliseconds: number) { this.layoutAt += milliseconds; }

  setPicture(key: string, url: string | null) {
    if (this.disposed || this.requestedPictures.get(key) === url) return;
    if (url === null) {
      this.requestedPictures.delete(key);
      this.pictures.delete(key);
    } else {
      // Keep the last decoded binding while a changed URL loads. A failed decode
      // must never replace a usable image with a tint or lower-resolution fallback.
      this.requestedPictures.set(key, url);
      this.loadTexture(url);
      if (this.textures.get(url)?.ready) this.pictures.set(key, url);
    }
    this.releaseUnusedTextures();
    this.onInvalidate?.();
  }
  /**
   * Names the picture that should be sharp. A picture that stops being that one keeps its large
   * preview until its tiles have scattered: dropping it at once showed the picture going soft for
   * the whole of its exit.
   */
  setPreview(key: string | null, url: string | null) {
    this.previewKey = key;
    if (!key) return;
    this.previewKeys.add(key);
    if (url !== null) this.setPicture(`large:${key}`, url);
  }
  private releasePreviews() {
    let released = false;
    for (const key of this.previewKeys) {
      if (key === this.previewKey) continue;
      const card = this.cards.find((candidate) => candidate.entry.key === key);
      if (card && card.amount > 0) continue;
      this.previewKeys.delete(key);
      this.requestedPictures.delete(`large:${key}`);
      this.pictures.delete(`large:${key}`);
      released = true;
    }
    if (released) this.releaseUnusedTextures();
  }
  private loadTexture(url: string) {
    const existing = this.textures.get(url);
    if (existing) { this.textures.delete(url); this.textures.set(url, existing); return; }
    const image = new Image();
    // Set before src: asset-protocol URLs have a separate origin in WebKitGTK.
    image.crossOrigin = 'anonymous';
    const texture = new three.Texture(image);
    texture.colorSpace = three.SRGBColorSpace;
    const cached: CachedTexture = { texture, image, ready: false };
    this.textures.set(url, cached);
    image.onload = () => {
      void image.decode().then(() => {
        if (this.disposed || this.textures.get(url) !== cached) return;
        cached.ready = true;
        texture.needsUpdate = true;
        for (const [key, requested] of this.requestedPictures) if (requested === url) this.pictures.set(key, url);
        this.releaseUnusedTextures();
        this.onInvalidate?.();
      }, () => { /* Keep the thumbnail/tint if decoding the replacement fails. */ });
    };
    image.src = url;
  }
  private releaseUnusedTextures() {
    const used = new Set([...this.pictures.values(), ...this.requestedPictures.values()]);
    // Map insertion order is LRU. Window exit is a hard release, never a retained GPU cache.
    for (const [url, cached] of this.textures) {
      // At most 121 thumbnail bindings + one large preview can be drawn. Pending
      // replacements have not been uploaded; the old GPU texture dies on decode.
      if (used.has(url)) continue;
      for (const card of this.cards) if (card.mesh.material.map === cached.texture) {
        card.mesh.material.map = null;
        card.mesh.material.color.copy(this.tint);
        card.mesh.material.needsUpdate = true;
      }
      cached.image.onload = cached.image.onerror = null;
      cached.image.removeAttribute('src');
      cached.texture.dispose();
      this.textures.delete(url);
    }
  }
  private texture(key: string) {
    const url = this.pictures.get(key), cached = url ? this.textures.get(url) : null;
    if (url && cached) { this.textures.delete(url); this.textures.set(url, cached); }
    return cached?.ready ? cached : null;
  }

  /** Only changing assembly/layout (or a new aspect/viewport) uploads vertices. */
  render(cameraT: number, now: number, dt: number, reduced: boolean, speedPictures = 0): { moving: boolean; focusedKey: string | null } {
    if (!this.available || document.hidden || this.path.length === 0) return { moving: false, focusedKey: null };
    this.reduced = reduced;
    // Before choosing textures, so a released picture draws its thumbnail in this same frame.
    if (this.previewKeys.size > (this.previewKey ? 1 : 0)) this.releasePreviews();
    const elapsed = (now - this.layoutAt) / 1000;
    const redistributing = this.transitioning;
    const pathProgress = redistributing ? redistributionProgress(elapsed, 0, 1, reduced) : 1;
    const point = lerp(curvePoint(this.fromPath, cameraT), curvePoint(this.path, cameraT), pathProgress);
    this.camera.position.set(point[0], point[1], point[2] + CAM_Z);
    this.camera.updateMatrixWorld();
    const layoutMoving = redistributing && !reduced && elapsed < REDISTRIBUTE_SECONDS + REDISTRIBUTE_STAGGER_SECONDS;
    let moving = layoutMoving;
    const selected = selectedIndex(cameraT, this.cards.length);
    let focusedKey: string | null = null;
    for (let index = 0; index < this.cards.length; index++) {
      const card = this.cards[index];
      const want = assembleWant(pictureLoopDistance(index / this.cards.length, cameraT, this.cards.length), speedPictures);
      const previous = card.amount;
      card.amount = stepAssemble(previous, want, dt, reduced);
      if (card.amount !== want) moving = true;
      if (index === selected && card.amount === 1 && !layoutMoving) focusedKey = card.entry.key;
      const cached = (this.previewKeys.has(card.entry.key) ? this.texture(`large:${card.entry.key}`) : null) ?? this.texture(card.entry.key);
      const material = card.mesh.material, texture = cached?.texture ?? null;
      if (material.map !== texture) {
        material.map = texture; material.needsUpdate = true;
        if (texture) material.color.set(0xffffff);
        else material.color.copy(this.tint);
      }
      const image = cached?.image;
      const aspect = pictureAspect(image?.naturalWidth && image.naturalHeight ? image.naturalWidth / image.naturalHeight : DEFAULT_ASPECT);
      if (aspect !== card.aspect) { card.aspect = aspect; card.dirty = true; }
      if (needsVertexUpdate(previous, card.amount, redistributing, card.dirty)) {
        const progress = redistributing ? redistributionProgress(elapsed, index, this.cards.length, reduced) : 1;
        card.position = lerp(card.from, card.to, progress);
        card.scattered = card.toTiles.map((p, tile) => lerp(card.fromTiles[tile], p, progress));
        const size = assembledSize(card.aspect, this.camera.aspect);
        const positions = card.mesh.geometry.attributes.position;
        for (let tile = 0; tile < TILE_COUNT; tile++) {
          const pose = tilePose(card.scattered[tile], scatteredTileSize(card.aspect, card.entry.id, tile),
            assembledCell(card.position, size, tile), tileAmount(card.amount, tile));
          const [x, y, z] = pose.centre, w = pose.width / 2, h = pose.height / 2;
          if (card.amount === 1) {
            // Shared cell edges use identical arithmetic before Float32 quantisation.
            const { left, right, top, bottom, z: depth } = assembledCellBounds(card.position, size, tile);
            positions.setXYZ(tile * 4, left, bottom, depth);
            positions.setXYZ(tile * 4 + 1, right, bottom, depth);
            positions.setXYZ(tile * 4 + 2, right, top, depth);
            positions.setXYZ(tile * 4 + 3, left, top, depth);
            continue;
          }
          positions.setXYZ(tile * 4, x - w, y - h, z);
          positions.setXYZ(tile * 4 + 1, x + w, y - h, z);
          positions.setXYZ(tile * 4 + 2, x + w, y + h, z);
          positions.setXYZ(tile * 4 + 3, x - w, y + h, z);
        }
        positions.needsUpdate = true;
        card.mesh.geometry.computeBoundingSphere();
        card.dirty = false;
      }
      card.mesh.userData.index = index;
    }
    if (reduced || elapsed >= REDISTRIBUTE_SECONDS + REDISTRIBUTE_STAGGER_SECONDS) this.transitioning = false;
    this.renderer.render(this.scene, this.camera);
    return { moving, focusedKey };
  }
  /** Whole enough to act on: the last hundredth is a sub-pixel move, not worth refusing a click for. */
  isAssembled(index: number) { return (this.cards[index]?.amount ?? 0) >= ASSEMBLED_AMOUNT && !this.transitioning; }
  pick(x: number, y: number): number | null {
    if (!this.available) return null;
    const rect = this.canvas.getBoundingClientRect();
    if (!rect.width || !rect.height) return null;
    this.raycaster.setFromCamera(new three.Vector2(x / rect.width * 2 - 1, 1 - y / rect.height * 2), this.camera);
    this.scene.updateMatrixWorld(true);
    const hits = this.raycaster.intersectObjects(this.cards.map((card) => card.mesh), false);
    // Fog-hidden surfaces should not intercept clicks on the empty stage.
    const hit = hits.find((candidate) => this.camera.position.z - candidate.point.z < FOG_FAR);
    return hit ? hit.object.userData.index as number : null;
  }
  dispose() {
    if (this.disposed) return;
    this.disposed = true;
    this.onInvalidate = null;
    this.canvas.removeEventListener('webglcontextlost', this.onLost);
    this.canvas.removeEventListener('webglcontextrestored', this.onRestored);
    for (const card of this.cards) { card.mesh.geometry.dispose(); card.mesh.material.dispose(); }
    this.pictures.clear(); this.requestedPictures.clear(); this.previewKeys.clear();
    this.releaseUnusedTextures();
    this.cards = [];
    this.scene.clear();
    this.renderer.dispose();
  }
}
