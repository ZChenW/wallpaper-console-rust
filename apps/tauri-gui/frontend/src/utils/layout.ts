export type WallpaperCardSize = 'small' | 'medium' | 'large';

export interface WallpaperCardMetrics {
  minWidth: number;
  rowHeight: number;
  thumbnailHeight: number;
}

export const WALLPAPER_CARD_METRICS: Readonly<Record<WallpaperCardSize, WallpaperCardMetrics>> = {
  small: {
    minWidth: 176,
    rowHeight: 164,
    thumbnailHeight: 96,
  },
  medium: {
    minWidth: 220,
    rowHeight: 188,
    thumbnailHeight: 120,
  },
  large: {
    minWidth: 288,
    rowHeight: 232,
    thumbnailHeight: 160,
  },
};

export function wallpaperCardMetrics(size: WallpaperCardSize = 'medium'): WallpaperCardMetrics {
  return WALLPAPER_CARD_METRICS[size];
}

export const COL_MIN_WIDTH = WALLPAPER_CARD_METRICS.medium.minWidth;
export const GRID_GAP = 10;

export const MAX_SLOW_OVERSCAN_CARDS = 12;
export const MAX_FAST_OVERSCAN_CARDS = 8;

export function calculateColumnCount(
  width: number,
  minCardWidth = COL_MIN_WIDTH,
  gap = GRID_GAP,
): number {
  if (!Number.isFinite(width) || width <= 0) return 1;
  const count = Math.floor((width + gap) / (minCardWidth + gap));
  return Math.max(1, count);
}

export interface GridLayout {
  colCount: number;
  columnWidth: number;
  rowWidth: number;
}

export function calculateGridLayout(
  width: number,
  minCardWidth = COL_MIN_WIDTH,
  gap = GRID_GAP,
): GridLayout {
  if (!Number.isFinite(width) || width <= 0) {
    return {
      colCount: 1,
      columnWidth: minCardWidth,
      rowWidth: minCardWidth,
    };
  }

  const colCount = calculateColumnCount(width, minCardWidth, gap);
  const totalGap = gap * Math.max(0, colCount - 1);
  const columnWidth = (width - totalGap) / colCount;
  return {
    colCount,
    columnWidth,
    rowWidth: width,
  };
}

export function calculateWallpaperGridLayout(
  width: number,
  size: WallpaperCardSize = 'medium',
  gap = GRID_GAP,
): GridLayout {
  return calculateGridLayout(width, wallpaperCardMetrics(size).minWidth, gap);
}

export function overscanRowsFor(colCount: number, fast: boolean): number {
  const maxCards = fast ? MAX_FAST_OVERSCAN_CARDS : MAX_SLOW_OVERSCAN_CARDS;
  return Math.max(1, Math.ceil(maxCards / Math.max(1, colCount)));
}
