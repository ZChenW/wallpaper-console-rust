import type { ThumbnailDTO } from '../api/bridge';

export type EnqueueOptions = { priority?: 'front' | 'back' };

type QueueItem = { path: string; generation: number; pathVersion: number };

interface QueueOptions {
  concurrency: number;
  load: (path: string) => Promise<ThumbnailDTO>;
  onThumbnail: (path: string, thumbnail: string) => void;
  onFailure: (path: string, reason?: string) => void;
}

export class ThumbnailRequestQueue {
  private readonly concurrency: number;
  private readonly load: (path: string) => Promise<ThumbnailDTO>;
  private readonly onThumbnail: (path: string, thumbnail: string) => void;
  private readonly onFailure: (path: string, reason?: string) => void;
  private cache = new Map<string, string>();
  private queue: QueueItem[] = [];
  private inFlight = new Map<string, number>();
  private disposed = false;
  private generation = 0;
  private pathVersions = new Map<string, number>();

  constructor(options: QueueOptions) {
    this.concurrency = Math.max(1, options.concurrency);
    this.load = options.load;
    this.onThumbnail = options.onThumbnail;
    this.onFailure = options.onFailure;
  }

  enqueue(paths: string[], options: EnqueueOptions = {}): void {
    const unique = Array.from(new Set(paths)).filter((path) => {
      if (!path || this.cache.has(path) || this.queue.some((item) => item.path === path)) {
        return false;
      }
      const inFlightVersion = this.inFlight.get(path);
      if (inFlightVersion === undefined) return true;
      return this.versionFor(path) > inFlightVersion;
    });
    const items = unique.map((path) => ({
      path,
      generation: this.generation,
      pathVersion: this.versionFor(path),
    }));
    if (options.priority === 'front') {
      this.queue = [...items, ...this.queue];
    } else {
      this.queue.push(...items);
    }
    this.pump();
  }

  forget(paths: string[]): void {
    const set = new Set(paths);
    for (const path of set) {
      this.cache.delete(path);
      this.pathVersions.set(path, this.versionFor(path) + 1);
    }
    this.queue = this.queue.filter((item) => !set.has(item.path));
  }

  reset(): void {
    this.generation += 1;
    this.cache.clear();
    this.queue = [];
    this.inFlight.clear();
  }

  dispose(): void {
    this.disposed = true;
    this.generation += 1;
    this.queue = [];
    this.inFlight.clear();
  }

  get(path: string): string | undefined {
    return this.cache.get(path);
  }

  snapshot() {
    return {
      pending: this.queue.map((item) => item.path),
      active: this.inFlight.size,
      cached: this.cache.size,
    };
  }

  private versionFor(path: string): number {
    return this.pathVersions.get(path) ?? 0;
  }

  private pump(): void {
    if (this.disposed) return;
    while (this.inFlight.size < this.concurrency && this.queue.length > 0) {
      const item = this.queue.shift();
      if (!item || this.cache.has(item.path)) continue;

      if (this.inFlight.has(item.path)) {
        this.queue.unshift(item);
        break;
      }
      this.inFlight.set(item.path, item.pathVersion);
      void this.load(item.path)
        .then((thumb) => {
          if (
            !this.disposed &&
            item.generation === this.generation &&
            item.pathVersion === this.versionFor(item.path) &&
            thumb.thumbnail
          ) {
            this.cache.set(item.path, thumb.thumbnail);
            this.onThumbnail(item.path, thumb.thumbnail);
          } else if (!this.disposed && item.generation === this.generation && item.pathVersion === this.versionFor(item.path)) {
            this.onFailure(item.path, thumb.failureReason);
          }
        })
        .catch((err) => {
          if (!this.disposed && item.generation === this.generation && item.pathVersion === this.versionFor(item.path)) {
            this.onFailure(item.path, String(err));
          }
        })
        .finally(() => {
          this.inFlight.delete(item.path);
          if (!this.disposed) this.pump();
        });
    }
  }
}
