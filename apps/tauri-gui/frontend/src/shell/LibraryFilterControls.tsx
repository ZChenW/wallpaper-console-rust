import { Heart } from 'lucide-react';
import SelectField from '../components/SelectField.tsx';
import type { SourceDTO } from '../api/types.ts';
import type { LibrarySort, LibraryTypeFilter, SourceFilter } from './shellPreferences.ts';
import type { UseShellPreferencesResult } from './useShellPreferences.ts';

export function sourceFilterValue(filter: SourceFilter): string {
  return filter.kind === 'source' ? `source:${filter.sourceId}` : 'all';
}

function sourceFilterFromValue(value: string): SourceFilter {
  if (!value.startsWith('source:')) return { kind: 'all' };
  const sourceId = Number(value.slice('source:'.length));
  return Number.isSafeInteger(sourceId) && sourceId > 0
    ? { kind: 'source', sourceId }
    : { kind: 'all' };
}

interface Props extends Pick<UseShellPreferencesResult, 'preferences' | 'updatePreferences'> {
  sources: readonly SourceDTO[];
  sourceError?: string;
  effectiveSrcFilter: SourceFilter;
  onDismiss: () => void;
}
export default function LibraryFilterControls({
  sources, sourceError, effectiveSrcFilter, preferences, updatePreferences, onDismiss,
}: Props) {
  return (
    <>
      {sourceError ? (
        <details className="single-page-source-warning">
          <summary>Source list unavailable</summary>
          <p>{sourceError}</p>
        </details>
      ) : null}
      <SelectField
        aria-label="Source filter"
        value={sourceFilterValue(effectiveSrcFilter)}
        disabled={Boolean(sourceError)}
        options={[
          { value: 'all', label: 'ALL SOURCES' },
          ...sources.map((source) => ({
            value: `source:${source.id}`,
            label: `${source.displayName}${source.availability === 'offline' ? ' · Offline' : ''}`,
          })),
        ]}
        onValueChange={(value) => {
          const sourceFilter = sourceFilterFromValue(value);
          updatePreferences((current) => ({ ...current, sourceFilter }));
        }}
        onCommit={onDismiss}
        variant="compact"
      />
      <SelectField
        aria-label="Wallpaper type filter"
        value={preferences.typeFilter}
        options={[
          { value: 'usable', label: 'ALL' },
          { value: 'image', label: 'Images' },
          { value: 'gif', label: 'GIFs' },
          { value: 'video', label: 'Videos' },
          { value: 'weScene', label: 'Wallpaper Engine scenes' },
          { value: 'unsupported', label: 'Unsupported' },
        ]}
        onValueChange={(value) => {
          const typeFilter = value as LibraryTypeFilter;
          updatePreferences((current) => ({ ...current, typeFilter }));
        }}
        onCommit={onDismiss}
        variant="compact"
      />
      <label
        className="single-page-favorite-filter"
        data-active={preferences.favoritesOnly}
      >
        <input
          type="checkbox"
          checked={preferences.favoritesOnly}
          onChange={(event) => {
            const favoritesOnly = event.currentTarget.checked;
            updatePreferences((current) => ({ ...current, favoritesOnly }));
          }}
        />
        <Heart
          aria-hidden="true"
          fill={preferences.favoritesOnly ? 'currentColor' : 'none'}
          size={15}
        />
        <span>FAVORITES</span>
      </label>
      <SelectField
        aria-label="Library sort"
        value={preferences.sort}
        options={[
          { value: 'recentlyAdded', label: 'Recently added' },
          { value: 'nameAsc', label: 'Name A–Z' },
          { value: 'nameDesc', label: 'Name Z–A' },
        ]}
        onValueChange={(value) => {
          const sort = value as LibrarySort;
          updatePreferences((current) => ({ ...current, sort }));
        }}
        onCommit={onDismiss}
        variant="compact"
      />
      {preferences.libraryViewMode === 'grid' ? (
        <SelectField
          aria-label="Card size"
          value={preferences.cardSize}
          options={[
            { value: 'small', label: 'Small' },
            { value: 'medium', label: 'Medium' },
            { value: 'large', label: 'Large' },
          ]}
          onValueChange={(value) => {
            const cardSize = value as typeof preferences.cardSize;
            updatePreferences((current) => ({ ...current, cardSize }));
          }}
          onCommit={onDismiss}
          variant="compact"
        />
      ) : null}
    </>
  );
}
