import type {
  LibrarySourceStatusDTO,
  LinuxWallpaperEngineStatusDTO,
  ThumbnailCacheDTO,
  WeDebugInfoDTO,
} from '../api/bridge.ts';

export interface SettingsStatusLoaders {
  librarySourceStatus?: () => Promise<LibrarySourceStatusDTO>;
  linuxWallpaperEngineStatus?: () => Promise<LinuxWallpaperEngineStatusDTO>;
  thumbnailCacheStatus?: () => Promise<ThumbnailCacheDTO>;
  weDebugInfo?: () => Promise<WeDebugInfoDTO>;
}

export interface SettingsStatusSnapshot {
  libraryStatus: LibrarySourceStatusDTO | null;
  libraryError: string | null;
  weStatus: LinuxWallpaperEngineStatusDTO | null;
  weError: string | null;
  thumbCache: ThumbnailCacheDTO | null;
  thumbError: string | null;
  weDebugInfo: WeDebugInfoDTO | null;
  weDebugError: string | null;
  loaded: {
    library: boolean;
    we: boolean;
    thumb: boolean;
    debug: boolean;
  };
}

function formatRejection(reason: unknown): string {
  if (reason instanceof Error) return reason.message;
  return String(reason);
}

function runLoader<T>(loader: () => Promise<T> | T): Promise<T> {
  return Promise.resolve().then(() => loader());
}

export function createSettingsStatusRequestSeq() {
  let latest = 0;
  return {
    begin(): number {
      latest += 1;
      return latest;
    },
    isLatest(requestId: number): boolean {
      return requestId === latest;
    },
    latest(): number {
      return latest;
    },
  };
}

export function shouldApplySettingsStatusSnapshot(requestId: number, latestRequestId: number): boolean {
  return requestId === latestRequestId;
}

export async function refreshSettingsStatusCore(
  loaders: SettingsStatusLoaders,
): Promise<SettingsStatusSnapshot> {
  const loaded = {
    library: loaders.librarySourceStatus !== undefined,
    we: loaders.linuxWallpaperEngineStatus !== undefined,
    thumb: loaders.thumbnailCacheStatus !== undefined,
    debug: loaders.weDebugInfo !== undefined,
  };

  const [library, we, thumb, debug] = await Promise.allSettled([
    loaded.library
      ? runLoader(loaders.librarySourceStatus!)
      : Promise.resolve(null as LibrarySourceStatusDTO | null),
    loaded.we
      ? runLoader(loaders.linuxWallpaperEngineStatus!)
      : Promise.resolve(null as LinuxWallpaperEngineStatusDTO | null),
    loaded.thumb
      ? runLoader(loaders.thumbnailCacheStatus!)
      : Promise.resolve(null as ThumbnailCacheDTO | null),
    loaded.debug
      ? runLoader(loaders.weDebugInfo!)
      : Promise.resolve(null as WeDebugInfoDTO | null),
  ]);

  return {
    libraryStatus: library.status === 'fulfilled' ? library.value : null,
    libraryError: loaded.library && library.status === 'rejected' ? formatRejection(library.reason) : null,
    weStatus: we.status === 'fulfilled' ? we.value : null,
    weError: loaded.we && we.status === 'rejected' ? formatRejection(we.reason) : null,
    thumbCache: thumb.status === 'fulfilled' ? thumb.value : null,
    thumbError: loaded.thumb && thumb.status === 'rejected' ? formatRejection(thumb.reason) : null,
    weDebugInfo: debug.status === 'fulfilled' ? debug.value : null,
    weDebugError: loaded.debug && debug.status === 'rejected' ? formatRejection(debug.reason) : null,
    loaded,
  };
}
