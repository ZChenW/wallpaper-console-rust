import type { SwitchOutcome, SwitchReportDTO } from './types.ts';

const outcomes = new Set<SwitchOutcome>(['applied', 'stopped', 'already_satisfied', 'unchanged',
  'restored_previous', 'partial', 'recovery_failed', 'unknown']);

export function parseSwitchReport(raw: unknown): SwitchReportDTO | undefined {
  try {
    const value = typeof raw === 'string' ? JSON.parse(raw) : raw;
    if (!value || typeof value !== 'object' || typeof value.operationId !== 'string'
      || !outcomes.has(value.outcome) || !Array.isArray(value.outputs)
      || !value.outputs.every((row: Record<string, unknown>) => row && typeof row.output === 'string'
        && outcomes.has(row.outcome as SwitchOutcome) && (row.error === null || typeof row.error === 'string'))
      || (value.originalError !== null && typeof value.originalError !== 'string')) return undefined;
    return value;
  } catch { return undefined; }
}

export const switchOutcomeLabel: Record<SwitchOutcome, string> = {
  applied: 'Applied', stopped: 'Stopped', already_satisfied: 'Already applied', unchanged: 'Unchanged',
  restored_previous: 'Previous wallpaper restored', partial: 'Partial failure',
  recovery_failed: 'Recovery failed', unknown: 'Needs verification',
};
