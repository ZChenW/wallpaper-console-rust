export interface RequestDeadlineScheduler {
  setTimer(callback: () => void, delayMs: number): unknown;
  clearTimer(handle: unknown): void;
}

const DEFAULT_SCHEDULER: RequestDeadlineScheduler = {
  setTimer: (callback, delayMs) => globalThis.setTimeout(callback, delayMs),
  clearTimer: (handle) => globalThis.clearTimeout(handle as number),
};

/**
 * Bound a bridge read without requiring transport-level cancellation.
 *
 * Late settlement remains observed by the attached handlers, but it cannot
 * overwrite the timeout result or keep the caller's polling loop blocked.
 */
export function withRequestDeadline<T>(
  promise: Promise<T>,
  timeoutMs: number,
  label: string,
  scheduler: RequestDeadlineScheduler = DEFAULT_SCHEDULER,
): Promise<T> {
  if (!Number.isFinite(timeoutMs) || timeoutMs <= 0) return promise;

  return new Promise<T>((resolve, reject) => {
    let settled = false;
    const timer = scheduler.setTimer(() => {
      if (settled) return;
      settled = true;
      reject(new Error(`${label} timed out after ${timeoutMs}ms`));
    }, timeoutMs);

    promise.then(
      (value) => {
        if (settled) return;
        settled = true;
        scheduler.clearTimer(timer);
        resolve(value);
      },
      (error) => {
        if (settled) return;
        settled = true;
        scheduler.clearTimer(timer);
        reject(error);
      },
    );
  });
}
