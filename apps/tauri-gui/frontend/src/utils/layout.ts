export const COL_MIN_WIDTH = 220;
export const GRID_GAP = 10;

export function calculateColumnCount(width: number, minCardWidth = COL_MIN_WIDTH, gap = GRID_GAP): number {
  if (!Number.isFinite(width) || width <= 0) return 1;
  const count = Math.floor((width + gap) / (minCardWidth + gap));
  return Math.max(1, count);
}
