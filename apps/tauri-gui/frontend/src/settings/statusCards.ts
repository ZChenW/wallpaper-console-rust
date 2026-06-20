import type {
  LibrarySourceStatusDTO,
  LinuxWallpaperEngineStatusDTO,
  ThumbnailCacheDTO,
} from '../api/bridge.ts';

export type StatusCardTone = 'neutral' | 'success' | 'warning' | 'danger';

export interface StatusCardView {
  value: string;
  detail?: string;
  tone: StatusCardTone;
}

export function resolveDatabaseStatusCard(
  libraryStatus: LibrarySourceStatusDTO | null,
  error: string | null,
  loading: boolean,
): StatusCardView {
  if (loading) {
    return { value: 'Checking...', tone: 'neutral' };
  }
  if (error) {
    return { value: 'Unavailable', detail: error, tone: 'warning' };
  }
  if (libraryStatus) {
    return {
      value: `${libraryStatus.sqliteRows} wallpapers indexed`,
      tone: 'neutral',
    };
  }
  return { value: 'Checking...', tone: 'neutral' };
}

export function resolveWeStatusCard(
  weStatus: LinuxWallpaperEngineStatusDTO | null,
  error: string | null,
  loading: boolean,
): StatusCardView {
  if (loading) {
    return { value: 'Checking...', tone: 'neutral' };
  }
  if (error) {
    return { value: 'Unavailable', detail: error, tone: 'warning' };
  }
  if (weStatus?.available) {
    return {
      value: weStatus.path ? `Ready — ${weStatus.path}` : 'Ready',
      detail: weStatus.message || undefined,
      tone: 'success',
    };
  }
  if (weStatus) {
    return {
      value: 'Missing',
      detail: weStatus.message || weStatus.detail || undefined,
      tone: 'warning',
    };
  }
  return { value: 'Checking...', tone: 'neutral' };
}

export function resolveThumbnailStatusCard(
  thumbCache: ThumbnailCacheDTO | null,
  error: string | null,
  loading: boolean,
  options?: { cleanupDays?: number },
): StatusCardView {
  if (loading) {
    return { value: 'Checking...', tone: 'neutral' };
  }
  if (error) {
    return { value: 'Unavailable', detail: error, tone: 'warning' };
  }
  if (thumbCache) {
    const failures = thumbCache.failureEntries > 0 ? ` · ${thumbCache.failureEntries} failed` : '';
    const detail = options?.cleanupDays != null
      ? `Cleanup: older than ${options.cleanupDays} days`
      : undefined;
    return {
      value: `${thumbCache.entries} thumbnails, ${thumbCache.size}${failures}`,
      detail,
      tone: 'neutral',
    };
  }
  return { value: 'Checking...', tone: 'neutral' };
}
