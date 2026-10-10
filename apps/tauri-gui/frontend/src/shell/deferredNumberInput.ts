/** Parse a numeric draft, preserving the last confirmed value for empty/invalid input. */
export function committedNumberDraft(
  raw: string,
  confirmed: number,
  min = -Infinity,
  max = Infinity,
  step?: number | string,
): number {
  const trimmed = raw.trim();
  if (trimmed.length === 0) return confirmed;
  const parsed = Number(trimmed);
  if (!Number.isFinite(parsed)) return confirmed;
  const bounded = Math.min(max, Math.max(min, parsed));
  // FPS and volume are integer fields in the backend DTO; duration allows decimals.
  return step === 1 ? Math.round(bounded) : bounded;
}
