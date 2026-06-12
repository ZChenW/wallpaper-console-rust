import { useState, useRef, useEffect, useMemo, useCallback } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { convertFileSrc } from '@tauri-apps/api/core';
import { WallpaperDTO } from '../api/bridge';
import ContextMenu from './ContextMenu';
import { useThumbnailStore } from '../state/ThumbnailStoreContext';
import { calculateColumnCount, COL_MIN_WIDTH, GRID_GAP } from '../utils/layout';

interface Props {
  entries: WallpaperDTO[];
  onApply: (path: string) => void;
  applying: boolean;
  emptyText?: string;
  contextActions?: ContextAction[];
  active?: boolean;
}

export interface ContextAction {
  label: string;
  action: (path: string) => void;
  danger?: boolean;
  visible?: (path: string) => boolean;
}

const CARD_HEIGHT = 188;
const OVERSCAN = 2;

export default function WallpaperGrid({
  entries,
  onApply,
  applying,
  emptyText = 'No wallpapers found',
  contextActions = [],
  active = true,
}: Props) {
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number; path: string } | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const { thumbs: thumbCache, enqueue } = useThumbnailStore();

  const prevEntriesRef = useRef(entries);
  const [colCount, setColCount] = useState(4);

  const remeasure = useCallback(() => {
    const el = containerRef.current;
    if (!el) return;
    const w = el.clientWidth;
    if (w > 0) {
      setColCount(calculateColumnCount(w));
    }
  }, []);

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const obs = new ResizeObserver(([entry]) => {
      if (!active) return;
      const w = entry.contentRect.width;
      setColCount(calculateColumnCount(w));
    });
    obs.observe(el);
    return () => obs.disconnect();
  }, [active]);

  useEffect(() => {
    if (!active) return;
    remeasure();
  }, [active, remeasure]);

  const rowCount = Math.ceil(entries.length / colCount);

  const virtualizer = useVirtualizer({
    count: rowCount,
    getScrollElement: () => containerRef.current,
    estimateSize: () => CARD_HEIGHT,
    overscan: OVERSCAN,
  });

  useEffect(() => {
    if (entries !== prevEntriesRef.current) {
      prevEntriesRef.current = entries;
      if (active) {
        virtualizer.scrollToIndex(0);
      }
    }
  }, [entries, active, virtualizer]);

  useEffect(() => {
    if (!active) return;
    const range = virtualizer.range;
    if (!range) return;
    const startIdx = range.startIndex * colCount;
    const endIdx = Math.min((range.endIndex + 1) * colCount, entries.length);
    enqueue(
      entries
        .slice(startIdx, endIdx)
        .filter((e) => !e.previewPath)
        .map((e) => e.path),
      { priority: 'front' },
    );
  }, [entries, colCount, virtualizer.range, enqueue, active]);

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
                gridTemplateColumns: `repeat(${colCount}, minmax(0, 1fr))`,
                gap: `${GRID_GAP}px`,
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
                    {e.previewPath ? (
                      <img src={safeFileSrc(e.previewPath)} alt="" loading="lazy" />
                    ) : thumbCache[e.path] ? (
                      <img src={safeFileSrc(thumbCache[e.path])} alt="" loading="lazy" />
                    ) : (
                      <div className="wallpaper-thumb-placeholder">
                        <span className="wallpaper-type-icon">{typeIcon(e.type)}</span>
                      </div>
                    )}
                    {weBadge(e) && <span className={weBadgeClass(e)}>{weBadge(e)}</span>}
                  </div>
                  <div className="wallpaper-info">
                    <span className="wallpaper-name">{displayName(e)}</span>
                    <span className="wallpaper-meta">{metaLine(e)}</span>
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
          actions={contextActions.filter((action) => !action.visible || action.visible(contextMenu.path))}
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
    case 'we_scene': return 'WE';
    case 'we_web': return 'WEB';
    default: return '\u{1F4C4}';
  }
}

function safeFileSrc(path: string): string {
  try {
    return convertFileSrc(path);
  } catch {
    return path;
  }
}

function displayName(e: WallpaperDTO): string {
  return e.title || e.workshopId || e.path.split('/').pop() || e.path;
}

function weBadge(e: WallpaperDTO): string | null {
  if (e.type === 'we_scene') {
    if (e.backendStatus === 'failed') return 'Scene incompatible';
    return 'WE Scene';
  }
  if (e.type === 'we_web') return 'WE Web';
  if (e.type === 'unsupported') return 'Unsupported';
  return null;
}

function weBadgeClass(e: WallpaperDTO): string {
  if (e.backendStatus === 'failed') return 'wallpaper-badge wallpaper-badge-danger';
  return 'wallpaper-badge';
}

function metaLine(e: WallpaperDTO): string {
  if (e.type === 'we_scene' || e.type === 'we_web' || e.type === 'unsupported') {
    if (e.type === 'unsupported' && e.unsupportedReason) {
      return e.unsupportedReason;
    }
    if (e.type === 'we_web') {
      return [e.backend === 'chromium-web' ? 'Chromium Web backend' : 'Web wallpaper', e.workshopId].filter(Boolean).join(' · ');
    }
    if (e.type === 'we_scene' && e.backendStatus === 'failed') {
      return e.backendErrorMessage || 'This scene is not compatible with linux-wallpaperengine.';
    }
    const kind = 'Wallpaper Engine Scene';
    return [kind, e.workshopId, e.backend].filter(Boolean).join(' · ');
  }
  return `${e.resolution} · ${e.type} · ${formatSize(e.size)}`;
}

function formatSize(bytes: number): string {
  if (bytes >= 1 << 30) return `${(bytes / (1 << 30)).toFixed(1)} GB`;
  if (bytes >= 1 << 20) return `${(bytes / (1 << 20)).toFixed(1)} MB`;
  if (bytes >= 1 << 10) return `${(bytes / (1 << 10)).toFixed(0)} KB`;
  return `${bytes} B`;
}
