import { useMemo } from 'react';
import { DISPLAY_APPLY_DISABLED_REASON, type LibraryViewModel } from '../components/libraryViewModel.ts';
import type { useLibraryBrowser } from './useLibraryBrowser.ts';
import type { RuntimeWallpaperCoordinatorResult } from './useRuntimeWallpaperCoordinator.ts';

type LibraryActions = Pick<LibraryViewModel,
  'isEntryApplicable' | 'onSelect' | 'onApply' | 'onToggleFavorite' | 'onDetails' | 'buildContextActions'>;
interface Options {
  browser: ReturnType<typeof useLibraryBrowser>;
  runtimeWallpaper: RuntimeWallpaperCoordinatorResult;
  selectedPath: string | null;
  favoritePendingPaths: ReadonlySet<string>;
  active: boolean;
  scanRunning: boolean;
  resetKey: string;
  displayCanApply: boolean;
  actions: LibraryActions;
}

/** Keep the Library snapshot stable when unrelated shell controls change. */
export function useLibraryViewModel({
  browser, runtimeWallpaper, selectedPath, favoritePendingPaths, active,
  scanRunning, resetKey, displayCanApply, actions,
}: Options): LibraryViewModel {
  const { observationReady, path: currentPath } = runtimeWallpaper.current;
  const { applying, activePath, pendingPath } = runtimeWallpaper.apply;
  const {
    isEntryApplicable: isLibraryEntryApplicable, onSelect: selectLibraryEntry,
    onApply: applyEntry, onToggleFavorite: toggleFavorite,
    onDetails: openLibraryDetails, buildContextActions,
  } = actions;
  return useMemo<LibraryViewModel>(() => ({
    entries: browser.entries,
    selectedPath: selectedPath,
    currentPath,
    currentObservationReady: observationReady,
    applying: applying,
    activePath: activePath,
    pendingPath: pendingPath,
    favoritePendingPaths,
    active: active,
    refreshing: browser.refreshing || scanRunning,
    resetKey,
    replaceCount: browser.replaceCount,
    queryReplacementPending: browser.criteriaReplacementPending,
    totalKnown: browser.totalKnown,
    total: browser.total,
    canAppend: browser.canAppend,
    canAutoAppend: browser.canAutoAppend,
    loadingMore: browser.appending,
    appendNeedsRetry: browser.canAppend && !browser.canAutoAppend,
    loadErrorDetail: browser.loadErrorDetail,
    canApplyToDisplay: displayCanApply && !browser.criteriaReplacementPending,
    displayApplyDisabledReason: browser.criteriaReplacementPending
      ? 'Library results are updating.'
      : displayCanApply
        ? null
        : DISPLAY_APPLY_DISABLED_REASON,
    isEntryApplicable: isLibraryEntryApplicable,
    onSelect: selectLibraryEntry,
    onApply: applyEntry,
    onToggleFavorite: toggleFavorite,
    onDetails: openLibraryDetails,
    buildContextActions,
    onRequestMoreIfNeeded: browser.requestMoreIfNeeded,
    onAppendMore: browser.appendMore,
  }), [
    applyEntry,
    browser.appending,
    browser.appendMore,
    browser.canAppend,
    browser.canAutoAppend,
    browser.entries,
    browser.loadErrorDetail,
    browser.criteriaReplacementPending,
    browser.refreshing,
    browser.replaceCount,
    browser.requestMoreIfNeeded,
    browser.total,
    browser.totalKnown,
    buildContextActions,
    currentPath,
    active,
    displayCanApply,
    favoritePendingPaths,
    isLibraryEntryApplicable,
    openLibraryDetails,
    resetKey,
    activePath,
    applying,
    pendingPath,
    observationReady,
    scanRunning,
    selectLibraryEntry,
    selectedPath,
    toggleFavorite,
  ]);
}
