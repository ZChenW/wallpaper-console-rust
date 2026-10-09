import type { BookFace, BookRect } from './wallpaperBookModel.ts';
import { BOOK_ZOOM_DURATION_MS } from './wallpaperBookModel.ts';

export const BOOK_ZOOM_OPEN_MS = BOOK_ZOOM_DURATION_MS;
export const BOOK_ZOOM_CLOSE_MS = 300;
export const BOOK_ZOOM_REVEAL_MS = 90;
export const BOOK_ZOOM_REST = 'translate3d(0px, 0px, 0px) rotateX(0deg) rotateY(0deg) scale(1)';

export interface BookZoomSample {
  readonly transform: string;
  readonly sceneOpacity: number;
  readonly backdropOpacity: number;
  readonly stillOpacity: number;
}

/** Each property has its own timing. Never reverse the open ease-out on close.
 * Paper inset/radii/box are fixed before flight; only transform and opacity move.
 */
export function bookZoomTimeline(direction: 'open' | 'close', pagePose: string, current?: BookZoomSample, rest = BOOK_ZOOM_REST) {
  const closing = direction === 'close';
  const sceneStops = closing
    ? [[0, 0], [0.35, 0.6], [0.8, 1], [1, 1]]
    : [[0, 1], [0.4, 0.15], [0.6, 0], [1, 0]];
  const startScene = current?.sceneOpacity ?? (closing ? 0 : 1);
  // Rebase the whole opacity curve on interruption so every segment remains
  // monotone, even if interrupted before the book has begun to fade.
  const scene = sceneStops.map(([offset, opacity]) => ({
    offset, easing: 'linear',
    opacity: closing ? startScene + (1 - startScene) * opacity : startScene * opacity,
  }));
  const startBackdrop = current?.backdropOpacity ?? (closing ? 1 : 0);
  const backdrop = sceneStops.map(([offset, opacity]) => ({
    offset, easing: 'linear',
    opacity: closing ? startBackdrop * (1 - opacity) : startBackdrop + (1 - startBackdrop) * (1 - opacity),
  }));
  return {
    duration: closing ? BOOK_ZOOM_CLOSE_MS : BOOK_ZOOM_OPEN_MS,
    page: [
      { offset: 0, transform: current?.transform ?? (closing ? rest : pagePose),
        easing: closing ? 'cubic-bezier(0.22, 1, 0.36, 1)' : 'cubic-bezier(0.16, 1, 0.3, 1)' },
      { offset: 1, transform: closing ? pagePose : rest },
    ],
    scene, backdrop,
    still: closing ? [
      { offset: 0, opacity: current?.stillOpacity ?? 0, easing: 'ease-out' },
      { offset: 1 / 3, opacity: 1, easing: 'linear' },
      { offset: 1, opacity: 1 },
    ] : [
      { offset: 0, opacity: current?.stillOpacity ?? 1, easing: 'linear' },
      { offset: 1, opacity: current?.stillOpacity ?? 1 },
    ],
  };
}

export function bookZoomReveal(opacity: number, reducedMotion = false) {
  return {
    duration: reducedMotion ? 0 : BOOK_ZOOM_REVEAL_MS,
    frames: [{ offset: 0, opacity, easing: 'ease-out' }, { offset: 1, opacity: 0 }],
  };
}

/** Forward navigation leaves to the left; backward leaves to the right. */
export function bookZoomSwapTimeline(direction: number, reducedMotion = false) {
  const travel = direction < 0 ? 12 : -12;
  return {
    duration: reducedMotion ? 0 : 200,
    outgoing: [{ transform: 'translateX(0%)', opacity: 1 }, { transform: `translateX(${travel}%)`, opacity: 0 }],
    incoming: [{ transform: `translateX(${-travel}%)`, opacity: 0 }, { transform: 'translateX(0%)', opacity: 1 }],
    easing: 'cubic-bezier(0.16, 1, 0.3, 1)',
  };
}

export interface BookPoint { readonly x: number; readonly y: number }
export interface BookZoomOrigin {
  readonly quad: readonly [BookPoint, BookPoint, BookPoint, BookPoint];
  readonly camera: BookPoint & { readonly distance: number };
  readonly width: number;
  readonly face: BookFace;
  readonly paperMargin: number;
}

/** Zero-size probes work in WebKitGTK, which does not expose getBoxQuads.
 * Read actual projected corners, including every ancestor's transform, without
 * changing the face's box, media, fan geometry or perspective origin.
 */
export function captureBookZoomOrigin(option: HTMLElement, scene: HTMLElement, face: BookFace): BookZoomOrigin {
  const css = getComputedStyle(option);
  const sceneCSS = getComputedStyle(scene);
  const rect = scene.getBoundingClientRect();
  const [x, y] = sceneCSS.perspectiveOrigin.split(' ').map(Number.parseFloat);
  const probes = ['0:0', '100%:0', '100%:100%', '0:100%'].map((corner) => {
    const [left, top] = corner.split(':');
    const probe = document.createElement('span');
    probe.setAttribute('aria-hidden', 'true');
    probe.style.cssText = `position:absolute;left:${left};top:${top};width:0;height:0;padding:0;border:0;pointer-events:none;`;
    option.append(probe);
    return probe;
  });
  try {
    const quad = probes.map((probe) => {
      const point = probe.getBoundingClientRect();
      return { x: point.left, y: point.top };
    }) as unknown as BookZoomOrigin['quad'];
    return { quad, camera: { x: rect.left + x, y: rect.top + y, distance: Number.parseFloat(sceneCSS.perspective) },
      width: Number.parseFloat(css.width) || option.offsetWidth, face,
      paperMargin: Number.parseFloat(css.paddingLeft) || 0 };
  } finally { probes.forEach((probe) => probe.remove()); }
}

/** Recover a rigid paper pose from its screen homography and calibrated camera.
 * Projection lives on a separate matrix wrapper. The animated page has only
 * translation, rotation and ONE scale, so CSS interpolation cannot stretch it.
 * In particular, do not animate the homography itself: matrix decomposition
 * would interpolate its projective skew as anisotropic scale.
 */
export function bookZoomTransform(origin: BookZoomOrigin, destination: BookRect) {
  const [p0, p1, p2, p3] = origin.quad;
  const { x: cx, y: cy, distance: f } = origin.camera;
  const dx1 = p1.x - p2.x, dx2 = p3.x - p2.x;
  const dy1 = p1.y - p2.y, dy2 = p3.y - p2.y;
  const dx3 = p0.x - p1.x + p2.x - p3.x;
  const dy3 = p0.y - p1.y + p2.y - p3.y;
  const denominator = dx1 * dy2 - dx2 * dy1;
  const g = denominator ? (dx3 * dy2 - dx2 * dy3) / denominator : 0;
  const h = denominator ? (dx1 * dy3 - dx3 * dy1) / denominator : 0;
  const u = [p1.x - p0.x + g * (p1.x - cx), p1.y - p0.y + g * (p1.y - cy), -f * g];
  const v = [p3.x - p0.x + h * (p3.x - cx), p3.y - p0.y + h * (p3.y - cy), -f * h];
  const width = Math.max(0.001, origin.width);
  const height = width / 1.6;
  const k = width / Math.max(0.001, Math.hypot(...u));
  const r1 = u.map((value) => value * k / width);
  const r2 = v.map((value) => value * k / height);
  const r3 = [r1[1] * r2[2] - r1[2] * r2[1], r1[2] * r2[0] - r1[0] * r2[2], r1[0] * r2[1] - r1[1] * r2[0]];
  const angle = Math.acos(Math.min(1, Math.max(-1, (r1[0] + r2[1] + r3[2] - 1) / 2)));
  const axis = angle < 1e-7 ? [0, 0, 1] : [r2[2] - r3[1], r3[0] - r1[2], r1[1] - r2[0]];
  const axisLength = Math.hypot(...axis) || 1;
  const rotation = axis.map((value) => value / axisLength).join(', ');
  const center = [cx + k * (p0.x - cx) + k * (u[0] + v[0]) / 2,
    cy + k * (p0.y - cy) + k * (u[1] + v[1]) / 2,
    f * (1 - k) + k * (u[2] + v[2]) / 2];
  const scale = width / Math.max(1, destination.width);
  const margin = Math.max(0, origin.paperMargin) / scale;
  return {
    transform: `translate3d(${center[0] - destination.left - destination.width / 2}px, ${center[1] - destination.top - destination.height / 2}px, ${center[2]}px) rotate3d(${rotation}, ${angle * 180 / Math.PI}deg) scale(${scale})`,
    rest: `translate3d(0px, 0px, 0px) rotate3d(${rotation}, 0deg) scale(1)`,
    projection: `matrix3d(1,0,0,0,0,1,0,0,0,0,1,${-1 / f},0,0,0,1)`,
    projectionOrigin: `${cx}px ${cy}px`,
    scale,
    paperInset: `${margin / 1.6}px ${margin}px`,
    paperRadius: `${4 / scale}px`,
    paperBorderRadius: origin.face === 'front'
      ? `${1 / scale}px ${9.6 / scale}px ${9.6 / scale}px ${1 / scale}px`
      : `${9.6 / scale}px ${1 / scale}px ${1 / scale}px ${9.6 / scale}px`,
  };
}
