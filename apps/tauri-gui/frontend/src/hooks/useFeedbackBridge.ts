import { useEffect } from 'react';
import { listen } from '@tauri-apps/api/event';
import type { CommandFeedback } from '../api/feedback';
import { APP_EVENTS, onFeedback } from '../events/appEvents';

export function useFeedbackBridge(setFeedback: (feedback: CommandFeedback) => void): void {
  useEffect(() => {
    const offFeedback = onFeedback(setFeedback);

    let unlisten: (() => void) | undefined;
    listen<CommandFeedback>(APP_EVENTS.feedback, (event) => {
      if (event.payload) setFeedback(event.payload);
    }).then((u) => { unlisten = u; });

    return () => {
      offFeedback();
      unlisten?.();
    };
  }, [setFeedback]);
}
