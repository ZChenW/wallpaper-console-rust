export type FeedbackSeverity = 'success' | 'info' | 'warning' | 'error';

export type FeedbackChannel = 'apply' | 'scan' | 'settings' | 'system';

export interface FeedbackNoticeAction {
  readonly label: string;
  readonly invoke: () => void;
}

export interface FeedbackNotice {
  readonly channel: FeedbackChannel;
  readonly severity: FeedbackSeverity;
  readonly message: string;
  readonly openedAtMs: number;
  readonly durationMs: number | null;
  readonly expiresAtMs: number | null;
  readonly pausedRemainingMs: number | null;
  readonly action?: FeedbackNoticeAction;
}

export interface FeedbackState {
  readonly notices: readonly FeedbackNotice[];
}

export const EMPTY_FEEDBACK_STATE: Readonly<FeedbackState> = Object.freeze({
  notices: Object.freeze([]),
});

export type FeedbackAction =
  | {
    readonly type: 'show';
    readonly channel: FeedbackChannel;
    readonly severity: FeedbackSeverity;
    readonly message: string;
    readonly nowMs: number;
    readonly action?: FeedbackNoticeAction;
  }
  | { readonly type: 'pause'; readonly channel: FeedbackChannel; readonly nowMs: number }
  | { readonly type: 'resume'; readonly channel: FeedbackChannel; readonly nowMs: number }
  | { readonly type: 'dismiss'; readonly channel: FeedbackChannel }
  | { readonly type: 'tick'; readonly nowMs: number };

function normalizedTimestamp(value: number, fallback: number): number {
  return Number.isFinite(value) ? value : fallback;
}

function clampDuration(value: number, durationMs: number): number {
  return Math.max(0, Math.min(durationMs, value));
}

export function feedbackLifetimeMs(severity: FeedbackSeverity): number | null {
  switch (severity) {
    case 'success':
      return 3_000;
    case 'info':
      return 5_000;
    case 'warning':
      return 8_000;
    case 'error':
      return null;
  }
}

function createNotice(action: Extract<FeedbackAction, { type: 'show' }>): FeedbackNotice {
  const durationMs = feedbackLifetimeMs(action.severity);
  const openedAtMs = normalizedTimestamp(action.nowMs, 0);
  return {
    channel: action.channel,
    severity: action.severity,
    message: action.message,
    openedAtMs,
    durationMs,
    expiresAtMs: durationMs === null ? null : openedAtMs + durationMs,
    pausedRemainingMs: null,
    action: action.action,
  };
}

function feedbackRemainingMs(notice: FeedbackNotice, nowMs: number): number | null {
  if (notice.durationMs === null) return null;
  if (notice.pausedRemainingMs !== null) {
    return clampDuration(notice.pausedRemainingMs, notice.durationMs);
  }

  const observedAtMs = normalizedTimestamp(nowMs, notice.openedAtMs);
  const expiresAtMs = notice.expiresAtMs ?? notice.openedAtMs + notice.durationMs;
  return clampDuration(expiresAtMs - observedAtMs, notice.durationMs);
}

function updateChannel(
  state: Readonly<FeedbackState>,
  channel: FeedbackChannel,
  update: (notice: FeedbackNotice) => FeedbackNotice,
): FeedbackState {
  return {
    notices: state.notices.map((notice) => (
      notice.channel === channel ? update(notice) : notice
    )),
  };
}

export function feedbackReducer(
  state: Readonly<FeedbackState>,
  action: FeedbackAction,
): FeedbackState {
  switch (action.type) {
    case 'show':
      return {
        notices: [
          ...state.notices.filter((notice) => notice.channel !== action.channel),
          createNotice(action),
        ],
      };
    case 'pause':
      if (!Number.isFinite(action.nowMs)) return state;
      return updateChannel(state, action.channel, (notice) => {
        if (notice.durationMs === null || notice.pausedRemainingMs !== null) return notice;
        const remainingMs = feedbackRemainingMs(notice, action.nowMs);
        if (remainingMs === null || remainingMs === 0) return notice;
        return {
          ...notice,
          expiresAtMs: null,
          pausedRemainingMs: remainingMs,
        };
      });
    case 'resume':
      if (!Number.isFinite(action.nowMs)) return state;
      return updateChannel(state, action.channel, (notice) => {
        if (notice.pausedRemainingMs === null) return notice;
        const remainingMs = notice.durationMs === null
          ? 0
          : clampDuration(notice.pausedRemainingMs, notice.durationMs);
        return {
          ...notice,
          expiresAtMs: action.nowMs + remainingMs,
          pausedRemainingMs: null,
        };
      });
    case 'dismiss':
      return { notices: state.notices.filter((notice) => notice.channel !== action.channel) };
    case 'tick':
      if (!Number.isFinite(action.nowMs)) return state;
      return {
        notices: state.notices.filter((notice) => (
          notice.durationMs === null
          || notice.pausedRemainingMs !== null
          || (notice.expiresAtMs !== null && notice.expiresAtMs > action.nowMs)
        )),
      };
  }
}

/** Remaining fraction for an auto-close bar: 1 when shown, 0 when due. */
export function feedbackCountdownProgress(notice: FeedbackNotice, nowMs: number): number | null {
  const remainingMs = feedbackRemainingMs(notice, nowMs);
  if (remainingMs === null || notice.durationMs === null) return null;
  return remainingMs / notice.durationMs;
}

export const SCAN_PRESENTATION_DELAY_MS = 500;

export type ScanState =
  | { readonly kind: 'idle' }
  | {
    readonly kind: 'running';
    readonly startedAtMs: number;
    readonly cancelRequestedAtMs: number | null;
  }
  | { readonly kind: 'cancelled'; readonly cancelledAtMs: number };

export const EMPTY_SCAN_STATE: ScanState = Object.freeze({ kind: 'idle' });

export type ScanAction =
  | { readonly type: 'started'; readonly nowMs: number }
  | { readonly type: 'cancelRequested'; readonly nowMs: number }
  | { readonly type: 'completed'; readonly nowMs: number }
  | { readonly type: 'cancelled'; readonly nowMs: number }
  | { readonly type: 'dismissed' };

export function scanReducer(state: ScanState, action: ScanAction): ScanState {
  switch (action.type) {
    case 'started':
      return {
        kind: 'running',
        startedAtMs: normalizedTimestamp(action.nowMs, 0),
        cancelRequestedAtMs: null,
      };
    case 'cancelRequested':
      return state.kind === 'running'
        ? {
          ...state,
          cancelRequestedAtMs: state.cancelRequestedAtMs
            ?? normalizedTimestamp(action.nowMs, state.startedAtMs),
        }
        : state;
    case 'cancelled':
      return state.kind === 'running'
        ? {
          kind: 'cancelled',
          cancelledAtMs: normalizedTimestamp(action.nowMs, state.startedAtMs),
        }
        : state;
    case 'completed':
      return state.kind === 'cancelled' ? state : EMPTY_SCAN_STATE;
    case 'dismissed':
      return EMPTY_SCAN_STATE;
  }
}

export type ScanPresentation =
  | { readonly kind: 'hidden' }
  | {
    readonly kind: 'running' | 'cancelling';
    readonly nonModal: true;
    readonly canCancel: boolean;
    readonly elapsedMs: number;
  }
  | { readonly kind: 'cancelled'; readonly nonModal: true };

export function scanPresentation(state: ScanState, nowMs: number): ScanPresentation {
  if (state.kind === 'idle') return { kind: 'hidden' };
  if (state.kind === 'cancelled') return { kind: 'cancelled', nonModal: true };

  const observedAtMs = normalizedTimestamp(nowMs, state.startedAtMs);
  const elapsedMs = Math.max(0, observedAtMs - state.startedAtMs);
  if (state.cancelRequestedAtMs !== null) {
    return { kind: 'cancelling', nonModal: true, canCancel: false, elapsedMs };
  }
  if (elapsedMs < SCAN_PRESENTATION_DELAY_MS) return { kind: 'hidden' };
  return { kind: 'running', nonModal: true, canCancel: true, elapsedMs };
}
