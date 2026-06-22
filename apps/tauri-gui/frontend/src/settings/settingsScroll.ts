export type ScrollTopTarget = { scrollTop: number } | null;

export function resetSettingsContentScroll(target: ScrollTopTarget): void {
  if (!target) return;
  target.scrollTop = 0;
}

export function scheduleSettingsContentScrollReset(target: ScrollTopTarget): void {
  if (!target) return;
  window.requestAnimationFrame(() => {
    resetSettingsContentScroll(target);
  });
}
