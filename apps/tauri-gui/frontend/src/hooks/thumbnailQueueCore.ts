import type { ThumbnailDTO } from '../api/bridge';

export type ThumbState = Record<string, string>;
export type EnqueueOptions = { priority?: 'front' | 'back' };

type QueueItem = { path: string };

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
  private inFlight = new Set<string>();
  private disposed = false;

  constructor(options: QueueOptions) {
    this.concurrency = Math.max(1, options.concurrency);
    this.load = options.load;
    this.onUpdate = options.onUpdate;
  }

  enqueue(paths: string[], options: EnqueueOptions = {}): void {
    const unique = Array.from(new Set(paths)).filter((path) =>
      path && !this.thumbs[path] && !this.inFlight.has(path) && !this.queue.some((item) => item.path === path)
    );
    const items = unique.map((path) => ({ path }));
    if (options.priority === 'front') {
      this.queue = [...items, ...this.queue];
    } else {
      this.queue.push(...items);
    }
    this.pump();
  }

  forget(paths: string[]): void {
    const set = new Set(paths);
    for (const path of set) delete this.thumbs[path];
    this.queue = this.queue.filter((item) => !set.has(item.path));
    this.emit();
  }

  reset(): void {
    this.thumbs = {};
    this.queue = [];
    this.inFlight.clear();
    this.emit();
  }

  dispose(): void {
    this.disposed = true;
    this.queue = [];
  }

  snapshot() {
    return { pending: this.queue.map((item) => item.path), active: this.inFlight.size };
  }

  private pump(): void {
    if (this.disposed) return;
    while (this.inFlight.size < this.concurrency && this.queue.length > 0) {
      const item = this.queue.shift();
      if (!item || this.thumbs[item.path] || this.inFlight.has(item.path)) continue;
      this.inFlight.add(item.path);
      void this.load(item.path)
        .then((thumb) => {
          if (thumb.thumbnail) {
            this.thumbs = { ...this.thumbs, [item.path]: thumb.thumbnail };
          }
          this.emit();
        })
        .catch(() => {
          this.thumbs = { ...this.thumbs };
          this.emit();
        })
        .finally(() => {
          this.inFlight.delete(item.path);
          this.pump();
        });
    }
  }

  private emit(): void {
    if (!this.disposed) this.onUpdate({ ...this.thumbs });
  }
}
