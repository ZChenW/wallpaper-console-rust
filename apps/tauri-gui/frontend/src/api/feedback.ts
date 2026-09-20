import type { CommandResult } from './bridge';
import { parseSwitchReport, switchOutcomeLabel } from './switchReport.ts';

export type CommandFeedback =
  | { state: 'idle' }
  | { state: 'running'; label: string; detail?: string }
  | { state: 'success'; label: string; detail?: string }
  | { state: 'warning'; label: string; detail: string }
  | { state: 'error'; label: string; detail: string };

export function commandResultMessage(result: CommandResult, fallback: string): string {
  return result.stdout.trim() || fallback;
}

export function commandSuccessFeedback(label: string, result?: CommandResult | void): CommandFeedback {
  return {
    state: 'success',
    label: `${label} complete`,
    detail: result && result.stdout ? result.stdout : undefined,
  };
}

export function commandErrorFeedback(label: string, resultOrError: CommandResult | unknown): CommandFeedback {
  if (isCommandResult(resultOrError)) {
    const report = parseSwitchReport(resultOrError.error?.detail);
    if (report) {
      const recovered = report.outcome === 'restored_previous' || report.outcome === 'unchanged';
      return {
        state: recovered ? 'warning' : 'error',
        label: `${label} failed — ${switchOutcomeLabel[report.outcome].toLowerCase()}`,
        detail: [report.originalError, ...report.outputs.map((row) =>
          `${row.output}: ${switchOutcomeLabel[row.outcome]}${row.error ? ` — ${row.error}` : ''}`)]
          .filter(Boolean).join('\n'),
      };
    }
    const detail = [
      resultOrError.error?.message || resultOrError.stderr || resultOrError.stdout || 'The command failed.',
      resultOrError.error?.suggestion,
      resultOrError.error?.detail && resultOrError.error.detail !== resultOrError.error.message
        ? resultOrError.error.detail
        : undefined,
    ]
      .filter(Boolean)
      .join('\n');
    return { state: 'error', label: `${label} failed`, detail };
  }
  return { state: 'error', label: `${label} failed`, detail: String(resultOrError) };
}

function isCommandResult(value: unknown): value is CommandResult {
  return Boolean(value && typeof value === 'object' && 'success' in value);
}
