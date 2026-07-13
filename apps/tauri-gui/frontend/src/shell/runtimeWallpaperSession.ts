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
  if ('kind' in candidate) return null;
  if (typeof candidate.path !== 'string' || candidate.path.trim().length === 0) return null;
  if (candidate.target === undefined) return { kind: 'allDisplays' };
  if (typeof candidate.target !== 'string') return null;
  const output = candidate.target.trim();
  return output.length > 0 ? { kind: 'output', output } : null;
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

  const target = targetedRequestTarget(action.request);
  const wallpaperPath = confirmedStatePath(action.result);
  if (target === null || wallpaperPath === null) return state;

  if (target.kind === 'allDisplays') {
    return {
      ...state,
      confirmations: state.connectedOutputs.map((output) => ({ output, wallpaperPath })),
    };
  }

  if (!state.connectedOutputs.includes(target.output)) return state;
  const paths = new Map(
    state.confirmations.map(({ output, wallpaperPath: confirmedPath }) => [output, confirmedPath]),
  );
  paths.set(target.output, wallpaperPath);
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
