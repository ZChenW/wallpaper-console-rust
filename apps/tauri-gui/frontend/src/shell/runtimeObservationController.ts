import type { RuntimeWallpaperObservationDTO } from '../api/types.ts';
import { normalizeDisplayOutputs } from './displayTargets.ts';
import { withRequestDeadline } from './requestDeadline.ts';

export interface RuntimeObservationApi {
  runtimeWallpaperObservations(): Promise<RuntimeWallpaperObservationDTO[]>;
}

export interface RuntimeObservationScheduler {
  setTimer(callback: () => void, delayMs: number): unknown;
  clearTimer(handle: unknown): void;
}

export interface RuntimeObservationControllerOptions {
  readonly api: RuntimeObservationApi;
  readonly connectedOutputs: readonly string[];
  readonly onObservations: (observations: readonly RuntimeWallpaperObservationDTO[]) => void;
  readonly scheduler?: RuntimeObservationScheduler;
  readonly pollMs?: number;
  readonly requestTimeoutMs?: number;
}

interface RuntimePoll {
  readonly token: object;
  readonly promise: Promise<void>;
}

const DEFAULT_POLL_MS = 5_000;
const DEFAULT_REQUEST_TIMEOUT_MS = 4_000;

const DEFAULT_SCHEDULER: RuntimeObservationScheduler = {
  setTimer: (callback, delayMs) => globalThis.setTimeout(callback, delayMs),
  clearTimer: (handle) => globalThis.clearTimeout(handle as number),
};

function pollInterval(value: number | undefined): number {
  return typeof value === 'number' && Number.isFinite(value) && value > 0
    ? Math.floor(value)
    : DEFAULT_POLL_MS;
}

function requestTimeout(value: number | undefined): number {
  if (value === undefined) return DEFAULT_REQUEST_TIMEOUT_MS;
  return typeof value === 'number' && Number.isFinite(value) && value > 0
    ? Math.floor(value)
    : 0;
}

/**
 * Reconciles actual renderer ownership without overlapping probes. Apply
 * completion invalidates any older response before an immediate fresh read.
 */
export class RuntimeObservationController {
  private readonly api: RuntimeObservationApi;
  private readonly connectedOutputs: readonly string[];
  private readonly onObservations: RuntimeObservationControllerOptions['onObservations'];
  private readonly scheduler: RuntimeObservationScheduler;
  private readonly pollMs: number;
  private readonly requestTimeoutMs: number;

  private started = false;
  private generation = 0;
  private timer: unknown | null = null;
  private currentPoll: RuntimePoll | null = null;
  private repollRequested = false;

  constructor(options: RuntimeObservationControllerOptions) {
    this.api = options.api;
    this.connectedOutputs = normalizeDisplayOutputs(options.connectedOutputs);
    this.onObservations = options.onObservations;
    this.scheduler = options.scheduler ?? DEFAULT_SCHEDULER;
    this.pollMs = pollInterval(options.pollMs);
    this.requestTimeoutMs = requestTimeout(options.requestTimeoutMs);
  }

  readonly start = (): void => {
    if (this.started) return;
    this.started = true;
    this.generation += 1;
    void this.refresh();
  };

  readonly stop = (): void => {
    if (!this.started) return;
    this.started = false;
    this.generation += 1;
    this.repollRequested = false;
    this.clearScheduledPoll();
    // Detach the stale bridge request so a visibility resume can issue a fresh
    // observation immediately. Its late result is generation-checked.
    this.currentPoll = null;
  };

  readonly refresh = (): Promise<void> => {
    if (!this.started) return Promise.resolve();
    this.clearScheduledPoll();
    if (this.currentPoll) {
      this.repollRequested = true;
      return this.currentPoll.promise;
    }

    const generation = this.generation;
    const token = {};
    const operation = this.readObservations(generation, token);
    this.currentPoll = { token, promise: operation };
    return operation;
  };

  /** Drops a pre-apply response and reads post-apply renderer state next. */
  readonly invalidateAndRefresh = (): Promise<void> => {
    if (!this.started) return Promise.resolve();
    this.generation += 1;
    this.clearScheduledPoll();
    if (this.currentPoll) {
      this.repollRequested = true;
      return this.currentPoll.promise;
    }
    return this.refresh();
  };

  private async readObservations(generation: number, token: object): Promise<void> {
    try {
      const observations = await withRequestDeadline(
        this.api.runtimeWallpaperObservations(),
        this.requestTimeoutMs,
        'Runtime wallpaper observation',
        this.scheduler,
      );
      if (this.isCurrent(generation)) this.publish(observations);
    } catch {
      if (this.isCurrent(generation)) {
        this.publish(this.connectedOutputs.map((output) => ({
          output,
          wallpaperPath: null,
          status: 'unknown' as const,
        })));
      }
    } finally {
      if (this.currentPoll?.token !== token) return;
      this.currentPoll = null;
      if (!this.started) return;
      if (generation !== this.generation || this.repollRequested) {
        this.repollRequested = false;
        void this.refresh();
        return;
      }
      this.timer = this.scheduler.setTimer(() => {
        this.timer = null;
        void this.refresh();
      }, this.pollMs);
    }
  }

  private isCurrent(generation: number): boolean {
    return this.started && generation === this.generation;
  }

  private publish(observations: readonly RuntimeWallpaperObservationDTO[]): void {
    try {
      this.onObservations(observations);
    } catch {
      // A view callback cannot stop future runtime reconciliation.
    }
  }

  private clearScheduledPoll(): void {
    if (this.timer === null) return;
    this.scheduler.clearTimer(this.timer);
    this.timer = null;
  }
}
