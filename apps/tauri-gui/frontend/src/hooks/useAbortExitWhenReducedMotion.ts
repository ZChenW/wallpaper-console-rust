import { useEffect, type Dispatch, type MutableRefObject, type SetStateAction } from 'react';

/** If reduced motion turns on mid-exit, drop the deferred unmount timer immediately. */
export function useAbortExitWhenReducedMotion(
  open: boolean,
  reducedMotion: boolean,
  exitTimerRef: MutableRefObject<ReturnType<typeof setTimeout> | null>,
  setShouldRender: Dispatch<SetStateAction<boolean>>,
): void {
  useEffect(() => {
    if (open || !reducedMotion) return;
    if (exitTimerRef.current !== null) {
      clearTimeout(exitTimerRef.current);
      exitTimerRef.current = null;
    }
    setShouldRender(false);
  }, [open, reducedMotion, exitTimerRef, setShouldRender]);
}
