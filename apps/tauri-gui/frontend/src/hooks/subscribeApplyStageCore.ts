import { listen } from '@tauri-apps/api/event';
import type { ApplyStagePayload } from '../events/appEvents';

export type ListenFn = typeof listen;

export function createSubscribeApplyStage(
  eventName: string,
  listenFn: ListenFn = listen,
) {
  return (handler: (event: ApplyStagePayload) => void) => {
    let unlisten: (() => void) | undefined;
    let disposed = false;

    try {
      void listenFn<ApplyStagePayload>(eventName, (event) => {
        if (event.payload) handler(event.payload);
      }).then((u) => {
        if (disposed) u();
        else unlisten = u;
      }).catch(() => {
        // Browser/mock surfaces do not expose the Tauri event bridge. Apply
        // still works; it simply has no fine-grained stage events.
      });
    } catch {
      // Some Tauri helpers throw synchronously when internals are absent.
    }

    return () => {
      disposed = true;
      unlisten?.();
    };
  };
}
