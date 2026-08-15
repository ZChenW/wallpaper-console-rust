import { createElement, type ReactNode } from 'react';
import {
  FileQuestion,
  FileVideo,
  Film,
  Image as ImageIcon,
} from 'lucide-react';

import type { WallpaperDTO } from '../api/bridge';

export function typeIcon(type: string): ReactNode {
  switch (type) {
    case 'image':
      return createElement(ImageIcon, { 'aria-hidden': true, size: 28, strokeWidth: 1.5 });
    case 'gif':
      return createElement(Film, { 'aria-hidden': true, size: 28, strokeWidth: 1.5 });
    case 'video':
      return createElement(FileVideo, { 'aria-hidden': true, size: 28, strokeWidth: 1.5 });
    case 'we_scene': return 'WE';
    case 'we_web': return 'WEB';
    default:
      return createElement(FileQuestion, { 'aria-hidden': true, size: 28, strokeWidth: 1.5 });
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

function isUserUnsupported(e: WallpaperDTO): boolean {
  return 'userUnsupported' in e && e.userUnsupported === true;
}

export function weBadge(e: WallpaperDTO): string | null {
  if (isUserUnsupported(e)) return 'Unsupported';
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
    isUserUnsupported(e) ||
    e.type === 'we_web' ||
    e.backendStatus === 'failed' ||
    e.backendStatus === 'renderer_limitation'
  )
    return 'wallpaper-badge wallpaper-badge-danger';
  return 'wallpaper-badge';
}

/**
 * Short single-line metadata for the card footer. Long explanations of why an
 * entry cannot be applied belong to `cardStateDetail` (title / details views),
 * not to this truncated line — the badge already carries the state.
 */
export function metaLine(e: WallpaperDTO): string {
  if (isUserUnsupported(e)) {
    return 'Excluded from Library choices';
  }
  if (e.type === 'we_scene' || e.type === 'we_web' || e.type === 'unsupported') {
    if (e.type === 'unsupported') {
      return 'Unsupported';
    }
    if (e.type === 'we_web') {
      return ['Web wallpaper', e.workshopId].filter(Boolean).join(' · ');
    }
    const kind = 'Wallpaper Engine Scene';
    return [kind, e.workshopId, e.backend].filter(Boolean).join(' · ');
  }
  return `${e.resolution} · ${e.type} · ${formatSize(e.size)}`;
}

/** Full explanation of an entry's apply limitation, for tooltips and details. */
export function cardStateDetail(e: WallpaperDTO): string | null {
  if (isUserUnsupported(e)) {
    return 'Excluded from Library choices. Restore it from the Unsupported view to use it again.';
  }
  if (e.type === 'unsupported') {
    return e.unsupportedReason ?? 'This wallpaper type is not supported.';
  }
  if (e.type === 'we_web') {
    return 'Wallpaper Engine Web wallpapers can be browsed but not applied.';
  }
  if (e.type === 'we_scene' && e.backendStatus === 'renderer_limitation') {
    return e.backendErrorMessage
      || 'This scene has renderer limitations with linux-wallpaperengine.';
  }
  if (e.type === 'we_scene' && e.backendStatus === 'failed') {
    return e.backendErrorMessage
      || 'This scene is not compatible with linux-wallpaperengine.';
  }
  return null;
}

export function formatSize(bytes: number): string {
  if (bytes >= 1 << 30) return `${(bytes / (1 << 30)).toFixed(1)} GB`;
  if (bytes >= 1 << 20) return `${(bytes / (1 << 20)).toFixed(1)} MB`;
  if (bytes >= 1 << 10) return `${(bytes / (1 << 10)).toFixed(0)} KB`;
  return `${bytes} B`;
}
