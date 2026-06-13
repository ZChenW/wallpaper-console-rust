import { createContext, ReactNode, useContext } from 'react';
import { useThumbnailQueue } from '../hooks/useThumbnailQueue';
import type { EnqueueOptions, ThumbState } from '../hooks/thumbnailQueueCore';

interface ThumbnailStoreValue {
  thumbs: ThumbState;
  enqueue: (paths: string[], options?: EnqueueOptions) => void;
  reset: () => void;
}

const ThumbnailStoreContext = createContext<ThumbnailStoreValue | null>(null);

export function ThumbnailStoreProvider({ children }: { children: ReactNode }) {
  const queue = useThumbnailQueue(4);
  return (
    <ThumbnailStoreContext.Provider value={queue}>
      {children}
    </ThumbnailStoreContext.Provider>
  );
}

export function useThumbnailStore(): ThumbnailStoreValue {
  const value = useContext(ThumbnailStoreContext);
  if (!value) throw new Error('useThumbnailStore must be used inside ThumbnailStoreProvider');
  return value;
}
