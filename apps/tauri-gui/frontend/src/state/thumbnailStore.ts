import type { ThumbnailDTO } from '../api/bridge';
import { ThumbnailRequestQueue, type EnqueueOptions } from '../hooks/thumbnailQueueCore';

export class ThumbnailStore {
  private cache = new Map<string, string>();
  private listeners = new Map<string, Set<() => void>>();
  private queue: ThumbnailRequestQueue;
  private enqueueScheduled = false;
  private pendingPaths: string[] = [];
  private pendingOptions: EnqueueOptions | undefined;

  constructor(concurrency: number, load: (path: string) => Promise<ThumbnailDTO>) {
    this.queue = new ThumbnailRequestQueue({
      concurrency,
      load,
      onThumbnail: (path, thumbnail) => {
        this.cache.set(path, thumbnail);
        this.listeners.get(path)?.forEach((cb) => cb());
      },
      onFailure: (path) => {
        this.listeners.get(path)?.forEach((cb) => cb());
      },
    });
  }

  get(path: string): string | undefined {
    return this.cache.get(path);
  }

  subscribe(path: string, cb: () => void): () => void {
    if (!this.listeners.has(path)) this.listeners.set(path, new Set());
    this.listeners.get(path)!.add(cb);
    return () => {
      this.listeners.get(path)?.delete(cb);
    };
  }

  enqueueVisible(paths: string[], options?: EnqueueOptions): void {
    this.pendingPaths.push(...paths);
    if (options) this.pendingOptions = options;
    if (this.enqueueScheduled) return;
    this.enqueueScheduled = true;
    const flush = () => {
      this.enqueueScheduled = false;
      const unique = Array.from(new Set(this.pendingPaths));
      this.pendingPaths = [];
      const opts = this.pendingOptions;
      this.pendingOptions = undefined;
      this.queue.enqueue(unique, opts);
    };
    if (typeof requestAnimationFrame === 'function') requestAnimationFrame(flush);
    else Promise.resolve().then(flush);
  }

  forget(paths: string[]): void {
    this.queue.forget(paths);
  }

  reset(): void {
    this.queue.reset();
    this.cache.clear();
    this.listeners.clear();
  }

  snapshot() {
    return this.queue.snapshot();
  }
}
