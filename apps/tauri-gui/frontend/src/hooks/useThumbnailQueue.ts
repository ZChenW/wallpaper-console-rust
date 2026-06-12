import { useCallback, useEffect, useRef, useState } from 'react';
import { api } from '../api/bridge';
import { ThumbnailRequestQueue, type EnqueueOptions, type ThumbState } from './thumbnailQueueCore';
import { recordMetric } from '../perf/metrics';

export function useThumbnailQueue(concurrency = 2) {
  const [thumbs, setThumbs] = useState<ThumbState>({});
  const queueRef = useRef<ThumbnailRequestQueue | null>(null);

  useEffect(() => {
    const queue = new ThumbnailRequestQueue({
      concurrency,
      load: async (path) => {
        const r = await api.thumbnailFor(path);
        recordMetric(r.cacheHit ? 'thumbnail.cache.hit' : 'thumbnail.cache.miss', 1);
        return r;
      },
      onUpdate: setThumbs,
    });
    queueRef.current = queue;

    const pollTimer = setInterval(() => {
      const snap = queue.snapshot();
      recordMetric('thumbnail.queue.pending', snap.pending.length);
      recordMetric('thumbnail.queue.inFlight', snap.active);
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
    setThumbs({});
  }, []);

  const enqueue = useCallback(
    (paths: string[], options?: EnqueueOptions) => {
      queueRef.current?.enqueue(paths, options);
    },
    [],
  );

  return { thumbs, enqueue, reset };
}
