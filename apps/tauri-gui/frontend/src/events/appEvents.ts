import type { CommandFeedback } from '../api/feedback';

export const APP_EVENTS = {
  feedback: 'wc-feedback',
  applyStage: 'wc-apply-stage',
  configChanged: 'wc-config-changed',
  favoritesInvalidated: 'favorites-cache-invalidated',
  historyInvalidated: 'history-cache-invalidated',
} as const;

export interface ApplyStagePayload {
  requestId?: string | null;
  stage: string;
  label: string;
  detail: string;
}

export interface ConfigChangedEvent {
  key: string;
  value: string;
}

export function emitFeedback(feedback: CommandFeedback): void {
  window.dispatchEvent(new CustomEvent(APP_EVENTS.feedback, { detail: feedback }));
}

export function onFeedback(handler: (feedback: CommandFeedback) => void): () => void {
  const listener = (event: Event) => {
    const detail = (event as CustomEvent<CommandFeedback>).detail;
    if (detail) handler(detail);
  };
  window.addEventListener(APP_EVENTS.feedback, listener);
  return () => window.removeEventListener(APP_EVENTS.feedback, listener);
}

export function emitConfigChanged(detail: ConfigChangedEvent): void {
  window.dispatchEvent(new CustomEvent(APP_EVENTS.configChanged, { detail }));
}

export function emitFavoritesInvalidated(): void {
  window.dispatchEvent(new CustomEvent(APP_EVENTS.favoritesInvalidated));
}

export function emitHistoryInvalidated(): void {
  window.dispatchEvent(new CustomEvent(APP_EVENTS.historyInvalidated));
}
