import { useSyncExternalStore } from 'react';

let query: MediaQueryList | undefined;
const listeners = new Set<() => void>();
function mediaQuery() {
  if (!query && typeof window !== 'undefined') {
    query = window.matchMedia?.('(prefers-reduced-motion: reduce)');
  }
  return query;
}
function notify() { for (const listener of listeners) listener(); }
function subscribe(listener: () => void) {
  const media = mediaQuery();
  if (listeners.size === 0) media?.addEventListener('change', notify);
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0) media?.removeEventListener('change', notify);
  };
}
function snapshot() { return mediaQuery()?.matches ?? false; }

/** One media query and browser listener, shared by every mounted consumer. */
export function useReducedMotion(): boolean {
  return useSyncExternalStore(subscribe, snapshot, () => false);
}
