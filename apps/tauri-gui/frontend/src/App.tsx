import { lazy, startTransition, Suspense, useCallback, useEffect, useRef, useState } from 'react';
import { api, CommandResult } from './api/bridge';
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

type View = 'library' | 'favorites' | 'history' | 'sources' | 'settings';

const SettingsView = lazy(() => import('./views/SettingsView'));

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
  const [applying, setApplying] = useState(false);
  const applyingRef = useRef(false);
  const {
    status,
    feedback,
    refreshStatus,
    setFeedbackWithAutoDismiss,
    clearFeedback,
  } = useAppState();

  // Listen for wc-feedback events from child components
  useEffect(() => {
    const handler = (e: Event) => {
      const detail = (e as CustomEvent).detail;
      if (detail) setFeedbackWithAutoDismiss(detail);
    };
    window.addEventListener('wc-feedback', handler);
    return () => window.removeEventListener('wc-feedback', handler);
  }, [setFeedbackWithAutoDismiss]);

  const pendingApplyRef = useRef<string | null>(null);

  const handleApply = useCallback(async (path: string) => {
    if (applyingRef.current) {
      pendingApplyRef.current = path;
      return;
    }
    applyingRef.current = true;
    setApplying(true);

    let currentPath: string | null = path;
    while (currentPath !== null) {
      const p: string = currentPath;
      currentPath = null; // Clear for this iteration
      setFeedbackWithAutoDismiss({ state: 'running', label: 'Applying wallpaper' });
      try {
        const r = await api.apply(p);
        if (r.success) {
          invalidateHistoryCache();
          setFeedbackWithAutoDismiss({ state: 'success', label: 'Applied', detail: p.split('/').pop() });
        } else {
          setFeedbackWithAutoDismiss(commandErrorFeedback('Apply', r));
        }
        await refreshStatus();
      } catch (e) {
        setFeedbackWithAutoDismiss({ state: 'error', label: 'Apply failed', detail: String(e) });
      }
      // Drain next pending intent
      const next = pendingApplyRef.current;
      pendingApplyRef.current = null;
      if (next && next !== p) {
        currentPath = next;
      }
    }

    setApplying(false);
    applyingRef.current = false;
  }, [refreshStatus, setFeedbackWithAutoDismiss]);

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
        <Sidebar view={view} onNavigate={handleNavigate} />
        <main className="main-content">
          {persistentViews.map(v => visitedViews.has(v) && (
            <div key={v} className="view-shell" style={{ display: view === v ? 'flex' : 'none' }}>
              {v === 'library' && <LibraryView onApply={handleApply} applying={applying} active={view === v} />}
              {v === 'favorites' && <FavoritesView onApply={handleApply} applying={applying} active={view === v} />}
              {v === 'history' && <HistoryView onApply={handleApply} applying={applying} active={view === v} />}
            </div>
          ))}
          {(view === 'sources' || view === 'settings') && (
            <div className="view-shell">
              {view === 'sources' && <SourcesView onRefresh={refreshStatus} onFeedback={handleFeedback} />}
              {view === 'settings' && (
                <Suspense fallback={<div className="loading">Loading settings...</div>}>
                  <SettingsView onRefresh={refreshStatus} onFeedback={handleFeedback} />
                </Suspense>
              )}
            </div>
          )}
        </main>
      </div>
      <StatusBar status={status} applying={applying} feedback={feedback} />
      <Toast feedback={feedback} onDismiss={clearFeedback} />
      <PerformanceOverlay />
    </div>
  );
}
