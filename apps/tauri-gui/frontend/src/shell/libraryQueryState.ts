import {
  DEFAULT_SHELL_PREFERENCES,
  normalizeShellPreferences,
  type LibrarySort,
  type LibraryTypeFilter,
  type ShellPreferences,
  type SourceFilter,
} from './shellPreferences.ts';

export const DEFAULT_LIBRARY_PAGE_SIZE = 120;

export interface LibraryQueryState {
  sourceFilter: SourceFilter;
  typeFilter: LibraryTypeFilter;
  favoritesOnly: boolean;
  sort: LibrarySort;
  /** Session-only text. It must never be copied into ShellPreferences. */
  search: string;
  offset: number;
  limit: number;
}

export const DEFAULT_LIBRARY_QUERY_STATE: Readonly<LibraryQueryState> = Object.freeze({
  sourceFilter: Object.freeze({ kind: 'all' as const }),
  typeFilter: 'usable',
  favoritesOnly: false,
  sort: 'recentlyAdded',
  search: '',
  offset: 0,
  limit: DEFAULT_LIBRARY_PAGE_SIZE,
});

export type LibraryQueryAction =
  | { readonly type: 'setSearch'; readonly search: string }
  | { readonly type: 'setSourceFilter'; readonly sourceFilter: SourceFilter }
  | { readonly type: 'setTypeFilter'; readonly typeFilter: LibraryTypeFilter }
  | { readonly type: 'setFavoritesOnly'; readonly favoritesOnly: boolean }
  | { readonly type: 'setSort'; readonly sort: LibrarySort }
  | { readonly type: 'setLimit'; readonly limit: number }
  | { readonly type: 'nextPage' }
  | { readonly type: 'resetPaging' };

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function normalizedOffset(value: unknown): number {
  return typeof value === 'number' && Number.isSafeInteger(value) && value >= 0 ? value : 0;
}

function normalizedLimit(value: unknown): number {
  return typeof value === 'number' && Number.isSafeInteger(value) && value > 0
    ? value
    : DEFAULT_LIBRARY_PAGE_SIZE;
}

export function normalizeLibraryQueryState(value: unknown): LibraryQueryState {
  const record = isRecord(value) ? value : {};
  const filters = normalizeShellPreferences(record);
  return {
    sourceFilter: filters.sourceFilter,
    typeFilter: filters.typeFilter,
    favoritesOnly: filters.favoritesOnly,
    sort: filters.sort,
    search: typeof record.search === 'string' ? record.search : '',
    offset: normalizedOffset(record.offset),
    limit: normalizedLimit(record.limit),
  };
}

/** Start a fresh query session from remembered filters, never transient fields. */
export function createLibraryQueryState(
  preferences: unknown = DEFAULT_SHELL_PREFERENCES,
): LibraryQueryState {
  const remembered: ShellPreferences = normalizeShellPreferences(preferences);
  return {
    sourceFilter: remembered.sourceFilter,
    typeFilter: remembered.typeFilter,
    favoritesOnly: remembered.favoritesOnly,
    sort: remembered.sort,
    search: '',
    offset: 0,
    limit: DEFAULT_LIBRARY_PAGE_SIZE,
  };
}

function sameSourceFilter(left: SourceFilter, right: SourceFilter): boolean {
  if (left.kind !== right.kind) return false;
  if (left.kind === 'all' || right.kind === 'all') return true;
  return left.sourceId === right.sourceId;
}

function resetWhenChanged<T extends keyof LibraryQueryState>(
  state: LibraryQueryState,
  key: T,
  value: LibraryQueryState[T],
): LibraryQueryState {
  return Object.is(state[key], value) ? state : { ...state, [key]: value, offset: 0 };
}

export function libraryQueryReducer(
  input: Readonly<LibraryQueryState>,
  action: LibraryQueryAction,
): LibraryQueryState {
  const state = normalizeLibraryQueryState(input);
  switch (action.type) {
    case 'setSearch':
      return resetWhenChanged(state, 'search', action.search);
    case 'setSourceFilter': {
      const sourceFilter = normalizeShellPreferences({ sourceFilter: action.sourceFilter }).sourceFilter;
      return sameSourceFilter(state.sourceFilter, sourceFilter)
        ? state
        : { ...state, sourceFilter, offset: 0 };
    }
    case 'setTypeFilter': {
      const typeFilter = normalizeShellPreferences({ typeFilter: action.typeFilter }).typeFilter;
      return resetWhenChanged(state, 'typeFilter', typeFilter);
    }
    case 'setFavoritesOnly':
      return resetWhenChanged(
        state,
        'favoritesOnly',
        typeof action.favoritesOnly === 'boolean' ? action.favoritesOnly : false,
      );
    case 'setSort': {
      const sort = normalizeShellPreferences({ sort: action.sort }).sort;
      return resetWhenChanged(state, 'sort', sort);
    }
    case 'setLimit': {
      const limit = normalizedLimit(action.limit);
      return resetWhenChanged(state, 'limit', limit);
    }
    case 'nextPage': {
      const offset = state.offset + state.limit;
      return Number.isSafeInteger(offset) ? { ...state, offset } : state;
    }
    case 'resetPaging':
      return state.offset === 0 ? state : { ...state, offset: 0 };
  }
}
