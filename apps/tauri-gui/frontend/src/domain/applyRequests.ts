import type { ApplyActionKind, ApplyRequestDTO, WallpaperDTO } from '../api/bridge';

const EXECUTABLE_ACTIONS = new Set<ApplyActionKind>([
  'apply',
  'retry_backend_apply',
  'apply_preview',
]);

export function buildApplyRequest(
  entry: WallpaperDTO,
  kind: ApplyActionKind,
): ApplyRequestDTO {
  if (!EXECUTABLE_ACTIONS.has(kind)) {
    throw new Error(`Action is not executable as apply: ${kind}`);
  }
  return {
    kind: kind as ApplyRequestDTO['kind'],
    path: entry.path,
    requestId: `${Date.now()}-${Math.random().toString(36).slice(2)}`,
  };
}
