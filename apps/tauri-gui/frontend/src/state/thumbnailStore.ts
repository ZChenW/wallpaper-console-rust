import type { ThumbnailDTO } from '../api/bridge.ts';
import { ThumbnailRequestQueue, type EnqueueOptions } from '../hooks/thumbnailQueueCore.ts';
import { recordMetric } from '../perf/metrics.ts';

export const MAX_REVEAL_PER_FRAME = 12;
export const DEFAULT_THUMBNAIL_CACHE_LIMIT = 256;

/**
 * ThumbnailSession — single deep module for load + URL cache + reveal batching.
 * Queue owns concurrency/generation only (no second cache Map).
 */
export class ThumbnailSession {
  private readonly cacheLimit: number;
  private cache = new Map<string, string>();
  private failures = new Map<string, string>();
  private listeners = new Map<string, Set<() => void>>();
  private queue: ThumbnailRequestQueue;
  private enqueueScheduled = false;
  private pendingPaths: string[] = [];
  private pendingOptions?: EnqueueOptions;
  private pendingNotifyPaths = new Set<string>();
  private pausedNotifyPaths = new Set<string>();
  private notifyScheduled = false;
  private revealPaused = false;
  private scrolling = false;
  private interacting = true;

  constructor(
    concurrency: number,
    load: (path: string) => Promise<ThumbnailDTO>,
    cacheLimit = DEFAULT_THUMBNAIL_CACHE_LIMIT,
  ) {
    this.cacheLimit = Number.isFinite(cacheLimit) && cacheLimit > 0
      ? Math.max(1, Math.floor(cacheLimit))
      : DEFAULT_THUMBNAIL_CACHE_LIMIT;
    this.queue = new ThumbnailRequestQueue({
      concurrency,
      load,
      isCached: (path) => this.cache.has(path),
      onThumbnail: (path, thumbnail) => {
        // Refresh insertion order so reads and replacements implement a small
        // LRU instead of retaining base64/file URLs for the entire session.
        this.cache.delete(path);
        this.cache.set(path, thumbnail);
        this.failures.delete(path);
        this.evictUnusedCacheEntries();
        this.scheduleNotify(path);
      },
      onFailure: (path, reason) => {
        // A completed refresh failure is authoritative. Keeping the previous
        // media here would make a changed or deleted project look healthy
        // indefinitely, so replace it with the explicit failure state.
        this.cache.delete(path);
        this.failures.delete(path);
        this.failures.set(path, reason ?? 'thumbnail_failed');
        this.evictUnusedFailures();
        this.scheduleNotify(path);
      },
    });
  }

  get(path: string): string | undefined {
    const thumbnail = this.cache.get(path);
    if (thumbnail === undefined) return undefined;
    this.cache.delete(path);
    this.cache.set(path, thumbnail);
    return thumbnail;
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
        this.evictUnusedCacheEntries();
        this.evictUnusedFailures();
      }
    };
  }

  /** Report the currently visible preview asset paths (rAF-coalesced). */
  observeVisible(paths: string[], options?: EnqueueOptions): void {
    this.pendingPaths = paths.slice();
    this.pendingOptions = options;
    if (this.enqueueScheduled) return;
    this.enqueueScheduled = true;
    const flush = () => {
      this.enqueueScheduled = false;
      const unique = Array.from(new Set(this.pendingPaths));
      const opts = this.pendingOptions;
      this.pendingPaths = [];
      this.pendingOptions = undefined;
      this.queue.replacePending(unique, opts);
    };
    if (typeof requestAnimationFrame === 'function') requestAnimationFrame(flush);
    else Promise.resolve().then(flush);
  }

  /** Viewport is scrolling — Session pauses reveal until idle. */
  setScrolling(scrolling: boolean): void {
    if (this.scrolling === scrolling) return;
    this.scrolling = scrolling;
    this.syncRevealPaused();
  }

  /** Viewport / card interaction is active (Grid active, Flow interacting). */
  setInteracting(interacting: boolean): void {
    if (this.interacting === interacting) return;
    this.interacting = interacting;
    this.syncRevealPaused();
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

  refreshSubscribed(): void {
    const listenerPaths = Array.from(this.listeners.keys());
    this.queue.reset();
    for (const path of this.cache.keys()) {
      if (!this.listeners.has(path)) this.cache.delete(path);
    }
    this.failures.clear();
    if (listenerPaths.length > 0) {
      this.queue.enqueue(listenerPaths, { priority: 'front', force: true });
    }
  }

  snapshot() {
    const base = this.queue.snapshot();
    return { ...base, cached: this.cache.size };
  }

  stats(): { pending: number; active: number; cached: number; failures: number } {
    const base = this.queue.stats();
    return { ...base, cached: this.cache.size, failures: this.failures.size };
  }

  private syncRevealPaused(): void {
    const paused = !this.interacting || this.scrolling;
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

  private evictUnusedCacheEntries(): void {
    while (this.cache.size > this.cacheLimit) {
      const candidate = this.firstUnsubscribedPath(this.cache.keys());
      if (candidate === null) return;
      this.cache.delete(candidate);
      this.queue.forget([candidate]);
      this.scheduleNotify(candidate);
    }
  }

  private evictUnusedFailures(): void {
    while (this.failures.size > this.cacheLimit) {
      const candidate = this.firstUnsubscribedPath(this.failures.keys());
      if (candidate === null) return;
      this.failures.delete(candidate);
    }
  }

  private firstUnsubscribedPath(paths: Iterable<string>): string | null {
    for (const path of paths) {
      if (!this.listeners.has(path)) return path;
    }
    return null;
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

/** @deprecated Prefer ThumbnailSession — alias for gradual imports. */
export const ThumbnailStore = ThumbnailSession;
