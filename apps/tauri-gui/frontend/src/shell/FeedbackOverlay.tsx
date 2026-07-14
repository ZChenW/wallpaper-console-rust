import type { CSSProperties } from 'react';

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

const severityColours: Readonly<Record<FeedbackSeverity, string>> = {
  success: '#48c78e',
  info: '#78a9ff',
  warning: '#e6a700',
  error: '#ff6b6b',
};

const overlayStyle: CSSProperties = {
  position: 'fixed',
  insetInlineEnd: '1rem',
  bottom: '1rem',
  zIndex: 1000,
  display: 'grid',
  width: 'min(26rem, calc(100vw - 2rem))',
  gap: '0.625rem',
  pointerEvents: 'none',
};

const cardStyle: CSSProperties = {
  position: 'relative',
  overflow: 'hidden',
  borderRadius: '0.75rem',
  pointerEvents: 'auto',
};

const bodyStyle: CSSProperties = {
  display: 'grid',
  gridTemplateColumns: 'minmax(0, 1fr) auto',
  gap: '0.5rem 0.75rem',
  padding: '0.75rem 0.875rem 0.875rem',
};

const severityStyle: CSSProperties = {
  display: 'inline-flex',
  alignItems: 'center',
  gap: '0.4rem',
  minWidth: 0,
  fontSize: '0.75rem',
  fontWeight: 700,
  letterSpacing: '0.02em',
  textTransform: 'uppercase',
};

const messageStyle: CSSProperties = {
  gridColumn: '1 / -1',
  margin: 0,
  overflowWrap: 'anywhere',
  fontSize: '0.9rem',
  lineHeight: 1.45,
};

const closeStyle: CSSProperties = {
  width: '1.75rem',
  height: '1.75rem',
  padding: 0,
  border: 0,
  borderRadius: '0.4rem',
  background: 'transparent',
  color: 'inherit',
  cursor: 'pointer',
  font: 'inherit',
  fontSize: '1.15rem',
  lineHeight: 1,
};

const detailsStyle: CSSProperties = {
  gridColumn: '1 / -1',
  fontSize: '0.78rem',
  opacity: 0.84,
};

const detailTextStyle: CSSProperties = {
  maxHeight: '10rem',
  margin: '0.5rem 0 0',
  overflow: 'auto',
  whiteSpace: 'pre-wrap',
  overflowWrap: 'anywhere',
  font: 'inherit',
};

const progressTrackStyle: CSSProperties = {
  height: '0.2rem',
  overflow: 'hidden',
  background: 'color-mix(in srgb, currentColor 10%, transparent)',
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
  const accentColour = severityColours[notice.severity];

  return (
    <section
      aria-label={`${severityLabel}: ${notice.message}`}
      className={`feedback-overlay__card feedback-overlay__card--${notice.severity}`}
      data-feedback-card={notice.channel}
      data-feedback-severity={notice.severity}
      onMouseEnter={() => dispatch({ type: 'pause', channel: notice.channel, nowMs })}
      onMouseLeave={() => dispatch({ type: 'resume', channel: notice.channel, nowMs })}
      role={notice.severity === 'error' ? 'alert' : 'status'}
      style={{ ...cardStyle, borderInlineStart: `0.25rem solid ${accentColour}` }}
    >
      <div style={bodyStyle}>
        <span style={{ ...severityStyle, color: accentColour }}>
          <span aria-hidden="true">●</span>
          {severityLabel}
        </span>
        <button
          aria-label={`Dismiss ${notice.channel} notification`}
          onClick={() => dispatch({ type: 'dismiss', channel: notice.channel })}
          style={closeStyle}
          type="button"
        >
          <span aria-hidden="true">×</span>
        </button>
        <p style={messageStyle}>{notice.message}</p>
        {technicalDetails ? (
          <details style={detailsStyle}>
            <summary>Technical details</summary>
            <pre style={detailTextStyle}>{technicalDetails}</pre>
          </details>
        ) : null}
      </div>
      {progressPercent === null ? null : (
        <div
          aria-label={`Time remaining for ${notice.channel} notification`}
          aria-valuemax={100}
          aria-valuemin={0}
          aria-valuenow={progressPercent}
          data-feedback-progress={notice.channel}
          role="progressbar"
          style={progressTrackStyle}
        >
          <span
            aria-hidden="true"
            style={{
              display: 'block',
              width: `${progressPercent}%`,
              height: '100%',
              background: accentColour,
              transition: 'width 100ms linear',
            }}
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
      style={overlayStyle}
    >
      {state.notices.map((notice) => (
        <FeedbackCard
          dispatch={dispatch}
          key={notice.channel}
          notice={notice}
          nowMs={nowMs}
          technicalDetails={technicalDetails[notice.channel]}
        />
      ))}
    </aside>
  );
}
