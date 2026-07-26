export interface ApplyIndicatorProps {
  readonly state: 'applying' | 'pending';
}

export function ApplyIndicator({ state }: ApplyIndicatorProps) {
  return (
    <span
      aria-hidden="true"
      className={`apply-indicator apply-indicator--${state}`}
      data-apply-state={state}
    />
  );
}
