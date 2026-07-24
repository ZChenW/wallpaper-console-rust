import {
  useCallback,
  useEffect,
  useMemo,
  useReducer,
  useRef,
  useState,
} from 'react';

import type { CommandFeedback } from '../api/feedback.ts';
import type { ApplyRequestDTO } from '../api/types.ts';
import {
  useApplyQueue,
  type ApplyQueueApi,
} from '../hooks/useApplyQueue.ts';
import {
  resolveCurrentWallpaperState,
  type CurrentWallpaperState,
} from './currentWallpaperState.ts';
import {
  createRuntimeWallpaperSession,
  reduceRuntimeWallpaperSession,
  toRuntimeDisplayWallpapers,
} from './runtimeWallpaperSession.ts';
import {
  RuntimeObservationController,
  type RuntimeObservationApi,
} from './runtimeObservationController.ts';
import type { DisplayTarget } from './shellPreferences.ts';

export interface RuntimeWallpaperCoordinatorApi
  extends RuntimeObservationApi, ApplyQueueApi {}

export interface RuntimeWallpaperCatalog {
  readonly ready: boolean;
  readonly connectedOutputs: readonly string[];
  readonly reloadDisplays: () => Promise<unknown>;
}

export interface UseRuntimeWallpaperCoordinatorOptions {
  readonly api: RuntimeWallpaperCoordinatorApi;
  readonly catalog: RuntimeWallpaperCatalog;
  readonly displayTarget: DisplayTarget;
  readonly reloadLibrary: () => Promise<unknown>;
  readonly setApplyFeedback: (feedback: CommandFeedback) => void;
}

export interface RuntimeWallpaperCoordinatorResult {
  readonly current: {
    readonly wallpaper: CurrentWallpaperState;
    readonly path: string | null;
    readonly observationReady: boolean;
  };
  readonly apply: {
    readonly applying: boolean;
    readonly activePath: string | null;
    readonly pendingPath: string | null;
    readonly applyActionToDisplay: (
      request: ApplyRequestDTO,
      target?: string,
    ) => void;
    readonly applyToDisplay: (path: string, target?: string) => void;
  };
}

/**
 * Owns renderer observation, optimistic apply evidence, queue state, and the
 * reconciled Current wallpaper. Display/apply policy remains in the view.
 */
export function useRuntimeWallpaperCoordinator({
  api,
  catalog,
  displayTarget,
  reloadLibrary,
  setApplyFeedback,
}: UseRuntimeWallpaperCoordinatorOptions): RuntimeWallpaperCoordinatorResult {
  const refreshStatus = useCallback(async (): Promise<void> => {
    await catalog.reloadDisplays();
  }, [catalog.reloadDisplays]);

  const [runtimeSession, dispatchRuntimeSession] = useReducer(
    reduceRuntimeWallpaperSession,
    [],
    createRuntimeWallpaperSession,
  );
  const [runtimeObservationReady, setRuntimeObservationReady] = useState(false);
  const connectedOutputsKey = catalog.connectedOutputs.join('\0');
  useEffect(() => {
    dispatchRuntimeSession({
      type: 'connectedOutputsChanged',
      connectedOutputs: catalog.connectedOutputs,
    });
  }, [connectedOutputsKey]);

  const runtimeObservationController = useRef<RuntimeObservationController | null>(null);
  useEffect(() => {
    const outputs = [...catalog.connectedOutputs];
    const invalidateRuntimeEvidence = () => {
      setRuntimeObservationReady(false);
      dispatchRuntimeSession({ type: 'runtimeInvalidated' });
    };
    invalidateRuntimeEvidence();
    if (!catalog.ready) {
      return undefined;
    }
    if (outputs.length === 0) {
      setRuntimeObservationReady(true);
      return undefined;
    }
    setRuntimeObservationReady(false);
    const controller = new RuntimeObservationController({
      api,
      connectedOutputs: outputs,
      onObservations: (observations) => {
        dispatchRuntimeSession({ type: 'runtimeReconciled', observations });
        setRuntimeObservationReady(true);
      },
    });
    runtimeObservationController.current = controller;
    let pollingVisible = false;
    const updatePolling = () => {
      if (document.visibilityState === 'hidden') {
        if (!pollingVisible) return;
        pollingVisible = false;
        invalidateRuntimeEvidence();
        controller.stop();
        return;
      }
      if (pollingVisible) return;
      pollingVisible = true;
      invalidateRuntimeEvidence();
      controller.start();
    };
    updatePolling();
    document.addEventListener('visibilitychange', updatePolling);
    return () => {
      document.removeEventListener('visibilitychange', updatePolling);
      if (runtimeObservationController.current === controller) {
        runtimeObservationController.current = null;
      }
      controller.stop();
    };
  }, [api, catalog.ready, connectedOutputsKey]);

  const onApplied = useCallback<
    NonNullable<Parameters<typeof useApplyQueue>[0]['onApplied']>
  >((request, result, transport) => {
    dispatchRuntimeSession({ type: 'applySucceeded', request, result, transport });
    void runtimeObservationController.current?.invalidateAndRefresh();
  }, []);
  const applyQueue = useApplyQueue({
    api,
    refreshStatus,
    setFeedbackWithAutoDismiss: setApplyFeedback,
    reloadLibrary,
    onApplied,
  });

  const currentWallpaper = useMemo(() => resolveCurrentWallpaperState({
    activeTarget: displayTarget,
    connectedOutputs: catalog.connectedOutputs,
    runtime: toRuntimeDisplayWallpapers(runtimeSession),
  }), [
    catalog.connectedOutputs,
    displayTarget,
    runtimeSession,
  ]);
  const currentPath = currentWallpaper.kind === 'confirmed'
    ? currentWallpaper.wallpaperPath
    : null;

  const current = useMemo(() => ({
    wallpaper: currentWallpaper,
    path: currentPath,
    observationReady: runtimeObservationReady,
  }), [currentPath, currentWallpaper, runtimeObservationReady]);
  const apply = useMemo(() => ({
    applying: applyQueue.applying,
    activePath: applyQueue.activePath ?? null,
    pendingPath: applyQueue.pendingPath ?? null,
    applyActionToDisplay: applyQueue.handleApplyActionToDisplay,
    applyToDisplay: applyQueue.handleApplyToDisplay,
  }), [
    applyQueue.activePath,
    applyQueue.applying,
    applyQueue.handleApplyActionToDisplay,
    applyQueue.handleApplyToDisplay,
    applyQueue.pendingPath,
  ]);

  return { current, apply };
}
