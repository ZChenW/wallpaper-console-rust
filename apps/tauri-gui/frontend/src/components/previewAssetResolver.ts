export const DEFAULT_PREVIEW_ASSET_CACHE_MAX = 64;

export class PreviewAssetResolver {
  private readonly entries = new Map<string, Promise<string>>();
  private readonly authorize: (path: string, wallpaperPath: string) => Promise<string>;
  private readonly maxEntries: number;

  constructor(
    authorize: (path: string, wallpaperPath: string) => Promise<string>,
    maxEntries = DEFAULT_PREVIEW_ASSET_CACHE_MAX,
  ) {
    if (!Number.isSafeInteger(maxEntries) || maxEntries <= 0) {
      throw new Error('PreviewAssetResolver maxEntries must be a positive integer');
    }
    this.authorize = authorize;
    this.maxEntries = maxEntries;
  }

  resolve(path: string, wallpaperPath = path): Promise<string> {
    const cached = this.entries.get(path);
    if (cached) {
      this.entries.delete(path);
      this.entries.set(path, cached);
      return cached;
    }

    const pending = this.authorize(path, wallpaperPath).catch((error: unknown) => {
      this.entries.delete(path);
      throw error;
    });
    this.entries.set(path, pending);
    while (this.entries.size > this.maxEntries) {
      const oldest = this.entries.keys().next().value as string | undefined;
      if (oldest === undefined) break;
      this.entries.delete(oldest);
    }
    return pending;
  }

  clear(): void {
    this.entries.clear();
  }
}
