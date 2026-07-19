/**
 * Pure controllers for the Library startup watchdog and library_ready delivery.
 *
 * These helpers are separated from React so they can be tested without
 * mounting components or manipulating real timers.
 */

// ── Library paint detection ─────────────────────────────────────────────

export interface LibraryPaintInput {
  readonly initialLoading: boolean;
  readonly hasEntries: boolean;
  readonly emptyConfirmed: boolean;
  readonly loadError: boolean;
  readonly timedOut: boolean;
}

/**
 * Returns true when the Library has achieved its first meaningful paint:
 * cards, an empty state, a load error, **or a timeout retry state**.
 *
 * The timeout retry state itself counts as a first Library paint so that
 * `library_ready` is sent even when the first browser request hangs forever.
 * Without this the backend would block freshness checks indefinitely waiting
 * for a signal that never arrives.
 */
export function shouldSignalLibraryPaint(state: LibraryPaintInput): boolean {
  // A timeout counts as paint regardless of initialLoading.
  if (state.timedOut) return true;
  return shouldClearLibraryTimeout(state);
}

/** Returns true only when a timed-out request has since reached a real result. */
export function shouldClearLibraryTimeout(state: LibraryPaintInput): boolean {
  if (state.initialLoading) return false;
  return state.hasEntries || state.emptyConfirmed || state.loadError;
}

// ── library_ready delivery ──────────────────────────────────────────────

export interface LibraryReadyTimers {
  setTimer(callback: () => void, delayMs: number): unknown;
  clearTimer(handle: unknown): void;
}

export interface LibraryReadyDelivery {
  /** Whether the backend has acknowledged library_ready. */
  readonly acknowledged: boolean;
  /** Begin or resume delivery. Safe to call while a send is already active. */
  activate(): void;
  /** Cancel pending retries while preserving in-flight and acknowledgement state. */
  deactivate(): void;
}

const LIBRARY_READY_RETRY_DELAYS_MS = [250, 1_000, 2_000, 5_000] as const;

const DEFAULT_LIBRARY_READY_TIMERS: LibraryReadyTimers = {
  setTimer: (callback, delayMs) => globalThis.setTimeout(callback, delayMs),
  clearTimer: (handle) => globalThis.clearTimeout(handle as number),
};

export function createLibraryReadyDelivery(
  send: () => Promise<void>,
  timers: LibraryReadyTimers = DEFAULT_LIBRARY_READY_TIMERS,
): LibraryReadyDelivery {
  let active = false;
  let acknowledged = false;
  let inFlight = false;
  let retryIndex = 0;
  let pendingRetry: { handle: unknown } | null = null;

  const scheduleRetry = (): void => {
    const delayIndex = Math.min(retryIndex, LIBRARY_READY_RETRY_DELAYS_MS.length - 1);
    const delayMs = LIBRARY_READY_RETRY_DELAYS_MS[delayIndex];
    retryIndex += 1;
    if (!active) return;

    const retry = {
      handle: timers.setTimer(() => {
        if (pendingRetry !== retry) return;
        pendingRetry = null;
        attemptSend();
      }, delayMs),
    };
    pendingRetry = retry;
  };

  const finishRejectedSend = (): void => {
    inFlight = false;
    scheduleRetry();
  };

  const attemptSend = (): void => {
    if (!active || acknowledged || inFlight || pendingRetry !== null) return;
    inFlight = true;

    let request: Promise<void>;
    try {
      request = send();
    } catch {
      finishRejectedSend();
      return;
    }

    void request.then(
      () => {
        inFlight = false;
        acknowledged = true;
      },
      () => finishRejectedSend(),
    );
  };

  return {
    get acknowledged() {
      return acknowledged;
    },
    activate() {
      active = true;
      attemptSend();
    },
    deactivate() {
      active = false;
      if (pendingRetry === null) return;
      timers.clearTimer(pendingRetry.handle);
      pendingRetry = null;
    },
  };
}

// ── Watchdog controller ─────────────────────────────────────────────────

export interface LibraryWatchdog {
  /**
   * Arm the watchdog. If a previous arm is still active it is cancelled
   * first. Returns a cleanup function that disarms the current timer.
   */
  arm(timeoutMs: number, onTimeout: () => void): () => void;
}

export function createLibraryWatchdog(): LibraryWatchdog {
  let timer: ReturnType<typeof setTimeout> | null = null;

  return {
    arm(timeoutMs, onTimeout) {
      // Cancel any previous timer.
      if (timer !== null) {
        clearTimeout(timer);
        timer = null;
      }

      timer = setTimeout(() => {
        timer = null;
        onTimeout();
      }, timeoutMs);

      return () => {
        if (timer !== null) {
          clearTimeout(timer);
          timer = null;
        }
      };
    },
  };
}
