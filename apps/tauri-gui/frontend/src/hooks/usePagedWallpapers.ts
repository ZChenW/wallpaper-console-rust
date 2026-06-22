import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { WallpaperDTO } from '../api/bridge';

export interface WallpaperPageDTO {
  total: number;
  items?: WallpaperDTO[] | null;
}

export type WallpaperPageLoader = (offset: number, limit: number) => Promise<WallpaperPageDTO>;

export type RequestKind = 'initial' | 'refresh' | 'append';

export interface LoadingState {
  initialLoading: boolean;
  refreshing: boolean;
}

interface UsePagedWallpapersOptions {
  pageSize: number;
  loadPage: WallpaperPageLoader;
  refreshEvent?: string;
  onPage?: (page: WallpaperPageDTO) => void;
}

const CONFIRM_EMPTY_DELAY_MS = 400;

export function mergePagedWallpaperItems(
  previous: WallpaperDTO[],
  incoming: WallpaperDTO[] | null | undefined,
  append: boolean,
): WallpaperDTO[] {
  const items = incoming ?? [];
  return append ? [...previous, ...items] : items;
}

export function resolveRequestKind(append: boolean, hasLoadedOnce: boolean): RequestKind {
  if (append) return 'append';
  return hasLoadedOnce ? 'refresh' : 'initial';
}

export function loadingStateForKind(kind: RequestKind): LoadingState {
  switch (kind) {
    case 'initial':
      return { initialLoading: true, refreshing: false };
    case 'refresh':
      return { initialLoading: false, refreshing: true };
    case 'append':
      return { initialLoading: false, refreshing: false };
  }
}

export function shouldConfirmEmpty(
  consecutiveZeroCount: number,
  hasLoadedOnce: boolean,
): boolean {
  return hasLoadedOnce && consecutiveZeroCount >= 2;
}

export function formatLoadPageError(error: unknown): string {
  if (error instanceof Error && error.message) {
    return error.message;
  }
  if (typeof error === 'string' && error) {
    return error;
  }
  return 'Failed to load library page';
}

export function usePagedWallpapers({
  pageSize,
  loadPage,
  refreshEvent,
  onPage,
}: UsePagedWallpapersOptions) {
  const [entries, setEntries] = useState<WallpaperDTO[]>([]);
  const [total, setTotal] = useState(0);
  const [hasLoadedOnce, setHasLoadedOnce] = useState(false);
  const [initialLoading, setInitialLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [lastRequestKind, setLastRequestKind] = useState<RequestKind>('initial');
  const [replaceCount, setReplaceCount] = useState(0);
  const [loadError, setLoadError] = useState(false);
  const [loadErrorDetail, setLoadErrorDetail] = useState<string | null>(null);
  const [emptyConfirmed, setEmptyConfirmed] = useState(false);
  const requestSeq = useRef(0);
  const hasLoadedOnceRef = useRef(false);
  const consecutiveZeroCountRef = useRef(0);
  const confirmEmptyTimerRef = useRef<number | null>(null);

  const clearConfirmEmptyTimer = useCallback(() => {
    if (confirmEmptyTimerRef.current !== null) {
      window.clearTimeout(confirmEmptyTimerRef.current);
      confirmEmptyTimerRef.current = null;
    }
  }, []);

  const load = useCallback(async (append = false, offset = 0) => {
    clearConfirmEmptyTimer();
    const requestId = requestSeq.current + 1;
    requestSeq.current = requestId;
    const isCurrent = () => requestSeq.current === requestId;
    const kind = resolveRequestKind(append, hasLoadedOnceRef.current);
    setLastRequestKind(kind);
    const next = loadingStateForKind(kind);
    setInitialLoading(next.initialLoading);
    setRefreshing(next.refreshing);

    let succeeded = false;
    try {
      const page = await loadPage(offset, pageSize);
      if (!isCurrent()) return;
      succeeded = true;
      setLoadError(false);
      setLoadErrorDetail(null);
      onPage?.(page);
      setTotal(page.total);
      setEntries((prev) => mergePagedWallpaperItems(prev, page.items, append));
      if (!append) {
        setReplaceCount((c) => c + 1);
        if (page.total === 0) {
          consecutiveZeroCountRef.current += 1;
          if (shouldConfirmEmpty(consecutiveZeroCountRef.current, true)) {
            setEmptyConfirmed(true);
          } else if (confirmEmptyTimerRef.current === null) {
            confirmEmptyTimerRef.current = window.setTimeout(() => {
              confirmEmptyTimerRef.current = null;
              void load(false, 0);
            }, CONFIRM_EMPTY_DELAY_MS);
          }
        } else {
          consecutiveZeroCountRef.current = 0;
          setEmptyConfirmed(false);
        }
      }
    } catch (error) {
      if (!isCurrent()) return;
      setLoadError(true);
      setLoadErrorDetail(formatLoadPageError(error));
      if (!append && !hasLoadedOnceRef.current) {
        setEntries([]);
        setTotal(0);
      }
    } finally {
      if (isCurrent()) {
        if (succeeded) {
          hasLoadedOnceRef.current = true;
          setHasLoadedOnce(true);
        }
        setInitialLoading(false);
        setRefreshing(false);
      }
    }
  }, [loadPage, onPage, pageSize, clearConfirmEmptyTimer]);

  const reload = useCallback(() => load(false, 0), [load]);
  const loadMore = useCallback(() => load(true, entries.length), [entries.length, load]);

  useEffect(() => {
    void reload();
  }, [reload]);

  useEffect(() => () => {
    requestSeq.current += 1;
    clearConfirmEmptyTimer();
  }, [clearConfirmEmptyTimer]);

  useEffect(() => {
    if (!refreshEvent) return undefined;
    const handler = () => {
      void reload();
    };
    window.addEventListener(refreshEvent, handler);
    return () => window.removeEventListener(refreshEvent, handler);
  }, [refreshEvent, reload]);

  useEffect(() => {
    consecutiveZeroCountRef.current = 0;
    setEmptyConfirmed(false);
    setLoadError(false);
    setLoadErrorDetail(null);
  }, [loadPage]);

  const entryByPath = useMemo(
    () => new Map(entries.map((entry) => [entry.path, entry])),
    [entries],
  );

  const loading = initialLoading || refreshing;

  return {
    entries,
    setEntries,
    total,
    loading,
    initialLoading,
    refreshing,
    hasLoadedOnce,
    lastRequestKind,
    replaceCount,
    loadError,
    loadErrorDetail,
    emptyConfirmed,
    load,
    reload,
    loadMore,
    entryByPath,
  };
}
