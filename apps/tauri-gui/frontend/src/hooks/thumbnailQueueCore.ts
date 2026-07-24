import type { ThumbnailDTO } from '../api/bridge';

export type EnqueueOptions = { priority?: 'front' | 'back' };

type QueueItem = { path: string; generation: number; pathVersion: number };
type ActiveItem = QueueItem & { token: number };

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
  private queuedPaths = new Set<string>();
  private inFlight = new Map<string, ActiveItem>();
  private nextActiveToken = 0;
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
      if (!path || this.cache.has(path) || this.queuedPaths.has(path)) {
        return false;
      }
      const inFlight = this.inFlight.get(path);
      if (inFlight === undefined) return true;
      return inFlight.generation !== this.generation
        || this.versionFor(path) > inFlight.pathVersion;
    });
    const items = unique.map((path) => ({
      path,
      generation: this.generation,
      pathVersion: this.versionFor(path),
    }));
    for (const item of items) {
      this.queuedPaths.add(item.path);
    }
    if (options.priority === 'front') {
      this.queue = [...items, ...this.queue];
    } else {
      this.queue.push(...items);
    }
    this.pump();
  }

  replacePending(paths: string[], options: EnqueueOptions = {}): void {
    this.queue = [];
    this.queuedPaths.clear();
    this.enqueue(paths, options);
  }

  forget(paths: string[]): void {
    const set = new Set(paths);
    for (const path of set) {
      this.cache.delete(path);
      this.queuedPaths.delete(path);
      this.pathVersions.set(path, this.versionFor(path) + 1);
    }
    this.queue = this.queue.filter((item) => !set.has(item.path));
    for (const path of set) this.cleanupPathVersion(path);
  }

  reset(): void {
    this.generation += 1;
    this.cache.clear();
    this.queue = [];
    this.queuedPaths.clear();
    // Generation invalidation is sufficient for old physical requests; no
    // per-path version metadata is needed after the logical cache reset.
    this.pathVersions.clear();
  }

  dispose(): void {
    this.disposed = true;
    this.generation += 1;
    this.queue = [];
    this.queuedPaths.clear();
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
      versioned: this.pathVersions.size,
    };
  }

  stats(): { pending: number; active: number; cached: number } {
    return {
      pending: this.queue.length,
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
      // A reset can queue a fresh generation for a path whose stale physical
      // request is still running. Skip that path until its real slot is free,
      // while allowing other paths to use any remaining capacity.
      const startableIndex = this.queue.findIndex((candidate) => (
        !this.inFlight.has(candidate.path)
      ));
      if (startableIndex < 0) break;
      const [item] = this.queue.splice(startableIndex, 1);
      if (!item) continue;

      if (this.cache.has(item.path)) {
        this.queuedPaths.delete(item.path);
        continue;
      }

      this.queuedPaths.delete(item.path);
      const active: ActiveItem = {
        ...item,
        token: ++this.nextActiveToken,
      };
      this.inFlight.set(item.path, active);
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
          if (this.inFlight.get(item.path)?.token === active.token) {
            this.inFlight.delete(item.path);
          }
          this.cleanupPathVersion(item.path);
          if (!this.disposed) this.pump();
        });
    }
  }

  private cleanupPathVersion(path: string): void {
    if (this.cache.has(path) || this.queuedPaths.has(path) || this.inFlight.has(path)) return;
    this.pathVersions.delete(path);
  }
}
