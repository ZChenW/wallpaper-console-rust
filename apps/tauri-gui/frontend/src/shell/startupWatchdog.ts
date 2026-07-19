/**
 * Pure controller for the Library startup watchdog and library_ready gate.
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
  // Otherwise we need the loading to have finished and some content signal.
  if (state.initialLoading) return false;
  return state.hasEntries || state.emptyConfirmed || state.loadError;
}

// ── library_ready gate ──────────────────────────────────────────────────

export interface LibraryReadyGate {
  /** Whether library_ready has already been signalled. */
  readonly called: boolean;
  /**
   * Returns true if `library_ready` should be sent given the current paint
   * state. Returns false if the gate has already been opened.
   */
  shouldSignal(state: LibraryPaintInput): boolean;
  /** Record that `library_ready` has been sent. */
  markCalled(): void;
}

export function createLibraryReadyGate(): LibraryReadyGate {
  let _called = false;

  return {
    get called() {
      return _called;
    },
    shouldSignal(state) {
      if (_called) return false;
      return shouldSignalLibraryPaint(state);
    },
    markCalled() {
      _called = true;
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
