import { lazy, startTransition, Suspense, useCallback, useEffect, useState } from 'react';
import { api, CommandResult, ApplyRequestDTO } from './api/bridge';
import { APP_EVENTS } from './events/appEvents';
import { setTheme as setTauriTheme } from '@tauri-apps/api/app';
import { CommandFeedback, commandErrorFeedback, commandSuccessFeedback } from './api/feedback';
import LibraryView from './views/LibraryView';
import FavoritesView, { invalidateFavoritesCache } from './views/FavoritesView';
import HistoryView, { invalidateHistoryCache } from './views/HistoryView';
import SourcesView from './views/SourcesView';
import StatusBar from './components/StatusBar';
import Toolbar from './components/Toolbar';
import Sidebar from './components/Sidebar';
import Toast from './components/Toast';
import PerformanceOverlay from './components/PerformanceOverlay';
import { AppStateProvider, useAppState } from './state/AppStateContext';
import { ThumbnailStoreProvider } from './state/ThumbnailStoreContext';
import { useApplyQueue } from './hooks/useApplyQueue';
import { useFeedbackBridge } from './hooks/useFeedbackBridge';

type View = 'library' | 'favorites' | 'history' | 'sources';

const SettingsView = lazy(() => import('./views/SettingsView'));

function normalizeTheme(value: string | null | undefined): 'light' | 'obsidian_warm' {
  if (value === 'obsidian_warm') return 'obsidian_warm';
  return 'light';
}

function applyTheme(value: string | null | undefined) {
  const theme = normalizeTheme(value);
  document.documentElement.dataset.theme = theme;

  const windowTheme = theme === 'obsidian_warm' ? 'dark' : 'light';
  void setTauriTheme(windowTheme).catch(() => {
    // Browser/mock/e2e environments may not have a real Tauri runtime.
  });
}

export default function App() {
  return (
    <AppStateProvider>
      <ThumbnailStoreProvider>
        <AppShell />
      </ThumbnailStoreProvider>
    </AppStateProvider>
  );
}

function AppShell() {
  const [view, setView] = useState<View>('library');
  const [visitedViews, setVisitedViews] = useState<Set<View>>(new Set(['library']));
  const [settingsOpen, setSettingsOpen] = useState(false);
  const {
    status,
    feedback,
    refreshStatus,
    setFeedbackWithAutoDismiss,
    clearFeedback,
    invalidateLibrary,
  } = useAppState();

  useFeedbackBridge(setFeedbackWithAutoDismiss);

  useEffect(() => {
    let cancelled = false;
    api.configGet('gui_theme')
      .then((value) => {
        if (!cancelled) applyTheme(value);
      })
      .catch(() => {
        if (!cancelled) applyTheme('current');
      });

    const handler = (event: Event) => {
      const detail = (event as CustomEvent<{ key: string; value: string }>).detail;
      if (detail?.key === 'gui_theme') applyTheme(detail.value);
    };
    window.addEventListener(APP_EVENTS.configChanged, handler);
    return () => {
      cancelled = true;
      window.removeEventListener(APP_EVENTS.configChanged, handler);
    };
  }, []);

  // Preload Settings chunk so first open doesn't show Suspense fallback
  useEffect(() => {
    void import('./views/SettingsView');
  }, []);

  const { applying, handleApply, handleApplyAction } = useApplyQueue({
    refreshStatus,
    setFeedbackWithAutoDismiss,
    invalidateHistory: invalidateHistoryCache,
    invalidateLibrary,
  });

  const handleToolbarAction = useCallback(async (
    action: () => Promise<CommandResult | void>,
    label: string,
  ) => {
    setFeedbackWithAutoDismiss({ state: 'running', label });
    try {
      const r = await action();
      if (r && !r.success) {
        setFeedbackWithAutoDismiss(commandErrorFeedback(label, r));
      } else {
        setFeedbackWithAutoDismiss(commandSuccessFeedback(label, r));
      }
      await refreshStatus();
    } catch (e) {
      setFeedbackWithAutoDismiss(commandErrorFeedback(label, e));
    }
  }, [refreshStatus, setFeedbackWithAutoDismiss]);

  const handleFeedback = useCallback((fb: CommandFeedback) => {
    setFeedbackWithAutoDismiss(fb);
  }, [setFeedbackWithAutoDismiss]);

  const handleNavigate = useCallback((nextView: View) => {
    startTransition(() => {
      setView(nextView);
      setVisitedViews(prev => new Set(prev).add(nextView));
    });
  }, []);

  const persistentViews: View[] = ['library', 'favorites', 'history'];

  return (
    <div className="app">
      <Toolbar
        view={view}
        onAction={handleToolbarAction}
        applying={applying}
      />
      <div className="app-body">
        <Sidebar view={view} settingsOpen={settingsOpen} onNavigate={handleNavigate} onOpenSettings={() => setSettingsOpen(true)} />
        <main className="main-content">
          {persistentViews.map(v => visitedViews.has(v) && (
            <div key={v} className="view-shell" style={{ display: view === v ? 'flex' : 'none' }}>
              {v === 'library' && <LibraryView onApply={handleApply} onApplyAction={handleApplyAction} applying={applying} active={view === v} />}
              {v === 'favorites' && <FavoritesView onApply={handleApply} onApplyAction={handleApplyAction} applying={applying} active={view === v} />}
              {v === 'history' && <HistoryView onApply={handleApply} onApplyAction={handleApplyAction} applying={applying} active={view === v} />}
            </div>
          ))}
          {view === 'sources' && (
            <div className="view-shell">
              <SourcesView onRefresh={refreshStatus} onFeedback={handleFeedback} />
            </div>
          )}
        </main>
      </div>
      {settingsOpen && (
        <Suspense fallback={null}>
          <SettingsView
            onRefresh={refreshStatus}
            onFeedback={handleFeedback}
            onClose={() => setSettingsOpen(false)}
          />
        </Suspense>
      )}
      <StatusBar status={status} applying={applying} feedback={feedback} />
      <Toast feedback={feedback} onDismiss={clearFeedback} />
      <PerformanceOverlay />
    </div>
  );
}
