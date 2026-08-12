import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import {
  ClockAlert,
  FolderPlus,
  Heart,
  LoaderCircle,
  ScanSearch,
  Search,
  SearchCheck,
  SearchX,
  Settings,
  SlidersHorizontal,
  Shuffle,
  TriangleAlert,
} from 'lucide-react';
import { Popover } from 'radix-ui';

import { api } from '../api/bridge.ts';
import type {
  CommandResult,
  LibraryBrowserItemDTO,
} from '../api/types.ts';
import { commandErrorFeedback, commandResultMessage } from '../api/feedback.ts';
import LibraryViewport from '../components/LibraryViewport.tsx';
import LibraryState from '../components/LibraryState.tsx';
import LibraryViewSwitch from '../components/LibraryViewSwitch.tsx';
import OverflowStrip from '../components/OverflowStrip.tsx';
import {
  DISPLAY_APPLY_DISABLED_REASON,
  resolveLibraryModeSwitchAnchor,
  userUnsupportedContextAction,
  type ContextAction,
  type LibraryViewModel,
} from '../components/libraryViewModel.ts';
import SelectField from '../components/SelectField.tsx';
import { primaryApplyKind } from '../domain/applyActions.ts';
import { useFeedbackBridge } from '../hooks/useFeedbackBridge.ts';
import {
  useThumbnailFailureCount,
  useThumbnailStore,
} from '../state/ThumbnailStoreContext.tsx';
import { displayName } from '../components/wallpaperCardHelpers.ts';
import { buildDisplayTargetModel } from './displayTargets.ts';
import DisplayTargetSelector from './DisplayTargetSelector.tsx';
import { FeedbackOverlay } from './FeedbackOverlay.tsx';
import FirstRunSuggestions from './FirstRunSuggestions.tsx';
import LibraryRepairPrompt from './LibraryRepairPrompt.tsx';
import WallpaperDetailsDialog from './AuthorizedWallpaperDetailsDialog.tsx';
import { ScanActivity } from './ScanActivity.tsx';
import { SourcePanel, type SourcePanelNotice } from './SourcePanel.tsx';
import CompactSettingsPanel from './CompactSettingsPanel.tsx';
import {
  canChooseRandomWallpaper,
  currentWallpaperLabel,
  effectiveSourceFilter,
  reconcileSelectedEntryByStableId,
  reconcileSourceFilter,
  targetArgument,
} from './singlePageShellModel.ts';

import type {
  LibrarySort,
  LibraryTypeFilter,
  SourceFilter,
} from './shellPreferences.ts';
import { useLibraryBrowser } from './useLibraryBrowser.ts';
import { useScanProgress } from './useScanProgress.ts';
import { useShellCatalog } from './useShellCatalog.ts';
import { useShellFeedback } from './useShellFeedback.ts';
import { useShellPreferences } from './useShellPreferences.ts';
import { useShellTheme } from './useShellTheme.ts';
import { useWallpaperBehaviorSettings } from './useWallpaperBehaviorSettings.ts';
import { useRendererStatuses } from './useRendererStatuses.ts';
import { addSuggestedDirectory } from './firstRunSourceActions.ts';
import { useLibraryLifecycle } from './useLibraryLifecycle.ts';
import { useRuntimeWallpaperCoordinator } from './useRuntimeWallpaperCoordinator.ts';
import { createRecurringErrorGate } from './recurringErrorGate.ts';

function commandDetails(result: CommandResult): string {
  return [
    result.error?.message,
    result.error?.suggestion,
    result.error?.detail,
    result.stderr,
    result.stdout,
  ].filter((part): part is string => Boolean(part?.trim())).join('\n');
}

function sourceFilterValue(filter: SourceFilter): string {
  return filter.kind === 'source' ? `source:${filter.sourceId}` : 'all';
}

function sourceFilterFromValue(value: string): SourceFilter {
  if (!value.startsWith('source:')) return { kind: 'all' };
  const sourceId = Number(value.slice('source:'.length));
  return Number.isSafeInteger(sourceId) && sourceId > 0
    ? { kind: 'source', sourceId }
    : { kind: 'all' };
}

function selectedDescription(entry: LibraryBrowserItemDTO | null): string {
  if (!entry) return 'Select a wallpaper to see its details.';
  const sources = entry.sources.map((source) => source.displayName).join(', ');
  return `Selected: ${displayName(entry)}${sources ? ` · ${sources}` : ''}`;
}

export default function SinglePageShell() {
  const [search, setSearch] = useState('');
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [filtersOpen, setFiltersOpen] = useState(false);
  const [sourcesOpen, setSourcesOpen] = useState(false);
  const [sourcesMounted, setSourcesMounted] = useState(false);
  const [sourcesReturnToSettings, setSourcesReturnToSettings] = useState(false);
  const [restoreSourceCardFocus, setRestoreSourceCardFocus] = useState(false);
  const [selectedEntry, setSelectedEntry] = useState<LibraryBrowserItemDTO | null>(null);
  const [detailsEntry, setDetailsEntry] = useState<LibraryBrowserItemDTO | null>(null);
  const [favoritePendingPaths, setFavoritePendingPaths] = useState<ReadonlySet<string>>(
    () => new Set(),
  );
  const favoritePendingPathsRef = useRef(new Set<string>());
  const overlayReturnFocusRef = useRef<HTMLElement | null>(null);
  const sourcePanelReturnFocusRef = useRef<HTMLButtonElement | null>(null);
  const detailsReturnFocusRef = useRef<HTMLElement | null>(null);
  const libraryViewportAnchorRef = useRef<number | null>(null);
  const [libraryViewportAnchorId, setLibraryViewportAnchorId] = useState<number | null>(null);
  const [libraryModeAnchorId, setLibraryModeAnchorId] = useState<number | null>(null);
  const [libraryViewFocusToken, setLibraryViewFocusToken] = useState(0);
  const {
    refreshSubscribed: refreshThumbnails,
    retryFailures: retryThumbnailFailures,
  } = useThumbnailStore();
  const thumbnailFailureCount = useThumbnailFailureCount();

  const rememberOverlayTrigger = useCallback((trigger: HTMLElement) => {
    overlayReturnFocusRef.current = trigger;
  }, []);
  const restoreOverlayFocus = useCallback(() => {
    const trigger = overlayReturnFocusRef.current;
    overlayReturnFocusRef.current = null;
    if (!trigger) return;
    window.requestAnimationFrame(() => trigger.focus());
  }, []);
  const closeDetails = useCallback(() => {
    setDetailsEntry(null);
    const trigger = detailsReturnFocusRef.current;
    detailsReturnFocusRef.current = null;
    if (!trigger?.isConnected) return;
    window.requestAnimationFrame(() => trigger.focus());
  }, []);
  const openLibraryDetails = useCallback((
    entry: LibraryBrowserItemDTO,
    returnFocus: HTMLElement | null = null,
  ) => {
    detailsReturnFocusRef.current = returnFocus;
    setDetailsEntry(entry);
  }, []);
  const openSources = useCallback((returnToSettings = false) => {
    setRestoreSourceCardFocus(false);
    setSourcesReturnToSettings(returnToSettings);
    setSourcesMounted(true);
    setSourcesOpen(true);
  }, []);

  useEffect(() => {
    if (!restoreSourceCardFocus || sourcesOpen || !settingsOpen) return;
    sourcePanelReturnFocusRef.current?.focus();
    setRestoreSourceCardFocus(false);
  }, [restoreSourceCardFocus, settingsOpen, sourcesOpen]);

  const {
    preferences,
    ready: preferencesReady,
    loadError: preferencesLoadError,
    saveError: preferencesSaveError,
    updatePreferences,
  } = useShellPreferences(api);
  useShellTheme();

  const behavior = useWallpaperBehaviorSettings(api);
  const rendererStatuses = useRendererStatuses(api, settingsOpen);
  const catalog = useShellCatalog(api);
  const scan = useScanProgress(api);
  const {
    state: feedbackState,
    nowMs: feedbackNowMs,
    technicalDetails,
    runningStatus,
    showNotice,
    setCommandFeedback,
    dispatchFeedback,
  } = useShellFeedback();

  const setSystemFeedback = useCallback(
    (feedback: Parameters<typeof setCommandFeedback>[0]) => setCommandFeedback(feedback, 'system'),
    [setCommandFeedback],
  );
  const setApplyFeedback = useCallback(
    (feedback: Parameters<typeof setCommandFeedback>[0]) => setCommandFeedback(feedback, 'apply'),
    [setCommandFeedback],
  );
  useFeedbackBridge(setSystemFeedback);

  // ── effective source filter ──────────────────────────────────────────
  // When the source catalog has an error, the effective filter is forced to
  // 'all' so the Library can still render. The persisted preference is NOT
  // overwritten — only the runtime value passed to the browser changes.
  const effectiveSrcFilter = effectiveSourceFilter(
    preferences.sourceFilter,
    catalog.errors.sources,
  );

  const browser = useLibraryBrowser({
    sourceFilter: effectiveSrcFilter,
    typeFilter: preferences.typeFilter,
    favoritesOnly: preferences.favoritesOnly,
    sort: preferences.sort,
    search,
  });
  const scanRunning = scan.progress?.running === true || scan.scanState.kind === 'running';
  const libraryLifecycle = useLibraryLifecycle({
    api,
    browser: {
      initialLoading: browser.initialLoading,
      entriesCount: browser.entries.length,
      emptyConfirmed: browser.emptyConfirmed,
      loadError: browser.loadError,
      replaceCount: browser.replaceCount,
      debouncedSearch: browser.debouncedSearch,
      reload: browser.reload,
    },
    catalog: {
      sources: catalog.sources,
      sourcesReady: catalog.sourcesReady,
      sourceError: catalog.errors.sources,
      reloadSources: catalog.reloadSources,
    },
    sourceFilter: effectiveSrcFilter,
    typeFilter: preferences.typeFilter,
    favoritesOnly: preferences.favoritesOnly,
    scan: {
      blocksFirstRun: scanRunning,
      backendReportedRunning: scan.progress?.running === true,
    },
    refreshThumbnails,
    showNotice,
    setSystemFeedback,
  });
  const firstRunEligible = libraryLifecycle.firstRun.eligible;
  const repairLibrary = libraryLifecycle.repair.run;
  const reconcileSourcesAndLibrary = libraryLifecycle.reconcileSourcesAndLibrary;

  const runtimeWallpaper = useRuntimeWallpaperCoordinator({
    api,
    catalog: {
      ready: catalog.ready,
      connectedOutputs: catalog.connectedOutputs,
      reloadDisplays: catalog.reloadDisplays,
    },
    displayTarget: preferences.displayTarget,
    reloadLibrary: libraryLifecycle.reloadLibrary,
    setApplyFeedback,
  });
  const currentWallpaper = runtimeWallpaper.current.wallpaper;
  const currentPath = runtimeWallpaper.current.path;
  const applyActionToDisplay = runtimeWallpaper.apply.applyActionToDisplay;
  const applyToDisplay = runtimeWallpaper.apply.applyToDisplay;
  const detectedDisplayModel = buildDisplayTargetModel(
    catalog.connectedOutputs,
    preferences.displayTarget,
  );
  const displayModel = catalog.errors.displays
    ? { ...detectedDisplayModel, canApply: false }
    : detectedDisplayModel;

  const applyEntry = useCallback((entry: LibraryBrowserItemDTO) => {
    if (browser.criteriaReplacementPending) {
      showNotice({
        channel: 'apply',
        severity: 'info',
        message: 'Library results are updating. Try again when the new results appear.',
      });
      return;
    }
    if (!displayModel.canApply) {
      showNotice({
        channel: 'apply',
        severity: 'error',
        message: DISPLAY_APPLY_DISABLED_REASON,
      });
      return;
    }
    const kind = primaryApplyKind(entry);
    if (kind === null) {
      showNotice({
        channel: 'apply',
        severity: 'warning',
        message: 'This wallpaper cannot be applied.',
        technicalDetails: entry.applyReason || entry.unsupportedReason,
      });
      return;
    }
    const target = targetArgument(preferences.displayTarget);
    if (kind === 'retry_backend_apply') {
      applyActionToDisplay({ kind, path: entry.path }, target);
      return;
    }
    applyToDisplay(entry.path, target);
  }, [
    applyActionToDisplay,
    applyToDisplay,
    browser.criteriaReplacementPending,
    displayModel.canApply,
    preferences.displayTarget,
    showNotice,
  ]);

  const selectLibraryEntry = useCallback((entry: LibraryBrowserItemDTO) => {
    setSelectedEntry(entry);
  }, []);
  const isLibraryEntryApplicable = useCallback(
    (entry: LibraryBrowserItemDTO) => primaryApplyKind(entry) !== null,
    [],
  );

  useEffect(() => {
    if (!catalog.sourcesReady || catalog.errors.sources) return;
    const sourceFilter = reconcileSourceFilter(preferences.sourceFilter, catalog.sources);
    if (
      sourceFilter.kind !== preferences.sourceFilter.kind
      || (
        sourceFilter.kind === 'source'
        && preferences.sourceFilter.kind === 'source'
        && sourceFilter.sourceId !== preferences.sourceFilter.sourceId
      )
    ) {
      updatePreferences((current) => ({ ...current, sourceFilter }));
    }
  }, [catalog.errors.sources, catalog.sources, catalog.sourcesReady, preferences.sourceFilter, updatePreferences]);

  useEffect(() => {
    setSelectedEntry((current) => reconcileSelectedEntryByStableId(current, browser.entries));
  }, [browser.entries, browser.replaceCount]);

  const selectedExistenceRequest = useRef(0);
  useEffect(() => {
    const selected = selectedEntry;
    const requestId = ++selectedExistenceRequest.current;
    if (!selected || browser.entries.some((entry) => entry.wallpaperId === selected.wallpaperId)) {
      return undefined;
    }
    void api.libraryWallpaperExists(selected.wallpaperId).then(
      (exists) => {
        if (exists || selectedExistenceRequest.current !== requestId) return;
        setSelectedEntry((current) => (
          current?.wallpaperId === selected.wallpaperId ? null : current
        ));
        setDetailsEntry((current) => (
          current?.wallpaperId === selected.wallpaperId ? null : current
        ));
        showNotice({
          channel: 'system',
          severity: 'info',
          message: 'The selected wallpaper is no longer in Library.',
        });
      },
      () => {
        // An existence probe is advisory. Preserve selection on transport failures.
      },
    );
    return () => {
      if (selectedExistenceRequest.current === requestId) {
        selectedExistenceRequest.current += 1;
      }
    };
  }, [browser.entries, browser.replaceCount, selectedEntry, showNotice]);

  const scanErrorGate = useRef(createRecurringErrorGate()).current;
  useEffect(() => {
    const error = scan.scanError ?? scan.transportError;
    if (error === null) {
      scanErrorGate.shouldNotify(null);
      return;
    }
    if (!error || !scanErrorGate.shouldNotify(error)) return;
    showNotice({
      channel: 'scan',
      severity: scan.scanError ? 'error' : 'warning',
      message: scan.scanError ? 'Wallpaper scan failed.' : 'Scan status is temporarily unavailable.',
      technicalDetails: error,
    });
  }, [scan.scanError, scan.transportError, scanErrorGate, showNotice]);

  useEffect(() => {
    const error = preferencesSaveError ?? preferencesLoadError;
    if (!error) return;
    showNotice({
      channel: 'settings',
      severity: 'warning',
      message: preferencesSaveError
        ? 'Some interface preferences could not be saved.'
        : 'Saved interface preferences could not be loaded; defaults are in use.',
      technicalDetails: error.message,
    });
  }, [preferencesLoadError, preferencesSaveError, showNotice]);

  const handleSourceNotice = useCallback((notice: SourcePanelNotice) => {
    showNotice(notice);
  }, [showNotice]);

  useEffect(() => {
    if (thumbnailFailureCount === 0) return;
    showNotice({
      channel: 'system',
      severity: 'warning',
      message: `${thumbnailFailureCount} preview${thumbnailFailureCount === 1 ? '' : 's'} could not be generated.`,
      action: {
        label: 'Retry',
        invoke: retryThumbnailFailures,
      },
    });
  }, [retryThumbnailFailures, showNotice, thumbnailFailureCount]);

  const addFirstRunDirectory = useCallback(async (path: string): Promise<void> => {
    try {
      const result = await addSuggestedDirectory(
        api,
        path,
        reconcileSourcesAndLibrary,
        scan.onScanStarted,
        scan.onScanFinished,
      );
      if (result.success) {
        showNotice({ channel: 'settings', severity: 'success', message: 'Folder added.' });
      } else {
        setCommandFeedback(commandErrorFeedback('Add folder', result), 'system');
      }
    } catch (error) {
      setCommandFeedback(commandErrorFeedback('Add folder', error), 'system');
    }
  }, [reconcileSourcesAndLibrary, scan.onScanFinished, scan.onScanStarted, setCommandFeedback, showNotice]);

  const toggleFavorite = useCallback(async (entry: LibraryBrowserItemDTO) => {
    if (favoritePendingPathsRef.current.has(entry.path)) return;
    favoritePendingPathsRef.current.add(entry.path);
    setFavoritePendingPaths((current) => new Set(current).add(entry.path));
    const label = entry.favorite ? 'Remove favorite' : 'Add favorite';
    try {
      const result = entry.favorite
        ? await api.favoriteRemove(entry.path)
        : await api.favoriteAdd(entry.path);
      if (!result.success) {
        setCommandFeedback(commandErrorFeedback(label, result), 'system');
        return;
      }
      showNotice({
        channel: 'system',
        severity: 'success',
        message: entry.favorite ? 'Removed from favorites.' : 'Added to favorites.',
      });
      await libraryLifecycle.reloadLibrary();
    } catch (error) {
      setCommandFeedback(commandErrorFeedback(label, error), 'system');
    } finally {
      favoritePendingPathsRef.current.delete(entry.path);
      setFavoritePendingPaths((current) => {
        const next = new Set(current);
        next.delete(entry.path);
        return next;
      });
    }
  }, [libraryLifecycle.reloadLibrary, setCommandFeedback, showNotice]);

  const openLocation = useCallback(async (entry: LibraryBrowserItemDTO) => {
    try {
      const result = await api.openProjectLocation(entry.path);
      if (!result.success) setCommandFeedback(commandErrorFeedback('Open location', result), 'system');
    } catch (error) {
      setCommandFeedback(commandErrorFeedback('Open location', error), 'system');
    }
  }, [setCommandFeedback]);

  const restoreUserUnsupported = useCallback(async (entry: LibraryBrowserItemDTO) => {
    try {
      const result = await api.userUnsupportedRemove(entry.wallpaperId);
      if (!result.success) {
        setCommandFeedback(commandErrorFeedback('Restore to Library', result), 'system');
        return;
      }
      await libraryLifecycle.reloadLibrary();
      showNotice({
        channel: 'system',
        severity: 'success',
        message: 'Restored to the Library.',
      });
    } catch (error) {
      setCommandFeedback(commandErrorFeedback('Restore to Library', error), 'system');
    }
  }, [libraryLifecycle.reloadLibrary, setCommandFeedback, showNotice]);

  const moveToUserUnsupported = useCallback(async (entry: LibraryBrowserItemDTO) => {
    try {
      const result = await api.userUnsupportedAdd(entry.wallpaperId);
      if (!result.success) {
        setCommandFeedback(commandErrorFeedback('Move to Unsupported', result), 'system');
        return;
      }
      await libraryLifecycle.reloadLibrary();
      showNotice({
        channel: 'system',
        severity: 'success',
        message: 'Moved to Unsupported. It will be excluded from Library choices.',
        action: {
          label: 'Undo',
          invoke: () => void restoreUserUnsupported(entry),
        },
      });
    } catch (error) {
      setCommandFeedback(commandErrorFeedback('Move to Unsupported', error), 'system');
    }
  }, [libraryLifecycle.reloadLibrary, restoreUserUnsupported, setCommandFeedback, showNotice]);

  const buildContextActions = useCallback((entry: LibraryBrowserItemDTO): ContextAction[] => {
    const actions: ContextAction[] = [
      {
        label: entry.favorite ? 'Remove from Favorites' : 'Add to Favorites',
        action: () => void toggleFavorite(entry),
      },
    ];
    const unsupportedAction = userUnsupportedContextAction(entry);
    if (unsupportedAction === 'move') {
      actions.push({
        label: 'Move to Unsupported',
        action: () => void moveToUserUnsupported(entry),
        danger: true,
      });
    } else if (unsupportedAction === 'restore') {
      actions.push({
        label: 'Restore to Library',
        action: () => void restoreUserUnsupported(entry),
      });
    }
    actions.push(
      {
        label: 'Open Location',
        action: () => void openLocation(entry),
      },
      {
        label: 'Information',
        action: (_path, returnFocus) => {
          if (preferences.libraryViewMode === 'grid') setSelectedEntry(entry);
          openLibraryDetails(entry, returnFocus);
        },
      },
    );
    const limitation = entry.applyReason || entry.unsupportedReason;
    if (limitation) {
      actions.push({
        label: 'Limitation Details',
        action: () => showNotice({
          channel: 'system',
          severity: 'warning',
          message: 'This wallpaper has a renderer limitation.',
          technicalDetails: limitation,
        }),
      });
    }
    return actions;
  }, [
    moveToUserUnsupported,
    openLibraryDetails,
    openLocation,
    preferences.libraryViewMode,
    restoreUserUnsupported,
    showNotice,
    toggleFavorite,
  ]);

  const chooseRandom = useCallback(async () => {
    const outcome = await browser.chooseRandom();
    if (outcome.kind === 'empty') {
      showNotice({
        channel: 'system',
        severity: 'info',
        message: 'No wallpaper matches the active filters.',
      });
      return;
    }
    if (outcome.kind === 'error') {
      showNotice({
        channel: 'system',
        severity: 'error',
        message: 'Could not choose a random wallpaper.',
        technicalDetails: outcome.message,
      });
      return;
    }
    if (outcome.kind === 'stale') return;
    setSelectedEntry(outcome.entry);
    applyEntry(outcome.entry);
  }, [applyEntry, browser.chooseRandom, showNotice]);

  const scanWallpaperEngine = useCallback(async () => {
    scan.onScanStarted();
    try {
      const result = await api.scanSteamWorkshop();
      if (result.success) {
        showNotice({
          channel: 'scan',
          severity: 'success',
          message: commandResultMessage(result, 'Wallpaper Engine scan finished.'),
        });
      } else {
        showNotice({
          channel: 'scan',
          severity: 'error',
          message: 'Wallpaper Engine scan failed.',
          technicalDetails: commandDetails(result),
        });
      }
    } catch (error) {
      showNotice({
        channel: 'scan',
        severity: 'error',
        message: 'Wallpaper Engine scan failed.',
        technicalDetails: error instanceof Error ? error.message : String(error),
      });
    } finally {
      await reconcileSourcesAndLibrary();
      scan.onScanFinished();
    }
  }, [reconcileSourcesAndLibrary, scan, showNotice]);

  const resetKey = [
    sourceFilterValue(effectiveSrcFilter),
    preferences.typeFilter,
    preferences.favoritesOnly ? 'favorites' : 'all',
    preferences.sort,
    browser.debouncedSearch,
  ].join('|');

  const rememberLibraryAnchor = useCallback((wallpaperId: number) => {
    libraryViewportAnchorRef.current = wallpaperId;
    setLibraryViewportAnchorId((current) => current === wallpaperId ? current : wallpaperId);
  }, []);
  const changeLibraryViewMode = useCallback((mode: typeof preferences.libraryViewMode) => {
    if (mode === preferences.libraryViewMode) return;
    const anchor = resolveLibraryModeSwitchAnchor(
      browser.entries,
      selectedEntry?.wallpaperId,
      libraryViewportAnchorRef.current,
    );
    setLibraryModeAnchorId(anchor?.wallpaperId ?? null);
    setLibraryViewFocusToken((token) => token + 1);
    updatePreferences((current) => ({ ...current, libraryViewMode: mode }));
  }, [browser.entries, preferences.libraryViewMode, selectedEntry, updatePreferences]);

  const libraryViewModel = useMemo<LibraryViewModel>(() => ({
    entries: browser.entries,
    selectedPath: selectedEntry?.path ?? null,
    currentPath,
    currentObservationReady: runtimeWallpaper.current.observationReady,
    applying: runtimeWallpaper.apply.applying,
    activePath: runtimeWallpaper.apply.activePath,
    pendingPath: runtimeWallpaper.apply.pendingPath,
    favoritePendingPaths,
    active: !settingsOpen && !sourcesOpen && detailsEntry === null,
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
    canApplyToDisplay: displayModel.canApply && !browser.criteriaReplacementPending,
    displayApplyDisabledReason: browser.criteriaReplacementPending
      ? 'Library results are updating.'
      : displayModel.canApply
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
    detailsEntry,
    displayModel.canApply,
    favoritePendingPaths,
    isLibraryEntryApplicable,
    openLibraryDetails,
    resetKey,
    runtimeWallpaper.apply.activePath,
    runtimeWallpaper.apply.applying,
    runtimeWallpaper.apply.pendingPath,
    runtimeWallpaper.current.observationReady,
    scanRunning,
    selectLibraryEntry,
    selectedEntry?.path,
    settingsOpen,
    sourcesOpen,
    toggleFavorite,
  ]);
  const flowAnchorEntry = useMemo(
    () => browser.entries.find((entry) => entry.wallpaperId === libraryViewportAnchorId) ?? null,
    [browser.entries, libraryViewportAnchorId],
  );

  const renderLibrary = () => {
    // Library loads independently of preferences, catalog, and display probes.
    // A failure in any of those services only disables the relevant controls.
    if (libraryLifecycle.startup.timedOut
      && browser.entries.length === 0
      && !browser.emptyConfirmed
      && !browser.loadError) {
      return (
        <LibraryState
          action={(
            <button
              className="btn"
              type="button"
              onClick={libraryLifecycle.startup.retry}
            >
              Retry
            </button>
          )}
          description="Wallpaper data has not arrived yet. Retry the library connection."
          icon={<ClockAlert size={28} />}
          role="alert"
          title="Library is taking longer than expected"
        />
      );
    }
    if (browser.initialLoading) {
      return (
        <LibraryState
          description="Preparing your saved wallpapers."
          icon={<LoaderCircle className="library-state__spinner" size={28} />}
          role="status"
          title="Loading wallpaper library"
        />
      );
    }
    if (firstRunEligible) {
      return (
        <LibraryState
          action={(
            <button
              className="btn primary"
              type="button"
              onClick={(event) => {
                rememberOverlayTrigger(event.currentTarget);
                openSources();
              }}
            >
              <FolderPlus size={16} aria-hidden="true" /> Add Folder
            </button>
          )}
          className="single-page-first-run"
          description="Add any number of folders. Nothing is scanned until you choose it."
          icon={<FolderPlus size={30} />}
          title="Choose where your wallpapers live"
        >
          <FirstRunSuggestions
            suggestions={libraryLifecycle.firstRun.suggestions}
            onAddDirectory={(path) => void addFirstRunDirectory(path)}
            onScanWallpaperEngine={() => void scanWallpaperEngine()}
          />
          {libraryLifecycle.firstRun.error ? (
            <div className="single-page-first-run__suggestion-error" role="status">
              <span>Optional source suggestions are unavailable.</span>
              <button
                className="btn"
                type="button"
                onClick={libraryLifecycle.firstRun.retrySuggestions}
              >
                Retry suggestions
              </button>
            </div>
          ) : null}
        </LibraryState>
      );
    }
    if (browser.entries.length > 0) {
      return (
        <>
          {browser.loadError ? (
            <div className="single-page-stale-results" role="alert">
              <span>
                Results could not be refreshed. Showing the previous library view.
                {browser.loadErrorDetail ? ` ${browser.loadErrorDetail}` : ''}
              </span>
              <button className="btn" type="button" onClick={() => void browser.reload()}>
                Retry
              </button>
            </div>
          ) : null}
          <LibraryViewport
            applyGesture={preferences.applyGesture}
            cardSize={preferences.cardSize}
            focusToken={libraryViewFocusToken}
            initialAnchorWallpaperId={libraryViewportAnchorId ?? libraryModeAnchorId}
            mode={preferences.libraryViewMode}
            model={libraryViewModel}
            onAnchorChange={rememberLibraryAnchor}
          />
          {!browser.refreshing
            && browser.canAppend
            && (preferences.libraryViewMode === 'grid' || !browser.canAutoAppend) ? (
            <div className="single-page-load-more">
              <button
                className="btn"
                disabled={browser.appending}
                type="button"
                onClick={() => void browser.appendMore()}
                title={!browser.canAutoAppend && browser.loadErrorDetail
                  ? browser.loadErrorDetail
                  : undefined}
              >
                {browser.appending
                  ? 'Loading more…'
                  : !browser.canAutoAppend
                    ? 'Retry loading more'
                    : browser.totalKnown
                      ? `Load more · ${Math.max(0, browser.total - browser.entries.length)} remaining`
                      : 'Load more'}
              </button>
            </div>
          ) : null}
        </>
      );
    }
    if (scanRunning) {
      return (
        <LibraryState
          description="New wallpapers will appear as the scan finds them."
          icon={<ScanSearch className="library-state__spinner" size={28} />}
          role="status"
          title="Indexing wallpapers"
        />
      );
    }
    if (browser.loadError) {
      return (
        <LibraryState
          action={(
            <button className="btn" type="button" onClick={() => void browser.reload()}>
              Retry
            </button>
          )}
          description={browser.loadErrorDetail ?? 'The library could not be read.'}
          icon={<TriangleAlert size={28} />}
          role="alert"
          title="Could not load the wallpaper library"
        />
      );
    }
    if (!browser.emptyConfirmed) {
      return (
        <LibraryState
          description="Confirming whether wallpapers match the current view."
          icon={<SearchCheck className="library-state__spinner" size={28} />}
          role="status"
          title="Checking the library"
        />
      );
    }
    return (
      <LibraryState
        action={(
          <button
            className="btn"
            type="button"
            onClick={() => {
              setSearch('');
              updatePreferences((current) => ({
                ...current,
                sourceFilter: { kind: 'all' },
                typeFilter: 'usable',
                favoritesOnly: false,
              }));
            }}
          >
            Clear filters
          </button>
        )}
        description="Try clearing the active filters or changing your search."
        icon={<SearchX size={28} />}
        title="No matching wallpapers"
      />
    );
  };

  const renderFilterControls = () => (
    <>
      {catalog.errors.sources ? (
        <details className="single-page-source-warning">
          <summary>Source list unavailable</summary>
          <p>{catalog.errors.sources}</p>
        </details>
      ) : null}
      <SelectField
        aria-label="Source filter"
        value={sourceFilterValue(effectiveSrcFilter)}
        disabled={Boolean(catalog.errors.sources)}
        options={[
          { value: 'all', label: 'ALL SOURCES' },
          ...catalog.sources.map((source) => ({
            value: `source:${source.id}`,
            label: `${source.displayName}${source.availability === 'offline' ? ' · Offline' : ''}`,
          })),
        ]}
        onValueChange={(value) => {
          const sourceFilter = sourceFilterFromValue(value);
          updatePreferences((current) => ({ ...current, sourceFilter }));
          setFiltersOpen(false);
        }}
        variant="compact"
      />
      <SelectField
        aria-label="Wallpaper type filter"
        value={preferences.typeFilter}
        options={[
          { value: 'usable', label: 'ALL' },
          { value: 'image', label: 'Images' },
          { value: 'gif', label: 'GIFs' },
          { value: 'video', label: 'Videos' },
          { value: 'weScene', label: 'Wallpaper Engine scenes' },
          { value: 'unsupported', label: 'Unsupported' },
        ]}
        onValueChange={(value) => {
          const typeFilter = value as LibraryTypeFilter;
          updatePreferences((current) => ({ ...current, typeFilter }));
          setFiltersOpen(false);
        }}
        variant="compact"
      />
      <label
        className="single-page-favorite-filter"
        data-active={preferences.favoritesOnly}
      >
        <input
          type="checkbox"
          checked={preferences.favoritesOnly}
          onChange={(event) => {
            const favoritesOnly = event.currentTarget.checked;
            updatePreferences((current) => ({ ...current, favoritesOnly }));
          }}
        />
        <Heart
          aria-hidden="true"
          fill={preferences.favoritesOnly ? 'currentColor' : 'none'}
          size={15}
        />
        <span>FAVORITES</span>
      </label>
      <SelectField
        aria-label="Library sort"
        value={preferences.sort}
        options={[
          { value: 'recentlyAdded', label: 'Recently added' },
          { value: 'nameAsc', label: 'Name A–Z' },
          { value: 'nameDesc', label: 'Name Z–A' },
        ]}
        onValueChange={(value) => {
          const sort = value as LibrarySort;
          updatePreferences((current) => ({ ...current, sort }));
          setFiltersOpen(false);
        }}
        variant="compact"
      />
      {preferences.libraryViewMode === 'grid' ? (
        <SelectField
          aria-label="Card size"
          value={preferences.cardSize}
          options={[
            { value: 'small', label: 'Small' },
            { value: 'medium', label: 'Medium' },
            { value: 'large', label: 'Large' },
          ]}
          onValueChange={(value) => {
            const cardSize = value as typeof preferences.cardSize;
            updatePreferences((current) => ({ ...current, cardSize }));
            setFiltersOpen(false);
          }}
          variant="compact"
        />
      ) : null}
    </>
  );
  const scanActivityVisible = scan.presentation.kind !== 'hidden';
  const feedbackVisible = feedbackState.notices.length > 0;
  const shellNotificationsVisible = scanActivityVisible || feedbackVisible;

  return (
    <div
      className={`single-page-shell library-view-${preferences.libraryViewMode}${
        settingsOpen ? ' settings-open' : ''
      }${shellNotificationsVisible ? ' has-notifications' : ''}${
        scanActivityVisible ? ' has-scan' : ''
      }${feedbackVisible ? ' has-feedback' : ''}`}
      onContextMenu={(event) => {
        const target = event.target;
        if (
          target instanceof Element
          && target.closest('input, textarea, [contenteditable="true"]')
        ) {
          return;
        }
        event.preventDefault();
      }}
    >
      <header className="single-page-topbar" data-tauri-drag-region="deep">
        <div className="single-page-brand" aria-label="Wallpaper Console">Wallpaper Console</div>
        <label className="single-page-search">
          <Search size={16} aria-hidden="true" />
          <input
            aria-label="Search wallpapers"
            type="search"
            placeholder="Search wallpapers"
            value={search}
            onChange={(event) => setSearch(event.currentTarget.value)}
          />
        </label>
        <DisplayTargetSelector
          connectedOutputs={catalog.connectedOutputs}
          value={preferences.displayTarget}
          onChange={(displayTarget) => updatePreferences((current) => ({
            ...current,
            displayTarget,
          }))}
          disabled={!catalog.ready || Boolean(catalog.errors.displays)}
        />
        <button
          aria-label="Apply a random wallpaper from active filters"
          className="single-page-icon-button"
          data-topbar-action="random"
          type="button"
          disabled={!canChooseRandomWallpaper({
            searchSettled: browser.searchSettled,
            randomPending: browser.randomPending,
            total: browser.total,
            canApply: displayModel.canApply && !browser.criteriaReplacementPending,
          })}
          onClick={() => void chooseRandom()}
          title="Apply a random wallpaper from active filters"
        >
          <Shuffle size={17} aria-hidden="true" />
        </button>
        <button
          aria-label="Open settings"
          className="single-page-icon-button"
          data-topbar-action="settings"
          type="button"
          onClick={(event) => {
            rememberOverlayTrigger(event.currentTarget);
            setSettingsOpen(true);
          }}
          title="Open settings"
        >
          <Settings size={18} aria-hidden="true" />
        </button>
      </header>

      <div className="single-page-library-controls">
        <OverflowStrip className="single-page-filters" aria-label="Library filters">
          {renderFilterControls()}
        </OverflowStrip>
        <Popover.Root open={filtersOpen} onOpenChange={setFiltersOpen}>
          <Popover.Trigger asChild>
            <button className="single-page-filter-popover__trigger" type="button">
              <SlidersHorizontal aria-hidden="true" size={15} />
              Filters
            </button>
          </Popover.Trigger>
          <Popover.Portal>
            <Popover.Content
              align="center"
              className="single-page-filter-popover"
              sideOffset={7}
            >
              <div className="single-page-filter-popover__controls">
                {renderFilterControls()}
              </div>
              <Popover.Arrow className="single-page-filter-popover__arrow" />
            </Popover.Content>
          </Popover.Portal>
        </Popover.Root>
        <LibraryViewSwitch
          disabled={!preferencesReady}
          onChange={changeLibraryViewMode}
          value={preferences.libraryViewMode}
        />
        {preferences.libraryViewMode === 'grid' ? (
          <span className="single-page-count" aria-live="polite">
            {browser.totalKnown
              ? `${browser.entries.length} / ${browser.total}`
              : `${browser.entries.length} loaded`}
          </span>
        ) : null}
      </div>

      <main className="single-page-library">
        {catalog.errors.displays || catalog.errors.displayState ? (
          <div className="single-page-discovery-error" role="alert">
            <span>
              Display detection failed.
              {' '}
              {[catalog.errors.displays, catalog.errors.displayState]
                .filter((detail): detail is string => Boolean(detail))
                .join(' ')}
            </span>
            <button className="btn" type="button" onClick={() => void catalog.reloadDisplays()}>
              Retry display detection
            </button>
          </div>
        ) : null}
        <LibraryRepairPrompt
          fault={libraryLifecycle.repair.fault}
          pending={libraryLifecycle.repair.pending}
          onRepair={() => { void repairLibrary(); }}
        />
        {renderLibrary()}
      </main>

      <footer className="single-page-statusbar">
        <span className="single-page-statusbar__selection">
          {preferences.libraryViewMode === 'flow'
            ? flowAnchorEntry
              ? `Viewing: ${displayName(flowAnchorEntry)}`
              : 'Flow is positioning the current wallpaper…'
            : selectedDescription(selectedEntry)}
        </span>
        <span className="single-page-statusbar__current">{currentWallpaperLabel(currentWallpaper)}</span>
        {runningStatus ? (
          <span className="single-page-statusbar__running" role="status">
            {runningStatus.message}
          </span>
        ) : null}
      </footer>

      <CompactSettingsPanel
        open={settingsOpen}
        obscured={sourcesOpen && sourcesReturnToSettings}
        preferences={preferences}
        updatePreferences={updatePreferences}
        behaviorSettings={behavior.settings}
        updateBehaviorSettings={behavior.updateSettings}
        behaviorReady={behavior.ready}
        loadError={behavior.loadError}
        saveError={behavior.saveError}
        rendererStatuses={rendererStatuses.statuses}
        rendererStatusesLoading={rendererStatuses.loading}
        rendererStatusesError={rendererStatuses.error}
        onReloadRendererStatuses={() => void rendererStatuses.reload()}
        onOpenSources={(trigger) => {
          sourcePanelReturnFocusRef.current = trigger;
          openSources(true);
        }}
        onClose={() => {
          setSettingsOpen(false);
          restoreOverlayFocus();
        }}
      />
      {sourcesMounted ? (
        <SourcePanel
          open={sourcesOpen}
          {...(sourcesReturnToSettings ? {
            onBack: () => {
              setSourcesOpen(false);
              setSourcesReturnToSettings(false);
              setRestoreSourceCardFocus(true);
            },
          } : {})}
          onClose={() => {
            const closesSettings = sourcesReturnToSettings;
            setSourcesOpen(false);
            setSourcesReturnToSettings(false);
            setRestoreSourceCardFocus(false);
            sourcePanelReturnFocusRef.current = null;
            if (closesSettings) setSettingsOpen(false);
            restoreOverlayFocus();
          }}
          onNotice={handleSourceNotice}
          onScanStarted={scan.onScanStarted}
          onScanFinished={scan.onScanFinished}
          sourceApi={api}
          onLibraryChanged={reconcileSourcesAndLibrary}
        />
      ) : null}
      <WallpaperDetailsDialog
        open={detailsEntry !== null}
        wallpaper={detailsEntry}
        onClose={closeDetails}
      />
      <div className="shell-notifications">
        <ScanActivity
          presentation={scan.presentation}
          progress={scan.progress}
          onCancel={() => void scan.requestCancel()}
          onDismiss={scan.dismissCancelled}
        />
        <FeedbackOverlay
          state={feedbackState}
          nowMs={feedbackNowMs}
          dispatch={dispatchFeedback}
          technicalDetails={technicalDetails}
        />
      </div>
    </div>
  );
}
