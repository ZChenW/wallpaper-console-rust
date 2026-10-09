import {
  MonitorCog,
  Search,
  Settings,
  Shuffle,
  SlidersHorizontal
} from 'lucide-react';
import { Popover } from 'radix-ui';
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import { libraryMetricsEnabled, recordMetric } from '../perf/metrics';
import LibraryContent from './LibraryContent.tsx';
import LibraryFilterControls, { sourceFilterValue } from './LibraryFilterControls.tsx';
import { useLibrarySelection } from './useLibrarySelection.ts';
import { useLibraryViewModel } from './useLibraryViewModel.ts';
import { useMpvpaperReapply } from './useMpvpaperReapply.ts';

import { api } from '../api/bridge.ts';
import { commandErrorFeedback, commandResultMessage } from '../api/feedback.ts';
import type {
  CommandResult,
  LibraryBrowserItemDTO,
} from '../api/types.ts';
import {
  DISPLAY_APPLY_DISABLED_REASON,
  resolveLibraryModeSwitchAnchor,
  userUnsupportedContextAction,
  type ContextAction
} from '../components/libraryViewModel.ts';
import LibraryViewSwitch from '../components/LibraryViewSwitch.tsx';
import OverflowStrip from '../components/OverflowStrip.tsx';
import { displayName } from '../components/wallpaperCardHelpers.ts';
import { primaryApplyKind } from '../domain/applyActions.ts';
import { useFeedbackBridge } from '../hooks/useFeedbackBridge.ts';
import {
  useThumbnailFailureCount,
  useThumbnailStore,
} from '../state/ThumbnailStoreContext.tsx';
import WallpaperDetailsDialog from './AuthorizedWallpaperDetailsDialog.tsx';
import CompactSettingsPanel from './CompactSettingsPanel.tsx';
import { buildDisplayTargetModel } from './displayTargets.ts';
import DisplayTargetSelector from './DisplayTargetSelector.tsx';
import { FeedbackOverlay } from './FeedbackOverlay.tsx';
import LibraryRepairPrompt from './LibraryRepairPrompt.tsx';
import LibraryResultAnnouncement from './LibraryResultAnnouncement.tsx';
import { ScanActivity } from './ScanActivity.tsx';
import {
  canChooseRandomWallpaper,
  currentWallpaperLabel,
  effectiveSourceFilter,
  reconcileSourceFilter,
  targetArgument
} from './singlePageShellModel.ts';
import { SourcePanel, type SourcePanelNotice } from './SourcePanel.tsx';

import { addSuggestedDirectory } from './firstRunSourceActions.ts';
import { createRecurringErrorGate } from './recurringErrorGate.ts';
import { useLibraryBrowser } from './useLibraryBrowser.ts';
import { useLibraryLifecycle } from './useLibraryLifecycle.ts';
import { useRendererStatuses } from './useRendererStatuses.ts';
import { useRuntimeWallpaperCoordinator } from './useRuntimeWallpaperCoordinator.ts';
import { useScanProgress } from './useScanProgress.ts';
import { useShellCatalog } from './useShellCatalog.ts';
import { useShellFeedback } from './useShellFeedback.ts';
import { useShellPreferences } from './useShellPreferences.ts';
import { useShellTheme } from './useShellTheme.ts';
import { useWallpaperBehaviorSettings } from './useWallpaperBehaviorSettings.ts';

function commandDetails(result: CommandResult): string {
  return [
    result.error?.message,
    result.error?.suggestion,
    result.error?.detail,
    result.stderr,
    result.stdout,
  ].filter((part): part is string => Boolean(part?.trim())).join('\n');
}

function selectedDescription(entry: LibraryBrowserItemDTO | null): string {
  if (!entry) return 'Select a wallpaper to see its details.';
  const sources = entry.sources.map((source) => source.displayName).join(', ');
  return `Selected: ${displayName(entry)}${sources ? ` · ${sources}` : ''}`;
}

export default function SinglePageShell() {
  if (libraryMetricsEnabled()) recordMetric('library.shell.render', 1);
  const [search, setSearch] = useState('');
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [filtersOpen, setFiltersOpen] = useState(false);
  const [sourcesOpen, setSourcesOpen] = useState(false);
  const [sourcesMounted, setSourcesMounted] = useState(false);
  const [sourcesReturnToSettings, setSourcesReturnToSettings] = useState(false);
  const [restoreSourceCardFocus, setRestoreSourceCardFocus] = useState(false);
  const [favoritePendingPaths, setFavoritePendingPaths] = useState<ReadonlySet<string>>(
    () => new Set(),
  );
  const favoritePendingPathsRef = useRef(new Set<string>());
  const overlayReturnFocusRef = useRef<HTMLElement | null>(null);
  const sourcePanelReturnFocusRef = useRef<HTMLButtonElement | null>(null);
  const libraryViewportAnchorRef = useRef<number | null>(null);
  const [libraryViewportAnchorId, setLibraryViewportAnchorId] = useState<number | null>(null);
  const [libraryModeAnchorId, setLibraryModeAnchorId] = useState<number | null>(null);
  const [libraryViewFocusToken, setLibraryViewFocusToken] = useState(0);
  const [libraryReturnFocusToken, setLibraryReturnFocusToken] = useState(0);
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
  const {
    selectedEntry, detailsEntry, selectLibraryEntry, openLibraryDetails, closeDetails,
  } = useLibrarySelection(browser.entries, browser.replaceCount, showNotice);
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
  const { applying: mpvpaperApplying, apply: applyMpvpaperOptions } = useMpvpaperReapply({
    saveOptions: behavior.saveMpvpaperOptions,
    reapply: () => {
      const arg = targetArgument(preferences.displayTarget);
      return api.reapplyMpvpaper(Array.isArray(arg) ? arg : [arg ?? 'all']);
    },
    refreshCurrent: runtimeWallpaper.current.refresh,
    wallpaperApplying: runtimeWallpaper.apply.applying,
    setFeedback: setSystemFeedback,
  });
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
          if (preferences.libraryViewMode === 'grid') selectLibraryEntry(entry);
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
    selectLibraryEntry,
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
    selectLibraryEntry(outcome.entry);
    applyEntry(outcome.entry);
  }, [applyEntry, browser.chooseRandom, selectLibraryEntry, showNotice]);

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

  // Choosing a filter or sort ends that errand, exactly like picking a display
  // target: hand the arrow keys back to the wallpapers instead of leaving them
  // on the select trigger, where they would keep changing the same control.
  const returnFocusToLibrary = useCallback(() => {
    setLibraryReturnFocusToken((token) => token + 1);
  }, []);
  const dismissLibraryFilters = useCallback(() => {
    setFiltersOpen(false);
    returnFocusToLibrary();
  }, [returnFocusToLibrary]);
  const rememberLibraryAnchor = useCallback((wallpaperId: number, settled = true) => {
    if (libraryMetricsEnabled()) recordMetric(settled ? "library.anchor.commit" : "library.anchor.preview", wallpaperId);
    libraryViewportAnchorRef.current = wallpaperId;
    if (settled) setLibraryViewportAnchorId((current) => current === wallpaperId ? current : wallpaperId);
  }, []);
  const outgoingLibraryMode = preferences.libraryViewMode;
  const changeLibraryViewMode = useCallback((mode: typeof outgoingLibraryMode) => {
    if (mode === outgoingLibraryMode) return;
    const anchor = resolveLibraryModeSwitchAnchor(
      browser.entries,
      selectedEntry?.wallpaperId,
      libraryViewportAnchorRef.current,
      outgoingLibraryMode,
    );
    setLibraryModeAnchorId(anchor?.wallpaperId ?? null);
    setLibraryViewFocusToken((token) => token + 1);
    updatePreferences((current) => ({ ...current, libraryViewMode: mode }));
  }, [browser.entries, outgoingLibraryMode, selectedEntry, updatePreferences]);

  const libraryViewModel = useLibraryViewModel({
    browser, runtimeWallpaper, favoritePendingPaths, scanRunning, resetKey,
    selectedPath: selectedEntry?.path ?? null,
    active: !settingsOpen && !sourcesOpen && detailsEntry === null,
    displayCanApply: displayModel.canApply,
    actions: {
      isEntryApplicable: isLibraryEntryApplicable, onSelect: selectLibraryEntry,
      onApply: applyEntry, onToggleFavorite: toggleFavorite,
      onDetails: openLibraryDetails, buildContextActions,
    },
  });
  const flowAnchorEntry = useMemo(
    () => browser.entries.find((entry) => entry.wallpaperId === libraryViewportAnchorId) ?? null,
    [browser.entries, libraryViewportAnchorId],
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
        if (!(target instanceof Element)) return;
        if (target.closest('[data-allow-context-menu], input, textarea, [contenteditable="true"]')) {
          return;
        }
        // Suppress the browser menu only on library surfaces; details, notices,
        // and other chrome keep native copy/inspect.
        if (!target.closest('.library-viewport, .wallpaper-card, .flow-preview-item')) {
          return;
        }
        event.preventDefault();
      }}
    >
      <header className="single-page-topbar" data-tauri-drag-region="deep">
        <h1 className="single-page-brand">Wallpaper Console</h1>
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
          onChange={(displayTarget) => {
            updatePreferences((current) => ({ ...current, displayTarget }));
          }}
          // Picking a target is the end of that errand. Leaving focus on the
          // trigger makes the next arrow key reopen this menu instead of
          // moving through wallpapers.
          onCommit={returnFocusToLibrary}
          disabled={!catalog.ready || Boolean(catalog.errors.displays)}
        />
        <button
          aria-label="Scan Wallpaper Engine"
          aria-busy={scanRunning}
          className="single-page-icon-button"
          data-topbar-action="scan-we"
          type="button"
          disabled={scanRunning}
          onClick={() => void scanWallpaperEngine()}
          title="Scan Wallpaper Engine"
        >
          <MonitorCog size={17} aria-hidden="true" />
        </button>
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
        <OverflowStrip className="single-page-filters" role="toolbar" aria-label="Library filters">
          <span aria-hidden="true" className="single-page-filters__label">01 / FILTER</span>
          <LibraryFilterControls
            sources={catalog.sources}
            sourceError={catalog.errors.sources}
            effectiveSrcFilter={effectiveSrcFilter}
            preferences={preferences}
            updatePreferences={updatePreferences}
            onDismiss={dismissLibraryFilters}
          />
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
                <LibraryFilterControls
            sources={catalog.sources}
            sourceError={catalog.errors.sources}
            effectiveSrcFilter={effectiveSrcFilter}
            preferences={preferences}
            updatePreferences={updatePreferences}
            onDismiss={dismissLibraryFilters}
          />
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
          <span className="single-page-count">
            <span aria-hidden="true" className="single-page-count__prefix">INDEX / </span>
            {browser.totalKnown
              ? `${browser.entries.length} / ${browser.total}`
              : `${browser.entries.length} loaded`}
          </span>
        ) : null}
      </div>

      <LibraryResultAnnouncement
        criteriaKey={browser.criteriaKey}
        totalKnown={browser.totalKnown}
        total={browser.total}
        pending={browser.criteriaReplacementPending || browser.loading}
      />
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
        <LibraryContent
          browser={browser}
          libraryLifecycle={libraryLifecycle}
          firstRunEligible={firstRunEligible}
          scanRunning={scanRunning}
          addFirstRunDirectory={addFirstRunDirectory}
          scanWallpaperEngine={scanWallpaperEngine}
          onOpenSources={(trigger) => { rememberOverlayTrigger(trigger); openSources(); }}
          onClearFilters={() => {
            setSearch('');
            updatePreferences((current) => ({
              ...current, sourceFilter: { kind: 'all' }, typeFilter: 'usable', favoritesOnly: false,
            }));
          }}
          viewport={{
            applyGesture: preferences.applyGesture,
            cardSize: preferences.cardSize,
            focusToken: libraryViewFocusToken,
            initialAnchorWallpaperId: libraryModeAnchorId,
            mode: preferences.libraryViewMode,
            model: libraryViewModel,
            onAnchorChange: rememberLibraryAnchor,
            returnFocusToken: libraryReturnFocusToken,
          }}
        />
      </main>

      <footer className="single-page-statusbar">
        <span className="single-page-statusbar__selection">
          {preferences.libraryViewMode !== 'grid'
            ? flowAnchorEntry
              ? `Viewing: ${displayName(flowAnchorEntry)}`
              : `${preferences.libraryViewMode === 'book' ? 'Book' : 'Flow'} is positioning the current wallpaper…`
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
        onApplyMpvpaperOptions={(options) => { void applyMpvpaperOptions(options); }}
        mpvpaperApplying={mpvpaperApplying}
        wallpaperApplying={runtimeWallpaper.apply.applying}
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
      <div className="shell-notifications" data-settings-open={settingsOpen}>
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
