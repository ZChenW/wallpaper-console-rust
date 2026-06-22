import { createContext, ReactNode, useContext, useMemo, useRef } from 'react';
import { api } from '../api/bridge';
import { recordMetric } from '../perf/metrics';
import { ThumbnailStore } from './thumbnailStore';
import type { EnqueueOptions } from '../hooks/thumbnailQueueCore';

interface ThumbnailStoreValue {
  get: (path: string) => string | undefined;
  subscribe: (path: string, cb: () => void) => () => void;
  enqueueVisible: (paths: string[], options?: EnqueueOptions) => void;
  forget: (paths: string[]) => void;
  reset: () => void;
  snapshot: () => { pending: string[]; active: number; cached: number };
  stats: () => { pending: number; active: number; cached: number };
  setRevealPaused: (paused: boolean) => void;
}

const ThumbnailStoreContext = createContext<ThumbnailStoreValue | null>(null);

export function ThumbnailStoreProvider({ children }: { children: ReactNode }) {
  const storeRef = useRef<ThumbnailStore | null>(null);
  if (!storeRef.current) {
    storeRef.current = new ThumbnailStore(2, async (path) => {
      const r = await api.thumbnailFor(path);
      recordMetric(r.cacheHit ? 'thumbnail.cache.hit' : 'thumbnail.cache.miss', 1);
      return r;
    });
  }
  const store = storeRef.current;
  const value = useMemo<ThumbnailStoreValue>(() => ({
    get: (path: string) => store.get(path),
    subscribe: (path: string, cb: () => void) => store.subscribe(path, cb),
    enqueueVisible: (paths: string[], options?: EnqueueOptions) => store.enqueueVisible(paths, options),
    forget: (paths: string[]) => store.forget(paths),
    reset: () => store.reset(),
    snapshot: () => store.snapshot(),
    stats: () => store.stats(),
    setRevealPaused: (paused: boolean) => store.setRevealPaused(paused),
  }), [store]);
  return (
    <ThumbnailStoreContext.Provider value={value}>
      {children}
    </ThumbnailStoreContext.Provider>
  );
}

export function useThumbnailStore(): ThumbnailStoreValue {
  const value = useContext(ThumbnailStoreContext);
  if (!value) throw new Error('useThumbnailStore must be used inside ThumbnailStoreProvider');
  return value;
}
