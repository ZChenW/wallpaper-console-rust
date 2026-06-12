import { useState } from 'react';
import { RefreshCw, Square, RotateCcw, Scan, Loader, XOctagon } from 'lucide-react';
import { api, CommandResult } from '../api/bridge';
import ConfirmDialog from './ConfirmDialog';
import { useAppState } from '../state/AppStateContext';

interface Props {
  view: string;
  onAction: (action: () => Promise<CommandResult | void>, label: string) => Promise<void>;
  applying: boolean;
}

export default function Toolbar({ view, onAction, applying }: Props) {
  const [running, setRunning] = useState<null | 'refresh' | 'rescan' | 'restore' | 'stop'>(null);
  const [showStop, setShowStop] = useState(false);
  const { scanProgress, beginScanPolling, finishScanPolling, cancelScan } = useAppState();

  const wrap = async (key: 'refresh' | 'rescan' | 'restore' | 'stop', fn: () => Promise<CommandResult | void>) => {
    setRunning(key);
    try {
      if (key === 'rescan') {
        beginScanPolling();
      }
      await onAction(fn, key.charAt(0).toUpperCase() + key.slice(1));
    } finally {
      if (key === 'rescan') {
        finishScanPolling(1000);
      }
      setRunning(null);
    }
  };

  const handleCancel = async () => {
    await cancelScan();
  };

  const isRunning = (key: string) => running === key;
  const anyRunning = running !== null;

  return (
    <header className="toolbar">
      <div className="toolbar-left">
        <span className="toolbar-title">Wallpaper Console</span>
        {scanProgress?.running && (
          <span className="statusbar-badge" style={{ marginLeft: 12 }}>
            {scanProgress.stage} {scanProgress.scanned}{scanProgress.totalHint ? `/${scanProgress.totalHint}` : ''}
          </span>
        )}
      </div>
      <div className="toolbar-actions">
        <button
          className={`toolbar-btn ${isRunning('refresh') ? 'running' : ''}`}
          onClick={() => wrap('refresh', async () => { await api.status(); })}
          disabled={anyRunning}
          title="Refresh"
        >
          {isRunning('refresh') ? <Loader size={16} className="spin" /> : <RefreshCw size={16} />}
        </button>
        {view === 'library' && (
          <>
            <button
              className={`toolbar-btn ${isRunning('rescan') ? 'running' : ''}`}
              onClick={() => wrap('rescan', () => api.rescan())}
              disabled={anyRunning}
              title="Rescan library"
            >
              {isRunning('rescan') ? <Loader size={16} className="spin" /> : <Scan size={16} />}
            </button>
            {scanProgress?.running && (
              <button className="toolbar-btn danger" onClick={handleCancel} title="Cancel scan">
                <XOctagon size={16} />
              </button>
            )}
          </>
        )}
        <button
          className={`toolbar-btn ${isRunning('restore') ? 'running' : ''}`}
          onClick={() => wrap('restore', api.restore)}
          disabled={anyRunning}
          title="Restore last wallpaper"
        >
          {isRunning('restore') ? <Loader size={16} className="spin" /> : <RotateCcw size={16} />}
        </button>
        <button
          className={`toolbar-btn danger ${isRunning('stop') ? 'running' : ''}`}
          onClick={() => setShowStop(true)}
          disabled={anyRunning}
          title="Stop all backends"
        >
          {isRunning('stop') ? <Loader size={16} className="spin" /> : <Square size={16} />}
        </button>
      </div>
      {showStop && (
        <ConfirmDialog
          title="Stop Backends"
          message="Stop all wallpaper backends? Your wallpaper will disappear until you apply or restore."
          onConfirm={async () => { setShowStop(false); await wrap('stop', api.stop); }}
          onCancel={() => setShowStop(false)}
          danger
          confirming={isRunning('stop')}
          confirmLabel="Stopping..."
        />
      )}
    </header>
  );
}
