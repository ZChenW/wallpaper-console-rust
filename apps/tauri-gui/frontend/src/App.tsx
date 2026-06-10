import { useState, useEffect, useCallback } from 'react';
import { api, StatusDTO } from './api/bridge';
import LibraryView from './views/LibraryView';
import FavoritesView from './views/FavoritesView';
import HistoryView from './views/HistoryView';
import SourcesView from './views/SourcesView';
import SettingsView from './views/SettingsView';
import StatusBar from './components/StatusBar';
import Toolbar from './components/Toolbar';
import Sidebar from './components/Sidebar';

type View = 'library' | 'favorites' | 'history' | 'sources' | 'settings';

export default function App() {
  const [view, setView] = useState<View>('library');
  const [status, setStatus] = useState<StatusDTO | null>(null);
  const [applying, setApplying] = useState(false);

  const refreshStatus = useCallback(async () => {
    try {
      const s = await api.status();
      setStatus(s);
    } catch {
      // Wails not connected yet — use fallback
    }
  }, []);

  useEffect(() => {
    refreshStatus();
    const interval = setInterval(refreshStatus, 5000);
    return () => clearInterval(interval);
  }, [refreshStatus]);

  const handleApply = useCallback(async (path: string) => {
    setApplying(true);
    try {
      await api.apply(path);
      await refreshStatus();
    } finally {
      setApplying(false);
    }
  }, [refreshStatus]);

  return (
    <div className="app">
      <Toolbar
        view={view}
        onRefresh={refreshStatus}
        applying={applying}
      />
      <div className="app-body">
        <Sidebar view={view} onNavigate={setView} />
        <main className="main-content">
          {view === 'library' && <LibraryView onApply={handleApply} applying={applying} />}
          {view === 'favorites' && <FavoritesView onApply={handleApply} applying={applying} />}
          {view === 'history' && <HistoryView onApply={handleApply} applying={applying} />}
          {view === 'sources' && <SourcesView onRefresh={refreshStatus} />}
          {view === 'settings' && <SettingsView onRefresh={refreshStatus} />}
        </main>
      </div>
      <StatusBar status={status} applying={applying} />
    </div>
  );
}
