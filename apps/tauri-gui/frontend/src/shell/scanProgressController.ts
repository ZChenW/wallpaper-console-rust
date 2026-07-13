import type { CommandResult, ScanProgressDTO } from '../api/types.ts';
import {
  EMPTY_SCAN_STATE,
  scanReducer,
  type ScanState,
} from './feedbackState.ts';

export interface ScanProgressApi {
  scanProgress(): Promise<ScanProgressDTO>;
  scanCancel(): Promise<CommandResult>;
}

export interface ScanProgressScheduler {
  setTimer(callback: () => void, delayMs: number): unknown;
  clearTimer(handle: unknown): void;
}

export type ScanPollingMode = 'stopped' | 'idle' | 'active';

export interface ScanProgressSnapshot {
  readonly progress: ScanProgressDTO | null;
  readonly scanState: ScanState;
  readonly observedAtMs: number;
  readonly transportError: string | null;
  readonly pollingMode: ScanPollingMode;
}

export interface ScanProgressControllerOptions {
  readonly api: ScanProgressApi;
  readonly now?: () => number;
  readonly scheduler?: ScanProgressScheduler;
  readonly activePollMs?: number;
  readonly idlePollMs?: number;
}

const DEFAULT_ACTIVE_POLL_MS = 250;
const DEFAULT_IDLE_POLL_MS = 2_000;

const DEFAULT_SCHEDULER: ScanProgressScheduler = {
  setTimer: (callback, delayMs) => globalThis.setTimeout(callback, delayMs),
  clearTimer: (handle) => {
    if (typeof handle === 'number') globalThis.clearTimeout(handle);
  },
};

function pollInterval(value: number | undefined, fallback: number): number {
  return typeof value === 'number' && Number.isFinite(value) && value > 0
    ? Math.floor(value)
    : fallback;
}

function errorMessage(error: unknown): string {
  if (error instanceof Error && error.message.trim()) return error.message;
  if (typeof error === 'string' && error.trim()) return error;
  return 'Scan progress is temporarily unavailable';
}

function commandError(result: CommandResult): string {
  return result.error?.message
    || result.stderr.trim()
    || result.stdout.trim()
    || 'Could not request scan cancellation';
}

function isCancelledTerminal(progress: ScanProgressDTO): boolean {
  return !progress.running && /cancel/i.test(progress.error ?? '');
}

/**
 * Owns scan polling independently of React so startup, cancellation, and timer
 * races can be verified deterministically.
 */
export class ScanProgressController {
  private readonly api: ScanProgressApi;
  private readonly now: () => number;
  private readonly scheduler: ScanProgressScheduler;
  private readonly activePollMs: number;
  private readonly idlePollMs: number;
  private readonly listeners = new Set<() => void>();

  private snapshot: ScanProgressSnapshot;
  private timer: unknown | null = null;
  private currentPoll: Promise<void> | null = null;
  private cancelRequest: Promise<CommandResult | null> | null = null;
  private repollRequested = false;
  private started = false;
  private generation = 0;

  constructor(options: ScanProgressControllerOptions) {
    this.api = options.api;
    this.now = options.now ?? Date.now;
    this.scheduler = options.scheduler ?? DEFAULT_SCHEDULER;
    this.activePollMs = pollInterval(options.activePollMs, DEFAULT_ACTIVE_POLL_MS);
    this.idlePollMs = pollInterval(options.idlePollMs, DEFAULT_IDLE_POLL_MS);
    this.snapshot = {
      progress: null,
      scanState: EMPTY_SCAN_STATE,
      observedAtMs: this.now(),
      transportError: null,
      pollingMode: 'stopped',
    };
  }

  readonly getSnapshot = (): ScanProgressSnapshot => this.snapshot;

  readonly subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  readonly start = (): void => {
    if (this.started) return;
    this.started = true;
    this.generation += 1;
    this.update({ pollingMode: 'idle', observedAtMs: this.now() });
    void this.refresh();
  };

  readonly stop = (): void => {
    if (!this.started && this.snapshot.pollingMode === 'stopped') return;
    this.started = false;
    this.generation += 1;
    this.repollRequested = false;
    this.clearScheduledPoll();
    this.update({ pollingMode: 'stopped', observedAtMs: this.now() });
  };

  /** Speeds up observation when a source action is about to start scanning. */
  readonly signalStarted = (): void => {
    const nowMs = this.now();
    const scanState = this.snapshot.scanState.kind === 'running'
      ? this.snapshot.scanState
      : scanReducer(this.snapshot.scanState, { type: 'started', nowMs });
    this.update({ scanState, observedAtMs: nowMs });
    if (this.started) this.scheduleNext('active');
  };

  /** Observes the backend immediately; command completion is not assumed to be scan success. */
  readonly signalFinished = (): void => {
    if (this.started) void this.refresh();
  };

  /** Requests a fresh snapshot without allowing overlapping bridge calls. */
  readonly refresh = (): Promise<void> => {
    if (!this.started) return Promise.resolve();
    this.clearScheduledPoll();
    if (this.currentPoll) {
      this.repollRequested = true;
      return this.currentPoll;
    }

    const generation = this.generation;
    const operation = this.readProgress(generation);
    this.currentPoll = operation;
    return operation;
  };

  readonly requestCancel = (): Promise<CommandResult | null> => {
    if (this.cancelRequest) return this.cancelRequest;
    const current = this.snapshot.scanState;
    if (current.kind !== 'running' || current.cancelRequestedAtMs !== null) {
      return Promise.resolve(null);
    }

    const nowMs = this.now();
    this.update({
      scanState: scanReducer(current, { type: 'cancelRequested', nowMs }),
      observedAtMs: nowMs,
      transportError: null,
    });
    if (this.started) this.scheduleNext('active');

    const operation = this.sendCancelRequest();
    this.cancelRequest = operation;
    return operation;
  };

  readonly dismissCancelled = (): void => {
    this.update({
      scanState: scanReducer(this.snapshot.scanState, { type: 'dismissed' }),
      observedAtMs: this.now(),
    });
  };

  private async readProgress(generation: number): Promise<void> {
    try {
      const progress = await this.api.scanProgress();
      if (!this.started || generation !== this.generation) return;
      this.observeProgress(progress);
    } catch (error) {
      if (!this.started || generation !== this.generation) return;
      this.update({
        observedAtMs: this.now(),
        transportError: errorMessage(error),
      });
    } finally {
      this.currentPoll = null;
      if (!this.started) return;
      if (generation !== this.generation || this.repollRequested) {
        this.repollRequested = false;
        void this.refresh();
        return;
      }
      this.scheduleNext(this.shouldPollActively() ? 'active' : 'idle');
    }
  }

  private observeProgress(progress: ScanProgressDTO): void {
    const nowMs = this.now();
    let scanState = this.snapshot.scanState;

    if (progress.running) {
      if (scanState.kind !== 'running') {
        scanState = scanReducer(scanState, { type: 'started', nowMs });
      }
      if (progress.cancelRequested) {
        scanState = scanReducer(scanState, { type: 'cancelRequested', nowMs });
      }
    } else if (scanState.kind === 'running') {
      scanState = scanReducer(scanState, {
        type: isCancelledTerminal(progress) ? 'cancelled' : 'completed',
        nowMs,
      });
    }

    this.update({
      progress,
      scanState,
      observedAtMs: nowMs,
      transportError: null,
    });
  }

  private async sendCancelRequest(): Promise<CommandResult | null> {
    try {
      const result = await this.api.scanCancel();
      if (!result.success) this.restoreCancelAction(commandError(result));
      else if (this.started) void this.refresh();
      return result;
    } catch (error) {
      this.restoreCancelAction(errorMessage(error));
      return null;
    } finally {
      this.cancelRequest = null;
    }
  }

  private restoreCancelAction(message: string): void {
    const current = this.snapshot.scanState;
    const scanState = current.kind === 'running' && current.cancelRequestedAtMs !== null
      ? { ...current, cancelRequestedAtMs: null }
      : current;
    this.update({
      scanState,
      observedAtMs: this.now(),
      transportError: message,
    });
    if (this.started) this.scheduleNext(this.shouldPollActively() ? 'active' : 'idle');
  }

  private shouldPollActively(): boolean {
    return this.snapshot.progress?.running === true || this.snapshot.scanState.kind === 'running';
  }

  private scheduleNext(mode: Exclude<ScanPollingMode, 'stopped'>): void {
    if (!this.started) return;
    this.clearScheduledPoll();
    this.update({ pollingMode: mode });
    const delayMs = mode === 'active' ? this.activePollMs : this.idlePollMs;
    this.timer = this.scheduler.setTimer(() => {
      this.timer = null;
      void this.refresh();
    }, delayMs);
  }

  private clearScheduledPoll(): void {
    if (this.timer === null) return;
    this.scheduler.clearTimer(this.timer);
    this.timer = null;
  }

  private update(change: Partial<ScanProgressSnapshot>): void {
    this.snapshot = { ...this.snapshot, ...change };
    for (const listener of this.listeners) {
      try {
        listener();
      } catch {
        // A view subscriber cannot stop backend progress observation.
      }
    }
  }
}
