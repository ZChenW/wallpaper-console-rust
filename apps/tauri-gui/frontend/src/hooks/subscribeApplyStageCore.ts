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

    void listenFn<ApplyStagePayload>(eventName, (event) => {
      if (event.payload) handler(event.payload);
    }).then((u) => {
      if (disposed) u();
      else unlisten = u;
    });

    return () => {
      disposed = true;
      unlisten?.();
    };
  };
}
