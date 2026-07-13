import { useCallback, useEffect, useRef, useState } from 'react';

import { api as defaultApi } from '../api/bridge.ts';
import type { CommandResult, SourceDTO } from '../api/types.ts';

export interface WallpaperSourcesApi {
  sourcesList(): Promise<SourceDTO[]>;
  browseDirectory(): Promise<string>;
  sourceAdd(path: string): Promise<CommandResult>;
  sourceRename(id: number, displayName: string): Promise<CommandResult>;
  sourceSetRecursive(id: number, recursive: boolean): Promise<CommandResult>;
  sourceRefresh(id: number): Promise<CommandResult>;
  sourceRemoveById(id: number): Promise<CommandResult>;
  scanSteamWorkshop(): Promise<CommandResult>;
}

export type AddSourceOutcome =
  | { readonly kind: 'cancelled' }
  | { readonly kind: 'completed'; readonly path: string; readonly result: CommandResult };

export interface UseWallpaperSourcesOptions {
  readonly sourceApi?: WallpaperSourcesApi;
  readonly onLibraryChanged?: () => void | Promise<void>;
}

/**
 * Reconciliation is intentionally best-effort and always runs. A source can
 * already be saved when its immediate scan reports failure or cancellation.
 */
export async function executeSourceMutation<T>(
  operation: () => Promise<T>,
  reconcile: () => Promise<void>,
): Promise<T> {
  let result: T;
  try {
    result = await operation();
  } catch (error) {
    try {
      await reconcile();
    } catch {
      // Preserve the primary transport/command exception.
    }
    throw error;
  }

  try {
    await reconcile();
  } catch {
    // The source reload exposes its own error state; it must not hide result.
  }
  return result;
}

export function formatSourceLoadError(error: unknown): string {
  if (error instanceof Error && error.message) return error.message;
  if (typeof error === 'string' && error) return error;
  return 'Failed to load wallpaper sources';
}

export function useWallpaperSources({
  sourceApi = defaultApi,
  onLibraryChanged,
}: UseWallpaperSourcesOptions = {}) {
  const [sources, setSources] = useState<SourceDTO[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [pendingOperation, setPendingOperation] = useState<string | null>(null);
  const loadRequestSeq = useRef(0);
  const activeOperation = useRef<string | null>(null);

  const reload = useCallback(async (): Promise<void> => {
    const requestId = loadRequestSeq.current + 1;
    loadRequestSeq.current = requestId;
    setLoading(true);
    try {
      const loaded = await sourceApi.sourcesList();
      if (loadRequestSeq.current !== requestId) return;
      setSources(loaded);
      setLoadError(null);
    } catch (error) {
      if (loadRequestSeq.current !== requestId) return;
      setLoadError(formatSourceLoadError(error));
    } finally {
      if (loadRequestSeq.current === requestId) setLoading(false);
    }
  }, [sourceApi]);

  useEffect(() => {
    void reload();
    return () => {
      loadRequestSeq.current += 1;
    };
  }, [reload]);

  const reconcile = useCallback(async (): Promise<void> => {
    const tasks: Promise<unknown>[] = [reload()];
    if (onLibraryChanged) {
      tasks.push(Promise.resolve().then(onLibraryChanged));
    }
    await Promise.allSettled(tasks);
  }, [onLibraryChanged, reload]);

  const runOperation = useCallback(async <T,>(
    key: string,
    operation: () => Promise<T>,
  ): Promise<T> => {
    if (activeOperation.current !== null) {
      throw new Error(`Source operation already running: ${activeOperation.current}`);
    }
    activeOperation.current = key;
    setPendingOperation(key);
    try {
      return await operation();
    } finally {
      if (activeOperation.current === key) {
        activeOperation.current = null;
        setPendingOperation(null);
      }
    }
  }, []);

  const addFromPicker = useCallback((): Promise<AddSourceOutcome> => runOperation(
    'add',
    async () => {
      const path = await sourceApi.browseDirectory();
      if (!path) return { kind: 'cancelled' };
      const result = await executeSourceMutation(() => sourceApi.sourceAdd(path), reconcile);
      return { kind: 'completed', path, result };
    },
  ), [reconcile, runOperation, sourceApi]);

  const rename = useCallback(
    (id: number, displayName: string) => runOperation(
      `rename:${id}`,
      () => executeSourceMutation(
        () => sourceApi.sourceRename(id, displayName),
        reconcile,
      ),
    ),
    [reconcile, runOperation, sourceApi],
  );

  const setRecursive = useCallback(
    (id: number, recursive: boolean) => runOperation(
      `recursive:${id}`,
      () => executeSourceMutation(
        () => sourceApi.sourceSetRecursive(id, recursive),
        reconcile,
      ),
    ),
    [reconcile, runOperation, sourceApi],
  );

  const refresh = useCallback(
    (id: number) => runOperation(
      `refresh:${id}`,
      () => executeSourceMutation(() => sourceApi.sourceRefresh(id), reconcile),
    ),
    [reconcile, runOperation, sourceApi],
  );

  const remove = useCallback(
    (id: number) => runOperation(
      `remove:${id}`,
      () => executeSourceMutation(() => sourceApi.sourceRemoveById(id), reconcile),
    ),
    [reconcile, runOperation, sourceApi],
  );

  const scanWallpaperEngine = useCallback(
    () => runOperation(
      'scanWallpaperEngine',
      () => executeSourceMutation(() => sourceApi.scanSteamWorkshop(), reconcile),
    ),
    [reconcile, runOperation, sourceApi],
  );

  return {
    sources,
    loading,
    loadError,
    pendingOperation,
    reload,
    addFromPicker,
    rename,
    setRecursive,
    refresh,
    remove,
    scanWallpaperEngine,
  };
}
