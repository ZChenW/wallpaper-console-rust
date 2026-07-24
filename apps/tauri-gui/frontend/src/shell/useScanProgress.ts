import { useEffect, useMemo, useSyncExternalStore } from 'react';

import { scanPresentation, type ScanPresentation } from './feedbackState.ts';
import {
  ScanProgressController,
  type ScanPollingMode,
  type ScanProgressApi,
  type ScanProgressScheduler,
  type ScanProgressSnapshot,
} from './scanProgressController.ts';

export interface ScanProgressView {
  readonly progress: ScanProgressSnapshot['progress'];
  readonly scanState: ScanProgressSnapshot['scanState'];
  readonly presentation: ScanPresentation;
  readonly scanError: string | null;
  readonly transportError: string | null;
  readonly pollingMode: ScanPollingMode;
}

export function toScanProgressView(snapshot: ScanProgressSnapshot): ScanProgressView {
  return {
    progress: snapshot.progress,
    scanState: snapshot.scanState,
    presentation: scanPresentation(snapshot.scanState, snapshot.observedAtMs),
    scanError: snapshot.progress?.error ?? null,
    transportError: snapshot.transportError,
    pollingMode: snapshot.pollingMode,
  };
}

export interface UseScanProgressOptions {
  readonly activePollMs?: number;
  readonly idlePollMs?: number;
  readonly requestTimeoutMs?: number;
  readonly now?: () => number;
  readonly scheduler?: ScanProgressScheduler;
}

/**
 * React adapter for the scan controller. Its callbacks can be passed directly
 * to SourcePanel and ScanActivity without exposing timer ownership to either.
 */
export function useScanProgress(
  api: ScanProgressApi,
  options: UseScanProgressOptions = {},
) {
  const controller = useMemo(() => new ScanProgressController({
    api,
    activePollMs: options.activePollMs,
    idlePollMs: options.idlePollMs,
    requestTimeoutMs: options.requestTimeoutMs,
    now: options.now,
    scheduler: options.scheduler,
  }), [
    api,
    options.activePollMs,
    options.idlePollMs,
    options.requestTimeoutMs,
    options.now,
    options.scheduler,
  ]);
  const snapshot = useSyncExternalStore(
    controller.subscribe,
    controller.getSnapshot,
    controller.getSnapshot,
  );

  useEffect(() => {
    const visible = () => document.visibilityState !== 'hidden';
    const updatePolling = () => {
      if (visible()) controller.start();
      else controller.stop();
    };
    updatePolling();
    document.addEventListener('visibilitychange', updatePolling);
    return () => {
      document.removeEventListener('visibilitychange', updatePolling);
      controller.stop();
    };
  }, [controller]);

  return {
    ...toScanProgressView(snapshot),
    onScanStarted: controller.signalStarted,
    onScanFinished: controller.signalFinished,
    requestCancel: controller.requestCancel,
    dismissCancelled: controller.dismissCancelled,
    refresh: controller.refresh,
  };
}
