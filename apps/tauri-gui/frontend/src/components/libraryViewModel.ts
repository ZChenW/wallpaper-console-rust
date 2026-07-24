import type { LibraryBrowserItemDTO } from '../api/types.ts';

export type LibraryAdapterMode = 'grid' | 'flow';

export const DISPLAY_APPLY_DISABLED_REASON = 'The selected display is unavailable.';

export interface LibraryStableAnchor {
  readonly wallpaperId: number;
  readonly index: number;
}

interface LibraryAnchorEntry {
  readonly wallpaperId: number;
  readonly path: string;
}

interface LibraryAdapterFactories<T> {
  readonly grid: () => T;
  readonly flow: () => T;
}

/**
 * Instantiate exactly one Library presentation adapter.
 *
 * Keeping the factories lazy is intentional: the inactive adapter must not
 * mount and compete for thumbnail scheduling or reveal-pause ownership.
 */
export function instantiateActiveLibraryAdapter<T>(
  mode: LibraryAdapterMode,
  factories: LibraryAdapterFactories<T>,
): T {
  return mode === 'flow' ? factories.flow() : factories.grid();
}

/** Resolve a mode switch by stable ID: Selected, outgoing center, then first. */
export function resolveLibraryModeSwitchAnchor(
  entries: readonly LibraryAnchorEntry[],
  selectedWallpaperId: number | null | undefined,
  outgoingWallpaperId: number | null | undefined,
): LibraryStableAnchor | null {
  return resolveLibraryAnchor(entries, [selectedWallpaperId, outgoingWallpaperId]);
}

/** Resolve Flow startup: explicit session anchor, loaded runtime Current, then first. */
export function resolveLibraryFlowStartupAnchor(
  entries: readonly LibraryAnchorEntry[],
  explicitAnchorWallpaperId: number | null | undefined,
  currentPath: string | null | undefined,
): LibraryStableAnchor | null {
  const explicitIndex = explicitAnchorWallpaperId == null
    ? -1
    : entries.findIndex((entry) => entry.wallpaperId === explicitAnchorWallpaperId);
  if (explicitIndex >= 0) {
    return {
      wallpaperId: entries[explicitIndex].wallpaperId,
      index: explicitIndex,
    };
  }

  const currentIndex = currentPath == null
    ? -1
    : entries.findIndex((entry) => entry.path === currentPath);
  if (currentIndex >= 0) {
    return {
      wallpaperId: entries[currentIndex].wallpaperId,
      index: currentIndex,
    };
  }

  return firstLibraryAnchor(entries);
}

/** A changed Library query always restarts at the first loaded result. */
export function resolveLibraryQueryResetAnchor(
  entries: readonly LibraryAnchorEntry[],
): LibraryStableAnchor | null {
  return firstLibraryAnchor(entries);
}

function resolveLibraryAnchor(
  entries: readonly LibraryAnchorEntry[],
  candidates: readonly (number | null | undefined)[],
): LibraryStableAnchor | null {
  for (const candidate of candidates) {
    if (candidate == null) continue;
    const index = entries.findIndex((entry) => entry.wallpaperId === candidate);
    if (index >= 0) return { wallpaperId: entries[index].wallpaperId, index };
  }
  return firstLibraryAnchor(entries);
}

function firstLibraryAnchor(
  entries: readonly LibraryAnchorEntry[],
): LibraryStableAnchor | null {
  const first = entries[0];
  return first ? { wallpaperId: first.wallpaperId, index: 0 } : null;
}

export interface ContextAction {
  readonly label: string;
  readonly action: (
    path: string,
    returnFocus: HTMLElement | null,
  ) => void | Promise<void>;
  readonly danger?: boolean;
  readonly visible?: (entry: LibraryBrowserItemDTO) => boolean;
}

/**
 * Shared Library state and semantic intents consumed by layout adapters.
 *
 * Every entry callback receives the rich browser item so an adapter can never
 * accidentally discard favorite, source, or stable-id metadata. `onApply`
 * applies the supplied entry; adapters that expose a path-only leaf control
 * must resolve that control back to its rendered entry before invoking it.
 */
export interface LibraryViewModel {
  readonly entries: readonly LibraryBrowserItemDTO[];
  readonly selectedPath: string | null;
  readonly currentPath: string | null;
  readonly currentObservationReady: boolean;
  readonly applying: boolean;
  readonly activePath: string | null;
  readonly pendingPath: string | null;
  readonly favoritePendingPaths: ReadonlySet<string>;
  readonly active: boolean;
  readonly refreshing: boolean;
  readonly resetKey: string;
  /** Monotonic count of successful page-one replacements for reset synchronization. */
  readonly replaceCount: number;
  /** The current filter/sort query has not yet produced its page-one replacement. */
  readonly queryReplacementPending: boolean;
  readonly totalKnown: boolean;
  readonly total: number | null;
  /** Explicit append may run (cursor remains; pause does not block). */
  readonly canAppend: boolean;
  /** Near-end auto-append may run. */
  readonly canAutoAppend: boolean;
  readonly loadingMore: boolean;
  /** True when auto-append paused but explicit retry is still possible. */
  readonly appendNeedsRetry: boolean;
  readonly loadErrorDetail: string | null;
  readonly canApplyToDisplay: boolean;
  /** When non-null, display targeting blocks apply for every entry. */
  readonly displayApplyDisabledReason: string | null;
  readonly isEntryApplicable: (entry: LibraryBrowserItemDTO) => boolean;
  readonly onSelect: (entry: LibraryBrowserItemDTO) => void;
  readonly onApply: (entry: LibraryBrowserItemDTO) => void;
  readonly onToggleFavorite: (entry: LibraryBrowserItemDTO) => void | Promise<void>;
  readonly onDetails: (
    entry: LibraryBrowserItemDTO,
    returnFocus?: HTMLElement | null,
  ) => void;
  readonly buildContextActions: (entry: LibraryBrowserItemDTO) => ContextAction[];
  /** Geometry-driven: no-op unless canAutoAppend. */
  readonly onRequestMoreIfNeeded: () => void | Promise<void>;
  /** Explicit Load more / End key: clears pause. */
  readonly onAppendMore: () => void | Promise<void>;
}

/** Shared Grid/Flow apply eligibility: display target must allow apply and entry must be applicable. */
export function libraryEntryApplyAvailable(
  canApplyToDisplay: boolean,
  isEntryApplicable: (entry: LibraryBrowserItemDTO) => boolean,
  entry: LibraryBrowserItemDTO,
): boolean {
  return canApplyToDisplay && isEntryApplicable(entry);
}

/** Prefer the display-level reason before per-entry renderer compatibility reasons. */
export function libraryEntryApplyDisabledReason(
  canApplyToDisplay: boolean,
  displayApplyDisabledReason: string | null,
  entry: LibraryBrowserItemDTO,
): string | null {
  if (!canApplyToDisplay) {
    return displayApplyDisabledReason;
  }
  if (entry.applyReason?.trim()) {
    return entry.applyReason.trim();
  }
  if (entry.unsupportedReason?.trim()) {
    return entry.unsupportedReason.trim();
  }
  return null;
}
