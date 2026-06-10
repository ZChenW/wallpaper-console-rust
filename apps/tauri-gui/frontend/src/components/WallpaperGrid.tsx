import { useState, useRef, useEffect, useMemo } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { convertFileSrc } from '@tauri-apps/api/core';
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

const COL_MIN_WIDTH = 180;
const CARD_HEIGHT = 176;
const OVERSCAN = 2;

export default function WallpaperGrid({
  entries,
  onApply,
  applying,
  emptyText = 'No wallpapers found',
  contextActions = [],
}: Props) {
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number; path: string } | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const { thumbs: thumbCache, enqueue, reset } = useThumbnailQueue(2);

  const [colCount, setColCount] = useState(4);
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const obs = new ResizeObserver(([entry]) => {
      const w = entry.contentRect.width;
      setColCount(Math.max(1, Math.floor(w / COL_MIN_WIDTH)));
    });
    obs.observe(el);
    return () => obs.disconnect();
  }, []);

  const rowCount = Math.ceil(entries.length / colCount);

  const virtualizer = useVirtualizer({
    count: rowCount,
    getScrollElement: () => containerRef.current,
    estimateSize: () => CARD_HEIGHT,
    overscan: OVERSCAN,
  });

  useEffect(() => {
    reset();
    virtualizer.scrollToIndex(0);
  }, [entries]);

  useEffect(() => {
    const range = virtualizer.range;
    if (!range) return;
    const startIdx = range.startIndex * colCount;
    const endIdx = Math.min((range.endIndex + 1) * colCount, entries.length);
    enqueue(entries.slice(startIdx, endIdx).map((e) => e.path));
  }, [entries, colCount, virtualizer.range, enqueue]);

  const handleContextMenu = (e: React.MouseEvent, path: string) => {
    e.preventDefault();
    setContextMenu({ x: e.clientX, y: e.clientY, path });
  };

  const handleDoubleClick = (path: string) => {
    onApply(path);
  };

  const rows = useMemo(() => {
    const r: WallpaperDTO[][] = [];
    for (let i = 0; i < entries.length; i += colCount) {
      r.push(entries.slice(i, i + colCount));
    }
    return r;
  }, [entries, colCount]);

  if (entries.length === 0) {
    return <div className="empty-state">{emptyText}</div>;
  }

  return (
    <div className="wallpaper-grid" ref={containerRef}>
      <div style={{ height: virtualizer.getTotalSize(), position: 'relative' }}>
        {virtualizer.getVirtualItems().map((virtualRow) => {
          const rowEntries = rows[virtualRow.index] ?? [];
          return (
            <div
              key={virtualRow.key}
              style={{
                position: 'absolute',
                top: 0,
                left: 0,
                width: '100%',
                height: virtualRow.size,
                transform: `translateY(${virtualRow.start}px)`,
                display: 'grid',
                gridTemplateColumns: `repeat(${colCount}, 1fr)`,
                gap: '10px',
              }}
            >
              {rowEntries.map((e) => (
                <div
                  key={e.path}
                  className={`wallpaper-card ${applying ? 'disabled' : ''}`}
                  onContextMenu={(ev) => handleContextMenu(ev, e.path)}
                  onDoubleClick={() => handleDoubleClick(e.path)}
                  title={e.path}
                >
                  <div className="wallpaper-thumb">
                    {thumbCache[e.path] ? (
                      <img src={convertFileSrc(thumbCache[e.path])} alt="" loading="lazy" />
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
            </div>
          );
        })}
      </div>

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
    case 'image': return '\u{1F5BC}';
    case 'gif': return '\u{1F39E}';
    case 'video': return '\u{1F3AC}';
    default: return '\u{1F4C4}';
  }
}

function formatSize(bytes: number): string {
  if (bytes >= 1 << 30) return `${(bytes / (1 << 30)).toFixed(1)} GB`;
  if (bytes >= 1 << 20) return `${(bytes / (1 << 20)).toFixed(1)} MB`;
  if (bytes >= 1 << 10) return `${(bytes / (1 << 10)).toFixed(0)} KB`;
  return `${bytes} B`;
}
