import type { DisplayTarget } from './shellPreferences.ts';

export interface RuntimeDisplayWallpaper {
  output: string;
  wallpaperPath: string | null;
  /** Only confirmed observations are evidence for the current-wallpaper badge. */
  status: 'confirmed' | 'unknown';
}

export interface PersistedDisplayWallpaper {
  target: DisplayTarget;
  wallpaperPath: string;
}

export interface CurrentWallpaperSnapshot {
  activeTarget: DisplayTarget;
  connectedOutputs: readonly string[];
  runtime: readonly RuntimeDisplayWallpaper[];
  /** Saved restoration data is intentionally not realtime evidence. */
  persisted?: readonly PersistedDisplayWallpaper[];
}

export type CurrentWallpaperState =
  | {
    readonly kind: 'confirmed';
    readonly wallpaperPath: string;
    readonly outputs: readonly string[];
  }
  | {
    readonly kind: 'mixed';
    readonly outputs: readonly {
      readonly output: string;
      readonly wallpaperPath: string;
    }[];
  }
  | {
    readonly kind: 'unknown';
    readonly outputs: readonly string[];
  };

type RuntimeEvidence =
  | { readonly kind: 'confirmed'; readonly wallpaperPath: string }
  | { readonly kind: 'ambiguous' };

function normalizedOutputNames(outputs: readonly string[]): string[] {
  const seen = new Set<string>();
  const normalized: string[] = [];
  for (const candidate of outputs) {
    const output = candidate.trim();
    if (output.length === 0 || seen.has(output)) continue;
    seen.add(output);
    normalized.push(output);
  }
  return normalized;
}

function runtimeEvidenceByOutput(
  observations: readonly RuntimeDisplayWallpaper[],
): Map<string, RuntimeEvidence> {
  const evidence = new Map<string, RuntimeEvidence>();
  for (const observation of observations) {
    const output = observation.output.trim();
    if (output.length === 0) continue;

    const pathIsUsable = observation.status === 'confirmed'
      && typeof observation.wallpaperPath === 'string'
      && observation.wallpaperPath.trim().length > 0;
    const next: RuntimeEvidence = pathIsUsable
      ? { kind: 'confirmed', wallpaperPath: observation.wallpaperPath as string }
      : { kind: 'ambiguous' };
    const previous = evidence.get(output);

    if (!previous) {
      evidence.set(output, next);
    } else if (
      previous.kind !== 'confirmed'
      || next.kind !== 'confirmed'
      || previous.wallpaperPath !== next.wallpaperPath
    ) {
      evidence.set(output, { kind: 'ambiguous' });
    }
  }
  return evidence;
}

function confirmedPath(evidence: ReadonlyMap<string, RuntimeEvidence>, output: string): string | null {
  const observation = evidence.get(output);
  return observation?.kind === 'confirmed' ? observation.wallpaperPath : null;
}

/**
 * Resolve current-card state exclusively from reconciled runtime evidence.
 * Persisted mappings remain available to callers for restore UI, but cannot
 * produce a confirmed or mixed result here.
 */
export function resolveCurrentWallpaperState(
  snapshot: CurrentWallpaperSnapshot,
): CurrentWallpaperState {
  void snapshot.persisted;
  const connectedOutputs = normalizedOutputNames(snapshot.connectedOutputs);
  const runtime = runtimeEvidenceByOutput(snapshot.runtime);

  if (snapshot.activeTarget.kind === 'output') {
    const output = snapshot.activeTarget.output.trim();
    const outputs = output.length > 0 ? [output] : [];
    if (!connectedOutputs.includes(output)) return { kind: 'unknown', outputs };
    const wallpaperPath = confirmedPath(runtime, output);
    return wallpaperPath === null
      ? { kind: 'unknown', outputs }
      : { kind: 'confirmed', wallpaperPath, outputs };
  }

  if (connectedOutputs.length === 0) return { kind: 'unknown', outputs: [] };
  const assignments = connectedOutputs.map((output) => ({
    output,
    wallpaperPath: confirmedPath(runtime, output),
  }));
  if (assignments.some((assignment) => assignment.wallpaperPath === null)) {
    return { kind: 'unknown', outputs: connectedOutputs };
  }

  const confirmedAssignments = assignments as Array<{ output: string; wallpaperPath: string }>;
  const firstPath = confirmedAssignments[0].wallpaperPath;
  if (confirmedAssignments.every((assignment) => assignment.wallpaperPath === firstPath)) {
    return {
      kind: 'confirmed',
      wallpaperPath: firstPath,
      outputs: connectedOutputs,
    };
  }
  return { kind: 'mixed', outputs: confirmedAssignments };
}
