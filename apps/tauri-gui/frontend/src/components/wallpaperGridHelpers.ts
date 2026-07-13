import type { WallpaperDTO } from '../api/bridge';

export interface GridRange {
  startIndex: number;
  endIndex: number;
}

export interface GridLayoutChange {
  scrollTop: number;
  previousColumns: number;
  previousRowHeight: number;
  nextColumns: number;
  nextRowHeight: number;
}

export function anchoredScrollTopForLayoutChange({
  scrollTop,
  previousColumns,
  previousRowHeight,
  nextColumns,
  nextRowHeight,
}: GridLayoutChange): number {
  const firstVisibleRow = Math.floor(Math.max(0, scrollTop) / previousRowHeight);
  const firstVisibleItem = firstVisibleRow * previousColumns;
  const nextRow = Math.floor(firstVisibleItem / nextColumns);
  return nextRow * nextRowHeight;
}

export function visibleThumbnailPaths(
  entries: WallpaperDTO[],
  colCount: number,
  range: GridRange | null | undefined,
  fallbackRows: number,
): string[] {
  const safeCols = Math.max(1, colCount);
  const safeFallbackRows = Math.max(1, fallbackRows);
  const startIdx = range ? range.startIndex * safeCols : 0;
  const endIdx = range
    ? Math.min((range.endIndex + 1) * safeCols, entries.length)
    : Math.min(safeCols * safeFallbackRows, entries.length);
  return entries
    .slice(startIdx, endIdx)
    .filter((entry) => !entry.previewPath)
    .map((entry) => entry.path);
}

export function shouldResetScroll(
  prevResetKey: string | undefined,
  resetKey: string | undefined,
): boolean {
  return prevResetKey !== resetKey;
}
