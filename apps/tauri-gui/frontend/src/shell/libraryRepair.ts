import type { CommandResult } from '../api/types.ts';
import type { LibraryTypeFilter, SourceFilter } from './shellPreferences.ts';

export interface LibraryRepairFault {
  readonly message: string;
  readonly technicalDetails: string;
}

export interface LibraryVerificationApi {
  sqliteVerify(): Promise<CommandResult>;
}

export type LibraryVerificationOutcome =
  | { readonly status: 'ok' }
  | { readonly status: 'corrupt'; readonly fault: LibraryRepairFault }
  | { readonly status: 'unavailable'; readonly technicalDetails: string };

export interface LibraryVerificationContext {
  readonly browserLoadError: boolean;
  readonly sourceLoadError: boolean;
  readonly sourceCount: number;
  readonly emptyConfirmed: boolean;
  readonly sourceFilter: SourceFilter;
  readonly typeFilter: LibraryTypeFilter;
  readonly favoritesOnly: boolean;
  readonly search: string;
}

export function commandResultTechnicalDetails(result: CommandResult): string {
  const parts = [
    result.error?.message,
    result.error?.suggestion,
    result.error?.detail,
    result.stderr,
    result.stdout,
  ].filter((part): part is string => Boolean(part?.trim()));
  return [...new Set(parts)].join('\n') || 'SQLite integrity verification failed.';
}

/** A repair action is offered only after the backend confirms corruption. */
export function classifyLibraryVerification(
  result: CommandResult,
): LibraryVerificationOutcome {
  if (result.success) return { status: 'ok' };
  const technicalDetails = commandResultTechnicalDetails(result);
  if (result.error?.kind === 'sqlite_integrity') {
    return {
      status: 'corrupt',
      fault: {
        message: 'Library database needs repair',
        technicalDetails,
      },
    };
  }
  return { status: 'unavailable', technicalDetails };
}

export function faultAfterVerification(
  current: LibraryRepairFault | null,
  outcome: LibraryVerificationOutcome,
): LibraryRepairFault | null {
  if (outcome.status === 'corrupt') return outcome.fault;
  if (outcome.status === 'ok') return null;
  return current;
}

export function shouldVerifyLibraryIntegrity(context: LibraryVerificationContext): boolean {
  if (context.browserLoadError || context.sourceLoadError) return true;
  return context.sourceCount > 0
    && context.emptyConfirmed
    && context.sourceFilter.kind === 'all'
    && context.typeFilter === 'usable'
    && !context.favoritesOnly
    && context.search.trim() === '';
}

export async function verifyLibraryIntegrity(
  api: LibraryVerificationApi,
): Promise<LibraryVerificationOutcome> {
  try {
    return classifyLibraryVerification(await api.sqliteVerify());
  } catch (error) {
    // A transport failure is not evidence that the database is corrupt.
    const technicalDetails = error instanceof Error
      ? error.message
      : typeof error === 'string' && error.trim()
        ? error.trim()
        : 'SQLite integrity verification is unavailable.';
    return { status: 'unavailable', technicalDetails };
  }
}
