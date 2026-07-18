import type { LibraryBrowserItemDTO, WallpaperDTO } from '../api/types.ts';

export type EnhancedMediaKind = 'image' | 'video';

export interface EnhancedMediaCandidate {
  readonly kind: EnhancedMediaKind;
  readonly path: string;
}

export interface EnhancedMediaEligibility {
  readonly active: boolean;
  readonly centered: boolean;
  readonly selected: boolean;
  readonly settled: boolean;
  readonly reducedMotion: boolean;
}

export interface ReleasableVideo {
  pause(): void;
  removeAttribute(name: string): void;
  load(): void;
}

export interface AttachableVideo extends ReleasableVideo {
  getAttribute(name: string): string | null;
  setAttribute(name: string, value: string): void;
}

export function staticPreviewAssetPath(entry: WallpaperDTO): string {
  return entry.previewPath || entry.path;
}

function canEnhance(entry: WallpaperDTO, eligibility: EnhancedMediaEligibility): boolean {
  if (
    !eligibility.active
    || !eligibility.centered
    || !eligibility.selected
    || !eligibility.settled
    || eligibility.reducedMotion
  ) {
    return false;
  }
  return entry.type === 'image' || entry.type === 'gif' || entry.type === 'video';
}

export function enhancedMediaCandidates(
  entry: LibraryBrowserItemDTO,
  eligibility: EnhancedMediaEligibility,
): EnhancedMediaCandidate[] {
  if (!canEnhance(entry, eligibility)) return [];

  const candidates: EnhancedMediaCandidate[] = [{
    kind: entry.type === 'video' ? 'video' : 'image',
    path: entry.path,
  }];
  const previewPath = entry.previewPath?.trim();
  if (previewPath && previewPath !== entry.path) {
    candidates.push({ kind: 'image', path: previewPath });
  }
  return candidates;
}

export function releaseVideoDecoder(video: ReleasableVideo | null | undefined): void {
  if (!video) return;
  video.pause();
  video.removeAttribute('src');
  video.load();
}

export function attachVideoDecoder<T extends AttachableVideo>(
  previous: T | null,
  next: T | null,
  expectedSource: string | null,
): T | null {
  if (previous && previous !== next) releaseVideoDecoder(previous);
  if (next && expectedSource && next.getAttribute('src') !== expectedSource) {
    next.setAttribute('src', expectedSource);
  }
  return next;
}
