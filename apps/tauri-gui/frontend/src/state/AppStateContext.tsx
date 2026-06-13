import { createContext, ReactNode, useCallback, useContext, useEffect, useRef, useState } from 'react';
import { api, ScanProgressDTO, StatusDTO } from '../api/bridge';
import type { CommandFeedback } from '../api/feedback';

interface AppStateValue {
  status: StatusDTO | null;
  feedback: CommandFeedback;
  scanProgress: ScanProgressDTO | null;
  libraryVersion: number;
  refreshStatus: () => Promise<void>;
  invalidateLibrary: () => void;
  setFeedbackWithAutoDismiss: (feedback: CommandFeedback | null) => void;
  clearFeedback: () => void;
  beginScanPolling: () => void;
  finishScanPolling: (delayMs?: number) => void;
  cancelScan: () => Promise<void>;
}

const AppStateContext = createContext<AppStateValue | null>(null);

export function AppStateProvider({ children }: { children: ReactNode }) {
  const [status, setStatus] = useState<StatusDTO | null>(null);
  const [feedback, setFeedback] = useState<CommandFeedback>({ state: 'idle' });
  const [scanProgress, setScanProgress] = useState<ScanProgressDTO | null>(null);
  const [libraryVersion, setLibraryVersion] = useState(0);
  const feedbackTimer = useRef<number | null>(null);
  const pollTimer = useRef<number | null>(null);

  const refreshStatus = useCallback(async () => {
    setStatus(await api.status());
  }, []);

  useEffect(() => {
    void refreshStatus();
    void api.scanProgress().then(setScanProgress).catch(() => {});
  }, [refreshStatus]);

  const clearFeedback = useCallback(() => {
    if (feedbackTimer.current != null) window.clearTimeout(feedbackTimer.current);
    feedbackTimer.current = null;
    setFeedback({ state: 'idle' });
  }, []);

  const setFeedbackWithAutoDismiss = useCallback((next: CommandFeedback | null) => {
    if (feedbackTimer.current != null) window.clearTimeout(feedbackTimer.current);
    setFeedback(next ?? { state: 'idle' });
    if (next && next.state !== 'running') {
      feedbackTimer.current = window.setTimeout(() => setFeedback({ state: 'idle' }), 4500);
    }
  }, []);

  const invalidateLibrary = useCallback(() => {
    setLibraryVersion((v) => v + 1);
  }, []);

  const stopPolling = useCallback(() => {
    if (pollTimer.current != null) {
      window.clearInterval(pollTimer.current);
      pollTimer.current = null;
    }
  }, []);

  const beginScanPolling = useCallback(() => {
    stopPolling();
    pollTimer.current = window.setInterval(() => {
      void api.scanProgress().then(setScanProgress).catch(() => {});
    }, 500);
  }, [stopPolling]);

  const finishScanPolling = useCallback((delayMs = 0) => {
    window.setTimeout(() => {
      void api.scanProgress().then(setScanProgress).catch(() => {});
      stopPolling();
      invalidateLibrary();
    }, delayMs);
  }, [invalidateLibrary, stopPolling]);

  const cancelScan = useCallback(async () => {
    await api.scanCancel();
    void api.scanProgress().then(setScanProgress).catch(() => {});
  }, []);

  useEffect(() => () => {
    stopPolling();
    if (feedbackTimer.current != null) window.clearTimeout(feedbackTimer.current);
  }, [stopPolling]);

  return (
    <AppStateContext.Provider value={{
      status,
      feedback,
      scanProgress,
      libraryVersion,
      refreshStatus,
      invalidateLibrary,
      setFeedbackWithAutoDismiss,
      clearFeedback,
      beginScanPolling,
      finishScanPolling,
      cancelScan,
    }}>
      {children}
    </AppStateContext.Provider>
  );
}

export function useAppState(): AppStateValue {
  const value = useContext(AppStateContext);
  if (!value) throw new Error('useAppState must be used inside AppStateProvider');
  return value;
}
