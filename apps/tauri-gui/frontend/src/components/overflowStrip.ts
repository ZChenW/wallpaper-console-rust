export type OverflowStripState = 'none' | 'start' | 'end' | 'both';

export function overflowStripVerticalWheelDelta(
  deltaX: number,
  deltaY: number,
  deltaMode: number,
  pageSize: number,
): number {
  if (
    !Number.isFinite(deltaX)
    || !Number.isFinite(deltaY)
    || deltaY === 0
    || Math.abs(deltaX) > Math.abs(deltaY)
  ) {
    return 0;
  }
  const multiplier = deltaMode === 1
    ? 16
    : deltaMode === 2
      ? Math.max(1, pageSize)
      : 1;
  return deltaY * multiplier;
}

export function overflowStripState(
  scrollLeft: number,
  clientWidth: number,
  scrollWidth: number,
): OverflowStripState {
  if (
    !Number.isFinite(scrollLeft)
    || !Number.isFinite(clientWidth)
    || !Number.isFinite(scrollWidth)
    || clientWidth <= 0
    || scrollWidth <= clientWidth + 1
  ) {
    return 'none';
  }

  const startHidden = scrollLeft > 1;
  const endHidden = scrollLeft + clientWidth < scrollWidth - 1;
  if (startHidden && endHidden) return 'both';
  if (startHidden) return 'start';
  if (endHidden) return 'end';
  return 'none';
}
