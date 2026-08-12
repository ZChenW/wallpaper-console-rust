import { Fragment, memo } from 'react';

import type { LibraryBrowserItemDTO } from '../api/types.ts';
import { presentWallpaper } from './wallpaperPresentation.ts';

export interface FlowMetadataRailProps {
  readonly centeredEntry: LibraryBrowserItemDTO | null;
  readonly centeredIndex: number;
  readonly loadedCount: number;
  readonly totalKnown: boolean;
  readonly total: number | null;
  readonly selected: boolean;
  readonly current: boolean;
  readonly applying: boolean;
  readonly pending: boolean;
  readonly favorite: boolean;
  readonly favoritePending: boolean;
  readonly applyAvailable: boolean;
  readonly applyDisabledReason: string | null;
  readonly activeQueueName: string | null;
  readonly pendingQueueName: string | null;
  readonly allViewed: boolean;
  readonly onApply: (entry: LibraryBrowserItemDTO) => void;
  readonly onFavorite: (entry: LibraryBrowserItemDTO) => void;
  readonly onDetails: (entry: LibraryBrowserItemDTO) => void;
}

interface MetadataRowProps {
  readonly label: string;
  readonly layout?: 'narrative';
  readonly value: string | null;
  readonly priority?: 'primary' | 'secondary';
}

function MetadataRow({ label, layout, value, priority = 'primary' }: MetadataRowProps) {
  return (
    <div
      className="flow-metadata-rail__metadata-row"
      data-flow-metadata-field={label.toLowerCase()}
      data-flow-metadata-layout={layout}
      data-flow-metadata-missing={value === null ? true : undefined}
      data-flow-metadata-priority={priority}
    >
      <dt>{label}</dt>
      <dd>{value ?? '—'}</dd>
    </div>
  );
}

export function FlowMetadataRailView({
  centeredEntry,
  centeredIndex,
  loadedCount,
  totalKnown,
  total,
  selected,
  current,
  applying,
  pending,
  favorite,
  favoritePending,
  applyAvailable,
  applyDisabledReason,
  activeQueueName,
  pendingQueueName,
  allViewed,
  onApply,
  onFavorite,
  onDetails,
}: FlowMetadataRailProps) {
  if (centeredEntry === null) {
    return (
      <aside aria-label="Centered wallpaper details" className="flow-metadata-rail">
        <p className="flow-metadata-rail__empty">No wallpaper centered</p>
      </aside>
    );
  }

  const presentation = presentWallpaper(centeredEntry);
  const displayedTotal = totalKnown && total !== null ? total : loadedCount;
  const disabledReason = applyAvailable
    ? null
    : applyDisabledReason?.trim() || 'This wallpaper cannot be applied.';
  const disabledReasonId = `flow-apply-disabled-${centeredEntry.wallpaperId}`;

  return (
    <aside
      aria-label="Centered wallpaper details"
      className="flow-metadata-rail"
      data-applying={applying || undefined}
      data-current={current || undefined}
      data-favorite={favorite || undefined}
      data-pending={pending || undefined}
      data-selected={selected || undefined}
    >
      <Fragment key={centeredEntry.wallpaperId}>
        <header className="flow-metadata-rail__header" data-flow-metadata-content={true}>
          <span className="flow-metadata-rail__ordinal">
            {centeredIndex + 1} of {displayedTotal}
          </span>
          <h3 className="flow-metadata-rail__title">{presentation.name}</h3>
        </header>

        <div
          aria-label="Wallpaper state"
          className="flow-metadata-rail__status-list"
          data-flow-metadata-content={true}
        >
          {selected ? <span className="flow-metadata-rail__status">Selected</span> : null}
          {current ? <span className="flow-metadata-rail__status">Current</span> : null}
          {applying ? <span className="flow-metadata-rail__status">Applying</span> : null}
          {pending ? <span className="flow-metadata-rail__status">Pending</span> : null}
          {favorite ? <span className="flow-metadata-rail__status">Favorite</span> : null}
        </div>

      </Fragment>

      <div className="flow-metadata-rail__body" data-flow-metadata-content={true}>
        <dl className="flow-metadata-rail__metadata">
          <MetadataRow label="Source" value={presentation.sources} />
          <MetadataRow label="Type" value={presentation.type} />
          <MetadataRow label="Resolution" value={presentation.resolution} />
          <MetadataRow
            label="Compatibility"
            layout={presentation.compatibility === null ? undefined : 'narrative'}
            value={presentation.compatibility}
          />
          <MetadataRow label="Size" priority="secondary" value={presentation.size} />
          <MetadataRow label="Added" priority="secondary" value={presentation.addedDate} />
          <MetadataRow label="Author" priority="secondary" value={presentation.author} />
          <MetadataRow label="Workshop" priority="secondary" value={presentation.workshopId} />
          <MetadataRow label="Backend" priority="secondary" value={presentation.backend} />
        </dl>

        {(activeQueueName !== null || pendingQueueName !== null) ? (
          <div aria-label="Apply queue" className="flow-metadata-rail__queue">
            {activeQueueName !== null ? (
              <p>
                <span>Applying now</span>
                <strong>{activeQueueName}</strong>
              </p>
            ) : null}
            {pendingQueueName !== null ? (
              <p>
                <span>Queued next</span>
                <strong>{pendingQueueName}</strong>
              </p>
            ) : null}
          </div>
        ) : null}

        {disabledReason ? (
          <p className="flow-metadata-rail__disabled-reason" id={disabledReasonId}>
            {disabledReason}
          </p>
        ) : null}

        {allViewed ? (
          <p className="flow-metadata-rail__completion">
            All {displayedTotal} wallpapers viewed
          </p>
        ) : null}
      </div>

      <div className="flow-metadata-rail__actions">
        <button
          aria-label="Apply centered wallpaper"
          aria-describedby={disabledReason ? disabledReasonId : undefined}
          className="flow-metadata-rail__action flow-metadata-rail__action--primary"
          data-flow-action="apply"
          disabled={!applyAvailable}
          onClick={(event) => {
            event.stopPropagation();
            onApply(centeredEntry);
          }}
          title={disabledReason ?? undefined}
          type="button"
        >
          Apply
        </button>
        <button
          aria-busy={favoritePending || undefined}
          aria-disabled={favoritePending || undefined}
          aria-pressed={favorite}
          className="flow-metadata-rail__action"
          data-flow-action="favorite"
          onClick={(event) => {
            event.stopPropagation();
            if (favoritePending) return;
            onFavorite(centeredEntry);
          }}
          type="button"
        >
          Favorite{favorite ? ' · Saved' : ''}
        </button>
        <button
          className="flow-metadata-rail__action"
          data-flow-action="details"
          onClick={(event) => {
            event.stopPropagation();
            onDetails(centeredEntry);
          }}
          type="button"
        >
          Details
        </button>
      </div>
    </aside>
  );
}

export const FlowMetadataRail = memo(FlowMetadataRailView);

export default FlowMetadataRail;
