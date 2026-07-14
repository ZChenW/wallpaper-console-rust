import { useEffect } from 'react';
import { listen } from '@tauri-apps/api/event';
import type { CommandFeedback } from '../api/feedback.ts';
import { APP_EVENTS, onFeedback } from '../events/appEvents.ts';

export type FeedbackListenFn = typeof listen;

export function subscribeFeedbackEvent(
  setFeedback: (feedback: CommandFeedback) => void,
  listenFn: FeedbackListenFn = listen,
): () => void {
  let unlisten: (() => void) | undefined;
  let disposed = false;
  try {
    void listenFn<CommandFeedback>(APP_EVENTS.feedback, (event) => {
      if (event.payload) setFeedback(event.payload);
    }).then((nextUnlisten) => {
      if (disposed) nextUnlisten();
      else unlisten = nextUnlisten;
    }).catch(() => {
      // Browser/mock surfaces keep the local DOM feedback channel only.
    });
  } catch {
    // Tauri's listen helper may throw synchronously without runtime internals.
  }

  return () => {
    disposed = true;
    unlisten?.();
  };
}

export function useFeedbackBridge(setFeedback: (feedback: CommandFeedback) => void): void {
  useEffect(() => {
    const offFeedback = onFeedback(setFeedback);

    const unsubscribeTauri = subscribeFeedbackEvent(setFeedback);

    return () => {
      offFeedback();
      unsubscribeTauri();
    };
  }, [setFeedback]);
}
