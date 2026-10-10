import type { ThumbnailDTO } from '../api/bridge.ts';
import { ThumbnailRequestQueue, type EnqueueOptions } from '../hooks/thumbnailQueueCore.ts';
import { recordMetric } from '../perf/metrics.ts';
import { LARGE_PREVIEW_PREFIX } from '../components/wallpaperPreviewMedia.ts';

export const MAX_REVEAL_PER_FRAME = 12;
export const DEFAULT_THUMBNAIL_CACHE_LIMIT = 256;
export const LARGE_PREVIEW_CONCURRENCY = 3;
export const LARGE_PREVIEW_RETRY_LIMIT = 2;
export const LARGE_PREVIEW_RETRY_DELAY_MS = 5_000;

type LargeRetry = { retries: number; nextAttemptAt: number; timer?: ReturnType<typeof setTimeout> };

/**
 * ThumbnailSession — single deep module for load + URL cache + reveal batching.
 * Queue owns concurrency/generation only (no second cache Map).
 */
export class ThumbnailSession {
  private readonly cacheLimit: number;
  private cache = new Map<string, string>();
  private failures = new Map<string, string>();
  private listeners = new Map<string, Set<() => void>>();
  private failureListeners = new Set<() => void>();
  private failureNotifyPending = false;
  private failureNotifyScheduled = false;
  private queue: ThumbnailRequestQueue;
  private largeQueue: ThumbnailRequestQueue;
  private largeRetries = new Map<string, LargeRetry>();
  private visiblePaths = new Set<string>();
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
    const callbacks = {
      load: (path: string) => {
        const retry = this.largeRetries.get(path);
        if (retry) {
          if (retry.timer !== undefined) clearTimeout(retry.timer);
          retry.timer = undefined;
          retry.retries += 1;
        }
        return load(path);
      },
      isCached: (path: string) => this.cache.has(path),
      onThumbnail: (path: string, thumbnail: string) => {
        const previousFailureCount = this.failures.size;
        // Refresh insertion order so reads and replacements implement a small
        // LRU instead of retaining base64/file URLs for the entire session.
        this.cache.delete(path);
        this.cache.set(path, thumbnail);
        this.failures.delete(path);
        this.clearLargeRetry(path);
        this.evictUnusedCacheEntries();
        this.scheduleNotify(path);
        if (this.failures.size !== previousFailureCount) this.scheduleFailureNotify();
      },
      onFailure: (path: string, reason?: string) => {
        const previousFailureCount = this.failures.size;
        // A completed refresh failure is authoritative. Keeping the previous
        // media here would make a changed or deleted project look healthy
        // indefinitely, so replace it with the explicit failure state.
        this.cache.delete(path);
        this.failures.delete(path);
        this.failures.set(path, reason ?? 'thumbnail_failed');
        if (path.startsWith(LARGE_PREVIEW_PREFIX)) {
          const retry = this.largeRetries.get(path) ?? { retries: 0, nextAttemptAt: 0 };
          retry.nextAttemptAt = Date.now() + LARGE_PREVIEW_RETRY_DELAY_MS;
          this.largeRetries.set(path, retry);
          this.scheduleLargeRetry(path);
        }
        this.evictUnusedFailures();
        this.scheduleNotify(path);
        if (this.failures.size !== previousFailureCount) this.scheduleFailureNotify();
      },
    };
    this.queue = new ThumbnailRequestQueue({ ...callbacks, concurrency });
    // A waiting Large command must not consume the Grid's four frontend slots.
    this.largeQueue = new ThumbnailRequestQueue({ ...callbacks, concurrency: LARGE_PREVIEW_CONCURRENCY });
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

  /**
   * Failures the user should hear about. A large preview is an upgrade over the grid-size one
   * that is already showing, so a bounded retry can end quietly with the smaller picture.
   */
  failureCount(): number {
    let count = 0;
    for (const key of this.failures.keys()) if (!key.startsWith(LARGE_PREVIEW_PREFIX)) count += 1;
    return count;
  }

  listenerPathCount(): number {
    return this.listeners.size;
  }

  subscribeFailures(cb: () => void): () => void {
    this.failureListeners.add(cb);
    return () => this.failureListeners.delete(cb);
  }

  subscribe(path: string, cb: () => void): () => void {
    let listeners = this.listeners.get(path);
    if (!listeners) {
      listeners = new Set();
      this.listeners.set(path, listeners);
    }
    listeners.add(cb);
    this.scheduleLargeRetry(path);
    return () => {
      listeners.delete(cb);
      if (listeners.size === 0 && this.listeners.get(path) === listeners) {
        this.listeners.delete(path);
        const retry = this.largeRetries.get(path);
        if (retry?.timer !== undefined && !this.isLargeRequested(path)) {
          clearTimeout(retry.timer);
          retry.timer = undefined;
        }
        this.evictUnusedCacheEntries();
        if (this.evictUnusedFailures()) this.scheduleFailureNotify();
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
      this.visiblePaths = new Set(unique);
      for (const [path, retry] of this.largeRetries) {
        if (!this.isLargeRequested(path) && retry.timer !== undefined) {
          clearTimeout(retry.timer);
          retry.timer = undefined;
        }
      }
      this.queue.replacePending(unique.filter((path) => !path.startsWith(LARGE_PREVIEW_PREFIX)), opts);
      this.largeQueue.replacePending(unique.filter((path) => (
        path.startsWith(LARGE_PREVIEW_PREFIX) && this.canLoadLarge(path)
      )), opts);
      for (const path of unique) this.scheduleLargeRetry(path);
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

  /** Whether the viewport is moving right now; pictures arriving meanwhile skip their fade-in. */
  isScrolling(): boolean { return this.scrolling; }

  /** Viewport / card interaction is active (Grid active, Flow interacting). */
  setInteracting(interacting: boolean): void {
    if (this.interacting === interacting) return;
    this.interacting = interacting;
    this.syncRevealPaused();
  }

  forget(paths: string[]): void {
    const previousFailureCount = this.failures.size;
    this.queue.forget(paths);
    this.largeQueue.forget(paths);
    for (const path of paths) {
      this.cache.delete(path);
      this.failures.delete(path);
      this.clearLargeRetry(path);
      this.scheduleNotify(path);
    }
    if (this.failures.size !== previousFailureCount) this.scheduleFailureNotify();
  }

  reset(): void {
    const listenerPaths = Array.from(this.listeners.keys());
    const previousFailureCount = this.failures.size;
    this.queue.reset();
    this.largeQueue.reset();
    this.clearLargeRetries();
    this.visiblePaths.clear();
    this.cache.clear();
    this.failures.clear();
    this.pendingNotifyPaths.clear();
    this.pausedNotifyPaths.clear();
    this.notifyScheduled = false;
    for (const path of listenerPaths) {
      this.scheduleNotify(path);
    }
    if (previousFailureCount > 0) this.scheduleFailureNotify();
  }

  refreshSubscribed(): void {
    const listenerPaths = Array.from(this.listeners.keys());
    const previousFailureCount = this.failures.size;
    this.queue.reset();
    this.largeQueue.reset();
    this.clearLargeRetries();
    for (const path of this.cache.keys()) {
      if (!this.listeners.has(path)) this.cache.delete(path);
    }
    this.failures.clear();
    if (previousFailureCount > 0) this.scheduleFailureNotify();
    if (listenerPaths.length > 0) {
      this.enqueue(listenerPaths, { priority: 'front', force: true });
    }
  }

  retryFailures(): void {
    const paths = Array.from(this.failures.keys());
    if (paths.length === 0) return;
    this.failures.clear();
    this.clearLargeRetries();
    this.scheduleFailureNotify();
    this.enqueue(paths, { priority: 'front', force: true });
  }

  snapshot() {
    const base = this.queue.snapshot();
    const large = this.largeQueue.snapshot();
    return {
      pending: [...base.pending, ...large.pending],
      active: base.active + large.active,
      versioned: base.versioned + large.versioned,
      cached: this.cache.size,
    };
  }

  stats(): { pending: number; active: number; cached: number; failures: number } {
    const base = this.queue.stats();
    const large = this.largeQueue.stats();
    return { pending: base.pending + large.pending, active: base.active + large.active, cached: this.cache.size, failures: this.failures.size };
  }

  private enqueue(paths: string[], options?: EnqueueOptions): void {
    this.queue.enqueue(paths.filter((path) => !path.startsWith(LARGE_PREVIEW_PREFIX)), options);
    this.largeQueue.enqueue(paths.filter((path) => path.startsWith(LARGE_PREVIEW_PREFIX) && this.canLoadLarge(path)), options);
  }

  private canLoadLarge(path: string): boolean {
    const retry = this.largeRetries.get(path);
    return !retry || (retry.retries < LARGE_PREVIEW_RETRY_LIMIT && Date.now() >= retry.nextAttemptAt);
  }

  private isLargeRequested(path: string): boolean {
    return this.visiblePaths.has(path) || this.listeners.has(path);
  }

  private scheduleLargeRetry(path: string): void {
    const retry = this.largeRetries.get(path);
    if (!retry || retry.timer !== undefined || retry.retries >= LARGE_PREVIEW_RETRY_LIMIT || !this.isLargeRequested(path)) return;
    retry.timer = setTimeout(() => {
      retry.timer = undefined;
      if (this.largeRetries.get(path) !== retry || !this.isLargeRequested(path) || !this.canLoadLarge(path)) return;
      this.largeQueue.enqueue([path]);
    }, Math.max(0, retry.nextAttemptAt - Date.now()));
  }

  private clearLargeRetry(path: string): void {
    const retry = this.largeRetries.get(path);
    if (retry?.timer !== undefined) clearTimeout(retry.timer);
    this.largeRetries.delete(path);
  }

  private clearLargeRetries(): void {
    for (const path of this.largeRetries.keys()) this.clearLargeRetry(path);
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
      if (this.failureNotifyPending) {
        this.scheduleFailureNotify();
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
      this.largeQueue.forget([candidate]);
      this.scheduleNotify(candidate);
    }
  }

  private evictUnusedFailures(): boolean {
    let changed = false;
    while (this.failures.size > this.cacheLimit) {
      const candidate = this.firstUnsubscribedPath(this.failures.keys());
      if (candidate === null) return changed;
      this.failures.delete(candidate);
      this.clearLargeRetry(candidate);
      changed = true;
    }
    return changed;
  }

  private scheduleFailureNotify(): void {
    this.failureNotifyPending = true;
    if (this.revealPaused || this.failureNotifyScheduled) return;
    this.failureNotifyScheduled = true;
    const flush = () => {
      this.failureNotifyScheduled = false;
      if (this.revealPaused || !this.failureNotifyPending) return;
      this.failureNotifyPending = false;
      this.failureListeners.forEach((listener) => listener());
    };
    if (typeof requestAnimationFrame === 'function') requestAnimationFrame(flush);
    else setTimeout(flush, 0);
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
