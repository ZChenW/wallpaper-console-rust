import { useEffect } from 'react';
import type { CommandFeedback } from '../api/feedback';

export function useFeedbackBridge(setFeedback: (feedback: CommandFeedback) => void): void {
  useEffect(() => {
    const handler = (event: Event) => {
      const detail = (event as CustomEvent<CommandFeedback>).detail;
      if (detail) setFeedback(detail);
    };
    window.addEventListener('wc-feedback', handler);
    return () => window.removeEventListener('wc-feedback', handler);
  }, [setFeedback]);
}
