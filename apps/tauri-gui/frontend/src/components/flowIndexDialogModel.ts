export type IndexDialogKeyIntent =
  | { readonly kind: 'focus'; readonly index: number }
  | { readonly kind: 'activate'; readonly index: number }
  | { readonly kind: 'close' };

export function clampIndexDialogActive(index: number, count: number): number {
  if (count <= 0) return -1;
  return Math.min(Math.max(0, Math.trunc(index)), count - 1);
}

export function shouldInitializeIndexDialogFocus(
  wasOpen: boolean,
  open: boolean,
): boolean {
  return open && !wasOpen;
}

export function resolveIndexDialogKey(
  key: string,
  currentIndex: number,
  count: number,
): IndexDialogKeyIntent | null {
  if (key === 'Escape') return { kind: 'close' };
  if (count <= 0) return null;
  const current = clampIndexDialogActive(currentIndex, count);
  switch (key) {
    case 'ArrowDown': return { kind: 'focus', index: Math.min(count - 1, current + 1) };
    case 'ArrowUp': return { kind: 'focus', index: Math.max(0, current - 1) };
    case 'Home': return { kind: 'focus', index: 0 };
    case 'End': return { kind: 'focus', index: count - 1 };
    case 'Enter':
    case ' ': return { kind: 'activate', index: current };
    default: return null;
  }
}
