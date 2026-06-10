import { useState, useCallback, useRef, useEffect } from 'react';
import { WallpaperDTO } from '../api/bridge';
import ContextMenu from './ContextMenu';
import { useThumbnailQueue } from '../hooks/useThumbnailQueue';

interface Props {
  entries: WallpaperDTO[];
  onApply: (path: string) => void;
  applying: boolean;
  emptyText?: string;
  contextActions?: ContextAction[];
}

export interface ContextAction {
  label: string;
  action: (path: string) => void;
  danger?: boolean;
}

const PAGE_SIZE = 36;

export default function WallpaperGrid({
  entries,
  onApply,
  applying,
  emptyText = 'No wallpapers found',
  contextActions = [],
}: Props) {
  const [visible, setVisible] = useState(PAGE_SIZE);
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number; path: string } | null>(null);
  const gridRef = useRef<HTMLDivElement>(null);
  const { thumbs: thumbCache, enqueue, reset } = useThumbnailQueue(2);

  // Reset visible count when entries change
  useEffect(() => {
    reset();
    setVisible(PAGE_SIZE);
  }, [entries, reset]);

  // Queue thumbnail work with bounded concurrency.
  useEffect(() => {
    enqueue(entries.slice(0, visible).map((entry) => entry.path));
  }, [entries, visible, enqueue]);

  const handleScroll = useCallback(() => {
    if (!gridRef.current) return;
    const { scrollTop, scrollHeight, clientHeight } = gridRef.current;
    if (scrollHeight - scrollTop - clientHeight < 200) {
      setVisible((v) => Math.min(v + PAGE_SIZE, entries.length));
    }
  }, [entries.length]);

  const handleContextMenu = (e: React.MouseEvent, path: string) => {
    e.preventDefault();
    setContextMenu({ x: e.clientX, y: e.clientY, path });
  };

  const handleDoubleClick = (path: string) => {
    onApply(path);
  };

  const visibleEntries = entries.slice(0, visible);

  if (entries.length === 0) {
    return <div className="empty-state">{emptyText}</div>;
  }

  return (
    <div className="wallpaper-grid" ref={gridRef} onScroll={handleScroll}>
      {visibleEntries.map((e) => (
        <div
          key={e.path}
          className={`wallpaper-card ${applying ? 'disabled' : ''}`}
          onContextMenu={(ev) => handleContextMenu(ev, e.path)}
          onDoubleClick={() => handleDoubleClick(e.path)}
          title={e.path}
        >
          <div className="wallpaper-thumb">
            {thumbCache[e.path] ? (
              <img src={`file://${thumbCache[e.path]}`} alt="" loading="lazy" />
            ) : (
              <div className="wallpaper-thumb-placeholder">
                <span className="wallpaper-type-icon">{typeIcon(e.type)}</span>
              </div>
            )}
          </div>
          <div className="wallpaper-info">
            <span className="wallpaper-name">{e.path.split('/').pop()}</span>
            <span className="wallpaper-meta">
              {e.resolution} · {e.type} · {formatSize(e.size)}
            </span>
          </div>
        </div>
      ))}
      {visible < entries.length && (
        <div className="load-more">
          <button onClick={() => setVisible((v) => v + PAGE_SIZE)}>
            Load more ({entries.length - visible} remaining)
          </button>
        </div>
      )}
      {contextMenu && (
        <ContextMenu
          x={contextMenu.x}
          y={contextMenu.y}
          path={contextMenu.path}
          onApply={onApply}
          actions={contextActions}
          onClose={() => setContextMenu(null)}
        />
      )}
    </div>
  );
}

function typeIcon(type: string): string {
  switch (type) {
    case 'image': return '🖼';
    case 'gif': return '🎞';
    case 'video': return '🎬';
    default: return '📄';
  }
}

function formatSize(bytes: number): string {
  if (bytes >= 1 << 30) return `${(bytes / (1 << 30)).toFixed(1)} GB`;
  if (bytes >= 1 << 20) return `${(bytes / (1 << 20)).toFixed(1)} MB`;
  if (bytes >= 1 << 10) return `${(bytes / (1 << 10)).toFixed(0)} KB`;
  return `${bytes} B`;
}
