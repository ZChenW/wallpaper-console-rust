import assert from 'node:assert/strict';
import test from 'node:test';

type ApplyRequestDTO = {
  kind: string;
  path: string;
  requestId?: string | null;
};

type CommandFeedback =
  | { state: 'idle' }
  | { state: 'running'; label: string }
  | { state: 'success'; label: string; detail?: string }
  | { state: 'warning'; label: string; detail: string }
  | { state: 'error'; label: string; detail: string };

interface ApplyQueueDeps {
  applyAction: (request: ApplyRequestDTO) => Promise<{ success: boolean; stdout: string; stderr: string; error?: { message: string } }>;
  refreshStatus: () => Promise<void>;
  invalidateHistory: () => void;
  setFeedback: (feedback: CommandFeedback) => void;
  makeErrorFeedback: (label: string, error: unknown) => CommandFeedback;
}

class ApplyQueueController {
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
      this.deps.setFeedback({ state: 'running', label: 'Applying wallpaper' });
      try {
        const result = await this.deps.applyAction(req);
        if (result.success) {
          this.deps.invalidateHistory();
          let detail: { preview?: boolean; appliedPath?: string } | undefined;
          try {
            detail = result.stdout ? JSON.parse(result.stdout) : undefined;
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

const req = (id: string, path = `/wall/${id}.jpg`): ApplyRequestDTO => ({
  kind: 'apply',
  path,
  requestId: id,
});

test('apply queue runs current request then latest pending request only', async () => {
  const calls: string[] = [];
  let releaseFirst: (() => void) | undefined;
  const firstBlock = new Promise<void>((resolve) => { releaseFirst = resolve; });
  const feedback: string[] = [];
  const applyingStates: boolean[] = [];

  const deps: ApplyQueueDeps = {
    applyAction: async (request) => {
      calls.push(request.requestId ?? '');
      if (request.requestId === 'a') await firstBlock;
      return {
        success: true,
        stdout: JSON.stringify({
          requestId: request.requestId,
          appliedPath: request.path,
          statePath: request.path,
          backend: 'awww',
          fileType: 'image',
          preview: false,
        }),
        stderr: '',
      };
    },
    refreshStatus: async () => {},
    invalidateHistory: () => {},
    setFeedback: (value) => feedback.push(`${value.state}:${value.label}`),
    makeErrorFeedback: (label) => ({ state: 'error', label, detail: 'test error' }),
  };

  const controller = new ApplyQueueController(deps, (value) => applyingStates.push(value));
  controller.enqueue(req('a'));
  controller.enqueue(req('b'));
  controller.enqueue(req('c'));
  releaseFirst?.();
  await new Promise((resolve) => setTimeout(resolve, 20));

  assert.deepEqual(calls, ['a', 'c']);
  assert.deepEqual(applyingStates, [true, false]);
  assert(feedback.includes('running:Applying wallpaper'));
  assert(feedback.includes('success:Applied'));
});
