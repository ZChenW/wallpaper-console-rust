import type { CSSProperties, ReactNode } from 'react';
import {
  CheckCircle2,
  CircleAlert,
  Info,
  TriangleAlert,
  X,
} from 'lucide-react';

import {
  feedbackCountdownProgress,
  type FeedbackAction,
  type FeedbackChannel,
  type FeedbackNotice,
  type FeedbackSeverity,
  type FeedbackState,
} from './feedbackState';

export interface FeedbackOverlayProps {
  readonly state: Readonly<FeedbackState>;
  readonly nowMs: number;
  readonly dispatch: (action: FeedbackAction) => void;
  readonly technicalDetails?: Readonly<Partial<Record<FeedbackChannel, string>>>;
}

const severityLabels: Readonly<Record<FeedbackSeverity, string>> = {
  success: 'Success',
  info: 'Information',
  warning: 'Warning',
  error: 'Error',
};

// Severity is carried by an icon as well as color so the distinction survives
// monochrome vision and the single-hue theme.
const severityIcons: Readonly<Record<FeedbackSeverity, ReactNode>> = {
  success: <CheckCircle2 aria-hidden="true" size={14} />,
  info: <Info aria-hidden="true" size={14} />,
  warning: <TriangleAlert aria-hidden="true" size={14} />,
  error: <CircleAlert aria-hidden="true" size={14} />,
};

function FeedbackCard({
  notice,
  nowMs,
  dispatch,
  technicalDetails,
}: {
  readonly notice: FeedbackNotice;
  readonly nowMs: number;
  readonly dispatch: (action: FeedbackAction) => void;
  readonly technicalDetails: string | undefined;
}) {
  const progress = feedbackCountdownProgress(notice, nowMs);
  const progressPercent = progress === null ? null : Math.round(progress * 100);
  const severityLabel = severityLabels[notice.severity];
  const progressFillStyle = progress === null ? undefined : {
    '--feedback-duration': `${notice.durationMs ?? 0}ms`,
    '--feedback-progress': progress,
  } as CSSProperties;

  return (
    <section
      aria-label={`${severityLabel}: ${notice.message}`}
      className="feedback-overlay__card"
      data-feedback-card={notice.channel}
      data-feedback-severity={notice.severity}
      onBlur={(event) => {
        if (event.currentTarget.contains(event.relatedTarget)) return;
        if (event.currentTarget.matches(':hover')) return;
        dispatch({ type: 'resume', channel: notice.channel, nowMs });
      }}
      onFocus={() => dispatch({ type: 'pause', channel: notice.channel, nowMs })}
      onMouseEnter={() => dispatch({ type: 'pause', channel: notice.channel, nowMs })}
      onMouseLeave={(event) => {
        if (event.currentTarget.matches(':focus-within')) return;
        dispatch({ type: 'resume', channel: notice.channel, nowMs });
      }}
      role={notice.severity === 'error' ? 'alert' : 'status'}
    >
      <div className="feedback-overlay__body">
        <span className="feedback-overlay__severity">
          {severityIcons[notice.severity]}
          {severityLabel}
        </span>
        <button
          aria-label={`Dismiss ${notice.channel} notification`}
          className="feedback-overlay__close"
          onClick={() => dispatch({ type: 'dismiss', channel: notice.channel })}
          type="button"
        >
          <X aria-hidden="true" size={14} />
        </button>
        <p className="feedback-overlay__message">{notice.message}</p>
        {notice.action ? (
          <button
            className="feedback-overlay__action"
            onClick={() => {
              notice.action?.invoke();
              dispatch({ type: 'dismiss', channel: notice.channel });
            }}
            type="button"
          >
            {notice.action.label}
          </button>
        ) : null}
        {technicalDetails ? (
          <details className="feedback-overlay__details">
            <summary>Technical details</summary>
            <pre className="feedback-overlay__detail-text">{technicalDetails}</pre>
          </details>
        ) : null}
      </div>
      {progressPercent === null ? null : (
        <div
          aria-label={`Time remaining for ${notice.channel} notification`}
          aria-valuemax={100}
          aria-valuemin={0}
          aria-valuenow={progressPercent}
          className="feedback-overlay__progress-track"
          data-paused={notice.pausedRemainingMs === null ? undefined : true}
          data-feedback-progress={notice.channel}
          role="progressbar"
        >
          <span
            aria-hidden="true"
            className="feedback-overlay__progress-fill"
            style={progressFillStyle}
          />
        </div>
      )}
    </section>
  );
}

export function FeedbackOverlay({
  state,
  nowMs,
  dispatch,
  technicalDetails = {},
}: FeedbackOverlayProps) {
  if (state.notices.length === 0) return null;

  return (
    <aside
      aria-label="Notifications"
      className="feedback-overlay"
    >
      {state.notices.map((notice) => (
        <FeedbackCard
          dispatch={dispatch}
          key={`${notice.channel}:${notice.openedAtMs}`}
          notice={notice}
          nowMs={nowMs}
          technicalDetails={technicalDetails[notice.channel]}
        />
      ))}
    </aside>
  );
}
