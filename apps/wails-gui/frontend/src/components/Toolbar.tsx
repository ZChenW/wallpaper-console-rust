import { RefreshCw, Play, Square, RotateCcw, Scan } from 'lucide-react';
import { api } from '../api/bridge';

interface Props {
  view: string;
  onRefresh: () => void;
  applying: boolean;
}

export default function Toolbar({ view, onRefresh, applying }: Props) {
  const handleRescan = async () => {
    await api.rescan();
    onRefresh();
  };

  const handleStop = async () => {
    await api.stop();
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
        <button className="toolbar-btn" onClick={handleStop} title="Stop all backends">
          <Square size={16} />
        </button>
      </div>
    </header>
  );
}
