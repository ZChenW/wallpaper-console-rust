import type { LibraryBrowserItemDTO } from '../api/types.ts';

export function detailsPreviewAssetPath(entry: LibraryBrowserItemDTO): string | null {
  if (entry.type === 'image' || entry.type === 'gif') return entry.path;
  return entry.previewPath?.trim() || null;
}

export function nextDetailsPreviewSource(
  current: string,
  fallback: string | null,
): string | null {
  return fallback && fallback !== current ? fallback : null;
}
