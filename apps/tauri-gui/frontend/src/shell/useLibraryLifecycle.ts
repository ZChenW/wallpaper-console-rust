import { listen } from '@tauri-apps/api/event';
import {
  useCallback,
  useEffect,
  useRef,
  useState,
} from 'react';

import type {
  SourceDTO,
} from '../api/types.ts';
import type { CommandFeedback } from '../api/feedback.ts';
import type { LibraryTypeFilter, SourceFilter } from './shellPreferences.ts';
import {
  LibraryLifecycleController,
  type LibraryLifecycleApi,
  type LibraryLifecycleSnapshot,
} from './libraryLifecycleController.ts';
import { shouldVerifyLibraryIntegrity } from './libraryRepair.ts';
import { shouldShowFirstRun } from './singlePageShellModel.ts';
import {
  shouldClearLibraryTimeout,
  shouldSignalLibraryPaint,
} from './startupWatchdog.ts';
import type { ShellNoticeInput } from './useShellFeedback.ts';
import { LIBRARY_REFRESH_EVENT } from './useLibraryBrowser.ts';

const LIBRARY_REVISION_EVENT = 'library-revision-changed';
const WATCHDOG_MS = 3_000;

export interface LibraryLifecycleBrowser {
  readonly initialLoading: boolean;
  readonly entriesCount: number;
  readonly emptyConfirmed: boolean;
  readonly loadError: boolean;
  readonly replaceCount: number;
  readonly debouncedSearch: string;
  readonly reload: () => Promise<unknown>;
}

export interface LibraryLifecycleCatalog {
  readonly sources: readonly SourceDTO[];
  readonly sourcesReady: boolean;
  readonly sourceError?: string;
  readonly reloadSources: () => Promise<unknown>;
}

export interface LibraryLifecycleScanState {
  /** Includes optimistic frontend scan startup and backend-reported activity. */
  readonly blocksFirstRun: boolean;
  /** Tracks the backend progress edge that requires catalog reconciliation. */
  readonly backendReportedRunning: boolean;
}

export interface UseLibraryLifecycleOptions {
  readonly api: LibraryLifecycleApi;
  readonly browser: LibraryLifecycleBrowser;
  readonly catalog: LibraryLifecycleCatalog;
  readonly sourceFilter: SourceFilter;
  readonly typeFilter: LibraryTypeFilter;
  readonly favoritesOnly: boolean;
  readonly scan: LibraryLifecycleScanState;
  readonly refreshThumbnails: () => void;
  readonly showNotice: (notice: ShellNoticeInput) => void;
  readonly setSystemFeedback: (feedback: CommandFeedback) => void;
}

export interface LibraryLifecycleResult {
  readonly firstRun: {
    readonly eligible: boolean;
    readonly suggestions: LibraryLifecycleSnapshot['firstRunSuggestions'];
    readonly error: string | null;
    readonly retrySuggestions: () => void;
  };
  readonly startup: {
    readonly timedOut: boolean;
    readonly retry: () => void;
  };
  readonly repair: {
    readonly fault: LibraryLifecycleSnapshot['repairFault'];
    readonly pending: boolean;
    readonly run: () => Promise<void>;
  };
  readonly reloadLibrary: () => Promise<unknown>;
  readonly reconcileSourcesAndLibrary: () => Promise<void>;
}

function useLatest<T>(value: T) {
  const ref = useRef(value);
  ref.current = value;
  return ref;
}

export function useLibraryLifecycle({
  api,
  browser,
  catalog,
  sourceFilter,
  typeFilter,
  favoritesOnly,
  scan,
  refreshThumbnails,
  showNotice,
  setSystemFeedback,
}: UseLibraryLifecycleOptions): LibraryLifecycleResult {
  const latest = useLatest({
    reloadLibrary: browser.reload,
    reloadSources: catalog.reloadSources,
    showNotice,
    setSystemFeedback,
  });
  const controllerRef = useRef<LibraryLifecycleController | null>(null);
  if (controllerRef.current === null) {
    controllerRef.current = new LibraryLifecycleController({
      api,
      reloadLibrary: () => latest.current.reloadLibrary(),
      reloadSources: () => latest.current.reloadSources(),
      showNotice: (notice) => latest.current.showNotice(notice),
      setSystemFeedback: (feedback) => latest.current.setSystemFeedback(feedback),
    });
  }
  const controller = controllerRef.current;
  const [snapshot, setSnapshot] = useState(controller.snapshot);
  const [suggestionRetry, setSuggestionRetry] = useState(0);

  useEffect(() => controller.subscribe(setSnapshot), [controller]);

  const firstRunEligible = shouldShowFirstRun({
    sources: catalog.sources,
    sourceError: catalog.sourceError,
    sourcesReady: catalog.sourcesReady,
    initialLoading: browser.initialLoading,
    emptyConfirmed: browser.emptyConfirmed,
    entryCount: browser.entriesCount,
    libraryError: browser.loadError,
    scanRunning: scan.blocksFirstRun,
  });
  useEffect(
    () => controller.requestFirstRunSuggestions(firstRunEligible),
    [controller, firstRunEligible, suggestionRetry],
  );

  const paintState = {
    initialLoading: browser.initialLoading,
    hasEntries: browser.entriesCount > 0,
    emptyConfirmed: browser.emptyConfirmed,
    loadError: browser.loadError,
    timedOut: snapshot.initialRequestTimedOut,
  };
  const paintActive = shouldSignalLibraryPaint(paintState);
  useEffect(
    () => controller.activateReadyDelivery(paintActive),
    [controller, paintActive],
  );

  const timeoutResolved = shouldClearLibraryTimeout(paintState);
  useEffect(() => {
    controller.clearTimeoutIf(timeoutResolved);
  }, [controller, timeoutResolved]);
  useEffect(
    () => controller.watchInitialRequest(paintActive, WATCHDOG_MS),
    [controller, paintActive, snapshot.watchdogRetry],
  );

  const shouldVerify = shouldVerifyLibraryIntegrity({
    browserLoadError: browser.loadError,
    sourceLoadError: Boolean(catalog.sourceError),
    sourceCount: catalog.sources.length,
    emptyConfirmed: browser.emptyConfirmed,
    sourceFilter,
    typeFilter,
    favoritesOnly,
    search: browser.debouncedSearch,
  });
  const sourceFilterKey = sourceFilter.kind === 'source'
    ? `source:${sourceFilter.sourceId}`
    : 'all';
  useEffect(
    () => controller.verifyIntegrity(shouldVerify),
    [
      browser.emptyConfirmed,
      browser.loadError,
      browser.replaceCount,
      catalog.sourceError,
      catalog.sources.length,
      controller,
      favoritesOnly,
      sourceFilterKey,
      typeFilter,
      browser.debouncedSearch,
      shouldVerify,
      snapshot.repairPending,
    ],
  );

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen<number>(LIBRARY_REVISION_EVENT, () => {
      window.dispatchEvent(new Event(LIBRARY_REFRESH_EVENT));
    }).then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    }).catch(() => {
      // Browser-only tests and the mock frontend do not provide Tauri events.
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    const handler = () => refreshThumbnails();
    window.addEventListener(LIBRARY_REFRESH_EVENT, handler);
    return () => window.removeEventListener(LIBRARY_REFRESH_EVENT, handler);
  }, [refreshThumbnails]);

  const previousScanRunning = useRef(false);
  useEffect(() => {
    if (previousScanRunning.current && !scan.backendReportedRunning) {
      void controller.reconcileSourcesAndLibrary();
    }
    previousScanRunning.current = scan.backendReportedRunning;
  }, [controller, scan.backendReportedRunning]);

  const retryInitialRequest = useCallback(() => {
    controller.retryInitialRequest();
    void controller.reloadLibrary();
  }, [controller]);
  const retrySuggestions = useCallback(() => {
    setSuggestionRetry((value) => value + 1);
  }, []);
  const runRepair = useCallback(() => controller.repairLibrary(), [controller]);
  const reloadLibrary = useCallback(() => controller.reloadLibrary(), [controller]);
  const reconcileSourcesAndLibrary = useCallback(
    () => controller.reconcileSourcesAndLibrary(),
    [controller],
  );

  return {
    firstRun: {
      eligible: firstRunEligible,
      suggestions: snapshot.firstRunSuggestions,
      error: snapshot.firstRunSuggestionsError,
      retrySuggestions,
    },
    startup: {
      timedOut: snapshot.initialRequestTimedOut,
      retry: retryInitialRequest,
    },
    repair: {
      fault: snapshot.repairFault,
      pending: snapshot.repairPending,
      run: runRepair,
    },
    reloadLibrary,
    reconcileSourcesAndLibrary,
  };
}
