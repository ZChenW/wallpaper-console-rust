import { useCallback, useEffect, useRef, useState } from 'react';

import { api as defaultApi } from '../api/bridge.ts';
import type {
  DisplayListDTO,
  DisplayStateDTO,
  SourceDTO,
} from '../api/types.ts';
import { normalizeDisplayOutputs } from './displayTargets.ts';

export interface ShellCatalogApi {
  displaysList(): Promise<DisplayListDTO>;
  displayStateList(): Promise<DisplayStateDTO[]>;
  sourcesList(): Promise<SourceDTO[]>;
}

export interface ShellCatalogErrors {
  readonly displays?: string;
  readonly displayState?: string;
  readonly sources?: string;
}

export interface ShellCatalogSnapshot {
  readonly connectedOutputs: readonly string[];
  readonly sources: readonly SourceDTO[];
  readonly persistedDisplayStates: readonly DisplayStateDTO[];
  readonly errors: ShellCatalogErrors;
}

const EMPTY_CATALOG: ShellCatalogSnapshot = Object.freeze({
  connectedOutputs: Object.freeze([]),
  sources: Object.freeze([]),
  persistedDisplayStates: Object.freeze([]),
  errors: Object.freeze({}),
});

function errorMessage(error: unknown): string {
  if (error instanceof Error && error.message) return error.message;
  if (typeof error === 'string' && error) return error;
  return 'Request failed';
}

export async function loadShellCatalogSnapshot(
  catalogApi: ShellCatalogApi,
): Promise<ShellCatalogSnapshot> {
  const [displays, sources, displayState] = await Promise.allSettled([
    catalogApi.displaysList(),
    catalogApi.sourcesList(),
    catalogApi.displayStateList(),
  ]);
  return {
    connectedOutputs: displays.status === 'fulfilled'
      ? normalizeDisplayOutputs(displays.value.outputs.map(({ name }) => name))
      : [],
    sources: sources.status === 'fulfilled' ? sources.value : [],
    persistedDisplayStates: displayState.status === 'fulfilled' ? displayState.value : [],
    errors: {
      ...(displays.status === 'rejected'
        ? { displays: errorMessage(displays.reason) }
        : {}),
      ...(sources.status === 'rejected'
        ? { sources: errorMessage(sources.reason) }
        : {}),
      ...(displayState.status === 'rejected'
        ? { displayState: errorMessage(displayState.reason) }
        : {}),
    },
  };
}

export function useShellCatalog(
  catalogApi: ShellCatalogApi = defaultApi,
  displayPollMs = 5_000,
) {
  const [catalog, setCatalog] = useState<ShellCatalogSnapshot>(EMPTY_CATALOG);
  const [ready, setReady] = useState(false);
  const fullRequestSeq = useRef(0);
  const sourceRequestSeq = useRef(0);
  const displayRequestSeq = useRef(0);

  const reload = useCallback(async (): Promise<void> => {
    const requestId = ++fullRequestSeq.current;
    const next = await loadShellCatalogSnapshot(catalogApi);
    if (requestId !== fullRequestSeq.current) return;
    setCatalog(next);
    setReady(true);
  }, [catalogApi]);

  const reloadSources = useCallback(async (): Promise<void> => {
    const requestId = ++sourceRequestSeq.current;
    try {
      const sources = await catalogApi.sourcesList();
      if (requestId !== sourceRequestSeq.current) return;
      setCatalog((current) => ({
        ...current,
        sources,
        errors: { ...current.errors, sources: undefined },
      }));
    } catch (error) {
      if (requestId !== sourceRequestSeq.current) return;
      setCatalog((current) => ({
        ...current,
        errors: { ...current.errors, sources: errorMessage(error) },
      }));
    }
  }, [catalogApi]);

  const reloadDisplays = useCallback(async (): Promise<void> => {
    const requestId = ++displayRequestSeq.current;
    const [displays, displayState] = await Promise.allSettled([
      catalogApi.displaysList(),
      catalogApi.displayStateList(),
    ]);
    if (requestId !== displayRequestSeq.current) return;
    setCatalog((current) => ({
      ...current,
      connectedOutputs: displays.status === 'fulfilled'
        ? normalizeDisplayOutputs(displays.value.outputs.map(({ name }) => name))
        : current.connectedOutputs,
      persistedDisplayStates: displayState.status === 'fulfilled'
        ? displayState.value
        : current.persistedDisplayStates,
      errors: {
        ...current.errors,
        displays: displays.status === 'rejected' ? errorMessage(displays.reason) : undefined,
        displayState: displayState.status === 'rejected'
          ? errorMessage(displayState.reason)
          : undefined,
      },
    }));
  }, [catalogApi]);

  useEffect(() => {
    void reload();
    return () => {
      fullRequestSeq.current += 1;
      sourceRequestSeq.current += 1;
      displayRequestSeq.current += 1;
    };
  }, [reload]);

  useEffect(() => {
    if (!Number.isFinite(displayPollMs) || displayPollMs <= 0) return undefined;
    const timer = window.setInterval(() => void reloadDisplays(), displayPollMs);
    return () => window.clearInterval(timer);
  }, [displayPollMs, reloadDisplays]);

  return {
    ...catalog,
    ready,
    reload,
    reloadSources,
    reloadDisplays,
  };
}
