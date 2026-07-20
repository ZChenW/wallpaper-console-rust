/** Parse a numeric draft, preserving the last confirmed value for empty/invalid input. */
export function committedNumberDraft(raw: string, confirmed: number): number {
  const trimmed = raw.trim();
  if (trimmed.length === 0) return confirmed;
  const parsed = Number(trimmed);
  if (!Number.isFinite(parsed)) return confirmed;
  return parsed;
}
