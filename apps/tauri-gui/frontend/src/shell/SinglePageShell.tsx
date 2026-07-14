import {
  useCallback,
  useEffect,
  useMemo,
  useReducer,
  useRef,
  useState,
} from 'react';
import { FolderPlus, Search, Settings, Shuffle } from 'lucide-react';

import { api } from '../api/bridge.ts';
import type {
  CommandResult,
  FirstRunSourceSuggestionDTO,
  LibraryBrowserItemDTO,
  WallpaperDTO,
} from '../api/types.ts';
import { commandErrorFeedback } from '../api/feedback.ts';
import WallpaperGrid, { type ContextAction } from '../components/WallpaperGrid.tsx';
import { primaryApplyKind } from '../domain/applyActions.ts';
import { useFeedbackBridge } from '../hooks/useFeedbackBridge.ts';
import { useApplyQueue } from '../hooks/useApplyQueue.ts';
import { displayName } from '../components/wallpaperCardHelpers.ts';
import { buildDisplayTargetModel } from './displayTargets.ts';
import DisplayTargetSelector from './DisplayTargetSelector.tsx';
import { FeedbackOverlay } from './FeedbackOverlay.tsx';
import FirstRunSuggestions from './FirstRunSuggestions.tsx';
import LibraryRepairPrompt from './LibraryRepairPrompt.tsx';
import WallpaperDetailsDialog from './WallpaperDetailsDialog.tsx';
import { safeFileSrc } from '../components/WallpaperCard.tsx';
import { ScanActivity } from './ScanActivity.tsx';
import { SourcePanel, type SourcePanelNotice } from './SourcePanel.tsx';
import CompactSettingsPanel from './CompactSettingsPanel.tsx';
import {
  resolveCurrentWallpaperState,
  type PersistedDisplayWallpaper,
} from './currentWallpaperState.ts';
import {
  createRuntimeWallpaperSession,
  reduceRuntimeWallpaperSession,
  toRuntimeDisplayWallpapers,
} from './runtimeWallpaperSession.ts';
import { RuntimeObservationController } from './runtimeObservationController.ts';
import {
  canChooseRandomWallpaper,
  currentWallpaperLabel,
  reconcileSelectedEntry,
  reconcileSourceFilter,
  shouldOfferFirstRun,
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
import {
  faultAfterVerification,
  type LibraryRepairFault,
  shouldVerifyLibraryIntegrity,
  verifyLibraryIntegrity,
} from './libraryRepair.ts';

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

function persistedDisplayWallpapers(
  states: ReturnType<typeof useShellCatalog>['persistedDisplayStates'],
): PersistedDisplayWallpaper[] {
  const persisted: PersistedDisplayWallpaper[] = [];
  for (const state of states) {
    if (!state.wallpaperPath) continue;
    if (state.kind === 'output' && state.output) {
      persisted.push({
        target: { kind: 'output', output: state.output },
        wallpaperPath: state.wallpaperPath,
      });
      continue;
    }
    persisted.push({
      target: { kind: 'allDisplays' },
      wallpaperPath: state.wallpaperPath,
    });
  }
  return persisted;
}

function selectedDescription(entry: LibraryBrowserItemDTO | null): string {
  if (!entry) return 'Select a wallpaper to see its details.';
  const sources = entry.sources.map((source) => source.displayName).join(', ');
  return `Selected: ${displayName(entry)}${sources ? ` · ${sources}` : ''}`;
}

export default function SinglePageShell() {
  const [search, setSearch] = useState('');
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [sourcesOpen, setSourcesOpen] = useState(false);
  const [sourcesMounted, setSourcesMounted] = useState(false);
  const [selectedEntry, setSelectedEntry] = useState<LibraryBrowserItemDTO | null>(null);
  const [detailsEntry, setDetailsEntry] = useState<LibraryBrowserItemDTO | null>(null);
  const [favoritePendingPaths, setFavoritePendingPaths] = useState<ReadonlySet<string>>(
    () => new Set(),
  );
  const favoritePendingPathsRef = useRef(new Set<string>());
  const [firstRunSuggestions, setFirstRunSuggestions] = useState<FirstRunSourceSuggestionDTO[]>([]);
  const [firstRunSuggestionsError, setFirstRunSuggestionsError] = useState<string | null>(null);
  const [firstRunSuggestionReload, setFirstRunSuggestionReload] = useState(0);
  const [libraryRepairFault, setLibraryRepairFault] = useState<LibraryRepairFault | null>(null);
  const [libraryRepairPending, setLibraryRepairPending] = useState(false);
  const overlayReturnFocusRef = useRef<HTMLElement | null>(null);

  const rememberOverlayTrigger = useCallback((trigger: HTMLElement) => {
    overlayReturnFocusRef.current = trigger;
  }, []);
  const restoreOverlayFocus = useCallback(() => {
    const trigger = overlayReturnFocusRef.current;
    overlayReturnFocusRef.current = null;
    if (!trigger) return;
    window.requestAnimationFrame(() => trigger.focus());
  }, []);
  const openSources = useCallback(() => {
    setSourcesMounted(true);
    setSourcesOpen(true);
  }, []);

  const {
    preferences,
    ready: preferencesReady,
    loadError: preferencesLoadError,
    saveError: preferencesSaveError,
    updatePreferences,
  } = useShellPreferences(api);
  useShellTheme(preferences.theme);

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

  const firstRunEligible = catalog.ready
    && shouldOfferFirstRun(catalog.sources, catalog.errors.sources);
  const firstRunSuggestionRequest = useRef(0);
  useEffect(() => {
    const requestId = ++firstRunSuggestionRequest.current;
    if (!firstRunEligible) {
      setFirstRunSuggestions([]);
      setFirstRunSuggestionsError(null);
      return undefined;
    }
    setFirstRunSuggestionsError(null);
    void api.firstRunSourceSuggestions().then(
      (suggestions) => {
        if (firstRunSuggestionRequest.current === requestId) {
          setFirstRunSuggestions(suggestions);
        }
      },
      (error: unknown) => {
        if (firstRunSuggestionRequest.current !== requestId) return;
        setFirstRunSuggestions([]);
        setFirstRunSuggestionsError(error instanceof Error ? error.message : String(error));
      },
    );
    return () => {
      if (firstRunSuggestionRequest.current === requestId) {
        firstRunSuggestionRequest.current += 1;
      }
    };
  }, [firstRunEligible, firstRunSuggestionReload]);

  const browser = useLibraryBrowser({
    sourceFilter: preferences.sourceFilter,
    typeFilter: preferences.typeFilter,
    favoritesOnly: preferences.favoritesOnly,
    sort: preferences.sort,
    search,
  });
  const libraryVerificationRequest = useRef(0);
  const shouldVerifyLibrary = shouldVerifyLibraryIntegrity({
    browserLoadError: browser.loadError,
    sourceLoadError: Boolean(catalog.errors.sources),
    sourceCount: catalog.sources.length,
    emptyConfirmed: browser.emptyConfirmed,
    sourceFilter: preferences.sourceFilter,
    typeFilter: preferences.typeFilter,
    favoritesOnly: preferences.favoritesOnly,
    search: browser.debouncedSearch,
  });
  useEffect(() => {
    const requestId = ++libraryVerificationRequest.current;
    if (!shouldVerifyLibrary) return undefined;
    if (libraryRepairPending) return undefined;

    void verifyLibraryIntegrity(api).then((outcome) => {
      if (libraryVerificationRequest.current === requestId) {
        setLibraryRepairFault((current) => faultAfterVerification(current, outcome));
      }
    });
    return () => {
      if (libraryVerificationRequest.current === requestId) {
        libraryVerificationRequest.current += 1;
      }
    };
  }, [
    browser.emptyConfirmed,
    browser.loadError,
    browser.replaceCount,
    catalog.errors.sources,
    catalog.sources.length,
    libraryRepairPending,
    preferences.favoritesOnly,
    preferences.sourceFilter,
    preferences.typeFilter,
    browser.debouncedSearch,
    shouldVerifyLibrary,
  ]);
  const reloadLibraryRef = useRef(browser.reload);
  reloadLibraryRef.current = browser.reload;
  const invalidateLibrary = useCallback(() => {
    void reloadLibraryRef.current();
  }, []);

  const repairLibrary = useCallback(async (): Promise<void> => {
    if (libraryRepairPending) return;
    setLibraryRepairPending(true);
    try {
      const result = await api.sqliteRepair();
      if (!result.success) {
        setSystemFeedback(commandErrorFeedback('Library repair', result));
        return;
      }
      const verification = await verifyLibraryIntegrity(api);
      if (verification.status !== 'ok') {
        setLibraryRepairFault((current) => faultAfterVerification(current, verification));
        showNotice({
          channel: 'system',
          severity: 'error',
          message: verification.status === 'corrupt'
            ? 'Library repair could not restore database integrity.'
            : 'Library repair finished, but database integrity could not be verified.',
          technicalDetails: verification.status === 'corrupt'
            ? verification.fault.technicalDetails
            : verification.technicalDetails,
        });
        return;
      }
      setLibraryRepairFault(null);
      await Promise.allSettled([reloadLibraryRef.current(), catalog.reloadSources()]);
      showNotice({
        channel: 'system',
        severity: 'success',
        message: 'Library index repaired',
      });
    } catch (error) {
      setSystemFeedback(commandErrorFeedback('Library repair', error));
    } finally {
      setLibraryRepairPending(false);
    }
  }, [catalog, libraryRepairPending, setSystemFeedback, showNotice]);

  const refreshStatus = useCallback(async (): Promise<void> => {
    await catalog.reloadDisplays();
  }, [catalog]);

  const [runtimeSession, dispatchRuntimeSession] = useReducer(
    reduceRuntimeWallpaperSession,
    [],
    createRuntimeWallpaperSession,
  );
  const connectedOutputsKey = catalog.connectedOutputs.join('\0');
  useEffect(() => {
    dispatchRuntimeSession({
      type: 'connectedOutputsChanged',
      connectedOutputs: catalog.connectedOutputs,
    });
  }, [connectedOutputsKey]);

  const runtimeObservationController = useRef<RuntimeObservationController | null>(null);
  useEffect(() => {
    const outputs = [...catalog.connectedOutputs];
    if (!catalog.ready || outputs.length === 0) return undefined;
    const controller = new RuntimeObservationController({
      api,
      connectedOutputs: outputs,
      onObservations: (observations) => {
        dispatchRuntimeSession({ type: 'runtimeReconciled', observations });
      },
    });
    runtimeObservationController.current = controller;
    controller.start();
    return () => {
      if (runtimeObservationController.current === controller) {
        runtimeObservationController.current = null;
      }
      controller.stop();
    };
  }, [catalog.ready, connectedOutputsKey]);

  const onApplied = useCallback<NonNullable<Parameters<typeof useApplyQueue>[0]['onApplied']>>(
    (request, result, transport) => {
      dispatchRuntimeSession({ type: 'applySucceeded', request, result, transport });
      void runtimeObservationController.current?.invalidateAndRefresh();
    },
    [],
  );
  const applyQueue = useApplyQueue({
    refreshStatus,
    setFeedbackWithAutoDismiss: setApplyFeedback,
    invalidateLibrary,
    onApplied,
  });

  const currentWallpaper = useMemo(() => resolveCurrentWallpaperState({
    activeTarget: preferences.displayTarget,
    connectedOutputs: catalog.connectedOutputs,
    runtime: toRuntimeDisplayWallpapers(runtimeSession),
    persisted: persistedDisplayWallpapers(catalog.persistedDisplayStates),
  }), [
    catalog.connectedOutputs,
    catalog.persistedDisplayStates,
    preferences.displayTarget,
    runtimeSession,
  ]);
  const currentPath = currentWallpaper.kind === 'confirmed'
    ? currentWallpaper.wallpaperPath
    : null;
  const displayModel = buildDisplayTargetModel(
    catalog.connectedOutputs,
    preferences.displayTarget,
  );

  const applyEntry = useCallback((entry: WallpaperDTO) => {
    if (!displayModel.canApply) {
      showNotice({
        channel: 'apply',
        severity: 'error',
        message: 'The selected display is not connected.',
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
      applyQueue.handleApplyActionToDisplay({ kind, path: entry.path }, target);
      return;
    }
    applyQueue.handleApplyToDisplay(entry.path, target);
  }, [applyQueue, displayModel.canApply, preferences.displayTarget, showNotice]);

  const applyPath = useCallback((path: string) => {
    const entry = browser.entryByPath.get(path);
    if (entry) applyEntry(entry);
  }, [applyEntry, browser.entryByPath]);

  useEffect(() => {
    if (!catalog.ready || catalog.errors.sources) return;
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
  }, [catalog.ready, catalog.sources, preferences.sourceFilter, updatePreferences]);

  useEffect(() => {
    setSelectedEntry((current) => reconcileSelectedEntry(current, browser.entryByPath));
  }, [browser.entryByPath, browser.replaceCount]);

  const previousScanRunning = useRef(false);
  useEffect(() => {
    const running = scan.progress?.running === true;
    if (previousScanRunning.current && !running) {
      void Promise.allSettled([reloadLibraryRef.current(), catalog.reloadSources()]);
    }
    previousScanRunning.current = running;
  }, [catalog.reloadSources, scan.progress?.running]);

  // A startup refresh can finish between the initial idle probe and the next
  // poll. One delayed reconciliation closes that small observation window.
  const startupReconciled = useRef(false);
  useEffect(() => {
    if (!catalog.ready || startupReconciled.current) return undefined;
    startupReconciled.current = true;
    const timer = window.setTimeout(() => {
      void Promise.allSettled([reloadLibraryRef.current(), catalog.reloadSources()]);
    }, 2_250);
    return () => window.clearTimeout(timer);
  }, [catalog.ready, catalog.reloadSources]);

  const lastScanError = useRef<string | null>(null);
  useEffect(() => {
    const error = scan.scanError ?? scan.transportError;
    if (!error || error === lastScanError.current) return;
    lastScanError.current = error;
    showNotice({
      channel: 'scan',
      severity: scan.scanError ? 'error' : 'warning',
      message: scan.scanError ? 'Wallpaper scan failed.' : 'Scan status is temporarily unavailable.',
      technicalDetails: error,
    });
  }, [scan.scanError, scan.transportError, showNotice]);

  useEffect(() => {
    const error = preferencesSaveError ?? preferencesLoadError;
    if (!error) return;
    showNotice({
      channel: 'settings',
      severity: 'warning',
      message: 'Some interface preferences could not be saved.',
      technicalDetails: error.message,
    });
  }, [preferencesLoadError, preferencesSaveError, showNotice]);

  const handleSourceNotice = useCallback((notice: SourcePanelNotice) => {
    showNotice(notice);
  }, [showNotice]);

  const reconcileSourcesAndLibrary = useCallback(async (): Promise<void> => {
    await Promise.allSettled([catalog.reloadSources(), reloadLibraryRef.current()]);
  }, [catalog.reloadSources]);

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
      await reloadLibraryRef.current();
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
  }, [setCommandFeedback, showNotice]);

  const openLocation = useCallback(async (entry: WallpaperDTO) => {
    try {
      const result = await api.openProjectLocation(entry.path, 'file_manager');
      if (!result.success) setCommandFeedback(commandErrorFeedback('Open location', result), 'system');
    } catch (error) {
      setCommandFeedback(commandErrorFeedback('Open location', error), 'system');
    }
  }, [setCommandFeedback]);

  const buildContextActions = useCallback((wallpaper: WallpaperDTO): ContextAction[] => {
    const entry = wallpaper as LibraryBrowserItemDTO;
    const actions: ContextAction[] = [
      {
        label: entry.favorite ? 'Remove from Favorites' : 'Add to Favorites',
        action: () => void toggleFavorite(entry),
      },
      {
        label: 'Open Location',
        action: () => void openLocation(entry),
      },
      {
        label: 'Information',
        action: () => {
          setSelectedEntry(entry);
          setDetailsEntry(entry);
        },
      },
    ];
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
  }, [openLocation, showNotice, toggleFavorite]);

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
        showNotice({ channel: 'scan', severity: 'success', message: 'Wallpaper Engine scan finished.' });
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

  const scanRunning = scan.progress?.running === true || scan.scanState.kind === 'running';
  const offlineSourceCount = catalog.sources.filter(
    (source) => source.availability === 'offline',
  ).length;
  const resetKey = [
    sourceFilterValue(preferences.sourceFilter),
    preferences.typeFilter,
    preferences.favoritesOnly ? 'favorites' : 'all',
    preferences.sort,
    browser.debouncedSearch,
  ].join('|');

  const renderLibrary = () => {
    if (!preferencesReady || !catalog.ready || browser.initialLoading) {
      return <div className="single-page-empty" role="status">Loading wallpaper library…</div>;
    }
    if (catalog.errors.sources && catalog.sources.length === 0) {
      return (
        <section className="single-page-empty" role="alert">
          <h2>Could not load wallpaper sources</h2>
          <p>{catalog.errors.sources}</p>
          <button className="btn" type="button" onClick={() => void catalog.reloadSources()}>
            Retry
          </button>
        </section>
      );
    }
    if (shouldOfferFirstRun(catalog.sources, catalog.errors.sources)) {
      return (
        <section className="single-page-empty single-page-first-run">
          <h2>Choose where your wallpapers live</h2>
          <p>Add any number of folders. Nothing is scanned until you choose it.</p>
          <div className="single-page-empty__actions">
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
          </div>
          <FirstRunSuggestions
            suggestions={firstRunSuggestions}
            onAddDirectory={(path) => void addFirstRunDirectory(path)}
            onScanWallpaperEngine={() => void scanWallpaperEngine()}
          />
          {firstRunSuggestionsError ? (
            <div className="single-page-first-run__suggestion-error" role="status">
              <span>Optional source suggestions are unavailable.</span>
              <button className="btn" type="button" onClick={() => setFirstRunSuggestionReload((value) => value + 1)}>
                Retry suggestions
              </button>
            </div>
          ) : null}
        </section>
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
          <WallpaperGrid
            entries={browser.entries}
            onApply={applyPath}
            onSelect={(entry) => setSelectedEntry(entry as LibraryBrowserItemDTO)}
            onToggleFavorite={toggleFavorite}
            applying={applyQueue.applying}
            favoritePendingPaths={favoritePendingPaths}
            buildContextActions={buildContextActions}
            active={true}
            refreshing={browser.refreshing || scanRunning}
            resetKey={resetKey}
            cardSize={preferences.cardSize}
            applyGesture={preferences.applyGesture}
            selectedPath={selectedEntry?.path ?? null}
            pendingPath={applyQueue.pendingPath ?? applyQueue.activePath ?? null}
            currentPath={currentPath}
            isEntryApplicable={(entry) => primaryApplyKind(entry) !== null}
            hasMore={browser.entries.length < browser.total && !browser.automaticAppendPaused}
            loadingMore={browser.appending}
            onLoadMore={browser.loadMore}
          />
          {!browser.refreshing && browser.entries.length < browser.total ? (
            <div className="single-page-load-more">
              <button
                className="btn"
                disabled={browser.appending}
                type="button"
                onClick={() => void browser.loadMore()}
                title={browser.automaticAppendPaused && browser.loadErrorDetail
                  ? browser.loadErrorDetail
                  : undefined}
              >
                {browser.appending
                  ? 'Loading more…'
                  : browser.automaticAppendPaused
                    ? 'Retry loading more'
                    : `Load more · ${browser.total - browser.entries.length} remaining`}
              </button>
            </div>
          ) : null}
        </>
      );
    }
    if (scanRunning) {
      return <div className="single-page-empty" role="status">Indexing wallpapers…</div>;
    }
    if (browser.loadError) {
      return (
        <div className="single-page-empty" role="alert">
          <p>Could not load the wallpaper library.</p>
          {browser.loadErrorDetail ? <p>{browser.loadErrorDetail}</p> : null}
          <button className="btn" type="button" onClick={() => void browser.reload()}>Retry</button>
        </div>
      );
    }
    if (!browser.emptyConfirmed) {
      return <div className="single-page-empty" role="status">Checking the library…</div>;
    }
    return (
      <div className="single-page-empty">
        <p>No wallpapers match the active filters.</p>
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
      </div>
    );
  };

  return (
    <div className={`single-page-shell${settingsOpen ? ' settings-open' : ''}`}>
      <header className="single-page-topbar">
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
          disabled={!catalog.ready}
        />
        <button
          aria-label="Apply a random wallpaper from active filters"
          className="single-page-icon-button"
          type="button"
          disabled={!canChooseRandomWallpaper({
            searchSettled: browser.searchSettled,
            randomPending: browser.randomPending,
            total: browser.total,
            canApply: displayModel.canApply,
          })}
          onClick={() => void chooseRandom()}
        >
          <Shuffle size={17} aria-hidden="true" />
        </button>
        <button
          aria-label="Open settings"
          className="single-page-icon-button"
          type="button"
          onClick={(event) => {
            rememberOverlayTrigger(event.currentTarget);
            setSettingsOpen(true);
          }}
        >
          <Settings size={18} aria-hidden="true" />
        </button>
      </header>

      <div className="single-page-filters" aria-label="Library filters">
        <select
          aria-label="Source filter"
          value={sourceFilterValue(preferences.sourceFilter)}
          onChange={(event) => {
            const sourceFilter = sourceFilterFromValue(event.currentTarget.value);
            updatePreferences((current) => ({ ...current, sourceFilter }));
          }}
        >
          <option value="all">ALL SOURCES</option>
          {catalog.sources.map((source) => (
            <option key={source.id} value={`source:${source.id}`}>
              {source.displayName}{source.availability === 'offline' ? ' · Offline' : ''}
            </option>
          ))}
        </select>
        <select
          aria-label="Wallpaper type filter"
          value={preferences.typeFilter}
          onChange={(event) => {
            const typeFilter = event.currentTarget.value as LibraryTypeFilter;
            updatePreferences((current) => ({ ...current, typeFilter }));
          }}
        >
          <option value="usable">ALL</option>
          <option value="image">Images</option>
          <option value="gif">GIFs</option>
          <option value="video">Videos</option>
          <option value="weScene">Wallpaper Engine scenes</option>
          <option value="unsupported">Unsupported</option>
        </select>
        <label className="single-page-checkbox">
          <input
            type="checkbox"
            checked={preferences.favoritesOnly}
            onChange={(event) => {
              const favoritesOnly = event.currentTarget.checked;
              updatePreferences((current) => ({ ...current, favoritesOnly }));
            }}
          />
          Favorites
        </label>
        <select
          aria-label="Library sort"
          value={preferences.sort}
          onChange={(event) => {
            const sort = event.currentTarget.value as LibrarySort;
            updatePreferences((current) => ({ ...current, sort }));
          }}
        >
          <option value="recentlyAdded">Recently added</option>
          <option value="nameAsc">Name A–Z</option>
          <option value="nameDesc">Name Z–A</option>
        </select>
        <select
          aria-label="Card size"
          value={preferences.cardSize}
          onChange={(event) => {
            const cardSize = event.currentTarget.value as typeof preferences.cardSize;
            updatePreferences((current) => ({ ...current, cardSize }));
          }}
        >
          <option value="small">Small</option>
          <option value="medium">Medium</option>
          <option value="large">Large</option>
        </select>
        <span className="single-page-count" aria-live="polite">
          {browser.entries.length} / {browser.total}
        </span>
      </div>

      <main className="single-page-library">
        <LibraryRepairPrompt
          fault={libraryRepairFault}
          pending={libraryRepairPending}
          onRepair={() => { void repairLibrary(); }}
        />
        {renderLibrary()}
      </main>

      <footer className="single-page-statusbar">
        <span className="single-page-statusbar__selection">{selectedDescription(selectedEntry)}</span>
        <span className="single-page-statusbar__current">{currentWallpaperLabel(currentWallpaper)}</span>
        {runningStatus ? (
          <span className="single-page-statusbar__running" role="status">
            {runningStatus.message}
          </span>
        ) : null}
      </footer>

      <CompactSettingsPanel
        open={settingsOpen}
        preferences={preferences}
        updatePreferences={updatePreferences}
        behaviorSettings={behavior.settings}
        updateBehaviorSettings={behavior.updateSettings}
        behaviorReady={behavior.ready}
        loadError={behavior.loadError}
        saveError={behavior.saveError}
        rendererStatuses={rendererStatuses.statuses}
        sourceCount={catalog.sources.length}
        offlineSourceCount={offlineSourceCount}
        onOpenSources={() => {
          setSettingsOpen(false);
          openSources();
        }}
        onClose={() => {
          setSettingsOpen(false);
          restoreOverlayFocus();
        }}
      />
      {sourcesMounted ? (
        <SourcePanel
          open={sourcesOpen}
          onClose={() => {
            setSourcesOpen(false);
            restoreOverlayFocus();
          }}
          onNotice={handleSourceNotice}
          onScanStarted={scan.onScanStarted}
          onScanFinished={scan.onScanFinished}
          sourceApi={api}
          onLibraryChanged={reconcileSourcesAndLibrary}
        />
      ) : null}
      <ScanActivity
        presentation={scan.presentation}
        progress={scan.progress}
        onCancel={() => void scan.requestCancel()}
        onDismiss={scan.dismissCancelled}
      />
      <WallpaperDetailsDialog
        open={detailsEntry !== null}
        wallpaper={detailsEntry}
        previewSrc={detailsEntry
          ? (
            detailsEntry.previewPath
              ? safeFileSrc(detailsEntry.previewPath)
              : (detailsEntry.type === 'image' || detailsEntry.type === 'gif')
                ? safeFileSrc(detailsEntry.path)
                : null
          )
          : null}
        onClose={() => setDetailsEntry(null)}
      />
      <FeedbackOverlay
        state={feedbackState}
        nowMs={feedbackNowMs}
        dispatch={dispatchFeedback}
        technicalDetails={technicalDetails}
      />
    </div>
  );
}
