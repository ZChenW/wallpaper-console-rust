import { useState, useRef, useEffect, useMemo, useCallback } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { convertFileSrc } from '@tauri-apps/api/core';
import { WallpaperDTO } from '../api/bridge';
import { isApplyAvailable } from '../domain/applyActions';
import ContextMenu from './ContextMenu';
import { useThumbnailStore } from '../state/ThumbnailStoreContext';
import { calculateColumnCount, GRID_GAP } from '../utils/layout';

interface Props {
  entries: WallpaperDTO[];
  onApply: (path: string) => void;
  applying: boolean;
  emptyText?: string;
  contextActions?: ContextAction[];
  buildContextActions?: (entry: WallpaperDTO) => ContextAction[];
  active?: boolean;
  selectedPaths?: Set<string>;
  onSelectionChange?: (paths: Set<string>) => void;
}

export interface ContextAction {
  label: string;
  action: (path: string) => void;
  danger?: boolean;
  visible?: (entry: WallpaperDTO) => boolean;
}

const CARD_HEIGHT = 188;
const OVERSCAN = 2;

export default function WallpaperGrid({
  entries,
  onApply,
  applying,
  emptyText = 'No wallpapers found',
  contextActions = [],
  buildContextActions,
  active = true,
  selectedPaths,
  onSelectionChange,
}: Props) {
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number; path: string } | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const { thumbs: thumbCache, enqueue } = useThumbnailStore();
  const lastClickedRef = useRef<string | null>(null);

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

  // Keyboard: Escape clears selection
  useEffect(() => {
    if (!onSelectionChange) return;
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        onSelectionChange(new Set());
      }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [onSelectionChange]);

  const handleContextMenu = (e: React.MouseEvent, path: string) => {
    e.preventDefault();
    setContextMenu({ x: e.clientX, y: e.clientY, path });
  };

  const handleDoubleClick = (entry: WallpaperDTO) => {
    if (!canApply(entry)) {
      window.dispatchEvent(new CustomEvent('wc-feedback', {
        detail: { state: 'warning', label: 'Cannot apply', detail: 'This item cannot be applied as a live wallpaper.' },
      }));
      return;
    }
    onApply(entry.path);
  };

  const canApply = (entry: WallpaperDTO): boolean => isApplyAvailable(entry);

  const findEntry = (path: string): WallpaperDTO | undefined => {
    return entries.find((e) => e.path === path);
  };

  const handleCardClick = (e: React.MouseEvent, entry: WallpaperDTO) => {
    if (!onSelectionChange) return;
    const sel = new Set(selectedPaths ?? []);
    if (e.ctrlKey || e.metaKey) {
      if (sel.has(entry.path)) sel.delete(entry.path);
      else sel.add(entry.path);
      onSelectionChange(sel);
      lastClickedRef.current = entry.path;
    } else if (e.shiftKey && lastClickedRef.current) {
      const idx = entries.findIndex((x) => x.path === entry.path);
      const prevIdx = entries.findIndex((x) => x.path === lastClickedRef.current);
      if (idx >= 0 && prevIdx >= 0) {
        const [start, end] = prevIdx < idx ? [prevIdx, idx] : [idx, prevIdx];
        for (let i = start; i <= end; i++) sel.add(entries[i].path);
        onSelectionChange(sel);
      }
    } else {
      lastClickedRef.current = entry.path;
    }
  };

  const rows = useMemo(() => {
    const r: WallpaperDTO[][] = [];
    for (let i = 0; i < entries.length; i += colCount) {
      r.push(entries.slice(i, i + colCount));
    }
    return r;
  }, [entries, colCount]);

  const contextEntry = contextMenu ? findEntry(contextMenu.path) : null;

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
              {rowEntries.map((e) => {
                const selected = selectedPaths?.has(e.path) ?? false;
                return (
                  <div
                    key={e.path}
                    className={`wallpaper-card${applying ? ' disabled' : ''}${selected ? ' selected' : ''}`}
                    onContextMenu={(ev) => handleContextMenu(ev, e.path)}
                    onClick={(ev) => handleCardClick(ev, e)}
                    onDoubleClick={() => handleDoubleClick(e)}
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
                );
              })}
            </div>
          );
        })}
      </div>

      {contextMenu && contextEntry && (
        <ContextMenu
          x={contextMenu.x}
          y={contextMenu.y}
          path={contextMenu.path}
          canApply={canApply(contextEntry)}
          onApply={onApply}
          actions={buildContextActions
            ? buildContextActions(contextEntry)
            : contextActions.filter((action) => !action.visible || action.visible(contextEntry))}
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
  if (e.type === 'we_web') {
    return 'WE Web · Unsupported';
  }
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
      return ['Web wallpaper — unsupported', e.workshopId].filter(Boolean).join(' · ');
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
