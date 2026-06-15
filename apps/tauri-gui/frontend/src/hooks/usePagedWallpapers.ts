import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { WallpaperDTO } from '../api/bridge';

export interface WallpaperPageDTO {
  total: number;
  items?: WallpaperDTO[] | null;
}

export type WallpaperPageLoader = (offset: number, limit: number) => Promise<WallpaperPageDTO>;

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

export function usePagedWallpapers({
  pageSize,
  loadPage,
  refreshEvent,
  onPage,
}: UsePagedWallpapersOptions) {
  const [entries, setEntries] = useState<WallpaperDTO[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(true);
  const requestSeq = useRef(0);

  const load = useCallback(async (append = false, offset = 0) => {
    const requestId = requestSeq.current + 1;
    requestSeq.current = requestId;
    const isCurrent = () => requestSeq.current === requestId;
    setLoading(true);

    try {
      const page = await loadPage(offset, pageSize);
      if (!isCurrent()) return;
      onPage?.(page);
      setTotal(page.total);
      setEntries((prev) => mergePagedWallpaperItems(prev, page.items, append));
    } catch {
      if (!isCurrent()) return;
      if (!append) {
        setEntries([]);
        setTotal(0);
      }
    } finally {
      if (isCurrent()) {
        setLoading(false);
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

  return {
    entries,
    setEntries,
    total,
    loading,
    load,
    reload,
    loadMore,
    entryByPath,
  };
}
