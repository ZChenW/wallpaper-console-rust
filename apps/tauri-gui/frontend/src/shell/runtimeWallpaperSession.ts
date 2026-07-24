import type {
  ApplyRequestDTO,
  ApplyResultDTO,
  TargetedApplyRequestDTO,
} from '../api/types.ts';
import { applyEvidenceFromResult } from './applyResultEvidence.ts';
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
    readonly type: 'runtimeInvalidated';
  }
  | {
    readonly type: 'runtimeReconciled';
    readonly observations: readonly RuntimeDisplayWallpaper[];
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

function responseMatchesRequest(
  request: ApplyRequestDTO | TargetedApplyRequestDTO,
  result: ApplyResultDTO | undefined,
): boolean {
  if (request.requestId === undefined) return true;
  return result?.requestId === request.requestId;
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

  if (action.type === 'runtimeInvalidated') {
    return state.confirmations.length === 0
      ? state
      : { ...state, confirmations: [] };
  }

  if (action.type === 'runtimeReconciled') {
    const observed = new Map<string, string | null>();
    for (const observation of action.observations) {
      const output = observation.output.trim();
      if (!state.connectedOutputs.includes(output)) continue;
      const next = observation.status === 'confirmed'
        && typeof observation.wallpaperPath === 'string'
        && observation.wallpaperPath.trim().length > 0
        ? observation.wallpaperPath
        : null;
      if (!observed.has(output)) {
        observed.set(output, next);
      } else if (observed.get(output) !== next) {
        observed.set(output, null);
      }
    }
    return {
      ...state,
      confirmations: state.connectedOutputs.flatMap((output) => {
        const wallpaperPath = observed.get(output);
        return wallpaperPath ? [{ output, wallpaperPath }] : [];
      }),
    };
  }

  if (action.transport !== 'targeted' || !responseMatchesRequest(action.request, action.result)) {
    return state;
  }

  const target = targetedRequestTarget(action.request);
  const evidence = applyEvidenceFromResult(action.result);
  if (target === null || evidence === null) return state;
  const { statePath: wallpaperPath, appliedOutputs } = evidence;

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
