import type { ScanProgressDTO } from '../api/types.ts';
import type { ScanPresentation } from './feedbackState';

export interface ScanActivityProps {
  readonly presentation: ScanPresentation;
  readonly progress?: Pick<ScanProgressDTO, 'scanned' | 'totalHint'> | null;
  readonly onCancel: () => void;
  readonly onDismiss: () => void;
}

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

function determinateProgress(
  progress: ScanActivityProps['progress'],
): { readonly max: number; readonly value: number } | null {
  const totalHint = progress?.totalHint;
  if (typeof totalHint !== 'number' || !Number.isFinite(totalHint) || totalHint <= 0) {
    return null;
  }

  const scannedValue = progress?.scanned ?? 0;
  const scanned = Number.isFinite(scannedValue) ? scannedValue : 0;
  return {
    max: totalHint,
    value: Math.min(totalHint, Math.max(0, scanned)),
  };
}

export function ScanActivity({ presentation, progress, onCancel, onDismiss }: ScanActivityProps) {
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
      >
        <div className="scan-activity__content">
          <span className="scan-activity__title">Scan cancelled</span>
          <span className="scan-activity__meta">Completed source updates were kept.</span>
        </div>
        <button className="scan-activity__action" onClick={onDismiss} type="button">
          Dismiss
        </button>
      </section>
    );
  }

  const isCancelling = presentation.kind === 'cancelling';
  const canCancel = !isCancelling && presentation.canCancel;
  const determinate = determinateProgress(progress);
  return (
    <section
      aria-atomic="true"
      aria-live="polite"
      className={`scan-activity scan-activity--${presentation.kind}`}
      data-non-modal="true"
      data-scan-kind={presentation.kind}
      role="status"
    >
      <div className="scan-activity__content">
        <span className="scan-activity__title">
          {isCancelling ? 'Cancelling scan…' : 'Scanning wallpapers…'}
        </span>
        <progress
          aria-label="Wallpaper scan progress"
          className="scan-activity__progress"
          {...(determinate ?? {})}
        />
        <span aria-hidden="true" className="scan-activity__meta">
          Elapsed {formatScanElapsed(presentation.elapsedMs)}
        </span>
      </div>
      <button
        className="scan-activity__action"
        disabled={!canCancel}
        onClick={canCancel ? onCancel : undefined}
        type="button"
      >
        {isCancelling ? 'Cancelling…' : 'Cancel'}
      </button>
    </section>
  );
}
