import type { WallpaperDTO } from '../api/bridge';
import { staticPreviewAssetPath } from './wallpaperPreviewMedia.ts';

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
  /** Paging module already decided auto-append is allowed. */
  readonly canAutoAppend: boolean;
  readonly loadingMore: boolean;
}

export interface StableViewportAnchor {
  readonly wallpaperId: number;
  readonly rowOffset: number;
}

export interface GridKeyInput {
  readonly key: string;
  readonly shiftKey?: boolean;
  readonly currentIndex: number;
  readonly colCount: number;
  readonly itemCount: number;
  readonly pageRows: number;
  readonly hasMore: boolean;
  readonly loadingMore: boolean;
}

export type GridKeyResolution =
  | {
    readonly type: 'navigate';
    readonly index: number;
    readonly requestLoadMore: boolean;
  }
  | { readonly type: 'activate'; readonly index: number }
  | { readonly type: 'context'; readonly index: number };

export function resolveGridKey({
  key,
  shiftKey = false,
  currentIndex,
  colCount,
  itemCount,
  pageRows,
  hasMore,
  loadingMore,
}: GridKeyInput): GridKeyResolution | null {
  if (itemCount <= 0) return null;

  const columns = Math.max(1, Math.trunc(colCount));
  const lastIndex = itemCount - 1;
  const index = Math.max(0, Math.min(lastIndex, Math.trunc(currentIndex)));
  const rowStart = Math.floor(index / columns) * columns;
  const rowEnd = Math.min(lastIndex, rowStart + columns - 1);
  let nextIndex: number | null = null;

  switch (key) {
    case 'ArrowLeft':
      nextIndex = Math.max(rowStart, index - 1);
      break;
    case 'ArrowRight':
      nextIndex = Math.min(rowEnd, index + 1);
      break;
    case 'ArrowUp':
      nextIndex = Math.max(0, index - columns);
      break;
    case 'ArrowDown':
      nextIndex = Math.min(lastIndex, index + columns);
      break;
    case 'PageUp':
      nextIndex = Math.max(0, index - columns * Math.max(1, Math.trunc(pageRows)));
      break;
    case 'PageDown':
      nextIndex = Math.min(
        lastIndex,
        index + columns * Math.max(1, Math.trunc(pageRows)),
      );
      break;
    case 'Home':
      nextIndex = 0;
      break;
    case 'End':
      nextIndex = lastIndex;
      break;
    case 'Enter':
    case ' ':
      return { type: 'activate', index };
    case 'ContextMenu':
      return { type: 'context', index };
    case 'F10':
      return shiftKey ? { type: 'context', index } : null;
    default:
      return null;
  }

  return {
    type: 'navigate',
    index: nextIndex,
    requestLoadMore: key === 'End' && hasMore && !loadingMore,
  };
}

export function wallpaperIdNearestGridViewportCenter<
  T extends { readonly wallpaperId: number },
>({
  entries,
  columns,
  rowHeight,
  scrollTop,
  viewportHeight,
}: {
  readonly entries: readonly T[];
  readonly columns: number;
  readonly rowHeight: number;
  readonly scrollTop: number;
  readonly viewportHeight: number;
}): number | null {
  if (entries.length === 0) return null;
  const safeColumns = Math.max(1, Math.trunc(columns));
  const safeRowHeight = Number.isFinite(rowHeight) ? Math.max(1, rowHeight) : 1;
  const safeScrollTop = Number.isFinite(scrollTop) ? Math.max(0, scrollTop) : 0;
  const safeViewportHeight = Number.isFinite(viewportHeight)
    ? Math.max(0, viewportHeight)
    : 0;
  const centerRow = Math.floor(
    (safeScrollTop + safeViewportHeight / 2) / safeRowHeight,
  );
  const centerColumn = Math.floor((safeColumns - 1) / 2);
  const index = Math.min(
    entries.length - 1,
    centerRow * safeColumns + centerColumn,
  );
  return entries[Math.max(0, index)]?.wallpaperId ?? null;
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

export function shouldStartAnimatedHover(
  scrolling: boolean,
  reducedMotion = false,
): boolean {
  return !scrolling && !reducedMotion;
}

export function wallpaperOrdinal(index: number): string {
  return String(Math.max(0, Math.trunc(index)) + 1).padStart(2, '0');
}

export function wallpaperApplyFlags(
  path: string,
  applying: boolean,
  activePath: string | null | undefined,
  pendingPath: string | null | undefined,
): { applying: boolean; pending: boolean } {
  return {
    applying: applying && activePath === path,
    pending: pendingPath === path,
  };
}

export function previewAssetPath(entry: WallpaperDTO): string {
  return staticPreviewAssetPath(entry);
}

export function animatedPreviewPath(
  entry: WallpaperDTO,
  hovered: boolean,
  scrolling: boolean,
  reducedMotion = false,
): string | null {
  if (!hovered || scrolling || reducedMotion || !entry.previewPath) return null;
  const pathWithoutQuery = entry.previewPath.split(/[?#]/, 1)[0].toLowerCase();
  return pathWithoutQuery.endsWith('.gif') ? entry.previewPath : null;
}

export function shouldRequestNextPage({
  rowCount,
  visibleEndRow,
  canAutoAppend,
  loadingMore,
}: NextPageRequestState): boolean {
  if (!canAutoAppend || loadingMore || rowCount <= 0 || visibleEndRow == null) return false;
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
  entries: readonly WallpaperDTO[],
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

export function shouldApplyFocusToken(prevToken: number, nextToken: number): boolean {
  return nextToken > 0 && nextToken !== prevToken;
}
