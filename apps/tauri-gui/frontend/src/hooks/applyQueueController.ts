import type { ApplyRequestDTO, ApplyResultDTO } from '../api/bridge';
import type { CommandFeedback } from '../api/feedback';

export type ApplyStage = 'queued' | 'starting backend' | 'settling' | 'applied';

export interface ApplyQueueDeps {
  applyAction: (request: ApplyRequestDTO) => Promise<{ success: boolean; stdout: string; stderr: string; error?: { message: string } }>;
  refreshStatus: () => Promise<void>;
  invalidateHistory: () => void;
  setFeedback: (feedback: CommandFeedback) => void;
  makeErrorFeedback: (label: string, error: unknown) => CommandFeedback;
  recordMetric?: (name: string, value: number) => void;
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
      this.emitStage('queued', false);
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
      const requestStart = performance.now();
      this.emitStage('starting backend', isBackendApply);
      try {
        const result = await this.deps.applyAction(req);
        if (result.success) {
          this.deps.invalidateHistory();
          let detail: ApplyResultDTO | undefined;
          try {
            detail = result.stdout ? (JSON.parse(result.stdout) as ApplyResultDTO) : undefined;
          } catch {
            detail = undefined;
          }
          this.emitStage('settling', isBackendApply);
          await this.deps.refreshStatus();
          this.deps.setFeedback({
            state: 'success',
            label: 'Applied',
            detail: detail?.preview ? 'Preview wallpaper applied.' : detail?.appliedPath?.split('/').pop(),
          });
          this.deps.recordMetric?.('apply.request.ms', performance.now() - requestStart);
        } else {
          this.deps.setFeedback(this.deps.makeErrorFeedback('Apply', result));
        }
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

  private emitStage(stage: ApplyStage, isBackendApply: boolean): void {
    switch (stage) {
      case 'queued':
        this.deps.setFeedback({
          state: 'running',
          label: 'Applying wallpaper',
          detail: 'Queued — waiting for current apply to finish.',
        });
        break;
      case 'starting backend':
        this.deps.setFeedback({
          state: 'running',
          label: 'Applying wallpaper',
          detail: isBackendApply
            ? 'Starting renderer. Scene wallpapers may take several seconds.'
            : 'Applying wallpaper.',
        });
        break;
      case 'settling':
        this.deps.setFeedback({
          state: 'running',
          label: 'Applying wallpaper',
          detail: 'Settling…',
        });
        break;
      case 'applied':
        break;
    }
  }
}
