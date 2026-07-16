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

export interface NextPageRequestState {
  readonly rowCount: number;
  readonly visibleEndRow: number | null | undefined;
  readonly hasMore: boolean;
  readonly loadingMore: boolean;
}

export interface StableViewportAnchor {
  readonly wallpaperId: number;
  readonly rowOffset: number;
}

export function captureStableViewportAnchor<T extends { readonly wallpaperId: number }>(
  entries: readonly T[],
  columns: number,
  rowHeight: number,
  scrollTop: number,
): StableViewportAnchor | null {
  const safeColumns = Math.max(1, columns);
  const safeRowHeight = Math.max(1, rowHeight);
  const top = Math.max(0, scrollTop);
  const row = Math.floor(top / safeRowHeight);
  const entry = entries[row * safeColumns];
  return entry
    ? { wallpaperId: entry.wallpaperId, rowOffset: top - row * safeRowHeight }
    : null;
}

export function restoreStableViewportAnchor<T extends { readonly wallpaperId: number }>(
  entries: readonly T[],
  anchor: StableViewportAnchor | null,
  columns: number,
  rowHeight: number,
): number | null {
  if (!anchor) return null;
  const index = entries.findIndex((entry) => entry.wallpaperId === anchor.wallpaperId);
  if (index < 0) return null;
  return Math.floor(index / Math.max(1, columns)) * Math.max(1, rowHeight)
    + anchor.rowOffset;
}

const NEXT_PAGE_PREFETCH_ROWS = 2;

export function shouldPauseThumbnailReveal(active: boolean, scrolling: boolean): boolean {
  return !active || scrolling;
}

export function shouldStartAnimatedHover(scrolling: boolean): boolean {
  return !scrolling;
}

export function previewAssetPath(entry: WallpaperDTO): string {
  return entry.previewPath || entry.path;
}

export function animatedPreviewPath(
  entry: WallpaperDTO,
  hovered: boolean,
  scrolling: boolean,
): string | null {
  if (!hovered || scrolling || !entry.previewPath) return null;
  const pathWithoutQuery = entry.previewPath.split(/[?#]/, 1)[0].toLowerCase();
  return pathWithoutQuery.endsWith('.gif') ? entry.previewPath : null;
}

export function shouldRequestNextPage({
  rowCount,
  visibleEndRow,
  hasMore,
  loadingMore,
}: NextPageRequestState): boolean {
  if (!hasMore || loadingMore || rowCount <= 0 || visibleEndRow == null) return false;
  return visibleEndRow >= rowCount - 1 - NEXT_PAGE_PREFETCH_ROWS;
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
    .map(previewAssetPath);
}

export function shouldResetScroll(
  prevResetKey: string | undefined,
  resetKey: string | undefined,
): boolean {
  return prevResetKey !== resetKey;
}
