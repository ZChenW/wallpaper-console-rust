import {
  BOOK_IMMERSIVE_DURATION_MS, BOOK_IMMERSIVE_EASING, bookSceneFlip, type BookRect,
} from '../components/wallpaperBookModel.ts';

const chromeSelector = '.single-page-topbar, .single-page-library-controls, .single-page-statusbar';

export interface BookLayoutSnapshot {
  readonly scene: HTMLElement;
  readonly rect: BookRect;
  readonly spreadWidth: number;
}

/** Read the old layout in one batch, then freeze fading chrome outside the grid. */
export function captureBookLayout(shell: HTMLElement): BookLayoutSnapshot | null {
  const scene = shell.querySelector<HTMLElement>('.wallpaper-book__scene');
  const spread = shell.querySelector<HTMLElement>('.wallpaper-book__spread');
  if (!scene || !spread) return null;
  const rect = scene.getBoundingClientRect();
  const spreadWidth = (Number.parseFloat(getComputedStyle(spread).width) || spread.offsetWidth)
    * rect.width / Math.max(1, scene.offsetWidth);
  const chrome = [...shell.querySelectorAll<HTMLElement>(chromeSelector)]
    .map((element) => ({ element, rect: element.getBoundingClientRect() }));
  for (const { element, rect } of chrome) {
    element.style.setProperty('--book-chrome-left', `${rect.left}px`);
    element.style.setProperty('--book-chrome-top', `${rect.top}px`);
    element.style.setProperty('--book-chrome-width', `${rect.width}px`);
  }
  return { scene, rect, spreadWidth };
}

/** Leaves have already been repainted by the child's layout effect at this point. */
export function animateBookLayout(shell: HTMLElement, previous: BookLayoutSnapshot | null, reducedMotion: boolean) {
  const scene = shell.querySelector<HTMLElement>('.wallpaper-book__scene');
  const spread = shell.querySelector<HTMLElement>('.wallpaper-book__spread');
  if (reducedMotion || !previous || !scene || scene !== previous.scene || !spread) return;
  const rect = scene.getBoundingClientRect();
  const spreadWidth = Number.parseFloat(getComputedStyle(spread).width) || spread.offsetWidth;
  const animation = scene.animate([
    { transform: bookSceneFlip(previous.rect, rect, previous.spreadWidth, spreadWidth) },
    { transform: 'none' },
  ], { duration: BOOK_IMMERSIVE_DURATION_MS, easing: BOOK_IMMERSIVE_EASING });
  return () => animation.cancel();
}
