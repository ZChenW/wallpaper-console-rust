import { useState, useEffect, useCallback, useRef, useLayoutEffect } from 'react';
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
import { emitConfigChanged } from '../events/appEvents';
import { refreshSettingsStatusCore, createSettingsStatusRequestSeq } from '../settings/refreshSettingsStatusCore';
import {
  resetSettingsContentScroll,
  scheduleSettingsContentScrollReset,
} from '../settings/settingsScroll';

const SETTINGS_STATUS_POLL_MS = 3000;
const STATUS_POLL_CATEGORIES = new Set<SettingsCategory>(['general', 'database', 'library', 'we']);

interface Props {
  onRefresh: () => void | Promise<void>;
  onFeedback: (fb: CommandFeedback) => void;
  onClose: () => void;
}

let cachedSettingsConfigs: Record<string, string> | null = null;

function defaultSettingsConfig(): Record<string, string> {
  const out: Record<string, string> = {};
  for (const setting of ALL_SETTINGS) {
    if (setting.options?.length) {
      out[setting.key] = setting.options[0];
    } else {
      out[setting.key] = setting.placeholder ?? '';
    }
  }
  out.image_backend = 'awww';
  out.gif_backend = 'awww';
  out.video_backend = 'mpvpaper';
  out.gui_theme = 'light';
  out.awww_transition_type = 'fade';
  out.awww_transition_duration = '1';
  out.wallpaper_transition_fps = '60';
  out.mpvpaper_output = '*';
  out.mpvpaper_options = '--loop-file=inf --panscan=1.0';
  return out;
}

export default function SettingsView({ onRefresh, onFeedback, onClose }: Props) {
  const { invalidateLibrary } = useAppState();
  const [activeCategory, setActiveCategory] = useState<SettingsCategory>('general');
  const [configs, setConfigs] = useState<Record<string, string>>(
    () => cachedSettingsConfigs ?? defaultSettingsConfig(),
  );
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState<string | null>(null);
  const [confirmAction, setConfirmAction] = useState<{ title: string; msg: string; fn: () => Promise<void> } | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [thumbCache, setThumbCache] = useState<ThumbnailCacheDTO | null>(null);
  const [thumbCacheError, setThumbCacheError] = useState<string | null>(null);
  const [thumbCacheLoading, setThumbCacheLoading] = useState(true);
  const [dbAction, setDbAction] = useState<DbAction | null>(null);
  const [libraryStatus, setLibraryStatus] = useState<LibrarySourceStatusDTO | null>(null);
  const [libraryStatusError, setLibraryStatusError] = useState<string | null>(null);
  const [libraryStatusLoading, setLibraryStatusLoading] = useState(true);
  const [weStatus, setWeStatus] = useState<LinuxWallpaperEngineStatusDTO | null>(null);
  const [weStatusError, setWeStatusError] = useState<string | null>(null);
  const [weStatusLoading, setWeStatusLoading] = useState(true);
  const [weDebugInfo, setWeDebugInfo] = useState<WeDebugInfoDTO | null>(null);
  const [weDebugError, setWeDebugError] = useState<string | null>(null);
  const [showRawConfig, setShowRawConfig] = useState(false);
  const [diagnosticsRunning, setDiagnosticsRunning] = useState(false);
  const [operationLock, setOperationLock] = useState(false);
  const restoreInputRef = useRef<HTMLInputElement>(null);
  const confirmActionRef = useRef(confirmAction);
  confirmActionRef.current = confirmAction;
  const dirtyKeysRef = useRef<Set<string>>(new Set());
  const libraryStatusRef = useRef(libraryStatus);
  const libraryStatusErrorRef = useRef(libraryStatusError);
  const weStatusRef = useRef(weStatus);
  const weStatusErrorRef = useRef(weStatusError);
  const thumbCacheRef = useRef(thumbCache);
  const thumbCacheErrorRef = useRef(thumbCacheError);
  libraryStatusRef.current = libraryStatus;
  libraryStatusErrorRef.current = libraryStatusError;
  weStatusRef.current = weStatus;
  weStatusErrorRef.current = weStatusError;
  thumbCacheRef.current = thumbCache;
  thumbCacheErrorRef.current = thumbCacheError;
  const statusRequestSeqRef = useRef(createSettingsStatusRequestSeq());
  const settingsContentRef = useRef<HTMLDivElement>(null);
  const activeCategoryRef = useRef(activeCategory);
  activeCategoryRef.current = activeCategory;
  const settingsOpenedRef = useRef(false);

  const applySettingsStatusSnapshot = useCallback((snapshot: Awaited<ReturnType<typeof refreshSettingsStatusCore>>) => {
    if (snapshot.loaded.library) {
      setLibraryStatus(snapshot.libraryStatus);
      setLibraryStatusError(snapshot.libraryError);
    }
    if (snapshot.loaded.we) {
      setWeStatus(snapshot.weStatus);
      setWeStatusError(snapshot.weError);
    }
    if (snapshot.loaded.thumb) {
      setThumbCache(snapshot.thumbCache);
      setThumbCacheError(snapshot.thumbError);
    }
    if (snapshot.loaded.debug) {
      setWeDebugInfo(snapshot.weDebugInfo);
      setWeDebugError(snapshot.weDebugError);
    }
  }, []);

  const refreshSettingsStatus = useCallback(async (reason?: string) => {
    const category = activeCategoryRef.current;
    const requestId = statusRequestSeqRef.current.begin();

    const loaders = {
      ...(category === 'general' || category === 'database'
        ? { librarySourceStatus: () => api.librarySourceStatus() }
        : {}),
      ...(category === 'general' || category === 'we'
        ? { linuxWallpaperEngineStatus: () => api.linuxWallpaperEngineStatus() }
        : {}),
      ...(category === 'general' || category === 'library'
        ? { thumbnailCacheStatus: () => api.thumbnailCacheStatus() }
        : {}),
      ...(category === 'we' || category === 'advanced'
        ? { weDebugInfo: () => api.weDebugInfo() }
        : {}),
    };

    const willLoadLibrary = loaders.librarySourceStatus !== undefined;
    const willLoadWe = loaders.linuxWallpaperEngineStatus !== undefined;
    const willLoadThumb = loaders.thumbnailCacheStatus !== undefined;

    if (willLoadLibrary) {
      setLibraryStatusLoading(
        libraryStatusRef.current === null && libraryStatusErrorRef.current === null,
      );
    }
    if (willLoadWe) {
      setWeStatusLoading(weStatusRef.current === null && weStatusErrorRef.current === null);
    }
    if (willLoadThumb) {
      setThumbCacheLoading(thumbCacheRef.current === null && thumbCacheErrorRef.current === null);
    }

    const snapshot = await refreshSettingsStatusCore(loaders);

    if (!statusRequestSeqRef.current.isLatest(requestId)) return;

    applySettingsStatusSnapshot(snapshot);
    if (willLoadLibrary) setLibraryStatusLoading(false);
    if (willLoadWe) setWeStatusLoading(false);
    if (willLoadThumb) setThumbCacheLoading(false);
    if (reason !== 'poll' && reason !== 'category') {
      void Promise.resolve(onRefresh()).catch(() => {});
    }
  }, [applySettingsStatusSnapshot, onRefresh]);

  const loadConfigs = useCallback(async () => {
    if (!cachedSettingsConfigs) setLoading(true);
    const allKeys = ALL_SETTINGS.map((c) => c.key);
    let values: Record<string, string> = {};

    try {
      values = await api.configGetMany(allKeys);
    } catch {
      const results = await Promise.allSettled(allKeys.map((key) => api.configGet(key)));
      allKeys.forEach((key, i) => {
        const r = results[i];
        values[key] = r.status === 'fulfilled' ? r.value : '';
      });
    }

    const raw = values['image_backend'] ?? null;
    const resolved = resolveImageBackendDisplay(raw, true);
    values['image_backend'] = resolved.display;

    if (resolved.shouldMigrate) {
      void api.configSet('image_backend', 'awww').catch(() => {});
    }

    if (values['gui_theme'] === 'current') {
      values['gui_theme'] = 'light';
      void api.configSet('gui_theme', 'light').catch(() => {});
    }

    setConfigs((prev) => {
      const merged = { ...values };
      for (const key of dirtyKeysRef.current) {
        if (prev[key] !== undefined) merged[key] = prev[key];
      }
      cachedSettingsConfigs = merged;
      return merged;
    });
    dirtyKeysRef.current.clear();
    setLoading(false);
  }, []);

  useEffect(() => {
    void loadConfigs();
  }, [loadConfigs]);

  useEffect(() => {
    if (!settingsOpenedRef.current) {
      settingsOpenedRef.current = true;
      void refreshSettingsStatus('open');
      return;
    }
    void refreshSettingsStatus('category');
  }, [activeCategory, refreshSettingsStatus]);

  useEffect(() => {
    if (!STATUS_POLL_CATEGORIES.has(activeCategory)) return;
    const id = window.setInterval(() => {
      void refreshSettingsStatus('poll');
    }, SETTINGS_STATUS_POLL_MS);
    return () => window.clearInterval(id);
  }, [activeCategory, refreshSettingsStatus]);

  // Esc key handler — respect confirmAction to avoid closing Settings behind ConfirmDialog
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        if (confirmActionRef.current) return;
        onClose();
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [onClose]);

  useLayoutEffect(() => {
    resetSettingsContentScroll(settingsContentRef.current);
  }, [activeCategory]);

  const handleAdvancedCollapse = useCallback(() => {
    scheduleSettingsContentScrollReset(settingsContentRef.current);
  }, []);

  const handleSet = async (key: string, value: string): Promise<boolean> => {
    const normalized = normalizeConfigValue(key, value);
    setSaving(key);
    try {
      const r = await api.configSet(key, normalized);
      if (r.success) {
        dirtyKeysRef.current.add(key);
        setConfigs((prev) => {
          const next = { ...prev, [key]: normalized };
          cachedSettingsConfigs = next;
          return next;
        });
        emitConfigChanged({ key, value: normalized });
        if (key.startsWith('linux_wallpaperengine_')) {
          void refreshSettingsStatus('we-config');
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
      void refreshSettingsStatus('db-action');
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
    } catch (e) {
      onFeedback(commandErrorFeedback('Cleanup', e));
    } finally {
      setOperationLock(false);
      void refreshSettingsStatus('thumbnail-cleanup');
    }
  }, [configs, thumbCache, onFeedback, refreshSettingsStatus, operationLock]);

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
          void refreshSettingsStatus('db-restore');
        }
      },
    });
    e.target.value = '';
  };

  const PAGE_MAP: Record<SettingsCategory, () => React.ReactNode> = {
    general: () => (
      <GeneralPage
        libraryStatus={libraryStatus}
        libraryStatusError={libraryStatusError}
        libraryStatusLoading={libraryStatusLoading}
        weStatus={weStatus}
        weStatusError={weStatusError}
        weStatusLoading={weStatusLoading}
        thumbCache={thumbCache}
        thumbCacheError={thumbCacheError}
        thumbCacheLoading={thumbCacheLoading}
        configs={configs}
        saving={saving}
        onSet={handleSet}
      />
    ),
    wallpaper: () => (
      <WallpaperPage
        configs={configs}
        saving={saving}
        onSet={handleSet}
        onAdvancedCollapse={handleAdvancedCollapse}
      />
    ),
    we: () => (
      <WallpaperEnginePage
        weStatus={weStatus}
        weStatusError={weStatusError}
        weStatusLoading={weStatusLoading}
        configs={configs}
        saving={saving}
        onSet={handleSet}
        onAdvancedCollapse={handleAdvancedCollapse}
      />
    ),
    library: () => (
      <LibraryPage
        configs={configs}
        saving={saving}
        onSet={handleSet}
        thumbCache={thumbCache}
        thumbCacheError={thumbCacheError}
        thumbCacheLoading={thumbCacheLoading}
        onFeedback={onFeedback}
        handleCleanupThumbnails={handleCleanupThumbnails}
        refreshSettingsStatus={refreshSettingsStatus}
        confirmAndRun={confirmAndRun}
        operationLock={operationLock}
        onAdvancedCollapse={handleAdvancedCollapse}
      />
    ),
    database: () => (
      <DatabasePage
        libraryStatus={libraryStatus}
        libraryStatusError={libraryStatusError}
        libraryStatusLoading={libraryStatusLoading}
        dbAction={dbAction}
        operationLock={operationLock}
        runDbAction={runDbAction}
        onFeedback={onFeedback}
        confirmAndRun={confirmAndRun}
        onRestore={handleRestore}
        restoreInputRef={restoreInputRef}
        onRestoreFileSelected={handleRestoreFileSelected}
        invalidateLibrary={invalidateLibrary}
        refreshSettingsStatus={refreshSettingsStatus}
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
        weDebugError={weDebugError}
        showRawConfig={showRawConfig}
        setShowRawConfig={setShowRawConfig}
      />
    ),
  };

  return (
    <div className="settings-modal-overlay" onMouseDown={onClose}>
      <section
        className="settings-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="settings-title"
        onMouseDown={(e) => e.stopPropagation()}
      >
        <header className="settings-modal-header">
          <div>
            <h2 id="settings-title">Settings</h2>
            <p>{loading ? 'Refreshing settings...' : 'Configure Wallpaper Console behavior.'}</p>
          </div>
          <button className="icon-btn" aria-label="Close settings" onClick={onClose}>
            <svg width="18" height="18" viewBox="0 0 18 18" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
              <path d="M4 4l10 10M14 4l-10 10" />
            </svg>
          </button>
        </header>

        <div className="settings-layout">
          <SettingsSidebar active={activeCategory} onChange={setActiveCategory} />

          <div className="settings-content" ref={settingsContentRef}>
            {PAGE_MAP[activeCategory]?.()}
          </div>
        </div>
      </section>

      {confirmAction && (
        <div onMouseDown={(e) => e.stopPropagation()}>
          <ConfirmDialog
            title={confirmAction.title}
            message={confirmAction.msg}
            onConfirm={confirmAction.fn}
            onCancel={() => { setConfirmAction(null); setConfirming(false); }}
            danger={confirmAction.title.includes('Clear') || confirmAction.title.includes('Export') || confirmAction.title.includes('Restore') || confirmAction.title.includes('Rebuild') || confirmAction.title.includes('Stop')}
            confirming={confirming}
          />
        </div>
      )}
    </div>
  );
}
