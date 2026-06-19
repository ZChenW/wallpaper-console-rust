import { listen } from '@tauri-apps/api/event';
import { APP_EVENTS } from '../events/appEvents';
import {
  createSubscribeApplyStage as createSubscribeApplyStageCore,
  type ListenFn,
} from './subscribeApplyStageCore';

export type { ListenFn };

export function createSubscribeApplyStage(listenFn: ListenFn = listen) {
  return createSubscribeApplyStageCore(APP_EVENTS.applyStage, listenFn);
}
