import type {
  ApplyRequestDTO,
  ApplyResultDTO,
  TargetedApplyRequestDTO,
} from '../api/bridge';
import type { CommandFeedback } from '../api/feedback';
import type { ApplyStagePayload } from '../events/appEvents';

export type ApplyStage = 'queued' | 'starting backend' | 'settling' | 'applied';

interface ApplyCommandResult {
  success: boolean;
  stdout: string;
  stderr: string;
  error?: { message: string };
}

export type AppliedRequest = ApplyRequestDTO | TargetedApplyRequestDTO;
export type ApplyTransport = 'action' | 'targeted';

export interface ApplyQueueState {
  applying: boolean;
  activePath: string | undefined;
  pendingPath: string | undefined;
}

export interface ApplyQueueDeps {
  applyAction: (request: ApplyRequestDTO) => Promise<ApplyCommandResult>;
  applyToDisplay: (request: TargetedApplyRequestDTO) => Promise<ApplyCommandResult>;
  refreshStatus: () => Promise<void>;
  invalidateLibrary: () => void;
  setFeedback: (feedback: CommandFeedback) => void;
  makeErrorFeedback: (label: string, error: unknown) => CommandFeedback;
  recordMetric?: (name: string, value: number) => void;
  subscribeApplyStage?: (handler: (event: ApplyStagePayload) => void) => () => void;
  onApplied?: (
    request: AppliedRequest,
    result: ApplyResultDTO | undefined,
    transport: ApplyTransport,
  ) => void;
}

type QueuedApply =
  | { transport: 'action'; request: ApplyRequestDTO }
  | { transport: 'targeted'; request: TargetedApplyRequestDTO };

export interface ApplyQueueEnqueuer {
  enqueue(request: ApplyRequestDTO): void;
  enqueueTargeted(request: TargetedApplyRequestDTO): void;
}

export function createApplyQueueHandlers(
  controller: ApplyQueueEnqueuer,
  makeRequestId: () => string = () => `${Date.now()}-${Math.random().toString(36).slice(2)}`,
) {
  return {
    handleApplyAction(request: ApplyRequestDTO): void {
      controller.enqueue(request);
    },
    handleApply(path: string): void {
      controller.enqueue({ kind: 'apply', path, requestId: makeRequestId() });
    },
    handleApplyToDisplay(path: string, target?: string): void {
      const request: TargetedApplyRequestDTO = {
        path,
        requestId: makeRequestId(),
        ...(target === undefined ? {} : { target }),
      };
      controller.enqueueTargeted(request);
    },
    handleApplyActionToDisplay(request: ApplyRequestDTO, target?: string): void {
      const targeted: TargetedApplyRequestDTO = {
        kind: request.kind,
        path: request.path,
        requestId: request.requestId ?? makeRequestId(),
        ...(target === undefined ? {} : { target }),
      };
      controller.enqueueTargeted(targeted);
    },
  };
}

function parseApplyResult(stdout: string): ApplyResultDTO | undefined {
  if (!stdout) return undefined;
  try {
    const parsed: unknown = JSON.parse(stdout);
    if (typeof parsed !== 'object' || parsed === null) return undefined;
    const value = parsed as Record<string, unknown>;
    if (
      typeof value.appliedPath !== 'string'
      || typeof value.statePath !== 'string'
      || typeof value.backend !== 'string'
      || typeof value.fileType !== 'string'
      || typeof value.preview !== 'boolean'
      || (
        value.appliedOutputs !== undefined
        && (
          !Array.isArray(value.appliedOutputs)
          || !value.appliedOutputs.every((output) => typeof output === 'string')
        )
      )
      || (
        value.requestId !== undefined
        && value.requestId !== null
        && typeof value.requestId !== 'string'
      )
    ) {
      return undefined;
    }
    return {
      ...(typeof value.requestId === 'string' ? { requestId: value.requestId } : {}),
      appliedPath: value.appliedPath,
      statePath: value.statePath,
      backend: value.backend,
      fileType: value.fileType,
      preview: value.preview,
      ...(Array.isArray(value.appliedOutputs)
        ? { appliedOutputs: value.appliedOutputs as string[] }
        : {}),
    };
  } catch {
    return undefined;
  }
}

export class ApplyQueueController {
  private applying = false;
  private active: QueuedApply | null = null;
  private pending: QueuedApply | null = null;
  private readonly onApplyingChange: (value: boolean) => void;
  private readonly onQueueStateChange: (state: ApplyQueueState) => void;
  private readonly deps: ApplyQueueDeps;

  constructor(
    deps: ApplyQueueDeps,
    onApplyingChange: (value: boolean) => void,
    onQueueStateChange: (state: ApplyQueueState) => void = () => {},
  ) {
    this.deps = deps;
    this.onApplyingChange = onApplyingChange;
    this.onQueueStateChange = onQueueStateChange;
  }

  isApplying(): boolean {
    return this.applying;
  }

  getState(): ApplyQueueState {
    return {
      applying: this.applying,
      activePath: this.active?.request.path,
      pendingPath: this.pending?.request.path,
    };
  }

  enqueue(request: ApplyRequestDTO): void {
    this.enqueueItem({ transport: 'action', request });
  }

  enqueueTargeted(request: TargetedApplyRequestDTO): void {
    this.enqueueItem({ transport: 'targeted', request });
  }

  private enqueueItem(item: QueuedApply): void {
    if (this.applying) {
      this.pending = item;
      this.emitQueueState();
      return;
    }
    void this.run(item);
  }

  private async run(first: QueuedApply): Promise<void> {
    this.applying = true;
    this.active = first;
    this.onApplyingChange(true);
    this.emitQueueState();

    while (this.active !== null) {
      const current: QueuedApply = this.active;
      await this.execute(current);

      const next: QueuedApply | null = this.pending;
      this.pending = null;
      const currentId: string | undefined = current.request.requestId;
      const nextId: string | undefined = next?.request.requestId;
      this.active = next && (!currentId || !nextId || nextId !== currentId) ? next : null;
      if (this.active !== null) this.emitQueueState();
    }

    this.applying = false;
    this.onApplyingChange(false);
    this.emitQueueState();
  }

  private async execute(item: QueuedApply): Promise<void> {
    const request = item.request;
    const requestStart = performance.now();
    const queuedSuffix = () => (this.pending !== null ? ' · Next wallpaper queued.' : '');
    const unsubscribeStage = this.deps.subscribeApplyStage?.((event) => {
      if (request.requestId) {
        if (event.requestId !== request.requestId) return;
      } else if (event.requestId) {
        return;
      }
      this.deps.setFeedback({
        state: 'running',
        label: event.label,
        detail: `${event.detail}${queuedSuffix()}`,
      });
    }) ?? (() => {});

    try {
      let result: ApplyCommandResult;
      try {
        result = item.transport === 'targeted'
          ? await this.deps.applyToDisplay(item.request)
          : await this.deps.applyAction(item.request);
      } catch (error) {
        this.reportFailure(error);
        return;
      }

      if (!result.success) {
        this.reportFailure(result);
        return;
      }

      const detail = parseApplyResult(result.stdout);
      const evidence = request.requestId !== undefined
        ? detail?.requestId === request.requestId ? detail : undefined
        : detail;
      try {
        this.deps.onApplied?.(request, evidence, item.transport);
      } catch {
        // A UI observer cannot turn a confirmed backend success into an apply failure.
      }
      if (request.kind === 'retry_backend_apply') {
        this.deps.invalidateLibrary();
      }

      this.emitStage('settling', false);
      try {
        await this.deps.refreshStatus();
      } catch {
        // Status refresh is secondary; the backend has already confirmed success.
      }
      this.deps.setFeedback({
        state: 'success',
        label: 'Applied',
        detail: detail?.preview ? 'Preview wallpaper applied.' : detail?.appliedPath.split('/').pop(),
      });
      this.deps.recordMetric?.('apply.request.ms', performance.now() - requestStart);
    } finally {
      unsubscribeStage();
    }
  }

  private reportFailure(error: unknown): void {
    this.deps.invalidateLibrary();
    this.deps.setFeedback(this.deps.makeErrorFeedback('Apply', error));
  }

  private emitQueueState(): void {
    this.onQueueStateChange(this.getState());
  }

  private emitStage(stage: ApplyStage, isBackendApply: boolean): void {
    const queuedSuffix = this.pending !== null ? ' · Next wallpaper queued.' : '';
    switch (stage) {
      case 'queued':
        break;
      case 'starting backend':
        this.deps.setFeedback({
          state: 'running',
          label: 'Applying wallpaper',
          detail: isBackendApply
            ? `Starting renderer. Scene wallpapers may take several seconds.${queuedSuffix}`
            : `Applying wallpaper.${queuedSuffix}`,
        });
        break;
      case 'settling':
        this.deps.setFeedback({
          state: 'running',
          label: 'Applying wallpaper',
          detail: `Settling…${queuedSuffix}`,
        });
        break;
      case 'applied':
        break;
    }
  }
}
