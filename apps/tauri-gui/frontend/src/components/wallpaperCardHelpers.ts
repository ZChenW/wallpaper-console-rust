import type { WallpaperDTO } from '../api/bridge';

export function typeIcon(type: string): string {
  switch (type) {
    case 'image': return '\u{1F5BC}';
    case 'gif': return '\u{1F39E}';
    case 'video': return '\u{1F3AC}';
    case 'we_scene': return 'WE';
    case 'we_web': return 'WEB';
    default: return '\u{1F4C4}';
  }
}

export function displayName(e: WallpaperDTO): string {
  return e.title || e.workshopId || e.path.split('/').pop() || e.path;
}

/** User-facing hover text deliberately avoids leaking a long absolute path. */
export function cardHoverLabel(e: WallpaperDTO): string {
  const rich = e as WallpaperDTO & {
    readonly sources?: ReadonlyArray<{ readonly displayName?: unknown }>;
  };
  const sourceNames = rich.sources
    ?.map((source) => typeof source.displayName === 'string' ? source.displayName.trim() : '')
    .filter(Boolean)
    .join(', ');
  return sourceNames ? `${displayName(e)} · ${sourceNames}` : displayName(e);
}

export function editorialActionLabel(
  canApply: boolean,
  applyGesture: 'single' | 'double',
): string {
  if (!canApply) return 'View details';
  return applyGesture === 'double'
    ? 'Select / double-click apply'
    : 'Select / apply';
}

export function weBadge(e: WallpaperDTO): string | null {
  if (e.type === 'we_scene') {
    if (e.backendStatus === 'renderer_limitation') return 'Renderer limitation';
    if (e.backendStatus === 'failed') return 'Scene incompatible';
    return 'WE Scene';
  }
  if (e.type === 'we_web') {
    return 'Web · browse only';
  }
  if (e.type === 'unsupported') return 'Unsupported';
  return null;
}

export function weBadgeClass(e: WallpaperDTO): string {
  if (
    e.type === 'we_web' ||
    e.backendStatus === 'failed' ||
    e.backendStatus === 'renderer_limitation'
  )
    return 'wallpaper-badge wallpaper-badge-danger';
  return 'wallpaper-badge';
}

export function metaLine(e: WallpaperDTO): string {
  if (e.type === 'we_scene' || e.type === 'we_web' || e.type === 'unsupported') {
    if (e.type === 'unsupported' && e.unsupportedReason) {
      return e.unsupportedReason;
    }
    if (e.type === 'we_web') {
      return ['Web wallpaper — unsupported', e.workshopId].filter(Boolean).join(' · ');
    }
    if (e.type === 'we_scene' && (e.backendStatus === 'failed' || e.backendStatus === 'renderer_limitation')) {
      if (e.backendStatus === 'renderer_limitation') {
        return e.backendErrorMessage || 'This scene has renderer limitations with linux-wallpaperengine.';
      }
      return e.backendErrorMessage || 'This scene is not compatible with linux-wallpaperengine.';
    }
    const kind = 'Wallpaper Engine Scene';
    return [kind, e.workshopId, e.backend].filter(Boolean).join(' · ');
  }
  return `${e.resolution} · ${e.type} · ${formatSize(e.size)}`;
}

export function formatSize(bytes: number): string {
  if (bytes >= 1 << 30) return `${(bytes / (1 << 30)).toFixed(1)} GB`;
  if (bytes >= 1 << 20) return `${(bytes / (1 << 20)).toFixed(1)} MB`;
  if (bytes >= 1 << 10) return `${(bytes / (1 << 10)).toFixed(0)} KB`;
  return `${bytes} B`;
}
