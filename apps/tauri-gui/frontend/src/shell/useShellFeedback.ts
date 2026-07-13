import {
  useCallback,
  useEffect,
  useReducer,
  useState,
} from 'react';

import type { CommandFeedback } from '../api/feedback.ts';
import {
  EMPTY_FEEDBACK_STATE,
  feedbackReducer,
  type FeedbackAction,
  type FeedbackChannel,
  type FeedbackSeverity,
  type FeedbackState,
} from './feedbackState.ts';

export interface ShellNoticeInput {
  readonly channel: FeedbackChannel;
  readonly severity: FeedbackSeverity;
  readonly message: string;
  readonly technicalDetails?: string;
}

export interface ShellRunningStatus {
  readonly channel: FeedbackChannel;
  readonly message: string;
  readonly technicalDetails?: string;
}

export type TranslatedCommandFeedback =
  | { readonly kind: 'clear'; readonly channel: FeedbackChannel }
  | ({ readonly kind: 'running' } & ShellRunningStatus)
  | ({ readonly kind: 'notice' } & ShellNoticeInput);

export function translateCommandFeedback(
  feedback: CommandFeedback,
  channel: FeedbackChannel,
): TranslatedCommandFeedback {
  if (feedback.state === 'idle') return { kind: 'clear', channel };
  if (feedback.state === 'running') {
    return {
      kind: 'running',
      channel,
      message: feedback.label,
      technicalDetails: feedback.detail,
    };
  }
  if (feedback.state === 'success') {
    const detail = feedback.detail?.trim();
    const conciseDetail = detail && detail.length <= 120 && !detail.includes('\n')
      ? detail
      : undefined;
    return {
      kind: 'notice',
      channel,
      severity: 'success',
      message: conciseDetail ? `${feedback.label}: ${conciseDetail}` : feedback.label,
      technicalDetails: detail && !conciseDetail ? detail : undefined,
    };
  }
  return {
    kind: 'notice',
    channel,
    severity: feedback.state,
    message: feedback.label,
    technicalDetails: feedback.detail,
  };
}

export function shouldTickFeedbackClock(state: Readonly<FeedbackState>): boolean {
  return state.notices.some((notice) => notice.durationMs !== null);
}

export function useShellFeedback(now: () => number = Date.now) {
  const [state, dispatch] = useReducer(feedbackReducer, EMPTY_FEEDBACK_STATE);
  const [nowMs, setNowMs] = useState(() => now());
  const [technicalDetails, setTechnicalDetails] = useState<
    Partial<Record<FeedbackChannel, string>>
  >({});
  const [runningStatus, setRunningStatus] = useState<ShellRunningStatus | null>(null);
  const tickClock = shouldTickFeedbackClock(state);

  const showNotice = useCallback((notice: ShellNoticeInput) => {
    const observedAtMs = now();
    setNowMs(observedAtMs);
    setTechnicalDetails((current) => {
      if (notice.technicalDetails) {
        if (current[notice.channel] === notice.technicalDetails) return current;
        return { ...current, [notice.channel]: notice.technicalDetails };
      }
      if (!(notice.channel in current)) return current;
      const next = { ...current };
      delete next[notice.channel];
      return next;
    });
    dispatch({
      type: 'show',
      channel: notice.channel,
      severity: notice.severity,
      message: notice.message,
      nowMs: observedAtMs,
    });
  }, [now]);

  const dispatchFeedback = useCallback((action: FeedbackAction) => {
    if (action.type === 'dismiss') {
      setTechnicalDetails((current) => {
        if (!(action.channel in current)) return current;
        const next = { ...current };
        delete next[action.channel];
        return next;
      });
    }
    dispatch(action);
  }, []);

  const setCommandFeedback = useCallback((
    feedback: CommandFeedback,
    channel: FeedbackChannel = 'system',
  ) => {
    const translated = translateCommandFeedback(feedback, channel);
    if (translated.kind === 'clear') {
      setRunningStatus((current) => current?.channel === channel ? null : current);
      dispatchFeedback({ type: 'dismiss', channel });
      return;
    }
    if (translated.kind === 'running') {
      setRunningStatus(translated);
      return;
    }
    setRunningStatus((current) => current?.channel === channel ? null : current);
    showNotice(translated);
  }, [dispatchFeedback, showNotice]);

  useEffect(() => {
    if (!tickClock) return undefined;
    const tick = () => {
      const observedAtMs = now();
      setNowMs(observedAtMs);
      dispatch({ type: 'tick', nowMs: observedAtMs });
    };
    const timer = window.setInterval(tick, 100);
    return () => window.clearInterval(timer);
  }, [now, tickClock]);

  return {
    state,
    nowMs,
    technicalDetails,
    runningStatus,
    showNotice,
    setCommandFeedback,
    dispatchFeedback,
    clearRunningStatus: () => setRunningStatus(null),
  };
}
