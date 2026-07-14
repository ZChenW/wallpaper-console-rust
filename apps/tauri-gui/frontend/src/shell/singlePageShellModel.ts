import type { SourceDTO } from '../api/types.ts';
import type { CurrentWallpaperState } from './currentWallpaperState.ts';
import type { DisplayTarget, SourceFilter } from './shellPreferences.ts';

/** Convert the persisted target into the Tauri wire format without inventing a sentinel. */
export function targetArgument(target: DisplayTarget): string | undefined {
  if (target.kind !== 'output') return undefined;
  const output = target.output.trim();
  return output.length > 0 ? output : undefined;
}

/** A removed source cannot remain an invisible active filter. */
export function reconcileSourceFilter(
  sourceFilter: SourceFilter,
  sources: readonly SourceDTO[],
): SourceFilter {
  if (sourceFilter.kind === 'all') return sourceFilter;
  return sources.some((source) => source.id === sourceFilter.sourceId)
    ? sourceFilter
    : { kind: 'all' };
}

export function reconcileSelectedEntry<T extends { readonly path: string }>(
  selected: T | null,
  entryByPath: ReadonlyMap<string, T>,
): T | null {
  if (!selected) return null;
  return entryByPath.get(selected.path) ?? null;
}

/** A failed source read is an error state, never evidence of a fresh install. */
export function shouldOfferFirstRun(
  sources: readonly SourceDTO[],
  sourceError: string | undefined,
): boolean {
  return sources.length === 0 && sourceError === undefined;
}

export interface RandomWallpaperAvailability {
  readonly searchSettled: boolean;
  readonly randomPending: boolean;
  readonly total: number;
  readonly canApply: boolean;
}

export function canChooseRandomWallpaper(input: RandomWallpaperAvailability): boolean {
  return input.searchSettled
    && !input.randomPending
    && input.total > 0
    && input.canApply;
}

function basename(path: string): string {
  const normalized = path.replace(/\/+$/, '');
  const separator = normalized.lastIndexOf('/');
  return separator >= 0 ? normalized.slice(separator + 1) : normalized;
}

/** Persisted display state is intentionally excluded; only runtime evidence reaches here. */
export function currentWallpaperLabel(state: CurrentWallpaperState): string {
  if (state.kind === 'confirmed') return `Current: ${basename(state.wallpaperPath)}`;
  if (state.kind === 'mixed') return 'Current: displays use different wallpapers';
  return 'Current: not verified';
}
