import type { LibraryBrowserItemDTO } from '../api/types.ts';

export function resolutionAspectRatio(resolution: string): number {
  const match = /^(\d+)\s*[x×]\s*(\d+)$/i.exec(resolution.trim());
  if (!match) return 16 / 9;
  const width = Number(match[1]);
  const height = Number(match[2]);
  return width > 0 && height > 0 ? width / height : 16 / 9;
}

export function aspectClass(entry: LibraryBrowserItemDTO): 'landscape' | 'square' | 'portrait' {
  const aspect = resolutionAspectRatio(entry.resolution);
  if (aspect > 1.16) return 'landscape';
  if (aspect < 0.86) return 'portrait';
  return 'square';
}
