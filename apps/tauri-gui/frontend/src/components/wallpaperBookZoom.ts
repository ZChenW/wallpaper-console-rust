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
export function bookZoomTimeline(direction: 'open' | 'close', pagePose: string, current?: BookZoomSample) {
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
      { offset: 0, transform: current?.transform ?? (closing ? BOOK_ZOOM_REST : pagePose),
        easing: closing ? 'cubic-bezier(0.22, 1, 0.36, 1)' : 'cubic-bezier(0.16, 1, 0.3, 1)' },
      { offset: 1, transform: closing ? pagePose : BOOK_ZOOM_REST },
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
