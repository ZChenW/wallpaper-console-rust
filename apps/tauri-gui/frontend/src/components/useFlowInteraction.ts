import { useCallback, useRef, useState } from 'react';

import {
  FlowInteractionController,
  type FlowInteractionInitialState,
  type FlowInteractionSnapshot,
} from './flowInteractionController.ts';

export interface FlowInteractionHandle {
  readonly controller: FlowInteractionController;
  readonly snapshot: FlowInteractionSnapshot;
  readonly update: <T>(operation: (controller: FlowInteractionController) => T) => T;
}

function snapshotsEqual(
  left: FlowInteractionSnapshot,
  right: FlowInteractionSnapshot,
): boolean {
  const anchorsEqual = (
    first: FlowInteractionSnapshot['committedAnchor'],
    second: FlowInteractionSnapshot['committedAnchor'],
  ) => first === second || (
    first !== null
    && second !== null
    && first.id === second.id
    && first.index === second.index
  );
  return left.phase === right.phase
    && anchorsEqual(left.committedAnchor, right.committedAnchor)
    && anchorsEqual(left.trackingCandidate, right.trackingCandidate)
    && anchorsEqual(left.programmaticTarget, right.programmaticTarget)
    && left.resizeAnchorId === right.resizeAnchorId
    && left.settled === right.settled
    && left.userInteracted === right.userInteracted;
}

export function useFlowInteraction(
  initial: FlowInteractionInitialState,
): FlowInteractionHandle {
  const controllerRef = useRef<FlowInteractionController | null>(null);
  if (controllerRef.current === null) {
    controllerRef.current = new FlowInteractionController(initial);
  }
  const controller = controllerRef.current;
  const [snapshot, setSnapshot] = useState(() => controller.snapshot());
  const update = useCallback(<T,>(
    operation: (current: FlowInteractionController) => T,
  ): T => {
    const result = operation(controller);
    const next = controller.snapshot();
    setSnapshot((current) => snapshotsEqual(current, next) ? current : next);
    return result;
  }, [controller]);
  return { controller, snapshot, update };
}
