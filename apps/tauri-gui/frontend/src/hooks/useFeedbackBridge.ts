import { useEffect } from 'react';
import { listen } from '@tauri-apps/api/event';
import type { CommandFeedback } from '../api/feedback';

export function useFeedbackBridge(setFeedback: (feedback: CommandFeedback) => void): void {
  useEffect(() => {
    const handler = (event: Event) => {
      const detail = (event as CustomEvent<CommandFeedback>).detail;
      if (detail) setFeedback(detail);
    };
    window.addEventListener('wc-feedback', handler);

    let unlisten: (() => void) | undefined;
    listen<CommandFeedback>('wc-feedback', (event) => {
      if (event.payload) setFeedback(event.payload);
    }).then((u) => { unlisten = u; });

    return () => {
      window.removeEventListener('wc-feedback', handler);
      unlisten?.();
    };
  }, [setFeedback]);
}
