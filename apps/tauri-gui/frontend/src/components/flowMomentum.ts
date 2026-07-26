import { animateValue } from 'motion-dom';

const FLOW_MAX_RELEASE_VELOCITY = 3_200;
const FLOW_MOMENTUM_PROJECTION_SECONDS = 0.115;
const FLOW_MOMENTUM_MAX_VIEWPORTS = 2.4;
const FLOW_DIRECTIONAL_VELOCITY_THRESHOLD = 120;
const FLOW_MULTI_TARGET_UNAMBIGUOUS_DIRECT_SPAN = 1.2;

export interface FlowMomentumTarget {
  readonly index: number;
  readonly offset: number;
}

interface FlowMomentumSample {
  readonly startedAt: number;
  readonly startOffset: number;
  readonly observationCount: number;
  readonly at: number;
  readonly offset: number;
  readonly velocity: number;
}

interface FlowMomentumTargetPolicy {
  readonly originOffset: number;
  readonly maximumTargetStep: number;
}

export interface FlowMomentumCapture {
  readonly offset: number;
  readonly viewportSize: number;
  readonly targets: readonly FlowMomentumTarget[];
  readonly reducedMotion: boolean;
  readonly onTarget: (target: FlowMomentumTarget) => void;
  readonly onUpdate: (offset: number) => void;
  readonly onComplete: (target: FlowMomentumTarget) => void;
}

interface FlowMomentumPlayback {
  stop(): void;
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

function nearestTargetPosition(
  offset: number,
  targets: readonly FlowMomentumTarget[],
): number {
  let nearestPosition = 0;
  let nearestDelta = Number.POSITIVE_INFINITY;
  targets.forEach((target, position) => {
    const delta = Math.abs(target.offset - offset);
    if (delta >= nearestDelta) return;
    nearestPosition = position;
    nearestDelta = delta;
  });
  return nearestPosition;
}

function applyTargetPolicy(
  target: FlowMomentumTarget,
  targets: readonly FlowMomentumTarget[],
  policy: FlowMomentumTargetPolicy | undefined,
  releaseOffset: number,
  direction: number,
): FlowMomentumTarget {
  if (!policy) return target;
  const originPosition = nearestTargetPosition(policy.originOffset, targets);
  const targetPosition = targets.indexOf(target);
  if (targetPosition < 0) return target;
  let limitedPosition = clamp(
    targetPosition,
    originPosition - policy.maximumTargetStep,
    originPosition + policy.maximumTargetStep,
  );
  if (direction > 0) {
    while (
      limitedPosition < targets.length - 1
      && targets[limitedPosition]!.offset < releaseOffset - 1
    ) {
      limitedPosition += 1;
    }
  } else if (direction < 0) {
    while (
      limitedPosition > 0
      && targets[limitedPosition]!.offset > releaseOffset + 1
    ) {
      limitedPosition -= 1;
    }
  }
  return targets[limitedPosition] ?? target;
}

export function resolveFlowMomentumTarget(
  offset: number,
  velocity: number,
  viewportSize: number,
  targets: readonly FlowMomentumTarget[],
  policy?: FlowMomentumTargetPolicy,
): FlowMomentumTarget | null {
  if (targets.length === 0) return null;
  const firstOffset = targets[0]!.offset;
  const lastOffset = targets[targets.length - 1]!.offset;
  const maxTravel = Math.max(1, viewportSize) * FLOW_MOMENTUM_MAX_VIEWPORTS;
  const projectedOffset = clamp(
    offset + clamp(
      clamp(
        velocity,
        -FLOW_MAX_RELEASE_VELOCITY,
        FLOW_MAX_RELEASE_VELOCITY,
      ) * FLOW_MOMENTUM_PROJECTION_SECONDS,
      -maxTravel,
      maxTravel,
    ),
    firstOffset,
    lastOffset,
  );
  const direction = Math.abs(velocity) < FLOW_DIRECTIONAL_VELOCITY_THRESHOLD
    ? 0
    : Math.sign(velocity);
  let nearest: FlowMomentumTarget | null = null;
  let nearestDelta = Number.POSITIVE_INFINITY;

  for (const target of targets) {
    if (direction > 0 && target.offset < offset - 1) continue;
    if (direction < 0 && target.offset > offset + 1) continue;
    const delta = Math.abs(target.offset - projectedOffset);
    if (delta >= nearestDelta) continue;
    nearest = target;
    nearestDelta = delta;
  }

  if (nearest !== null) {
    return applyTargetPolicy(nearest, targets, policy, offset, direction);
  }
  const fallback = targets.reduce((best, target) => (
    Math.abs(target.offset - projectedOffset) < Math.abs(best.offset - projectedOffset)
      ? target
      : best
  ));
  return applyTargetPolicy(fallback, targets, policy, offset, direction);
}

function directGestureSpan(
  sample: FlowMomentumSample | null,
  offset: number,
  targets: readonly FlowMomentumTarget[],
): number {
  if (sample === null || targets.length < 2) return Number.POSITIVE_INFINITY;
  const direction = Math.sign(offset - sample.startOffset);
  if (direction === 0) return 0;
  const originPosition = nearestTargetPosition(sample.startOffset, targets);
  const adjacentPosition = originPosition + direction;
  const adjacent = targets[adjacentPosition];
  const origin = targets[originPosition];
  if (!origin || !adjacent) return Number.POSITIVE_INFINITY;
  const adjacentSpan = Math.abs(adjacent.offset - origin.offset);
  if (adjacentSpan < 1) return Number.POSITIVE_INFINITY;
  return Math.abs(offset - sample.startOffset) / adjacentSpan;
}

export class FlowMomentumController {
  private sample: FlowMomentumSample | null = null;
  private playback: FlowMomentumPlayback | null = null;

  begin(
    offset: number,
    at = performance.now(),
    originOffset = offset,
    continueGesture = false,
  ): void {
    this.cancel();
    if (continueGesture && this.sample !== null) {
      this.sample = {
        ...this.sample,
        at,
        offset,
      };
      return;
    }
    this.sample = {
      startedAt: at,
      startOffset: originOffset,
      observationCount: 0,
      at,
      offset,
      velocity: 0,
    };
  }

  observe(
    offset: number,
    at = performance.now(),
    inputDirection = 0,
  ): void {
    const previous = this.sample;
    if (previous === null) {
      this.sample = {
        startedAt: at,
        startOffset: offset,
        observationCount: 0,
        at,
        offset,
        velocity: 0,
      };
      return;
    }
    const elapsedMs = at - previous.at;
    const distance = offset - previous.offset;
    if (Math.abs(distance) < 0.25) return;
    if (elapsedMs <= 0) {
      this.sample = {
        startedAt: previous.startedAt,
        startOffset: previous.startOffset,
        observationCount: previous.observationCount + 1,
        at,
        offset,
        velocity: previous.velocity,
      };
      return;
    }
    const sampledElapsedMs = Math.max(8, elapsedMs);
    const measuredVelocityMagnitude = Math.abs(clamp(
      distance / (sampledElapsedMs / 1_000),
      -FLOW_MAX_RELEASE_VELOCITY,
      FLOW_MAX_RELEASE_VELOCITY,
    ));
    const measuredVelocity = inputDirection === 0
      ? measuredVelocityMagnitude * Math.sign(distance)
      : measuredVelocityMagnitude * Math.sign(inputDirection);
    const sameDirection = Math.sign(measuredVelocity) === Math.sign(previous.velocity);
    const velocity = previous.velocity === 0 || !sameDirection
      ? measuredVelocity
      : previous.velocity * 0.28 + measuredVelocity * 0.72;
    this.sample = {
      startedAt: previous.startedAt,
      startOffset: previous.startOffset,
      observationCount: previous.observationCount + 1,
      at,
      offset,
      velocity,
    };
  }

  capture(input: FlowMomentumCapture, at = performance.now()): FlowMomentumTarget | null {
    this.playback?.stop();
    this.playback = null;
    const ageMs = this.sample === null ? 0 : Math.max(0, at - this.sample.at);
    const sampledVelocity = this.sample?.velocity ?? 0;
    const velocity = clamp(
      sampledVelocity * Math.exp(-ageMs / 240),
      -FLOW_MAX_RELEASE_VELOCITY,
      FLOW_MAX_RELEASE_VELOCITY,
    );
    const gestureSpan = directGestureSpan(this.sample, input.offset, input.targets);
    const multiTargetIntent = gestureSpan >= FLOW_MULTI_TARGET_UNAMBIGUOUS_DIRECT_SPAN;
    const targetPolicy = !multiTargetIntent && this.sample !== null
      ? {
        originOffset: this.sample.startOffset,
        maximumTargetStep: 1,
      }
      : undefined;
    const target = resolveFlowMomentumTarget(
      input.offset,
      velocity,
      input.viewportSize,
      input.targets,
      targetPolicy,
    );
    if (target === null) return null;

    input.onTarget(target);
    if (input.reducedMotion || Math.abs(target.offset - input.offset) < 0.5) {
      input.onUpdate(target.offset);
      input.onComplete(target);
      return target;
    }

    let playback: FlowMomentumPlayback | null = null;
    let centeredFrames = 0;
    let completed = false;
    const complete = () => {
      if (completed || this.playback !== playback) return;
      completed = true;
      playback?.stop();
      this.playback = null;
      input.onUpdate(target.offset);
      input.onComplete(target);
    };
    playback = animateValue({
      keyframes: [input.offset, target.offset],
      type: 'spring',
      stiffness: 560,
      damping: 43,
      mass: 0.85,
      velocity,
      restDelta: 2,
      restSpeed: 90,
      onUpdate: (offset) => {
        input.onUpdate(offset);
        centeredFrames = Math.abs(target.offset - offset) <= 3
          ? centeredFrames + 1
          : 0;
        if (centeredFrames >= 2) complete();
      },
      onComplete: complete,
    });
    this.playback = playback;
    return target;
  }

  cancel(): void {
    this.playback?.stop();
    this.playback = null;
  }
}
