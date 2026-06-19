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
  const requestSeq = useRef(0);
  const hasLoadedOnceRef = useRef(false);

  const load = useCallback(async (append = false, offset = 0) => {
    const requestId = requestSeq.current + 1;
    requestSeq.current = requestId;
    const isCurrent = () => requestSeq.current === requestId;
    const kind = resolveRequestKind(append, hasLoadedOnceRef.current);
    setLastRequestKind(kind);
    const next = loadingStateForKind(kind);
    setInitialLoading(next.initialLoading);
    setRefreshing(next.refreshing);

    try {
      const page = await loadPage(offset, pageSize);
      if (!isCurrent()) return;
      onPage?.(page);
      setTotal(page.total);
      setEntries((prev) => mergePagedWallpaperItems(prev, page.items, append));
      if (!append) setReplaceCount((c) => c + 1);
    } catch {
      if (!isCurrent()) return;
      if (!append && !hasLoadedOnceRef.current) {
        setEntries([]);
        setTotal(0);
      }
    } finally {
      if (isCurrent()) {
        hasLoadedOnceRef.current = true;
        setHasLoadedOnce(true);
        setInitialLoading(false);
        setRefreshing(false);
      }
    }
  }, [loadPage, onPage, pageSize]);

  const reload = useCallback(() => load(false, 0), [load]);
  const loadMore = useCallback(() => load(true, entries.length), [entries.length, load]);

  useEffect(() => {
    void reload();
  }, [reload]);

  useEffect(() => () => {
    requestSeq.current += 1;
  }, []);

  useEffect(() => {
    if (!refreshEvent) return undefined;
    const handler = () => {
      void reload();
    };
    window.addEventListener(refreshEvent, handler);
    return () => window.removeEventListener(refreshEvent, handler);
  }, [refreshEvent, reload]);

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
    load,
    reload,
    loadMore,
    entryByPath,
  };
}
