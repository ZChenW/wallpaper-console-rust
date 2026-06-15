import { useCallback, useRef, useState } from 'react';
import { api, ApplyRequestDTO, ApplyResultDTO } from '../api/bridge';
import { commandErrorFeedback, CommandFeedback } from '../api/feedback';

export interface ApplyQueueDeps {
  applyAction: (request: ApplyRequestDTO) => Promise<{ success: boolean; stdout: string; stderr: string; error?: { message: string } }>;
  refreshStatus: () => Promise<void>;
  invalidateHistory: () => void;
  setFeedback: (feedback: CommandFeedback) => void;
  makeErrorFeedback: (label: string, error: unknown) => CommandFeedback;
}

export class ApplyQueueController {
  private applying = false;
  private pending: ApplyRequestDTO | null = null;
  private readonly onApplyingChange: (value: boolean) => void;
  private readonly deps: ApplyQueueDeps;

  constructor(deps: ApplyQueueDeps, onApplyingChange: (value: boolean) => void) {
    this.deps = deps;
    this.onApplyingChange = onApplyingChange;
  }

  isApplying(): boolean {
    return this.applying;
  }

  enqueue(request: ApplyRequestDTO): void {
    if (this.applying) {
      this.pending = request;
      return;
    }
    void this.run(request);
  }

  private async run(first: ApplyRequestDTO): Promise<void> {
    this.applying = true;
    this.onApplyingChange(true);
    let current: ApplyRequestDTO | null = first;

    while (current !== null) {
      const req = current;
      current = null;
      const isBackendApply = req.kind === 'apply' || req.kind === 'retry_backend_apply';
      this.deps.setFeedback({
        state: 'running',
        label: 'Applying wallpaper',
        detail: isBackendApply ? 'Starting renderer. Scene wallpapers may take several seconds.' : undefined,
      });
      try {
        const result = await this.deps.applyAction(req);
        if (result.success) {
          this.deps.invalidateHistory();
          let detail: ApplyResultDTO | undefined;
          try {
            detail = result.stdout ? JSON.parse(result.stdout) as ApplyResultDTO : undefined;
          } catch {
            detail = undefined;
          }
          this.deps.setFeedback({
            state: 'success',
            label: 'Applied',
            detail: detail?.preview ? 'Preview wallpaper applied.' : detail?.appliedPath?.split('/').pop(),
          });
        } else {
          this.deps.setFeedback(this.deps.makeErrorFeedback('Apply', result));
        }
        await this.deps.refreshStatus();
      } catch (error) {
        this.deps.setFeedback(this.deps.makeErrorFeedback('Apply', error));
      }

      const next = this.pending;
      this.pending = null;
      if (next && next.requestId !== req.requestId) {
        current = next;
      }
    }

    this.applying = false;
    this.onApplyingChange(false);
  }
}

export function useApplyQueue(args: {
  refreshStatus: () => Promise<void>;
  setFeedbackWithAutoDismiss: (feedback: CommandFeedback) => void;
  invalidateHistory: () => void;
}) {
  const [applying, setApplying] = useState(false);
  const controllerRef = useRef<ApplyQueueController | null>(null);

  if (!controllerRef.current) {
    controllerRef.current = new ApplyQueueController(
      {
        applyAction: api.applyAction,
        refreshStatus: args.refreshStatus,
        invalidateHistory: args.invalidateHistory,
        setFeedback: args.setFeedbackWithAutoDismiss,
        makeErrorFeedback: (label, error) => commandErrorFeedback(label, error),
      },
      setApplying,
    );
  }

  const handleApplyAction = useCallback((request: ApplyRequestDTO) => {
    controllerRef.current?.enqueue(request);
  }, []);

  const handleApply = useCallback((path: string) => {
    handleApplyAction({
      kind: 'apply',
      path,
      requestId: `${Date.now()}-${Math.random().toString(36).slice(2)}`,
    });
  }, [handleApplyAction]);

  return { applying, handleApply, handleApplyAction };
}
