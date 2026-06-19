export function shouldResetScroll(
  prevResetKey: string | undefined,
  resetKey: string | undefined,
): boolean {
  return prevResetKey !== resetKey;
}
