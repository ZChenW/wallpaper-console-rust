import { useRef, useState } from 'react';
import type {
  ApplyRequestDTO,
  ApplyResultDTO,
  CommandResult,
  TargetedApplyRequestDTO,
} from '../api/bridge';
import { commandErrorFeedback } from '../api/feedback';
import type { CommandFeedback } from '../api/feedback';
import { recordMetric } from '../perf/metrics';
import { ApplyQueueController, createApplyQueueHandlers } from './applyQueueController';
import type {
  AppliedRequest,
  ApplyQueueState,
  ApplyTransport,
} from './applyQueueController';
import { createSubscribeApplyStage } from './subscribeApplyStage';

export type {
  AppliedRequest,
  ApplyQueueController,
  ApplyQueueDeps,
  ApplyQueueState,
  ApplyStage,
  ApplyTransport,
} from './applyQueueController';

export interface ApplyQueueApi {
  applyAction(request: ApplyRequestDTO): Promise<CommandResult>;
  applyToDisplay(request: TargetedApplyRequestDTO): Promise<CommandResult>;
}

export function useApplyQueue(args: {
  api: ApplyQueueApi;
  refreshStatus: () => Promise<void>;
  setFeedbackWithAutoDismiss: (feedback: CommandFeedback) => void;
  reloadLibrary: () => Promise<unknown>;
  onApplied?: (
    request: AppliedRequest,
    result: ApplyResultDTO | undefined,
    transport: ApplyTransport,
  ) => void;
}) {
  const [queueState, setQueueState] = useState<ApplyQueueState>({
    applying: false,
    activePath: undefined,
    pendingPath: undefined,
  });
  const argsRef = useRef(args);
  argsRef.current = args;
  const controllerRef = useRef<ApplyQueueController | null>(null);
  const handlersRef = useRef<ReturnType<typeof createApplyQueueHandlers> | null>(null);

  if (!controllerRef.current) {
    controllerRef.current = new ApplyQueueController(
      {
        applyAction: (request) => argsRef.current.api.applyAction(request),
        applyToDisplay: (request) => argsRef.current.api.applyToDisplay(request),
        refreshStatus: async () => {
          await argsRef.current.refreshStatus();
        },
        invalidateLibrary: () => {
          void argsRef.current.reloadLibrary();
        },
        setFeedback: (feedback) => {
          argsRef.current.setFeedbackWithAutoDismiss(feedback);
        },
        makeErrorFeedback: (label, error) => commandErrorFeedback(label, error),
        recordMetric,
        subscribeApplyStage: createSubscribeApplyStage(),
        onApplied: (request, result, transport) =>
          argsRef.current.onApplied?.(request, result, transport),
      },
      () => {},
      setQueueState,
    );
  }

  if (!handlersRef.current) {
    handlersRef.current = createApplyQueueHandlers(controllerRef.current);
  }

  return {
    applying: queueState.applying,
    activePath: queueState.activePath,
    pendingPath: queueState.pendingPath,
    ...handlersRef.current,
  };
}
