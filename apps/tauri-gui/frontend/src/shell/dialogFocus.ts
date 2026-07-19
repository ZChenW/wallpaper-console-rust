import type { KeyboardEvent } from 'react';

const FOCUSABLE_SELECTOR = [
  'a[href]',
  'button:not([disabled])',
  'input:not([disabled])',
  'select:not([disabled])',
  'textarea:not([disabled])',
  'summary',
  '[contenteditable="true"]',
  '[tabindex]:not([tabindex="-1"])',
].join(',');

export function wrappedDialogFocusIndex(
  activeIndex: number,
  focusableCount: number,
  backwards: boolean,
): number | null {
  if (focusableCount <= 0) return null;
  if (activeIndex < 0) return backwards ? focusableCount - 1 : 0;
  if (backwards && activeIndex === 0) return focusableCount - 1;
  if (!backwards && activeIndex === focusableCount - 1) return 0;
  return null;
}

function isActuallyFocusable(element: HTMLElement): boolean {
  return element.tabIndex >= 0
    && !element.hidden
    && element.getAttribute('aria-hidden') !== 'true'
    && !element.closest('[inert]')
    && element.getClientRects().length > 0;
}

export function trapDialogFocus(
  event: KeyboardEvent<HTMLElement>,
  dialog: HTMLElement,
): void {
  if (event.key !== 'Tab') return;
  const focusable = Array.from(
    dialog.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR),
  ).filter(isActuallyFocusable);
  const activeIndex = focusable.indexOf(document.activeElement as HTMLElement);
  const targetIndex = wrappedDialogFocusIndex(activeIndex, focusable.length, event.shiftKey);
  if (targetIndex === null) return;
  event.preventDefault();
  focusable[targetIndex]?.focus();
}
