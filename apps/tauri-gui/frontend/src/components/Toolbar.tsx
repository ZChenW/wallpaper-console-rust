import { useState } from 'react';
import { RefreshCw, Square, RotateCcw, Scan } from 'lucide-react';
import { api } from '../api/bridge';
import ConfirmDialog from './ConfirmDialog';

interface Props {
  view: string;
  onRefresh: () => void;
  applying: boolean;
}

export default function Toolbar({ view, onRefresh, applying }: Props) {
  const [showStop, setShowStop] = useState(false);

  const handleRescan = async () => {
    await api.rescan();
    onRefresh();
  };

  const handleStop = async () => {
    await api.stop();
    setShowStop(false);
    onRefresh();
  };

  const handleRestore = async () => {
    await api.restore();
    onRefresh();
  };

  return (
    <header className="toolbar">
      <div className="toolbar-left">
        <span className="toolbar-title">Wallpaper Console</span>
      </div>
      <div className="toolbar-actions">
        <button className="toolbar-btn" onClick={onRefresh} title="Refresh">
          <RefreshCw size={16} />
        </button>
        {view === 'library' && (
          <button className="toolbar-btn" onClick={handleRescan} title="Rescan library">
            <Scan size={16} />
          </button>
        )}
        <button className="toolbar-btn" onClick={handleRestore} title="Restore last wallpaper">
          <RotateCcw size={16} />
        </button>
        <button className="toolbar-btn danger" onClick={() => setShowStop(true)} title="Stop all backends">
          <Square size={16} />
        </button>
      </div>

      {showStop && (
        <ConfirmDialog
          title="Stop All Backends"
          message="This will stop all running wallpaper backends (awww/mpvpaper). Your current wallpaper will disappear until you apply a new one."
          onConfirm={handleStop}
          onCancel={() => setShowStop(false)}
          danger
        />
      )}
    </header>
  );
}
