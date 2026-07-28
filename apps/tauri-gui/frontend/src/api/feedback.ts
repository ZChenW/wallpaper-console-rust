import type { CommandResult } from './bridge';

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
