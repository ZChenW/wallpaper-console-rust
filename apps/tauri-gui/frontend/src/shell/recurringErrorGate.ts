export interface RecurringErrorGate {
  shouldNotify(error: string | null): boolean;
}

/** Deduplicates one continuous error occurrence and resets after recovery. */
export function createRecurringErrorGate(): RecurringErrorGate {
  let lastError: string | null = null;

  return {
    shouldNotify(error) {
      if (error === null) {
        lastError = null;
        return false;
      }
      if (error === lastError) return false;
      lastError = error;
      return true;
    },
  };
}
