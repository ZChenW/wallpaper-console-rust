import { useCallback, useEffect, useRef } from 'react';
import { api } from '../api/bridge';
import { ThumbnailRequestQueue, type EnqueueOptions } from './thumbnailQueueCore';
import { recordMetric } from '../perf/metrics';

export function useThumbnailQueue(concurrency = 2) {
  const queueRef = useRef<ThumbnailRequestQueue | null>(null);

  useEffect(() => {
    const queue = new ThumbnailRequestQueue({
      concurrency,
      load: async (path) => {
        const r = await api.thumbnailFor(path);
        recordMetric(r.cacheHit ? 'thumbnail.cache.hit' : 'thumbnail.cache.miss', 1);
        return r;
      },
      onThumbnail: () => {},
      onFailure: () => {},
    });
    queueRef.current = queue;

    const pollTimer = setInterval(() => {
      const snap = queue.stats();
      recordMetric('thumbnail.queue.pending', snap.pending);
      recordMetric('thumbnail.queue.inFlight', snap.active);
      recordMetric('thumbnail.queue.cached', snap.cached);
    }, 1000);

    return () => {
      queue.dispose();
      clearInterval(pollTimer);
      if (queueRef.current === queue) {
        queueRef.current = null;
      }
    };
  }, [concurrency]);

  const reset = useCallback(() => {
    queueRef.current?.reset();
  }, []);

  const enqueue = useCallback(
    (paths: string[], options?: EnqueueOptions) => {
      queueRef.current?.enqueue(paths, options);
    },
    [],
  );

  const forget = useCallback((paths: string[]) => {
    queueRef.current?.forget(paths);
  }, []);

  return { queueRef, enqueue, reset, forget };
}
