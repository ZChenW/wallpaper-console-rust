import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { WallpaperDTO } from '../api/bridge';
import { isRevisionChangedError } from '../api/types.ts';

export interface WallpaperPageDTO<T extends WallpaperDTO = WallpaperDTO> {
  revision: number;
  nextCursor: string | null;
  total: number | null;
  items?: T[] | null;
}

export type WallpaperPageLoader<T extends WallpaperDTO = WallpaperDTO> = (
  cursor: string | null,
  limit: number,
) => Promise<WallpaperPageDTO<T>>;

export type RequestKind = 'initial' | 'refresh' | 'append';

export interface LoadingState {
  initialLoading: boolean;
  refreshing: boolean;
}

export type AutomaticAppendOutcome =
  | { kind: 'error' }
  | {
    kind: 'success';
    itemCount: number;
    nextCursor: string | null;
  };

interface UsePagedWallpapersOptions<T extends WallpaperDTO = WallpaperDTO> {
  pageSize: number;
  loadPage: WallpaperPageLoader<T>;
  refreshEvent?: string;
  onPage?: (page: WallpaperPageDTO<T>) => void;
}

const CONFIRM_EMPTY_DELAY_MS = 400;

export function mergePagedWallpaperItems<T extends WallpaperDTO = WallpaperDTO>(
  previous: T[],
  incoming: T[] | null | undefined,
  append: boolean,
): T[] {
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
  if (typeof error === 'object' && error !== null && 'message' in error) {
    const message = (error as { message?: unknown }).message;
    if (typeof message === 'string' && message) return message;
  }
  return 'Failed to load library page';
}

export function shouldPauseAutomaticAppend(outcome: AutomaticAppendOutcome): boolean {
  if (outcome.kind === 'error') return true;
  return outcome.itemCount === 0;
}

export function usePagedWallpapers<T extends WallpaperDTO = WallpaperDTO>({
  pageSize,
  loadPage,
  refreshEvent,
  onPage,
}: UsePagedWallpapersOptions<T>) {
  const [entries, setEntries] = useState<T[]>([]);
  const [total, setTotal] = useState<number | null>(null);
  const [revision, setRevision] = useState<number | null>(null);
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const revisionRef = useRef<number | null>(null);
  const [hasLoadedOnce, setHasLoadedOnce] = useState(false);
  const [initialLoading, setInitialLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [appending, setAppending] = useState(false);
  const [automaticAppendPaused, setAutomaticAppendPaused] = useState(false);
  const [lastRequestKind, setLastRequestKind] = useState<RequestKind>('initial');
  const [replaceCount, setReplaceCount] = useState(0);
  const [loadError, setLoadError] = useState(false);
  const [loadErrorDetail, setLoadErrorDetail] = useState<string | null>(null);
  const [emptyConfirmed, setEmptyConfirmed] = useState(false);
  const requestSeq = useRef(0);
  const hasLoadedOnceRef = useRef(false);
  const consecutiveZeroCountRef = useRef(0);
  const confirmEmptyTimerRef = useRef<number | null>(null);
  const appendInFlightRef = useRef(false);
  const replacementRequestRef = useRef<number | null>(null);

  const clearConfirmEmptyTimer = useCallback(() => {
    if (confirmEmptyTimerRef.current !== null) {
      window.clearTimeout(confirmEmptyTimerRef.current);
      confirmEmptyTimerRef.current = null;
    }
  }, []);

  const load = useCallback(async (append = false, cursor: string | null = null) => {
    if (append && (
      appendInFlightRef.current
      || replacementRequestRef.current !== null
    )) return;
    if (append) {
      appendInFlightRef.current = true;
      setAppending(true);
    }
    clearConfirmEmptyTimer();
    const requestId = requestSeq.current + 1;
    requestSeq.current = requestId;
    if (!append) replacementRequestRef.current = requestId;
    const isCurrent = () => requestSeq.current === requestId;
    const kind = resolveRequestKind(append, hasLoadedOnceRef.current);
    if (kind === 'initial' && typeof performance !== 'undefined') {
      performance.clearMarks('wc-library-request-start');
      performance.clearMarks('wc-library-request-settled');
      performance.clearMeasures('wc-library-first-request');
      performance.mark('wc-library-request-start');
    }
    setLastRequestKind(kind);
    const next = loadingStateForKind(kind);
    setInitialLoading(next.initialLoading);
    setRefreshing(next.refreshing);

    let succeeded = false;
    try {
      const page = await loadPage(cursor, pageSize);
      if (!isCurrent()) return;
      if (append && revisionRef.current !== null && page.revision !== revisionRef.current) {
        throw { kind: 'revision_changed', message: 'Library snapshot changed.' };
      }
      succeeded = true;
      setLoadError(false);
      setLoadErrorDetail(null);
      onPage?.(page);
      setRevision(page.revision);
      revisionRef.current = page.revision;
      const appendOutcome = append ? {
        kind: 'success' as const,
        itemCount: page.items?.length ?? 0,
        nextCursor: page.nextCursor,
      } : null;
      const appendPaused = appendOutcome !== null && shouldPauseAutomaticAppend(appendOutcome);
      // An empty append is not evidence that the prior cursor was exhausted:
      // preserve it for an explicit retry even if a transient backend response
      // omitted nextCursor.
      setNextCursor(appendPaused ? cursor : page.nextCursor);
      setTotal(page.total);
      setEntries((prev) => mergePagedWallpaperItems(prev, page.items, append));
      if (append) {
        setAutomaticAppendPaused(appendPaused);
      }
      if (!append) {
        setReplaceCount((c) => c + 1);
        if ((page.items?.length ?? 0) === 0 && page.nextCursor === null) {
          consecutiveZeroCountRef.current += 1;
          if (shouldConfirmEmpty(consecutiveZeroCountRef.current, true)) {
            setEmptyConfirmed(true);
          } else if (confirmEmptyTimerRef.current === null) {
            confirmEmptyTimerRef.current = window.setTimeout(() => {
              confirmEmptyTimerRef.current = null;
              void load(false, null);
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
      if (append && isRevisionChangedError(error)) {
        // Keep the old list visible while atomically replacing it with page 1
        // from the new revision.
        void load(false, null);
        return;
      }
      if (append) {
        setAutomaticAppendPaused(shouldPauseAutomaticAppend({ kind: 'error' }));
      }
      if (!append && !hasLoadedOnceRef.current) {
        setEntries([]);
        setTotal(null);
      }
    } finally {
      if (append) {
        appendInFlightRef.current = false;
        setAppending(false);
      }
      if (!append && replacementRequestRef.current === requestId) {
        replacementRequestRef.current = null;
      }
      if (isCurrent()) {
        if (succeeded) {
          hasLoadedOnceRef.current = true;
          setHasLoadedOnce(true);
        }
        setInitialLoading(false);
        setRefreshing(false);
        if (kind === 'initial' && typeof performance !== 'undefined') {
          performance.mark('wc-library-request-settled');
          performance.measure(
            'wc-library-first-request',
            'wc-library-request-start',
            'wc-library-request-settled',
          );
        }
      }
    }
  }, [loadPage, onPage, pageSize, clearConfirmEmptyTimer]);

  const reload = useCallback(() => {
    setAutomaticAppendPaused(false);
    return load(false, null);
  }, [load]);
  const loadMore = useCallback(() => {
    if (nextCursor === null || replacementRequestRef.current !== null) {
      return Promise.resolve();
    }
    setAutomaticAppendPaused(false);
    return load(true, nextCursor);
  }, [load, nextCursor]);

  useEffect(() => {
    void reload();
  }, [reload]);

  useEffect(() => () => {
    requestSeq.current += 1;
    replacementRequestRef.current = null;
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
    revisionRef.current = null;
    setRevision(null);
    setNextCursor(null);
    setTotal(null);
    setEmptyConfirmed(false);
    setLoadError(false);
    setLoadErrorDetail(null);
    setAutomaticAppendPaused(false);
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
    revision,
    nextCursor,
    hasMore: nextCursor !== null,
    loading,
    initialLoading,
    refreshing,
    appending,
    automaticAppendPaused,
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
