import {
  useEffect,
  useRef,
  useState,
  type KeyboardEvent,
} from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';

import type { LibraryBrowserItemDTO } from '../api/types.ts';
import { trapDialogFocus } from '../shell/dialogFocus.ts';
import { displayName } from './wallpaperCardHelpers.ts';
import { flowStateLabels } from './wallpaperFlowModel.ts';
import {
  clampIndexDialogActive,
  resolveIndexDialogKey,
  shouldInitializeIndexDialogFocus,
} from './flowIndexDialogModel.ts';

export interface FlowIndexDialogProps {
  readonly open: boolean;
  readonly entries: readonly LibraryBrowserItemDTO[];
  readonly centeredWallpaperId: number | null;
  readonly selectedPath: string | null;
  readonly currentPath: string | null;
  readonly totalKnown: boolean;
  readonly total: number | null;
  readonly onActivate: (entry: LibraryBrowserItemDTO) => void;
  readonly onClose: () => void;
}

const INDEX_ROW_HEIGHT = 44;

export default function FlowIndexDialog({
  open,
  entries,
  centeredWallpaperId,
  selectedPath,
  currentPath,
  totalKnown,
  total,
  onActivate,
  onClose,
}: FlowIndexDialogProps) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const dialogRef = useRef<HTMLElement>(null);
  const wasOpenRef = useRef(false);
  const centeredIndex = Math.max(
    0,
    entries.findIndex((entry) => entry.wallpaperId === centeredWallpaperId),
  );
  const [activeIndex, setActiveIndex] = useState(centeredIndex);
  const virtualizer = useVirtualizer({
    count: open ? entries.length : 0,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => INDEX_ROW_HEIGHT,
    overscan: 8,
    getItemKey: (index) => entries[index]?.wallpaperId ?? index,
  });

  const focusIndex = (index: number) => {
    const next = clampIndexDialogActive(index, entries.length);
    if (next < 0) return;
    setActiveIndex(next);
    virtualizer.scrollToIndex(next, { align: 'auto' });
    window.requestAnimationFrame(() => {
      dialogRef.current
        ?.querySelector<HTMLButtonElement>(`[data-flow-index="${next}"]`)
        ?.focus();
    });
  };

  useEffect(() => {
    const shouldInitialize = shouldInitializeIndexDialogFocus(wasOpenRef.current, open);
    wasOpenRef.current = open;
    if (!shouldInitialize) return;
    const next = Math.max(
      0,
      entries.findIndex((entry) => entry.wallpaperId === centeredWallpaperId),
    );
    setActiveIndex(next);
    window.requestAnimationFrame(() => focusIndex(next));
  }, [centeredWallpaperId, entries.length, open]);

  useEffect(() => {
    setActiveIndex((current) => clampIndexDialogActive(current, entries.length));
  }, [entries.length]);

  if (!open) return null;

  const activate = (index: number) => {
    const entry = entries[index];
    if (entry) onActivate(entry);
  };
  const handleKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    if (event.key === 'Tab') {
      trapDialogFocus(event, event.currentTarget);
      return;
    }
    const intent = resolveIndexDialogKey(event.key, activeIndex, entries.length);
    if (!intent) return;
    event.preventDefault();
    event.stopPropagation();
    if (intent.kind === 'close') {
      onClose();
      return;
    }
    if (intent.kind === 'activate') {
      activate(intent.index);
      return;
    }
    focusIndex(intent.index);
  };
  const countLabel = totalKnown && total !== null
    ? `${entries.length} of ${total} loaded`
    : `${entries.length} loaded`;

  return (
    <div
      className="flow-index-dialog__overlay"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <section
        aria-labelledby="flow-index-dialog-title"
        aria-modal="true"
        className="flow-index-dialog"
        onKeyDown={handleKeyDown}
        ref={dialogRef}
        role="dialog"
      >
        <header className="flow-index-dialog__header">
          <div>
            <p className="flow-eyebrow">Library index</p>
            <h2 id="flow-index-dialog-title">Loaded wallpapers</h2>
            <p>{countLabel}</p>
          </div>
          <button aria-label="Close wallpaper index" onClick={onClose} type="button">
            Close
          </button>
        </header>
        <div
          aria-label="Loaded wallpaper names"
          className="flow-index-dialog__list"
          ref={scrollRef}
          role="navigation"
        >
          <div
            className="flow-index-dialog__virtual"
            style={{ height: virtualizer.getTotalSize(), position: 'relative' }}
          >
            {virtualizer.getVirtualItems().map((row) => {
              const entry = entries[row.index];
              if (!entry) return null;
              const selected = selectedPath === entry.path;
              const current = currentPath === entry.path;
              return (
                <button
                  aria-current={current ? 'true' : undefined}
                  className="flow-index-dialog__item"
                  data-active={row.index === activeIndex || undefined}
                  data-current={current || undefined}
                  data-flow-index={row.index}
                  data-selected={selected || undefined}
                  key={row.key}
                  onClick={() => activate(row.index)}
                  onFocus={() => setActiveIndex(row.index)}
                  ref={virtualizer.measureElement}
                  style={{
                    position: 'absolute',
                    insetInline: 0,
                    top: 0,
                    minHeight: row.size,
                    transform: `translateY(${row.start}px)`,
                  }}
                  tabIndex={row.index === activeIndex ? 0 : -1}
                  type="button"
                >
                  <span aria-hidden="true">{String(row.index + 1).padStart(2, '0')}</span>
                  <span>{displayName(entry)}</span>
                  <span className="flow-index-dialog__states">
                    {flowStateLabels({
                      selected,
                      current,
                      favorite: entry.favorite,
                    }).join(' · ')}
                  </span>
                </button>
              );
            })}
          </div>
        </div>
      </section>
    </div>
  );
}
