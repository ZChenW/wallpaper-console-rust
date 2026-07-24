import type { CommandFeedback } from '../api/feedback.ts';
import { commandErrorFeedback } from '../api/feedback.ts';
import type {
  CommandResult,
  FirstRunSourceSuggestionDTO,
} from '../api/types.ts';
import {
  faultAfterVerification,
  type LibraryRepairFault,
  type LibraryVerificationApi,
  verifyLibraryIntegrity,
} from './libraryRepair.ts';
import {
  createLibraryReadyDelivery,
  createLibraryWatchdog,
  type LibraryReadyDelivery,
  type LibraryReadyTimers,
  type LibraryWatchdog,
} from './startupWatchdog.ts';
import type { ShellNoticeInput } from './useShellFeedback.ts';

export interface LibraryLifecycleApi extends LibraryVerificationApi {
  firstRunSourceSuggestions(): Promise<FirstRunSourceSuggestionDTO[]>;
  libraryReady(): Promise<void>;
  sqliteRepair(): Promise<CommandResult>;
}

export interface LibraryLifecycleDependencies {
  readonly api: LibraryLifecycleApi;
  readonly reloadLibrary: () => Promise<unknown>;
  readonly reloadSources: () => Promise<unknown>;
  readonly showNotice: (notice: ShellNoticeInput) => void;
  readonly setSystemFeedback: (feedback: CommandFeedback) => void;
  readonly watchdog?: LibraryWatchdog;
  readonly readyTimers?: LibraryReadyTimers;
}

export interface LibraryLifecycleSnapshot {
  readonly firstRunSuggestions: readonly FirstRunSourceSuggestionDTO[];
  readonly firstRunSuggestionsError: string | null;
  readonly initialRequestTimedOut: boolean;
  readonly watchdogRetry: number;
  readonly repairFault: LibraryRepairFault | null;
  readonly repairPending: boolean;
}

const EMPTY_SUGGESTIONS: readonly FirstRunSourceSuggestionDTO[] = Object.freeze([]);

const INITIAL_SNAPSHOT: LibraryLifecycleSnapshot = Object.freeze({
  firstRunSuggestions: EMPTY_SUGGESTIONS,
  firstRunSuggestionsError: null,
  initialRequestTimedOut: false,
  watchdogRetry: 0,
  repairFault: null,
  repairPending: false,
});

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function snapshotsEqual(
  left: LibraryLifecycleSnapshot,
  right: LibraryLifecycleSnapshot,
): boolean {
  return left.firstRunSuggestions === right.firstRunSuggestions
    && left.firstRunSuggestionsError === right.firstRunSuggestionsError
    && left.initialRequestTimedOut === right.initialRequestTimedOut
    && left.watchdogRetry === right.watchdogRetry
    && left.repairFault === right.repairFault
    && left.repairPending === right.repairPending;
}

/**
 * Owns the non-React state machine for Library startup, optional first-run
 * suggestions, integrity verification, and repair.
 *
 * React wiring lives in `useLibraryLifecycle`; callers observe this module
 * only through snapshots and high-level operations.
 */
export class LibraryLifecycleController {
  private readonly dependencies: LibraryLifecycleDependencies;
  private readonly readyDelivery: LibraryReadyDelivery;
  private readonly watchdog: LibraryWatchdog;
  private snapshotValue: LibraryLifecycleSnapshot = INITIAL_SNAPSHOT;
  private listeners = new Set<(snapshot: LibraryLifecycleSnapshot) => void>();
  private firstRunRequest = 0;
  private verificationRequest = 0;
  private watchdogCleanup: (() => void) | null = null;

  constructor(dependencies: LibraryLifecycleDependencies) {
    this.dependencies = dependencies;
    this.watchdog = dependencies.watchdog ?? createLibraryWatchdog();
    this.readyDelivery = createLibraryReadyDelivery(
      () => dependencies.api.libraryReady(),
      dependencies.readyTimers,
    );
  }

  get snapshot(): LibraryLifecycleSnapshot {
    return this.snapshotValue;
  }

  subscribe(listener: (snapshot: LibraryLifecycleSnapshot) => void): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  private update(
    update: (current: LibraryLifecycleSnapshot) => LibraryLifecycleSnapshot,
  ): void {
    const next = update(this.snapshotValue);
    if (next === this.snapshotValue || snapshotsEqual(next, this.snapshotValue)) return;
    Object.freeze(next);
    this.snapshotValue = next;
    for (const listener of this.listeners) listener(next);
  }

  requestFirstRunSuggestions(eligible: boolean): () => void {
    const requestId = ++this.firstRunRequest;
    if (!eligible) {
      this.update((current) => ({
        ...current,
        firstRunSuggestions: EMPTY_SUGGESTIONS,
        firstRunSuggestionsError: null,
      }));
      return () => {};
    }

    this.update((current) => ({
      ...current,
      firstRunSuggestionsError: null,
    }));
    void this.dependencies.api.firstRunSourceSuggestions().then(
      (suggestions) => {
        if (this.firstRunRequest !== requestId) return;
        this.update((current) => ({
          ...current,
          firstRunSuggestions: Object.freeze([...suggestions]),
        }));
      },
      (error: unknown) => {
        if (this.firstRunRequest !== requestId) return;
        this.update((current) => ({
          ...current,
          firstRunSuggestions: EMPTY_SUGGESTIONS,
          firstRunSuggestionsError: errorMessage(error),
        }));
      },
    );

    return () => {
      if (this.firstRunRequest === requestId) this.firstRunRequest += 1;
    };
  }

  activateReadyDelivery(active: boolean): () => void {
    if (!active) return () => {};
    this.readyDelivery.activate();
    return () => this.readyDelivery.deactivate();
  }

  watchInitialRequest(paintActive: boolean, timeoutMs: number): () => void {
    this.clearWatchdog();
    if (!paintActive) {
      let cleanup = () => {};
      cleanup = this.watchdog.arm(timeoutMs, () => {
        if (this.watchdogCleanup === cleanup) this.watchdogCleanup = null;
        this.update((current) => ({
          ...current,
          initialRequestTimedOut: true,
        }));
      });
      this.watchdogCleanup = cleanup;
    }
    return () => this.clearWatchdog();
  }

  clearTimeoutIf(resolved: boolean): void {
    if (!resolved || !this.snapshotValue.initialRequestTimedOut) return;
    this.update((current) => ({
      ...current,
      initialRequestTimedOut: false,
    }));
  }

  retryInitialRequest(): void {
    this.update((current) => ({
      ...current,
      initialRequestTimedOut: false,
      watchdogRetry: current.watchdogRetry + 1,
    }));
  }

  private clearWatchdog(): void {
    const cleanup = this.watchdogCleanup;
    this.watchdogCleanup = null;
    cleanup?.();
  }

  verifyIntegrity(shouldVerify: boolean): () => void {
    const requestId = ++this.verificationRequest;
    if (!shouldVerify || this.snapshotValue.repairPending) return () => {};

    void verifyLibraryIntegrity(this.dependencies.api).then((outcome) => {
      if (this.verificationRequest !== requestId) return;
      this.update((current) => ({
        ...current,
        repairFault: faultAfterVerification(current.repairFault, outcome),
      }));
    });

    return () => {
      if (this.verificationRequest === requestId) this.verificationRequest += 1;
    };
  }

  async repairLibrary(): Promise<void> {
    if (this.snapshotValue.repairPending) return;
    this.update((current) => ({ ...current, repairPending: true }));
    try {
      const result = await this.dependencies.api.sqliteRepair();
      if (!result.success) {
        this.dependencies.setSystemFeedback(commandErrorFeedback('Library repair', result));
        return;
      }
      const verification = await verifyLibraryIntegrity(this.dependencies.api);
      if (verification.status !== 'ok') {
        this.update((current) => ({
          ...current,
          repairFault: faultAfterVerification(current.repairFault, verification),
        }));
        this.dependencies.showNotice({
          channel: 'system',
          severity: 'error',
          message: verification.status === 'corrupt'
            ? 'Library repair could not restore database integrity.'
            : 'Library repair finished, but database integrity could not be verified.',
          technicalDetails: verification.status === 'corrupt'
            ? verification.fault.technicalDetails
            : verification.technicalDetails,
        });
        return;
      }
      this.update((current) => ({ ...current, repairFault: null }));
      await Promise.allSettled([
        this.dependencies.reloadLibrary(),
        this.dependencies.reloadSources(),
      ]);
      this.dependencies.showNotice({
        channel: 'system',
        severity: 'success',
        message: 'Library index repaired',
      });
    } catch (error) {
      this.dependencies.setSystemFeedback(commandErrorFeedback('Library repair', error));
    } finally {
      this.update((current) => ({ ...current, repairPending: false }));
    }
  }

  reloadLibrary(): Promise<unknown> {
    return this.dependencies.reloadLibrary();
  }

  async reconcileSourcesAndLibrary(): Promise<void> {
    await Promise.allSettled([
      this.dependencies.reloadSources(),
      this.dependencies.reloadLibrary(),
    ]);
  }
}
