import {
  createContext,
  ReactNode,
  useCallback,
  useContext,
  useMemo,
  useRef,
  useSyncExternalStore,
} from 'react';
import { api } from '../api/bridge';
import { recordMetric } from '../perf/metrics';
import { ThumbnailSession } from './thumbnailStore';
import type { EnqueueOptions } from '../hooks/thumbnailQueueCore';

interface ThumbnailSessionValue {
  get: (path: string) => string | undefined;
  getFailure: (path: string) => string | undefined;
  failureCount: () => number;
  subscribe: (path: string, cb: () => void) => () => void;
  observeVisible: (paths: string[], options?: EnqueueOptions) => void;
  setScrolling: (scrolling: boolean) => void;
  setInteracting: (interacting: boolean) => void;
  forget: (paths: string[]) => void;
  reset: () => void;
  refreshSubscribed: () => void;
  snapshot: () => { pending: string[]; active: number; cached: number };
  stats: () => { pending: number; active: number; cached: number; failures: number };
}

const ThumbnailStoreContext = createContext<ThumbnailSessionValue | null>(null);

export function ThumbnailStoreProvider({ children }: { children: ReactNode }) {
  const storeRef = useRef<ThumbnailSession | null>(null);
  if (!storeRef.current) {
    storeRef.current = new ThumbnailSession(2, async (path) => {
      const r = await api.thumbnailFor(path);
      recordMetric(r.cacheHit ? 'thumbnail.cache.hit' : 'thumbnail.cache.miss', 1);
      return r;
    });
  }
  const session = storeRef.current;
  const value = useMemo<ThumbnailSessionValue>(() => ({
    get: (path: string) => session.get(path),
    getFailure: (path: string) => session.getFailure(path),
    failureCount: () => session.failureCount(),
    subscribe: (path: string, cb: () => void) => session.subscribe(path, cb),
    observeVisible: (paths: string[], options?: EnqueueOptions) => {
      session.observeVisible(paths, options);
    },
    setScrolling: (scrolling: boolean) => session.setScrolling(scrolling),
    setInteracting: (interacting: boolean) => session.setInteracting(interacting),
    forget: (paths: string[]) => session.forget(paths),
    reset: () => session.reset(),
    refreshSubscribed: () => session.refreshSubscribed(),
    snapshot: () => session.snapshot(),
    stats: () => session.stats(),
  }), [session]);
  return (
    <ThumbnailStoreContext.Provider value={value}>
      {children}
    </ThumbnailStoreContext.Provider>
  );
}

export function useThumbnailStore(): ThumbnailSessionValue {
  const value = useContext(ThumbnailStoreContext);
  if (!value) throw new Error('useThumbnailStore must be used inside ThumbnailStoreProvider');
  return value;
}

/** Read one path through the ThumbnailSession seam (subscribe + get). */
export function useThumbnail(path: string): {
  thumbnail: string | undefined;
  failure: string | undefined;
} {
  const session = useThumbnailStore();
  const subscribe = useCallback(
    (cb: () => void) => session.subscribe(path, cb),
    [path, session],
  );
  const getThumbnail = useCallback(() => session.get(path), [path, session]);
  const getFailure = useCallback(() => session.getFailure(path), [path, session]);
  const thumbnail = useSyncExternalStore(subscribe, getThumbnail, getThumbnail);
  const failure = useSyncExternalStore(subscribe, getFailure, getFailure);
  return { thumbnail, failure };
}
