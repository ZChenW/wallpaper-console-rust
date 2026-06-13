import { useState, useEffect, useCallback, useRef } from 'react';
import {
  api,
  CommandResult,
  LibrarySourceStatusDTO,
  LinuxWallpaperEngineStatusDTO,
  ThumbnailCacheDTO,
  WeDebugInfoDTO,
} from '../api/bridge';
import { CommandFeedback, commandErrorFeedback, commandSuccessFeedback } from '../api/feedback';
import ConfirmDialog from '../components/ConfirmDialog';
import { useAppState } from '../state/AppStateContext';
import SettingsSidebar from '../settings/SettingsSidebar';
import { resolveImageBackendDisplay } from '../settings/imageBackendDisplay';
import {
  ALL_SETTINGS,
  SettingsCategory,
  normalizeConfigValue,
  cleanupDays,
} from '../settings/configSchema';
import GeneralPage from '../settings/pages/GeneralPage';
import WallpaperPage from '../settings/pages/WallpaperPage';
import WallpaperEnginePage from '../settings/pages/WallpaperEnginePage';
import LibraryPage from '../settings/pages/LibraryPage';
import DatabasePage from '../settings/pages/DatabasePage';
import AdvancedPage from '../settings/pages/AdvancedPage';
import type { DbAction } from '../settings/types';

interface Props {
  onRefresh: () => void;
  onFeedback: (fb: CommandFeedback) => void;
}

export default function SettingsView({ onRefresh: _onRefresh, onFeedback }: Props) {
  const { invalidateLibrary } = useAppState();
  const [activeCategory, setActiveCategory] = useState<SettingsCategory>('general');
  const [configs, setConfigs] = useState<Record<string, string>>({});
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState<string | null>(null);
  const [confirmAction, setConfirmAction] = useState<{ title: string; msg: string; fn: () => Promise<void> } | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [thumbCache, setThumbCache] = useState<ThumbnailCacheDTO | null>(null);
  const [dbAction, setDbAction] = useState<DbAction | null>(null);
  const [libraryStatus, setLibraryStatus] = useState<LibrarySourceStatusDTO | null>(null);
  const [weStatus, setWeStatus] = useState<LinuxWallpaperEngineStatusDTO | null>(null);
  const [weDebugInfo, setWeDebugInfo] = useState<WeDebugInfoDTO | null>(null);
  const [showRawConfig, setShowRawConfig] = useState(false);
  const [diagnosticsRunning, setDiagnosticsRunning] = useState(false);
  const [operationLock, setOperationLock] = useState(false);
  const restoreInputRef = useRef<HTMLInputElement>(null);

  const loadConfigs = useCallback(async () => {
    setLoading(true);
    const allKeys = ALL_SETTINGS.map((c) => c.key);
    const results = await Promise.allSettled(allKeys.map((key) => api.configGet(key)));
    const map: Record<string, string> = {};
    const ibIdx = allKeys.indexOf('image_backend');
    allKeys.forEach((key, i) => {
      const r = results[i];
      map[key] = r.status === 'fulfilled' ? r.value : '';
    });
    const imageBackendResult = results[ibIdx];
    const raw = imageBackendResult?.status === 'fulfilled' ? imageBackendResult.value : null;
    const resolved = resolveImageBackendDisplay(raw, imageBackendResult?.status === 'fulfilled');
    map['image_backend'] = resolved.display;
    if (resolved.shouldMigrate) {
      void api.configSet('image_backend', 'awww').catch(() => {});
    }
    setConfigs(map);
    setLoading(false);
  }, []);

  const loadThumbCache = useCallback(async () => {
    try { setThumbCache(await api.thumbnailCacheStatus()); } catch { /* */ }
  }, []);

  const loadWeStatus = useCallback(async () => {
    try { setWeStatus(await api.linuxWallpaperEngineStatus()); } catch { /* */ }
  }, []);

  const loadWeDebugInfo = useCallback(async () => {
    try { setWeDebugInfo(await api.weDebugInfo()); } catch { /* */ }
  }, []);

  useEffect(() => {
    loadConfigs();
    loadThumbCache();
    loadWeStatus();
    loadWeDebugInfo();
    void api.librarySourceStatus().then(setLibraryStatus);
  }, [loadConfigs, loadThumbCache, loadWeStatus, loadWeDebugInfo]);

  const handleSet = async (key: string, value: string): Promise<boolean> => {
    const normalized = normalizeConfigValue(key, value);
    setSaving(key);
    try {
      const r = await api.configSet(key, normalized);
      if (r.success) {
        setConfigs((prev) => ({ ...prev, [key]: normalized }));
        window.dispatchEvent(new CustomEvent('wc-config-changed', { detail: { key, value: normalized } }));
        if (key.startsWith('linux_wallpaperengine_')) {
          void loadWeStatus();
        }
        onFeedback({ state: 'success', label: 'Setting saved', detail: key });
        return true;
      } else {
        onFeedback(commandErrorFeedback('Setting', r));
        return false;
      }
    } catch (e) {
      onFeedback(commandErrorFeedback('Setting', e));
      return false;
    } finally {
      setSaving(null);
    }
  };

  const runDbAction = async (action: DbAction, label: string, fn: () => Promise<CommandResult>) => {
    if (operationLock) return;
    setOperationLock(true);
    setConfirming(true);
    setDbAction(action);
    onFeedback({ state: 'running', label });
    try {
      const r = await fn();
      if (r.success) {
        if (r.stdout && r.stdout.includes('WITH WARNINGS')) {
          onFeedback({ state: 'warning', label: `${label} complete`, detail: r.stdout });
        } else {
          onFeedback(commandSuccessFeedback(label, r));
        }
      } else {
        onFeedback(commandErrorFeedback(label, r));
      }
    } catch (e) {
      onFeedback(commandErrorFeedback(label, e));
    } finally {
      setDbAction(null);
      setConfirming(false);
      setConfirmAction(null);
      setOperationLock(false);
    }
  };

  const confirmAndRun = useCallback(
    (title: string, msg: string, fn: () => Promise<void>, danger?: boolean, action?: DbAction) => {
      if (operationLock) return;
      setConfirmAction({
        title,
        msg: danger ? `Warning: ${msg}` : msg,
        fn: async () => {
          setOperationLock(true);
          setDbAction(action ?? null);
          setConfirming(true);
          try { await fn(); } finally {
            setDbAction(null);
            setConfirming(false);
            setConfirmAction(null);
            setOperationLock(false);
          }
        },
      });
    },
    [operationLock],
  );

  const handleCleanupThumbnails = useCallback(async () => {
    if (operationLock) return;
    setOperationLock(true);
    const days = cleanupDays(configs, thumbCache);
    onFeedback({ state: 'running', label: 'Cleanup' });
    try {
      const r = await api.thumbnailCacheCleanupOld(days);
      if (r.success) onFeedback(commandSuccessFeedback('Cleanup', r));
      else onFeedback(commandErrorFeedback('Cleanup', r));
      loadThumbCache();
    } catch (e) {
      onFeedback(commandErrorFeedback('Cleanup', e));
    } finally {
      setOperationLock(false);
    }
  }, [configs, thumbCache, onFeedback, loadThumbCache, operationLock]);

  const runDiagnosticsExport = useCallback(async () => {
    if (operationLock) return;
    setOperationLock(true);
    setDiagnosticsRunning(true);
    onFeedback({ state: 'running', label: 'Exporting diagnostics' });
    try {
      const r = await api.exportDiagnostics();
      if (r.success) {
        onFeedback({ state: 'success', label: 'Diagnostics exported', detail: r.stdout });
      } else {
        onFeedback(commandErrorFeedback('Export diagnostics', r));
      }
    } catch (e) {
      onFeedback(commandErrorFeedback('Export diagnostics', e));
    } finally {
      setDiagnosticsRunning(false);
      setOperationLock(false);
    }
  }, [operationLock, onFeedback]);

  const handleRestore = () => {
    if (operationLock) return;
    restoreInputRef.current?.click();
  };

  const handleRestoreFileSelected = async (e: React.ChangeEvent<HTMLInputElement>) => {
    if (operationLock) return;
    const file = e.target.files?.[0];
    if (!file) return;
    const path = (file as unknown as { path?: string }).path ?? file.name;
    setConfirmAction({
      title: 'Restore Database',
      msg: `Restore wallpapers.db from:\n${path}\n\nCurrent database will be backed up first.`,
      fn: async () => {
        setOperationLock(true);
        setConfirming(true);
        onFeedback({ state: 'running', label: 'Restore' });
        try {
          const r = await api.sqliteRestore(path);
          if (r.success) {
            onFeedback(commandSuccessFeedback('Restore', r));
          } else {
            onFeedback(commandErrorFeedback('Restore', r));
          }
        } catch (e) {
          onFeedback(commandErrorFeedback('Restore', e));
        } finally {
          setConfirming(false);
          setConfirmAction(null);
          setOperationLock(false);
        }
      },
    });
    e.target.value = '';
  };

  const PAGE_MAP: Record<SettingsCategory, () => React.ReactNode> = {
    general: () => (
      <GeneralPage
        libraryStatus={libraryStatus}
        weStatus={weStatus}
        thumbCache={thumbCache}
      />
    ),
    wallpaper: () => (
      <WallpaperPage configs={configs} saving={saving} onSet={handleSet} />
    ),
    we: () => (
      <WallpaperEnginePage weStatus={weStatus} configs={configs} saving={saving} onSet={handleSet} />
    ),
    library: () => (
      <LibraryPage
        configs={configs}
        saving={saving}
        onSet={handleSet}
        thumbCache={thumbCache}
        onFeedback={onFeedback}
        handleCleanupThumbnails={handleCleanupThumbnails}
        loadThumbCache={loadThumbCache}
        confirmAndRun={confirmAndRun}
        operationLock={operationLock}
      />
    ),
    database: () => (
      <DatabasePage
        libraryStatus={libraryStatus}
        dbAction={dbAction}
        operationLock={operationLock}
        runDbAction={runDbAction}
        onFeedback={onFeedback}
        confirmAndRun={confirmAndRun}
        onRestore={handleRestore}
        restoreInputRef={restoreInputRef}
        onRestoreFileSelected={handleRestoreFileSelected}
        invalidateLibrary={invalidateLibrary}
        diagnosticsRunning={diagnosticsRunning}
        runDiagnosticsExport={runDiagnosticsExport}
      />
    ),
    advanced: () => (
      <AdvancedPage
        configs={configs}
        saving={saving}
        onSet={handleSet}
        weDebugInfo={weDebugInfo}
        showRawConfig={showRawConfig}
        setShowRawConfig={setShowRawConfig}
      />
    ),
  };

  if (loading) {
    return (
      <div className="view settings-view">
        <h2>Settings</h2>
        <div className="loading">Loading...</div>
      </div>
    );
  }

  return (
    <div className="view settings-view">
      <h2>Settings</h2>
      <div className="settings-layout">
        <SettingsSidebar active={activeCategory} onChange={setActiveCategory} />

        <div className="settings-content">
          {PAGE_MAP[activeCategory]?.()}
        </div>
      </div>

      {confirmAction && (
        <ConfirmDialog
          title={confirmAction.title}
          message={confirmAction.msg}
          onConfirm={confirmAction.fn}
          onCancel={() => { setConfirmAction(null); setConfirming(false); }}
          danger={confirmAction.title.includes('Clear') || confirmAction.title.includes('Export') || confirmAction.title.includes('Restore') || confirmAction.title.includes('Rebuild') || confirmAction.title.includes('Stop')}
          confirming={confirming}
        />
      )}
    </div>
  );
}
