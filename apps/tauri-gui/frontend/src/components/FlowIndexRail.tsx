import {
  memo,
  useEffect,
  useRef,
  type Ref,
} from 'react';

import type { LibraryBrowserItemDTO } from '../api/types.ts';
import {
  flowStateLabels,
  nextFlowIndexAlignmentOffset,
} from './wallpaperFlowModel.ts';
import { displayName } from './wallpaperCardHelpers.ts';

export interface FlowIndexRailEntry {
  readonly entry: LibraryBrowserItemDTO;
  readonly index: number;
  readonly selected: boolean;
  readonly current: boolean;
  readonly favorite: boolean;
}

export interface FlowIndexRailProps {
  readonly entries: readonly FlowIndexRailEntry[];
  readonly centeredWallpaperId: number | null;
  readonly loadedCount: number;
  readonly totalKnown: boolean;
  readonly total: number | null;
  readonly onActivate: (entry: LibraryBrowserItemDTO) => void;
  readonly onHover: (wallpaperId: number | null) => void;
  readonly onOpenIndex: () => void;
  readonly onViewportHeightChange?: (height: number) => void;
}

interface FlowIndexRailViewProps extends FlowIndexRailProps {
  readonly viewportRef?: Ref<HTMLDivElement>;
}

function progressLabel(
  loadedCount: number,
  totalKnown: boolean,
  total: number | null,
): string {
  return totalKnown && total !== null
    ? `${loadedCount} loaded / ${total} total`
    : `${loadedCount} loaded`;
}

function observeFlowIndexAlignment(list: HTMLOListElement | null): (() => void) | void {
  if (list === null) return;
  const viewport = list.parentElement;
  if (viewport === null) return;
  const preview = list
    .closest<HTMLElement>('.wallpaper-flow')
    ?.querySelector<HTMLElement>('.flow-preview-stream') ?? viewport;
  let alignmentFrame: number | null = null;

  const align = () => {
    alignmentFrame = null;
    const centeredItem = list.querySelector<HTMLElement>('[data-centered]');
    if (centeredItem === null || !list.isConnected || !preview.isConnected) return;
    const previewBounds = preview.getBoundingClientRect();
    const itemBounds = centeredItem.getBoundingClientRect();
    const currentOffset = Number.parseFloat(
      list.style.getPropertyValue('--flow-index-alignment'),
    );
    const offset = nextFlowIndexAlignmentOffset(currentOffset, {
      railStart: previewBounds.top,
      railSize: previewBounds.height,
      itemStart: itemBounds.top,
      itemSize: itemBounds.height,
    });
    list.style.setProperty('--flow-index-alignment', `${offset}px`);
  };

  const scheduleAlign = () => {
    if (alignmentFrame !== null) return;
    alignmentFrame = window.requestAnimationFrame(align);
  };

  scheduleAlign();
  const resizeObserver = typeof ResizeObserver === 'undefined'
    ? null
    : new ResizeObserver(scheduleAlign);
  resizeObserver?.observe(preview);
  resizeObserver?.observe(list);
  const mutationObserver = typeof MutationObserver === 'undefined'
    ? null
    : new MutationObserver(scheduleAlign);
  mutationObserver?.observe(list, {
    attributeFilter: ['data-centered'],
    attributes: true,
    childList: true,
    subtree: true,
  });
  return () => {
    if (alignmentFrame !== null) window.cancelAnimationFrame(alignmentFrame);
    resizeObserver?.disconnect();
    mutationObserver?.disconnect();
  };
}

export function FlowIndexRailView({
  entries,
  centeredWallpaperId,
  loadedCount,
  totalKnown,
  total,
  onActivate,
  onHover,
  onOpenIndex,
  viewportRef,
}: FlowIndexRailViewProps) {
  const centeredPosition = entries.find(({ entry }) => (
    entry.wallpaperId === centeredWallpaperId
  ));
  const centeredOrdinal = centeredPosition ? centeredPosition.index + 1 : null;
  return (
    <nav aria-label="Loaded wallpaper index" className="flow-index-rail">
      <header className="flow-index-rail__header">
        <button
          aria-haspopup="dialog"
          aria-label={centeredOrdinal === null
            ? 'Index'
            : `Index, wallpaper ${centeredOrdinal} of ${loadedCount}`}
          className="flow-index-rail__open"
          data-flow-index-open={true}
          onClick={onOpenIndex}
          type="button"
        >
          Index
        </button>
        {centeredOrdinal === null ? null : (
          <span className="flow-index-rail__position">
            {centeredOrdinal} / {loadedCount}
          </span>
        )}
        <span className="flow-index-rail__progress">
          {progressLabel(loadedCount, totalKnown, total)}
        </span>
      </header>

      <div className="flow-index-rail__viewport" ref={viewportRef}>
        <ol className="flow-index-rail__list" ref={observeFlowIndexAlignment}>
          {entries.map(({ entry, index, selected, current, favorite }) => {
            const centered = entry.wallpaperId === centeredWallpaperId;
            return (
              <li
                className="flow-index-rail__item"
                data-centered={centered || undefined}
                data-current={current || undefined}
                data-favorite={favorite || undefined}
                data-selected={selected || undefined}
                data-wallpaper-id={entry.wallpaperId}
                key={entry.wallpaperId}
              >
                <button
                  aria-current={current ? 'true' : undefined}
                  className="flow-index-rail__entry"
                  data-wallpaper-id={entry.wallpaperId}
                  data-wallpaper-path={entry.path}
                  onClick={() => onActivate(entry)}
                  onMouseEnter={() => onHover(entry.wallpaperId)}
                  onMouseLeave={() => onHover(null)}
                  type="button"
                >
                  <span aria-hidden="true" className="flow-index-rail__ordinal">
                    {String(index + 1).padStart(2, '0')}
                  </span>
                  <span className="flow-index-rail__name">{displayName(entry)}</span>
                  <span className="flow-index-rail__states">
                    {flowStateLabels({ selected, current, favorite }).join(' · ')}
                  </span>
                </button>
              </li>
            );
          })}
        </ol>
      </div>
    </nav>
  );
}

function ObservedFlowIndexRail(props: FlowIndexRailProps) {
  const viewportRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const viewport = viewportRef.current;
    if (!viewport || !props.onViewportHeightChange) return undefined;
    const reportHeight = () => props.onViewportHeightChange?.(viewport.clientHeight);
    reportHeight();
    if (typeof ResizeObserver === 'undefined') return undefined;
    const observer = new ResizeObserver(reportHeight);
    observer.observe(viewport);
    return () => observer.disconnect();
  }, [props.onViewportHeightChange]);

  return <FlowIndexRailView {...props} viewportRef={viewportRef} />;
}

export const FlowIndexRail = memo(ObservedFlowIndexRail);

export default FlowIndexRail;
