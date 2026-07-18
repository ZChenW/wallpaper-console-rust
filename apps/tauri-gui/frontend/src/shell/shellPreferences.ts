import type { WallpaperCardSize } from '../utils/layout.ts';
import { isShellTheme, type ShellTheme } from './shellThemes.ts';

export type { ShellTheme } from './shellThemes.ts';

export type SourceFilter =
  | { readonly kind: 'all' }
  | { readonly kind: 'source'; readonly sourceId: number };

export type LibraryTypeFilter =
  | 'usable'
  | 'image'
  | 'gif'
  | 'video'
  | 'weScene'
  | 'unsupported';

export type LibrarySort = 'recentlyAdded' | 'nameAsc' | 'nameDesc';
export type LibraryViewMode = 'grid' | 'flow';

export type DisplayTarget =
  | { readonly kind: 'allDisplays' }
  | { readonly kind: 'output'; readonly output: string };

export type ApplyGesture = 'single' | 'double';
/**
 * The complete persisted App Shell state. Search, selection, scroll position,
 * scan progress, and feedback deliberately have no place in this interface.
 */
export interface ShellPreferences {
  sourceFilter: SourceFilter;
  typeFilter: LibraryTypeFilter;
  favoritesOnly: boolean;
  sort: LibrarySort;
  cardSize: WallpaperCardSize;
  displayTarget: DisplayTarget;
  applyGesture: ApplyGesture;
  theme: ShellTheme;
  libraryViewMode: LibraryViewMode;
}

export const DEFAULT_SHELL_PREFERENCES: Readonly<ShellPreferences> = Object.freeze({
  sourceFilter: Object.freeze({ kind: 'all' as const }),
  typeFilter: 'usable',
  favoritesOnly: false,
  sort: 'recentlyAdded',
  cardSize: 'medium',
  displayTarget: Object.freeze({ kind: 'allDisplays' as const }),
  applyGesture: 'single',
  theme: 'system',
  libraryViewMode: 'grid',
});

const TYPE_FILTERS = new Set<LibraryTypeFilter>([
  'usable',
  'image',
  'gif',
  'video',
  'weScene',
  'unsupported',
]);
const SORTS = new Set<LibrarySort>(['recentlyAdded', 'nameAsc', 'nameDesc']);
const CARD_SIZES = new Set<WallpaperCardSize>(['small', 'medium', 'large']);
const APPLY_GESTURES = new Set<ApplyGesture>(['single', 'double']);
const LIBRARY_VIEW_MODES = new Set<LibraryViewMode>(['grid', 'flow']);

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function defaultSourceFilter(): SourceFilter {
  return { kind: 'all' };
}

function normalizeSourceFilter(value: unknown): SourceFilter {
  if (!isRecord(value)) return defaultSourceFilter();
  if (value.kind === 'all') return defaultSourceFilter();
  if (
    value.kind === 'source'
    && typeof value.sourceId === 'number'
    && Number.isSafeInteger(value.sourceId)
    && value.sourceId > 0
  ) {
    return { kind: 'source', sourceId: value.sourceId };
  }
  return defaultSourceFilter();
}

function normalizeDisplayTarget(value: unknown): DisplayTarget {
  if (!isRecord(value)) return { kind: 'allDisplays' };
  if (value.kind === 'allDisplays') return { kind: 'allDisplays' };
  if (value.kind === 'output' && typeof value.output === 'string') {
    const output = value.output.trim();
    if (output.length > 0) return { kind: 'output', output };
  }
  return { kind: 'allDisplays' };
}

function memberOf<T extends string>(value: unknown, values: ReadonlySet<T>, fallback: T): T {
  return typeof value === 'string' && values.has(value as T) ? value as T : fallback;
}

/** Repair an untrusted decoded value without coercing legacy or unknown values. */
export function normalizeShellPreferences(value: unknown): ShellPreferences {
  const record = isRecord(value) ? value : {};
  return {
    sourceFilter: normalizeSourceFilter(record.sourceFilter),
    typeFilter: memberOf(record.typeFilter, TYPE_FILTERS, 'usable'),
    favoritesOnly: typeof record.favoritesOnly === 'boolean' ? record.favoritesOnly : false,
    sort: memberOf(record.sort, SORTS, 'recentlyAdded'),
    cardSize: memberOf(record.cardSize, CARD_SIZES, 'medium'),
    displayTarget: normalizeDisplayTarget(record.displayTarget),
    applyGesture: memberOf(record.applyGesture, APPLY_GESTURES, 'single'),
    theme: isShellTheme(record.theme) ? record.theme : 'system',
    libraryViewMode: memberOf(record.libraryViewMode, LIBRARY_VIEW_MODES, 'grid'),
  };
}

export function parseShellPreferences(raw: string | null | undefined): ShellPreferences {
  if (typeof raw !== 'string') return normalizeShellPreferences(undefined);
  try {
    return normalizeShellPreferences(JSON.parse(raw));
  } catch {
    return normalizeShellPreferences(undefined);
  }
}

/**
 * Serialize through an explicit allow-list so transient fields cannot leak
 * into storage even when a wider runtime object is passed by a caller.
 */
export function serializeShellPreferences(preferences: ShellPreferences): string {
  const normalized = normalizeShellPreferences(preferences);
  return JSON.stringify({
    version: 1,
    sourceFilter: normalized.sourceFilter,
    typeFilter: normalized.typeFilter,
    favoritesOnly: normalized.favoritesOnly,
    sort: normalized.sort,
    cardSize: normalized.cardSize,
    displayTarget: normalized.displayTarget,
    applyGesture: normalized.applyGesture,
    theme: normalized.theme,
    libraryViewMode: normalized.libraryViewMode,
  });
}
