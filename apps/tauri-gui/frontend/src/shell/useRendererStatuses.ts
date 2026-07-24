import { useCallback, useEffect, useRef, useState } from 'react';

import { api as defaultApi } from '../api/bridge.ts';
import type { RendererStatusesDTO } from '../api/types.ts';
import { withRequestDeadline } from './requestDeadline.ts';

export interface RendererStatusesApi {
  rendererStatuses(): Promise<RendererStatusesDTO>;
}

export interface RendererStatusRequestSequence {
  begin(): number;
  isLatest(requestId: number): boolean;
  invalidate(): void;
}

export function rendererStatusErrorMessage(error: unknown): string {
  if (error instanceof Error && error.message) return error.message;
  if (typeof error === 'string' && error) return error;
  return 'Renderer status is unavailable';
}

export function createRendererStatusRequestSequence(): RendererStatusRequestSequence {
  let latest = 0;
  return {
    begin() {
      latest += 1;
      return latest;
    },
    isLatest(requestId) {
      return requestId === latest;
    },
    invalidate() {
      latest += 1;
    },
  };
}

export async function loadRendererStatuses(
  statusApi: RendererStatusesApi,
  timeoutMs = 3_000,
): Promise<RendererStatusesDTO> {
  return withRequestDeadline(
    statusApi.rendererStatuses(),
    timeoutMs,
    'Renderer detection',
  );
}

export function useRendererStatuses(
  statusApi: RendererStatusesApi = defaultApi,
  enabled = true,
) {
  const [statuses, setStatuses] = useState<RendererStatusesDTO | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const requestSequence = useRef(createRendererStatusRequestSequence());

  const reload = useCallback(async (): Promise<void> => {
    const requestId = requestSequence.current.begin();
    setLoading(true);
    try {
      const loaded = await loadRendererStatuses(statusApi);
      if (!requestSequence.current.isLatest(requestId)) return;
      setStatuses(loaded);
      setError(null);
    } catch (failure) {
      if (!requestSequence.current.isLatest(requestId)) return;
      setError(rendererStatusErrorMessage(failure));
    } finally {
      if (requestSequence.current.isLatest(requestId)) setLoading(false);
    }
  }, [statusApi]);

  useEffect(() => {
    if (!enabled) {
      requestSequence.current.invalidate();
      setLoading(false);
      return undefined;
    }
    void reload();
    return () => requestSequence.current.invalidate();
  }, [enabled, reload]);

  return {
    statuses,
    loading: enabled && (loading || (statuses === null && error === null)),
    error,
    reload,
  };
}
