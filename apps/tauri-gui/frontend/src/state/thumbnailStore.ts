import type { ThumbnailDTO } from '../api/bridge.ts';
import { ThumbnailRequestQueue, type EnqueueOptions } from '../hooks/thumbnailQueueCore.ts';
import { recordMetric } from '../perf/metrics.ts';

export const MAX_REVEAL_PER_FRAME = 12;

export class ThumbnailStore {
  private cache = new Map<string, string>();
  private failures = new Map<string, string>();
  private listeners = new Map<string, Set<() => void>>();
  private queue: ThumbnailRequestQueue;
  private enqueueScheduled = false;
  private pendingPaths: string[] = [];
  private pendingNotifyPaths = new Set<string>();
  private pausedNotifyPaths = new Set<string>();
  private notifyScheduled = false;
  private revealPaused = false;

  constructor(concurrency: number, load: (path: string) => Promise<ThumbnailDTO>) {
    this.queue = new ThumbnailRequestQueue({
      concurrency,
      load,
      onThumbnail: (path, thumbnail) => {
        this.cache.set(path, thumbnail);
        this.failures.delete(path);
        this.scheduleNotify(path);
      },
      onFailure: (path, reason) => {
        this.failures.set(path, reason ?? 'thumbnail_failed');
        this.scheduleNotify(path);
      },
    });
  }

  get(path: string): string | undefined {
    return this.cache.get(path);
  }

  getFailure(path: string): string | undefined {
    return this.failures.get(path);
  }

  failureCount(): number {
    return this.failures.size;
  }

  listenerPathCount(): number {
    return this.listeners.size;
  }

  subscribe(path: string, cb: () => void): () => void {
    let listeners = this.listeners.get(path);
    if (!listeners) {
      listeners = new Set();
      this.listeners.set(path, listeners);
    }
    listeners.add(cb);
    return () => {
      listeners.delete(cb);
      if (listeners.size === 0 && this.listeners.get(path) === listeners) {
        this.listeners.delete(path);
      }
    };
  }

  enqueueVisible(paths: string[], _options?: EnqueueOptions): void {
    this.pendingPaths = paths.slice();
    if (this.enqueueScheduled) return;
    this.enqueueScheduled = true;
    const flush = () => {
      this.enqueueScheduled = false;
      const unique = Array.from(new Set(this.pendingPaths));
      this.pendingPaths = [];
      this.queue.replacePending(unique);
    };
    if (typeof requestAnimationFrame === 'function') requestAnimationFrame(flush);
    else Promise.resolve().then(flush);
  }

  forget(paths: string[]): void {
    this.queue.forget(paths);
    for (const path of paths) {
      this.cache.delete(path);
      this.failures.delete(path);
      this.scheduleNotify(path);
    }
  }

  reset(): void {
    const listenerPaths = Array.from(this.listeners.keys());
    this.queue.reset();
    this.cache.clear();
    this.failures.clear();
    this.pendingNotifyPaths.clear();
    this.pausedNotifyPaths.clear();
    this.notifyScheduled = false;
    for (const path of listenerPaths) {
      this.scheduleNotify(path);
    }
  }

  snapshot() {
    return this.queue.snapshot();
  }

  stats(): { pending: number; active: number; cached: number; failures: number } {
    const base = this.queue.stats();
    return { ...base, cached: this.cache.size, failures: this.failures.size };
  }

  setRevealPaused(paused: boolean): void {
    if (this.revealPaused === paused) return;
    this.revealPaused = paused;
    recordMetric('thumbnail.reveal.paused', paused ? 1 : 0);
    if (!paused) {
      for (const path of this.pausedNotifyPaths) {
        this.pendingNotifyPaths.add(path);
      }
      this.pausedNotifyPaths.clear();
      if (this.pendingNotifyPaths.size > 0) {
        this.scheduleNotifyFlush();
      }
    }
    this.recordRevealPending();
  }

  private scheduleNotify(path: string): void {
    if (this.revealPaused) {
      this.pausedNotifyPaths.add(path);
      return;
    }
    this.pendingNotifyPaths.add(path);
    this.scheduleNotifyFlush();
  }

  private recordRevealPending(): void {
    recordMetric('thumbnail.reveal.pending', this.pendingNotifyPaths.size + this.pausedNotifyPaths.size);
  }

  private movePathsToPaused(paths: Iterable<string>): void {
    for (const path of paths) {
      this.pausedNotifyPaths.add(path);
    }
  }

  private scheduleNotifyFlush(): void {
    if (this.notifyScheduled) return;
    this.notifyScheduled = true;
    const flush = () => {
      this.notifyScheduled = false;

      if (this.revealPaused) {
        this.movePathsToPaused(this.pendingNotifyPaths);
        this.pendingNotifyPaths.clear();
        this.recordRevealPending();
        return;
      }

      const allPaths = Array.from(this.pendingNotifyPaths);
      this.pendingNotifyPaths.clear();
      const batch = allPaths.slice(0, MAX_REVEAL_PER_FRAME);
      const remaining = allPaths.slice(MAX_REVEAL_PER_FRAME);

      for (const path of remaining) {
        this.pendingNotifyPaths.add(path);
      }

      for (const p of batch) {
        this.listeners.get(p)?.forEach((cb) => cb());
      }

      recordMetric('thumbnail.reveal.batchSize', batch.length);
      this.recordRevealPending();

      if (this.pendingNotifyPaths.size > 0) {
        this.scheduleNotifyFlush();
      }
    };
    if (typeof requestAnimationFrame === 'function') requestAnimationFrame(flush);
    else Promise.resolve().then(flush);
  }
}
