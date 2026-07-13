import type {
  ApplyRequestDTO,
  ApplyResultDTO,
  TargetedApplyRequestDTO,
} from '../api/types.ts';
import type { RuntimeDisplayWallpaper } from './currentWallpaperState.ts';
import { normalizeDisplayOutputs } from './displayTargets.ts';

export interface RuntimeWallpaperConfirmation {
  readonly output: string;
  readonly wallpaperPath: string;
}

export interface RuntimeWallpaperSession {
  readonly connectedOutputs: readonly string[];
  readonly confirmations: readonly RuntimeWallpaperConfirmation[];
}

export type RuntimeWallpaperSessionAction =
  | {
    readonly type: 'connectedOutputsChanged';
    readonly connectedOutputs: readonly string[];
  }
  | {
    readonly type: 'applySucceeded';
    readonly transport: 'action' | 'targeted';
    readonly request: ApplyRequestDTO | TargetedApplyRequestDTO;
    readonly result: ApplyResultDTO | undefined;
  };

type TargetedRequestTarget =
  | { readonly kind: 'allDisplays' }
  | { readonly kind: 'output'; readonly output: string };

export function createRuntimeWallpaperSession(
  connectedOutputs: readonly string[],
): RuntimeWallpaperSession {
  return {
    connectedOutputs: normalizeDisplayOutputs(connectedOutputs),
    confirmations: [],
  };
}

function targetedRequestTarget(
  request: ApplyRequestDTO | TargetedApplyRequestDTO,
): TargetedRequestTarget | null {
  const candidate = request as unknown as Record<string, unknown>;
  if (typeof candidate.path !== 'string' || candidate.path.trim().length === 0) return null;
  if (candidate.target === undefined) return { kind: 'allDisplays' };
  if (typeof candidate.target !== 'string') return null;
  const output = candidate.target.trim();
  return output.length > 0 ? { kind: 'output', output } : null;
}

function confirmedAppliedOutputs(result: ApplyResultDTO | undefined): string[] | null {
  const candidate = result as unknown as Record<string, unknown> | undefined;
  if (!candidate || !Array.isArray(candidate.appliedOutputs)) return null;
  if (!candidate.appliedOutputs.every((output) => typeof output === 'string')) return null;
  const outputs = normalizeDisplayOutputs(candidate.appliedOutputs as string[]);
  return outputs.length > 0 ? outputs : null;
}

function responseMatchesRequest(
  request: ApplyRequestDTO | TargetedApplyRequestDTO,
  result: ApplyResultDTO | undefined,
): boolean {
  if (request.requestId === undefined) return true;
  return result?.requestId === request.requestId;
}

function confirmedStatePath(result: ApplyResultDTO | undefined): string | null {
  const candidate = result as unknown as Record<string, unknown> | undefined;
  if (!candidate) return null;
  if (
    typeof candidate.appliedPath !== 'string'
    || candidate.appliedPath.trim().length === 0
    || typeof candidate.statePath !== 'string'
    || candidate.statePath.trim().length === 0
    || typeof candidate.backend !== 'string'
    || candidate.backend.trim().length === 0
    || typeof candidate.fileType !== 'string'
    || candidate.fileType.trim().length === 0
    || typeof candidate.preview !== 'boolean'
  ) {
    return null;
  }
  return candidate.statePath;
}

function confirmationsInOutputOrder(
  connectedOutputs: readonly string[],
  confirmations: readonly RuntimeWallpaperConfirmation[],
): RuntimeWallpaperConfirmation[] {
  const paths = new Map(confirmations.map(({ output, wallpaperPath }) => [output, wallpaperPath]));
  return connectedOutputs.flatMap((output) => {
    const wallpaperPath = paths.get(output);
    return wallpaperPath === undefined ? [] : [{ output, wallpaperPath }];
  });
}

export function reduceRuntimeWallpaperSession(
  state: RuntimeWallpaperSession,
  action: RuntimeWallpaperSessionAction,
): RuntimeWallpaperSession {
  if (action.type === 'connectedOutputsChanged') {
    const connectedOutputs = normalizeDisplayOutputs(action.connectedOutputs);
    return {
      connectedOutputs,
      confirmations: confirmationsInOutputOrder(connectedOutputs, state.confirmations),
    };
  }

  if (action.transport !== 'targeted' || !responseMatchesRequest(action.request, action.result)) {
    return state;
  }

  const target = targetedRequestTarget(action.request);
  const wallpaperPath = confirmedStatePath(action.result);
  const appliedOutputs = confirmedAppliedOutputs(action.result);
  if (target === null || wallpaperPath === null || appliedOutputs === null) return state;

  const affectedOutputs = target.kind === 'allDisplays'
    ? appliedOutputs.filter((output) => state.connectedOutputs.includes(output))
    : appliedOutputs.includes(target.output) && state.connectedOutputs.includes(target.output)
      ? [target.output]
      : [];
  if (affectedOutputs.length === 0) return state;

  const paths = new Map(
    state.confirmations.map(({ output, wallpaperPath: confirmedPath }) => [output, confirmedPath]),
  );
  for (const output of affectedOutputs) paths.set(output, wallpaperPath);
  return {
    ...state,
    confirmations: state.connectedOutputs.flatMap((output) => {
      const confirmedPath = paths.get(output);
      return confirmedPath === undefined ? [] : [{ output, wallpaperPath: confirmedPath }];
    }),
  };
}

export function toRuntimeDisplayWallpapers(
  state: RuntimeWallpaperSession,
): RuntimeDisplayWallpaper[] {
  const paths = new Map(
    state.confirmations.map(({ output, wallpaperPath }) => [output, wallpaperPath]),
  );
  return state.connectedOutputs.map((output) => {
    const wallpaperPath = paths.get(output);
    return wallpaperPath === undefined
      ? { output, wallpaperPath: null, status: 'unknown' }
      : { output, wallpaperPath, status: 'confirmed' };
  });
}
