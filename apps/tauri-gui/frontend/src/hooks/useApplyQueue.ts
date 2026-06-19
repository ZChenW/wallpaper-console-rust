import { useCallback, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { api, ApplyRequestDTO } from '../api/bridge';
import { commandErrorFeedback, CommandFeedback } from '../api/feedback';
import { APP_EVENTS, type ApplyStagePayload } from '../events/appEvents';
import { recordMetric } from '../perf/metrics';
import { ApplyQueueController } from './applyQueueController';

export type { ApplyQueueController, ApplyQueueDeps, ApplyStage } from './applyQueueController';

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
        recordMetric,
        subscribeApplyStage: (handler) => {
          let unlisten: (() => void) | undefined;
          void listen<ApplyStagePayload>(APP_EVENTS.applyStage, (event) => {
            if (event.payload) handler(event.payload);
          }).then((u) => { unlisten = u; });
          return () => { unlisten?.(); };
        },
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
