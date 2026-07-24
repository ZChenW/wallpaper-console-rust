import type { ApplyResultDTO } from '../api/types.ts';
import { normalizeDisplayOutputs } from './displayTargets.ts';

/**
 * Shared ApplyResult evidence for RuntimeWallpaper: queue parse + session confirm.
 * One place for field/trim/requestId rules so feedback and Current stay aligned.
 */
export function parseApplyResult(stdout: string): ApplyResultDTO | undefined {
  if (!stdout) return undefined;
  try {
    const parsed: unknown = JSON.parse(stdout);
    if (typeof parsed !== 'object' || parsed === null) return undefined;
    const value = parsed as Record<string, unknown>;
    if (
      typeof value.appliedPath !== 'string'
      || typeof value.statePath !== 'string'
      || typeof value.backend !== 'string'
      || typeof value.fileType !== 'string'
      || typeof value.preview !== 'boolean'
      || (
        value.appliedOutputs !== undefined
        && (
          !Array.isArray(value.appliedOutputs)
          || !value.appliedOutputs.every((output) => typeof output === 'string')
        )
      )
      || (
        value.requestId !== undefined
        && value.requestId !== null
        && typeof value.requestId !== 'string'
      )
    ) {
      return undefined;
    }
    return {
      ...(typeof value.requestId === 'string' ? { requestId: value.requestId } : {}),
      appliedPath: value.appliedPath,
      statePath: value.statePath,
      backend: value.backend,
      fileType: value.fileType,
      preview: value.preview,
      ...(Array.isArray(value.appliedOutputs)
        ? { appliedOutputs: value.appliedOutputs as string[] }
        : {}),
    };
  } catch {
    return undefined;
  }
}

export function applyEvidenceFromResult(
  result: ApplyResultDTO | undefined,
): { statePath: string; appliedOutputs: string[] } | null {
  if (!result) return null;
  const appliedPath = result.appliedPath.trim();
  const statePath = result.statePath.trim();
  const backend = result.backend.trim();
  const fileType = result.fileType.trim();
  if (
    appliedPath.length === 0
    || statePath.length === 0
    || backend.length === 0
    || fileType.length === 0
    || typeof result.preview !== 'boolean'
  ) {
    return null;
  }
  if (!Array.isArray(result.appliedOutputs)) return null;
  if (!result.appliedOutputs.every((output) => typeof output === 'string')) return null;
  const appliedOutputs = normalizeDisplayOutputs(result.appliedOutputs);
  if (appliedOutputs.length === 0) return null;
  return { statePath, appliedOutputs };
}

export function applyResultMatchesRequestId(
  requestId: string | undefined,
  result: ApplyResultDTO | undefined,
): boolean {
  if (requestId === undefined) return true;
  return result?.requestId === requestId;
}
