import { useCallback, useEffect, useRef, useState } from 'react';

import { api as defaultApi } from '../api/bridge.ts';
import type {
  DisplayListDTO,
  DisplayStateDTO,
  SourceDTO,
} from '../api/types.ts';
import { normalizeDisplayOutputs } from './displayTargets.ts';
import { withRequestDeadline } from './requestDeadline.ts';

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

export interface CatalogChannelTimeouts {
  /** Timeout in ms for the displays channel (default 3000). */
  displaysTimeoutMs?: number;
  /** Timeout in ms for the sources channel (default 3000). */
  sourcesTimeoutMs?: number;
  /** Timeout in ms for the display-state channel (default 3000). */
  displayStateTimeoutMs?: number;
}

const DEFAULT_CHANNEL_TIMEOUT_MS = 3_000;

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

// ── withTimeout ─────────────────────────────────────────────────────────

/**
 * Race `promise` against a timeout. Returns the resolved value on success;
 * rejects with a descriptive error on timeout.
 */
export function withTimeout<T>(
  promise: Promise<T>,
  timeoutMs: number,
  label: string,
): Promise<T> {
  return withRequestDeadline(promise, timeoutMs, label);
}

// ── loadShellCatalogSnapshot (kept for backward compatibility) ──────────

/**
 * Legacy loader that uses Promise.allSettled without timeouts.
 * Kept for useShellCatalog's initial reload path — the hook itself now
 * uses `loadShellCatalogIndependent` for its initial load.
 */
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

// ── loadShellCatalogIndependent ─────────────────────────────────────────

/**
 * Load the three catalog channels independently, each with its own bounded
 * timeout. A hung transport does **not** block the other channels — each
 * timeout enters its own error state and the snapshot is published as soon
 * as all channels have resolved, rejected, or timed out.
 */
export async function loadShellCatalogIndependent(
  catalogApi: ShellCatalogApi,
  timeouts?: CatalogChannelTimeouts,
): Promise<ShellCatalogSnapshot> {
  const displaysTimeout = timeouts?.displaysTimeoutMs ?? DEFAULT_CHANNEL_TIMEOUT_MS;
  const sourcesTimeout = timeouts?.sourcesTimeoutMs ?? DEFAULT_CHANNEL_TIMEOUT_MS;
  const displayStateTimeout = timeouts?.displayStateTimeoutMs ?? DEFAULT_CHANNEL_TIMEOUT_MS;

  // Each channel is independently timed out — fire all three concurrently.
  const displaysPromise = withTimeout(
    catalogApi.displaysList(),
    displaysTimeout,
    'displays',
  );
  const sourcesPromise = withTimeout(
    catalogApi.sourcesList(),
    sourcesTimeout,
    'sources',
  );
  const displayStatePromise = withTimeout(
    catalogApi.displayStateList(),
    displayStateTimeout,
    'displayState',
  );

  const [displays, sources, displayState] = await Promise.allSettled([
    displaysPromise,
    sourcesPromise,
    displayStatePromise,
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

// ── subscribeCatalogChannels ─────────────────────────────────────────────

export interface CatalogChannelSubscriber {
  onDisplays(connectedOutputs: string[], error?: string): void;
  onSources(sources: readonly SourceDTO[], error?: string): void;
  onDisplayState(states: readonly DisplayStateDTO[], error?: string): void;
  onReady(): void;
}

/** Test-only event collector for asserting per-channel publishing order. */
export interface CatalogChannelEvents {
  displays: { outputs: string[]; error?: string; ms: number }[];
  sources: { sources: readonly SourceDTO[]; error?: string; ms: number }[];
  displayState: { states: readonly DisplayStateDTO[]; error?: string; ms: number }[];
  readyCalls: number;
}

/**
 * Subscribe to the three catalog channels. Each channel publishes
 * independently — onDisplays/onSources/onDisplayState fire as soon as that
 * channel resolves, rejects, or times out. onReady fires exactly once after
 * all three channels have reached a terminal state.
 *
 * Returns a cleanup function that prevents any future callbacks and clears
 * pending timers.
 */
export function subscribeCatalogChannels(
  catalogApi: ShellCatalogApi,
  subscriber: CatalogChannelSubscriber,
  timeouts?: CatalogChannelTimeouts,
): () => void {
  let active = true;
  let settled = 0;
  const total = 3;
  const timers: ReturnType<typeof setTimeout>[] = [];

  function clearTimers(): void {
    for (const t of timers) clearTimeout(t);
    timers.length = 0;
  }

  function checkReady(): void {
    settled++;
    if (settled === total) {
      if (active) subscriber.onReady();
    }
  }

  function runChannel<T>(
    label: string,
    promise: Promise<T>,
    timeoutMs: number,
    onOk: (value: T) => void,
  ): void {
    let channelSettled = false;

    // Direct timer so we can track it in `timers` and clear on cleanup.
    const timer = setTimeout(() => {
      if (channelSettled || !active) return;
      channelSettled = true;
      const msg = `${label} timed out after ${timeoutMs}ms`;
      if (label === 'displays') subscriber.onDisplays([], msg);
      else if (label === 'sources') subscriber.onSources([], msg);
      else subscriber.onDisplayState([], msg);
      checkReady();
    }, timeoutMs);
    timers.push(timer);

    promise.then(
      (value) => {
        if (channelSettled || !active) return;
        channelSettled = true;
        clearTimeout(timer);
        onOk(value);
        checkReady();
      },
      (err) => {
        if (channelSettled || !active) return;
        channelSettled = true;
        clearTimeout(timer);
        const msg = errorMessage(err);
        if (label === 'displays') subscriber.onDisplays([], msg);
        else if (label === 'sources') subscriber.onSources([], msg);
        else subscriber.onDisplayState([], msg);
        checkReady();
      },
    );
  }

  const displaysTimeout = timeouts?.displaysTimeoutMs ?? DEFAULT_CHANNEL_TIMEOUT_MS;
  const sourcesTimeout = timeouts?.sourcesTimeoutMs ?? DEFAULT_CHANNEL_TIMEOUT_MS;
  const displayStateTimeout = timeouts?.displayStateTimeoutMs ?? DEFAULT_CHANNEL_TIMEOUT_MS;

  runChannel('displays', catalogApi.displaysList(), displaysTimeout, (value) => {
    if (!active) return;
    subscriber.onDisplays(
      normalizeDisplayOutputs(value.outputs.map(({ name }) => name)),
    );
  });

  runChannel('sources', catalogApi.sourcesList(), sourcesTimeout, (value) => {
    if (!active) return;
    subscriber.onSources(value);
  });

  runChannel('displayState', catalogApi.displayStateList(), displayStateTimeout, (value) => {
    if (!active) return;
    subscriber.onDisplayState(value);
  });

  return () => {
    active = false;
    clearTimers();
  };
}

// ── useShellCatalog hook ────────────────────────────────────────────────

export function useShellCatalog(
  catalogApi: ShellCatalogApi = defaultApi,
  displayPollMs = 5_000,
  channelTimeouts?: CatalogChannelTimeouts,
) {
  const [catalog, setCatalog] = useState<ShellCatalogSnapshot>(EMPTY_CATALOG);
  const [ready, setReady] = useState(false);
  const [sourcesReady, setSourcesReady] = useState(false);

  // Per-channel generations — shared between the initial load and explicit
  // reloads so a late initial callback cannot overwrite a fresher reload.
  // - `fullGeneration` is bumped on mount / unmount only (clean teardown).
  // - `sourceGeneration` is bumped by both the initial load AND reloadSources.
  // - `displayGeneration` is bumped by both the initial load AND reloadDisplays.
  const fullGeneration = useRef(0);
  const sourceGeneration = useRef(0);
  const displayGeneration = useRef(0);

  // Initial load — each channel publishes independently so a hung display
  // probe does not block source discovery or display-state publishing.
  useEffect(() => {
    const fullGen = ++fullGeneration.current;
    const sourceGen = ++sourceGeneration.current;
    const displayGen = ++displayGeneration.current;
    setReady(false);
    setSourcesReady(false);

    const cleanup = subscribeCatalogChannels(catalogApi, {
      onDisplays(connectedOutputs, error) {
        if (displayGen !== displayGeneration.current) return;
        setCatalog((current) => ({
          ...current,
          connectedOutputs,
          errors: { ...current.errors, displays: error },
        }));
      },
      onSources(sources, error) {
        if (sourceGen !== sourceGeneration.current) return;
        setCatalog((current) => ({
          ...current,
          sources,
          errors: { ...current.errors, sources: error },
        }));
        setSourcesReady(true);
      },
      onDisplayState(states, error) {
        if (displayGen !== displayGeneration.current) return;
        setCatalog((current) => ({
          ...current,
          persistedDisplayStates: states,
          errors: { ...current.errors, displayState: error },
        }));
      },
      onReady() {
        if (fullGen !== fullGeneration.current) return;
        setReady(true);
      },
    }, channelTimeouts);

    return () => {
      cleanup();
      // Invalidate all in-flight initial callbacks on unmount.
      fullGeneration.current += 1;
      sourceGeneration.current += 1;
      displayGeneration.current += 1;
    };
    // Only run once on mount / when the api or timeouts change.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [catalogApi, channelTimeouts]);

  const reloadSources = useCallback(async (): Promise<void> => {
    const gen = ++sourceGeneration.current;
    const timeoutMs = channelTimeouts?.sourcesTimeoutMs ?? DEFAULT_CHANNEL_TIMEOUT_MS;
    try {
      const sources = await withTimeout(
        catalogApi.sourcesList(),
        timeoutMs,
        'sources',
      );
      if (gen !== sourceGeneration.current) return;
      setCatalog((current) => ({
        ...current,
        sources,
        errors: { ...current.errors, sources: undefined },
      }));
      setSourcesReady(true);
    } catch (error) {
      if (gen !== sourceGeneration.current) return;
      setCatalog((current) => ({
        ...current,
        errors: { ...current.errors, sources: errorMessage(error) },
      }));
      setSourcesReady(true);
    }
  }, [catalogApi, channelTimeouts]);

  const reloadDisplays = useCallback(async (): Promise<void> => {
    const gen = ++displayGeneration.current;
    const displaysTimeout = channelTimeouts?.displaysTimeoutMs ?? DEFAULT_CHANNEL_TIMEOUT_MS;
    const displayStateTimeout = channelTimeouts?.displayStateTimeoutMs ?? DEFAULT_CHANNEL_TIMEOUT_MS;

    const displaysPromise = withTimeout(
      catalogApi.displaysList(),
      displaysTimeout,
      'displays',
    );
    const displayStatePromise = withTimeout(
      catalogApi.displayStateList(),
      displayStateTimeout,
      'displayState',
    );

    const [displays, displayState] = await Promise.allSettled([
      displaysPromise,
      displayStatePromise,
    ]);

    if (gen !== displayGeneration.current) return;
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
  }, [catalogApi, channelTimeouts]);

  // ── Unmount: invalidate all in-flight work ───────────────────────────
  useEffect(() => {
    return () => {
      fullGeneration.current += 1;
      sourceGeneration.current += 1;
      displayGeneration.current += 1;
    };
  }, []);

  useEffect(() => {
    if (!Number.isFinite(displayPollMs) || displayPollMs <= 0) return undefined;
    let timer: number | null = null;
    const visible = () => (
      typeof document === 'undefined' || document.visibilityState !== 'hidden'
    );
    const stopTimer = () => {
      if (timer === null) return;
      window.clearInterval(timer);
      timer = null;
    };
    const startTimer = () => {
      stopTimer();
      if (!visible()) return;
      timer = window.setInterval(() => void reloadDisplays(), displayPollMs);
    };
    const handleVisibilityChange = () => {
      if (!visible()) {
        stopTimer();
        return;
      }
      void reloadDisplays();
      startTimer();
    };

    startTimer();
    document.addEventListener('visibilitychange', handleVisibilityChange);
    return () => {
      stopTimer();
      document.removeEventListener('visibilitychange', handleVisibilityChange);
    };
  }, [displayPollMs, reloadDisplays]);

  return {
    ...catalog,
    ready,
    sourcesReady,
    reloadSources,
    reloadDisplays,
  };
}
