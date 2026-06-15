import type { ThumbnailDTO } from '../api/bridge';

export type ThumbState = Record<string, string>;
export type EnqueueOptions = { priority?: 'front' | 'back' };

type QueueItem = { path: string; generation: number; pathVersion: number };

interface QueueOptions {
  concurrency: number;
  load: (path: string) => Promise<ThumbnailDTO>;
  onUpdate: (state: ThumbState) => void;
}

export class ThumbnailRequestQueue {
  private readonly concurrency: number;
  private readonly load: (path: string) => Promise<ThumbnailDTO>;
  private readonly onUpdate: (state: ThumbState) => void;
  private thumbs: ThumbState = {};
  private queue: QueueItem[] = [];
  private inFlight = new Map<string, number>();
  private disposed = false;
  private generation = 0;
  private pathVersions = new Map<string, number>();

  constructor(options: QueueOptions) {
    this.concurrency = Math.max(1, options.concurrency);
    this.load = options.load;
    this.onUpdate = options.onUpdate;
  }

  enqueue(paths: string[], options: EnqueueOptions = {}): void {
    const unique = Array.from(new Set(paths)).filter((path) => {
      if (!path || this.thumbs[path] || this.queue.some((item) => item.path === path)) {
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
      delete this.thumbs[path];
      this.pathVersions.set(path, this.versionFor(path) + 1);
    }
    this.queue = this.queue.filter((item) => !set.has(item.path));
    this.emit();
  }

  reset(): void {
    this.generation += 1;
    this.thumbs = {};
    this.queue = [];
    this.inFlight.clear();
    this.emit();
  }

  dispose(): void {
    this.disposed = true;
    this.generation += 1;
    this.queue = [];
    this.inFlight.clear();
  }

  snapshot() {
    return { pending: this.queue.map((item) => item.path), active: this.inFlight.size };
  }

  private versionFor(path: string): number {
    return this.pathVersions.get(path) ?? 0;
  }

  private pump(): void {
    if (this.disposed) return;
    while (this.inFlight.size < this.concurrency && this.queue.length > 0) {
      const item = this.queue.shift();
      if (!item || this.thumbs[item.path]) continue;

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
            this.thumbs = { ...this.thumbs, [item.path]: thumb.thumbnail };
          }
          this.emit();
        })
        .catch(() => {
          this.emit();
        })
        .finally(() => {
          this.inFlight.delete(item.path);
          if (!this.disposed) this.pump();
        });
    }
  }

  private emit(): void {
    if (!this.disposed) this.onUpdate({ ...this.thumbs });
  }
}
