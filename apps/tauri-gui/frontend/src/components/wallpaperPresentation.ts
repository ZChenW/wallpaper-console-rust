import type { LibraryBrowserItemDTO } from '../api/types.ts';
import { displayName, formatSize } from './wallpaperCardHelpers.ts';

export interface WallpaperPresentation {
  readonly name: string;
  readonly sources: string | null;
  readonly type: string;
  readonly resolution: string | null;
  readonly size: string | null;
  readonly addedDate: string | null;
  readonly author: string | null;
  readonly workshopId: string | null;
  readonly backend: string | null;
  readonly compatibility: string | null;
}

const addedDateFormatter = new Intl.DateTimeFormat('en-US', {
  day: '2-digit',
  month: 'short',
  year: 'numeric',
  timeZone: 'UTC',
});

function optionalText(value: string | null | undefined): string | null {
  const text = value?.trim();
  return text ? text : null;
}

export function wallpaperTypeLabel(type: string): string {
  switch (type) {
    case 'image': return 'Image';
    case 'gif': return 'GIF';
    case 'video': return 'Video';
    case 'we_scene': return 'Wallpaper Engine Scene';
    case 'we_web': return 'Wallpaper Engine Web';
    case 'unsupported': return 'Unsupported';
    default: {
      const label = type.trim().replace(/[_-]+/g, ' ');
      return label || 'Unknown';
    }
  }
}

export function formatAddedDate(value: string | null | undefined): string | null {
  const text = optionalText(value);
  if (!text) return null;
  const zoneLess = /^(\d{4})-(\d{2})-(\d{2})[ T](\d{2}):(\d{2}):(\d{2})(?:\.(\d{1,9}))?$/.exec(text);
  let date: Date;
  if (zoneLess) {
    const [, yearText, monthText, dayText, hourText, minuteText, secondText, fraction = ''] = zoneLess;
    const [year, month, day, hour, minute, second] = [
      yearText,
      monthText,
      dayText,
      hourText,
      minuteText,
      secondText,
    ].map(Number);
    const millisecond = Number(fraction.slice(0, 3).padEnd(3, '0'));
    date = new Date(Date.UTC(year, month - 1, day, hour, minute, second, millisecond));
    if (
      date.getUTCFullYear() !== year
      || date.getUTCMonth() !== month - 1
      || date.getUTCDate() !== day
      || date.getUTCHours() !== hour
      || date.getUTCMinutes() !== minute
      || date.getUTCSeconds() !== second
    ) {
      return null;
    }
  } else {
    date = new Date(text);
  }
  return Number.isNaN(date.getTime()) ? null : addedDateFormatter.format(date);
}

function formatResolution(value: string | null | undefined): string | null {
  const text = optionalText(value);
  if (!text || /^(?:unknown|n\/?a|none|-)$/i.test(text)) return null;
  const dimensions = /^(\d+)\s*[x×]\s*(\d+)$/i.exec(text);
  return dimensions ? `${dimensions[1]} × ${dimensions[2]}` : text;
}

function formatSources(entry: LibraryBrowserItemDTO): string | null {
  const names = entry.sources
    .map((source) => source.displayName.trim())
    .filter(Boolean);
  const uniqueNames = [...new Set(names)];
  return uniqueNames.length > 0 ? uniqueNames.join(', ') : null;
}

export function presentWallpaper(entry: LibraryBrowserItemDTO): WallpaperPresentation {
  return {
    name: displayName(entry),
    sources: formatSources(entry),
    type: wallpaperTypeLabel(entry.type),
    resolution: formatResolution(entry.resolution),
    size: Number.isFinite(entry.size) && entry.size >= 0 ? formatSize(entry.size) : null,
    addedDate: formatAddedDate(entry.addedAt),
    author: optionalText(entry.author),
    workshopId: optionalText(entry.workshopId),
    backend: optionalText(entry.backend),
    compatibility: optionalText(entry.applyReason)
      ?? optionalText(entry.unsupportedReason)
      ?? optionalText(entry.rendererCompatibility),
  };
}
