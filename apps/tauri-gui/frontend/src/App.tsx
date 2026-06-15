import { lazy, startTransition, Suspense, useCallback, useEffect, useRef, useState } from 'react';
import { api, CommandResult, ApplyRequestDTO, ApplyResultDTO } from './api/bridge';
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

type View = 'library' | 'favorites' | 'history' | 'sources';

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
  const [settingsOpen, setSettingsOpen] = useState(false);
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

  const pendingApplyActionRef = useRef<ApplyRequestDTO | null>(null);

  const handleApplyAction = useCallback(async (request: ApplyRequestDTO) => {
    if (applyingRef.current) {
      pendingApplyActionRef.current = request;
      return;
    }
    applyingRef.current = true;
    setApplying(true);

    let currentRequest: ApplyRequestDTO | null = request;
    while (currentRequest !== null) {
      const req = currentRequest;
      currentRequest = null;
      setFeedbackWithAutoDismiss({ state: 'running', label: 'Applying wallpaper' });
      try {
        const r = await api.applyAction(req);
        if (r.success) {
          invalidateHistoryCache();
          let detail: ApplyResultDTO | undefined;
          try {
            detail = r.stdout ? JSON.parse(r.stdout) as ApplyResultDTO : undefined;
          } catch {
            detail = undefined;
          }
          setFeedbackWithAutoDismiss({
            state: 'success',
            label: 'Applied',
            detail: detail?.preview ? 'Preview wallpaper applied.' : detail?.appliedPath?.split('/').pop(),
          });
        } else {
          setFeedbackWithAutoDismiss(commandErrorFeedback('Apply', r));
        }
        await refreshStatus();
      } catch (e) {
        setFeedbackWithAutoDismiss(commandErrorFeedback('Apply', e));
      }
      const next = pendingApplyActionRef.current;
      pendingApplyActionRef.current = null;
      if (next && next.requestId !== req.requestId) {
        currentRequest = next;
      }
    }

    setApplying(false);
    applyingRef.current = false;
  }, [refreshStatus, setFeedbackWithAutoDismiss]);

  const handleApply = useCallback((path: string) => {
    handleApplyAction({
      kind: 'apply',
      path,
      requestId: `${Date.now()}-${Math.random().toString(36).slice(2)}`,
    });
  }, [handleApplyAction]);

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
        <Suspense fallback={<div className="settings-modal-overlay"><div className="settings-modal">Loading settings...</div></div>}>
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
