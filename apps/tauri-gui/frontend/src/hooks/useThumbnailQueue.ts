import { useCallback, useEffect, useRef, useState } from 'react';
import { api, ThumbnailDTO } from '../api/bridge';

type ThumbState = Record<string, string>;

interface QueueItem {
  path: string;
  generation: number;
}

export function useThumbnailQueue(concurrency = 2) {
  const [thumbs, setThumbs] = useState<ThumbState>({});
  const thumbsRef = useRef<ThumbState>({});
  const queue = useRef<QueueItem[]>([]);
  const active = useRef(0);
  const generation = useRef(0);
  const failed = useRef(new Set<string>());
  const pending = useRef(new Set<string>());
  const buffered = useRef<ThumbState>({});
  const flushTimer = useRef<number | null>(null);

  const flush = useCallback(() => {
    const next = buffered.current;
    buffered.current = {};
    if (Object.keys(next).length > 0) {
      setThumbs((prev) => {
        const merged = { ...prev, ...next };
        thumbsRef.current = merged;
        return merged;
      });
    }
    flushTimer.current = null;
  }, []);

  const scheduleFlush = useCallback(() => {
    if (flushTimer.current !== null) return;
    flushTimer.current = window.setTimeout(flush, 50);
  }, [flush]);

  const pump = useCallback(() => {
    while (active.current < concurrency && queue.current.length > 0) {
      const item = queue.current.shift()!;
      if (item.generation !== generation.current) continue;
      if (failed.current.has(item.path)) continue;
      active.current += 1;
      api.thumbnailFor(item.path)
        .then((result: ThumbnailDTO) => {
          if (item.generation !== generation.current) return;
          if (result.thumbnail) {
            buffered.current[item.path] = result.thumbnail;
            scheduleFlush();
          } else {
            failed.current.add(item.path);
          }
        })
        .catch(() => failed.current.add(item.path))
        .finally(() => {
          pending.current.delete(item.path);
          active.current -= 1;
          pump();
        });
    }
  }, [concurrency, scheduleFlush]);

  const reset = useCallback(() => {
    generation.current += 1;
    queue.current = [];
    active.current = 0;
    pending.current.clear();
    failed.current.clear();
    buffered.current = {};
    thumbsRef.current = {};
    setThumbs({});
  }, []);

  const enqueue = useCallback(
    (paths: string[]) => {
      const gen = generation.current;
      for (const path of paths) {
        if (thumbsRef.current[path] || pending.current.has(path) || failed.current.has(path)) {
          continue;
        }
        pending.current.add(path);
        queue.current.push({ path, generation: gen });
      }
      pump();
    },
    [pump],
  );

  useEffect(() => {
    return () => {
      if (flushTimer.current !== null) {
        window.clearTimeout(flushTimer.current);
      }
    };
  }, []);

  return { thumbs, enqueue, reset };
}
