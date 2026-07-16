import { useCallback, useEffect, useRef, useState } from 'react';

import { api as defaultApi } from '../api/bridge.ts';
import type {
  LibraryBrowserItemDTO,
  LibraryBrowserPageDTO,
  LibraryBrowserQueryDTO,
  LibraryBrowserTotalDTO,
  WallpaperConsoleApi,
} from '../api/types.ts';
import {
  usePagedWallpapers,
  type WallpaperPageDTO,
} from '../hooks/usePagedWallpapers.ts';
import { DEFAULT_LIBRARY_PAGE_SIZE } from './libraryQueryState.ts';
import type {
  LibrarySort,
  LibraryTypeFilter,
  SourceFilter,
} from './shellPreferences.ts';

export interface LibraryBrowserCriteria {
  readonly sourceFilter: SourceFilter;
  readonly typeFilter: LibraryTypeFilter;
  readonly favoritesOnly: boolean;
  readonly sort: LibrarySort;
  readonly search: string;
}

export interface LibraryBrowserApi {
  libraryBrowserPage(query: LibraryBrowserQueryDTO): Promise<LibraryBrowserPageDTO>;
  libraryBrowserTotal(
    query: LibraryBrowserQueryDTO,
    expectedRevision: number,
  ): Promise<LibraryBrowserTotalDTO>;
  libraryBrowserRandom(query: LibraryBrowserQueryDTO): Promise<LibraryBrowserItemDTO | null>;
}

export interface UseLibraryBrowserOptions extends LibraryBrowserCriteria {
  readonly pageSize?: number;
  readonly searchDebounceMs?: number;
  readonly refreshEvent?: string;
  readonly browserApi?: LibraryBrowserApi;
}

function normalizedSearch(search: string): string {
  return search.trim();
}

export function createLibraryBrowserQuery(
  criteria: LibraryBrowserCriteria,
  cursor: string | null,
  limit: number,
): LibraryBrowserQueryDTO {
  const query: LibraryBrowserQueryDTO = {
    typeFilter: criteria.typeFilter,
    favoritesOnly: criteria.favoritesOnly,
    search: normalizedSearch(criteria.search),
    sort: criteria.sort,
    cursor,
    limit,
  };
  if (criteria.sourceFilter.kind === 'source') {
    query.sourceId = criteria.sourceFilter.sourceId;
  }
  return query;
}

export function createRandomLibraryBrowserQuery(
  criteria: LibraryBrowserCriteria,
): LibraryBrowserQueryDTO {
  return createLibraryBrowserQuery(criteria, null, 1);
}

export function formatRandomWallpaperError(error: unknown): string {
  if (error instanceof Error && error.message) return error.message;
  if (typeof error === 'string' && error) return error;
  return 'Failed to choose a random wallpaper';
}

export type RandomWallpaperOutcome =
  | { readonly kind: 'selected'; readonly entry: LibraryBrowserItemDTO }
  | { readonly kind: 'empty' }
  | { readonly kind: 'error'; readonly message: string }
  | { readonly kind: 'stale' };

export function randomWallpaperErrorOutcome(
  error: unknown,
): Extract<RandomWallpaperOutcome, { readonly kind: 'error' }> {
  return { kind: 'error', message: formatRandomWallpaperError(error) };
}

export function isCurrentQueryEmpty(
  emptyConfirmed: boolean,
  resolvedCriteriaKey: string | null,
  currentCriteriaKey: string,
): boolean {
  return emptyConfirmed && resolvedCriteriaKey === currentCriteriaKey;
}

export function isRandomRequestCurrent(
  requestId: number,
  currentRequestId: number,
  requestedSearch: string,
  currentSearch: string,
): boolean {
  return requestId === currentRequestId && requestedSearch === currentSearch;
}

export function useDebouncedValue<T>(value: T, delayMs: number): T {
  const [debounced, setDebounced] = useState(value);

  useEffect(() => {
    const delay = Number.isFinite(delayMs) ? Math.max(0, delayMs) : 0;
    const timer = window.setTimeout(() => setDebounced(value), delay);
    return () => window.clearTimeout(timer);
  }, [delayMs, value]);

  return debounced;
}

/**
 * Owns the only paging cursor for the unified library browser. Persisted
 * filters feed this hook, while search and loaded offsets remain session-only.
 */
export function useLibraryBrowser({
  sourceFilter,
  typeFilter,
  favoritesOnly,
  sort,
  search,
  pageSize = DEFAULT_LIBRARY_PAGE_SIZE,
  searchDebounceMs = 180,
  refreshEvent,
  browserApi = defaultApi satisfies WallpaperConsoleApi,
}: UseLibraryBrowserOptions) {
  const debouncedSearch = useDebouncedValue(search, searchDebounceMs);
  const sourceId = sourceFilter.kind === 'source' ? sourceFilter.sourceId : null;
  const criteriaKey = JSON.stringify([
    sourceId,
    typeFilter,
    favoritesOnly,
    sort,
    normalizedSearch(debouncedSearch),
  ]);
  const [resolvedCriteriaKey, setResolvedCriteriaKey] = useState<string | null>(null);

  const criteria = useCallback((): LibraryBrowserCriteria => ({
    sourceFilter: sourceId === null
      ? { kind: 'all' }
      : { kind: 'source', sourceId },
    typeFilter,
    favoritesOnly,
    sort,
    search: debouncedSearch,
  }), [debouncedSearch, favoritesOnly, sort, sourceId, typeFilter]);

  const loadPage = useCallback(
    (cursor: string | null, limit: number) => browserApi.libraryBrowserPage(
      createLibraryBrowserQuery(criteria(), cursor, limit),
    ),
    [browserApi, criteria],
  );
  const [exactTotal, setExactTotal] = useState<{
    criteriaKey: string;
    revision: number;
    total: number;
  } | null>(null);
  const totalRequestKeyRef = useRef<string | null>(null);
  const totalRequestSeq = useRef(0);
  const markCriteriaResolved = useCallback((page: WallpaperPageDTO<LibraryBrowserItemDTO>) => {
    setResolvedCriteriaKey(criteriaKey);
    const requestKey = `${criteriaKey}:${page.revision}`;
    if (totalRequestKeyRef.current === requestKey) return;
    totalRequestKeyRef.current = requestKey;
    const requestId = ++totalRequestSeq.current;
    void browserApi.libraryBrowserTotal(
      createLibraryBrowserQuery(criteria(), null, pageSize),
      page.revision,
    ).then((result) => {
      if (requestId !== totalRequestSeq.current || result.revision !== page.revision) return;
      setExactTotal({ criteriaKey, revision: result.revision, total: result.total });
    }, () => {
      if (requestId === totalRequestSeq.current) totalRequestKeyRef.current = null;
    });
  }, [browserApi, criteria, criteriaKey, pageSize]);

  const pages = usePagedWallpapers<LibraryBrowserItemDTO>({
    pageSize,
    loadPage,
    refreshEvent,
    onPage: markCriteriaResolved,
  });

  useEffect(() => {
    totalRequestSeq.current += 1;
    totalRequestKeyRef.current = null;
    setExactTotal(null);
  }, [criteriaKey]);

  const [randomPending, setRandomPending] = useState(false);
  const [randomError, setRandomError] = useState<string | null>(null);
  const randomRequestSeq = useRef(0);
  const rawSearchRef = useRef(search);
  rawSearchRef.current = search;

  useEffect(() => {
    randomRequestSeq.current += 1;
    setRandomPending(false);
    setRandomError(null);
  }, [criteria, search]);

  useEffect(() => () => {
    randomRequestSeq.current += 1;
  }, []);

  const chooseRandom = useCallback(async (): Promise<RandomWallpaperOutcome> => {
    const requestId = randomRequestSeq.current + 1;
    randomRequestSeq.current = requestId;
    const requestedSearch = search;
    const isCurrent = () => isRandomRequestCurrent(
      requestId,
      randomRequestSeq.current,
      requestedSearch,
      rawSearchRef.current,
    );
    setRandomPending(true);
    setRandomError(null);
    try {
      const entry = await browserApi.libraryBrowserRandom(
        createRandomLibraryBrowserQuery(criteria()),
      );
      if (!isCurrent()) return { kind: 'stale' };
      return entry ? { kind: 'selected', entry } : { kind: 'empty' };
    } catch (error) {
      if (isCurrent()) {
        const outcome = randomWallpaperErrorOutcome(error);
        setRandomError(outcome.message);
        return outcome;
      }
      return { kind: 'stale' };
    } finally {
      if (isCurrent()) setRandomPending(false);
    }
  }, [browserApi, criteria, search]);

  return {
    ...pages,
    total: exactTotal
      && exactTotal.criteriaKey === criteriaKey
      && exactTotal.revision === pages.revision
      ? exactTotal.total
      : pages.entries.length,
    totalKnown: Boolean(
      exactTotal
      && exactTotal.criteriaKey === criteriaKey
      && exactTotal.revision === pages.revision,
    ),
    emptyConfirmed: isCurrentQueryEmpty(
      pages.emptyConfirmed,
      resolvedCriteriaKey,
      criteriaKey,
    ),
    debouncedSearch,
    searchSettled: debouncedSearch === search,
    chooseRandom,
    randomPending,
    randomError,
  };
}
