export type FlowMotionPrototypeVariant = 'balanced' | 'continuous';

export const FLOW_MOTION_PROTOTYPE_VARIANTS:
readonly FlowMotionPrototypeVariant[] = ['balanced', 'continuous'];

export function resolveFlowMotionPrototypeVariant(
  search: string,
  enabled: boolean,
): FlowMotionPrototypeVariant | null {
  if (!enabled) return null;
  const variant = new URLSearchParams(search).get('variant');
  return variant === 'balanced' || variant === 'continuous' ? variant : null;
}
