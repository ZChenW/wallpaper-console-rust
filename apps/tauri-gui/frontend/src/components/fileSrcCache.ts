export const DEFAULT_FILE_SRC_CACHE_MAX = 2000;

export class BoundedFileSrcCache {
  private readonly max: number;
  private readonly resolve: (path: string) => string;
  private readonly cache = new Map<string, string>();

  constructor(resolve: (path: string) => string, max = DEFAULT_FILE_SRC_CACHE_MAX) {
    this.resolve = resolve;
    this.max = Math.max(1, max);
  }

  get(path: string): string {
    const existing = this.cache.get(path);
    if (existing !== undefined) {
      this.cache.delete(path);
      this.cache.set(path, existing);
      return existing;
    }
    const value = this.resolve(path);
    this.cache.set(path, value);
    if (this.cache.size > this.max) {
      const oldest = this.cache.keys().next().value;
      if (oldest !== undefined) this.cache.delete(oldest);
    }
    return value;
  }

  clear(): void {
    this.cache.clear();
  }

  get size(): number {
    return this.cache.size;
  }
}
