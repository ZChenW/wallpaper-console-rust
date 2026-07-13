import type { CSSProperties } from 'react';

import type { ScanPresentation } from './feedbackState';

export interface ScanActivityProps {
  readonly presentation: ScanPresentation;
  readonly onCancel: () => void;
  readonly onDismiss: () => void;
}

const activityStyle: CSSProperties = {
  position: 'fixed',
  insetInlineStart: '1rem',
  bottom: '1rem',
  zIndex: 900,
  display: 'flex',
  alignItems: 'center',
  width: 'min(28rem, calc(100vw - 2rem))',
  gap: '0.75rem',
  padding: '0.7rem 0.8rem',
  border: '1px solid color-mix(in srgb, currentColor 15%, transparent)',
  borderRadius: '0.75rem',
  background: 'color-mix(in srgb, Canvas 94%, transparent)',
  color: 'CanvasText',
  boxShadow: '0 0.6rem 1.8rem rgb(0 0 0 / 18%)',
  backdropFilter: 'blur(14px)',
};

const textStyle: CSSProperties = {
  display: 'grid',
  minWidth: 0,
  flex: 1,
  gap: '0.15rem',
};

const titleStyle: CSSProperties = {
  fontSize: '0.87rem',
  fontWeight: 650,
};

const metaStyle: CSSProperties = {
  fontSize: '0.75rem',
  opacity: 0.72,
};

const actionStyle: CSSProperties = {
  flex: '0 0 auto',
  minHeight: '2rem',
  padding: '0.3rem 0.65rem',
  border: '1px solid color-mix(in srgb, currentColor 18%, transparent)',
  borderRadius: '0.45rem',
  background: 'transparent',
  color: 'inherit',
  cursor: 'pointer',
  font: 'inherit',
  fontSize: '0.78rem',
};

/** Formats elapsed scan time at one-second granularity so the label stays stable. */
export function formatScanElapsed(elapsedMs: number): string {
  const safeElapsedMs = Number.isFinite(elapsedMs) ? Math.max(0, elapsedMs) : 0;
  const totalSeconds = Math.floor(safeElapsedMs / 1_000);
  if (totalSeconds === 0) return '< 1s';

  const seconds = totalSeconds % 60;
  const totalMinutes = Math.floor(totalSeconds / 60);
  if (totalMinutes === 0) return `${seconds}s`;

  const minutes = totalMinutes % 60;
  const hours = Math.floor(totalMinutes / 60);
  if (hours === 0) return `${minutes}m ${seconds}s`;
  return `${hours}h ${minutes}m`;
}

export function ScanActivity({ presentation, onCancel, onDismiss }: ScanActivityProps) {
  if (presentation.kind === 'hidden') return null;

  if (presentation.kind === 'cancelled') {
    return (
      <section
        aria-atomic="true"
        aria-live="polite"
        className="scan-activity scan-activity--cancelled"
        data-non-modal="true"
        data-scan-kind="cancelled"
        role="status"
        style={activityStyle}
      >
        <div style={textStyle}>
          <span style={titleStyle}>Scan cancelled</span>
          <span style={metaStyle}>Completed source updates were kept.</span>
        </div>
        <button onClick={onDismiss} style={actionStyle} type="button">
          Dismiss
        </button>
      </section>
    );
  }

  const isCancelling = presentation.kind === 'cancelling';
  const canCancel = !isCancelling && presentation.canCancel;
  return (
    <section
      aria-atomic="true"
      aria-live="polite"
      className={`scan-activity scan-activity--${presentation.kind}`}
      data-non-modal="true"
      data-scan-kind={presentation.kind}
      role="status"
      style={activityStyle}
    >
      <div style={textStyle}>
        <span style={titleStyle}>{isCancelling ? 'Cancelling scan…' : 'Scanning wallpapers…'}</span>
        <span style={metaStyle}>Elapsed {formatScanElapsed(presentation.elapsedMs)}</span>
      </div>
      <button
        disabled={!canCancel}
        onClick={canCancel ? onCancel : undefined}
        style={{ ...actionStyle, cursor: canCancel ? 'pointer' : 'wait', opacity: canCancel ? 1 : 0.62 }}
        type="button"
      >
        {isCancelling ? 'Cancelling…' : 'Cancel'}
      </button>
    </section>
  );
}
