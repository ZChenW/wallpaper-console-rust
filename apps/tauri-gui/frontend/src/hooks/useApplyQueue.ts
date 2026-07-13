import { useRef, useState } from 'react';
import { api } from '../api/bridge';
import type { ApplyResultDTO } from '../api/bridge';
import { commandErrorFeedback } from '../api/feedback';
import type { CommandFeedback } from '../api/feedback';
import { recordMetric } from '../perf/metrics';
import { ApplyQueueController, createApplyQueueHandlers } from './applyQueueController';
import type { AppliedRequest, ApplyQueueState } from './applyQueueController';
import { createSubscribeApplyStage } from './subscribeApplyStage';

export type {
  AppliedRequest,
  ApplyQueueController,
  ApplyQueueDeps,
  ApplyQueueState,
  ApplyStage,
} from './applyQueueController';

export function useApplyQueue(args: {
  refreshStatus: () => Promise<void>;
  setFeedbackWithAutoDismiss: (feedback: CommandFeedback) => void;
  invalidateLibrary: () => void;
  onApplied?: (request: AppliedRequest, result: ApplyResultDTO | undefined) => void;
}) {
  const [applying, setApplying] = useState(false);
  const [queueState, setQueueState] = useState<ApplyQueueState>({
    applying: false,
    activePath: undefined,
    pendingPath: undefined,
  });
  const onAppliedRef = useRef(args.onApplied);
  onAppliedRef.current = args.onApplied;
  const controllerRef = useRef<ApplyQueueController | null>(null);
  const handlersRef = useRef<ReturnType<typeof createApplyQueueHandlers> | null>(null);

  if (!controllerRef.current) {
    controllerRef.current = new ApplyQueueController(
      {
        applyAction: api.applyAction,
        applyToDisplay: api.applyToDisplay,
        refreshStatus: args.refreshStatus,
        invalidateLibrary: args.invalidateLibrary,
        setFeedback: args.setFeedbackWithAutoDismiss,
        makeErrorFeedback: (label, error) => commandErrorFeedback(label, error),
        recordMetric,
        subscribeApplyStage: createSubscribeApplyStage(),
        onApplied: (request, result) => onAppliedRef.current?.(request, result),
      },
      setApplying,
      setQueueState,
    );
  }

  if (!handlersRef.current) {
    handlersRef.current = createApplyQueueHandlers(controllerRef.current);
  }

  return {
    applying,
    activePath: queueState.activePath,
    pendingPath: queueState.pendingPath,
    ...handlersRef.current,
  };
}
