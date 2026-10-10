import type { ComponentProps } from 'react';
import { ClockAlert, FolderPlus, LoaderCircle, ScanSearch, SearchCheck, SearchX, TriangleAlert } from 'lucide-react';
import LibraryState from '../components/LibraryState.tsx';
import LibraryViewSwitch from '../components/LibraryViewSwitch.tsx';
import LibraryViewport from '../components/LibraryViewport.tsx';
import FirstRunSuggestions from './FirstRunSuggestions.tsx';
import type { useLibraryBrowser } from './useLibraryBrowser.ts';
import type { LibraryLifecycleResult } from './useLibraryLifecycle.ts';

interface Props {
  browser: ReturnType<typeof useLibraryBrowser>;
  libraryLifecycle: LibraryLifecycleResult;
  firstRunEligible: boolean;
  scanRunning: boolean;
  viewport: ComponentProps<typeof LibraryViewport>;
  addFirstRunDirectory: (path: string) => Promise<void>;
  scanWallpaperEngine: () => Promise<void>;
  onOpenSources: (trigger: HTMLElement) => void;
  onClearFilters: () => void;
}

/** Library loading and recovery stay independent of settings and display probes. */
function LibraryContentState({
  browser, libraryLifecycle, firstRunEligible, scanRunning, viewport,
  addFirstRunDirectory, scanWallpaperEngine, onOpenSources, onClearFilters,
}: Props) {

    // Library loads independently of preferences, catalog, and display probes.
    // A failure in any of those services only disables the relevant controls.
    if (libraryLifecycle.startup.timedOut
      && browser.entries.length === 0
      && !browser.emptyConfirmed
      && !browser.loadError) {
      return (
        <LibraryState
          action={(
            <button
              className="btn"
              type="button"
              onClick={libraryLifecycle.startup.retry}
            >
              Retry
            </button>
          )}
          description="Wallpaper data has not arrived yet. Retry the library connection."
          icon={<ClockAlert size={28} />}
          role="alert"
          title="Library is taking longer than expected"
        />
      );
    }
    if (browser.initialLoading) {
      return (
        <LibraryState
          description="Preparing your saved wallpapers."
          icon={<LoaderCircle className="library-state__spinner" size={28} />}
          role="status"
          title="Loading wallpaper library"
        />
      );
    }
    if (firstRunEligible) {
      return (
        <LibraryState
          action={(
            <button
              className="btn primary"
              type="button"
              onClick={(event) => {
                onOpenSources(event.currentTarget);
              }}
            >
              <FolderPlus size={16} aria-hidden="true" /> Add Folder
            </button>
          )}
          className="single-page-first-run"
          description="Add any number of folders. Nothing is scanned until you choose it."
          icon={<FolderPlus size={30} />}
          title="Choose where your wallpapers live"
        >
          <FirstRunSuggestions
            suggestions={libraryLifecycle.firstRun.suggestions}
            onAddDirectory={(path) => void addFirstRunDirectory(path)}
            onScanWallpaperEngine={() => void scanWallpaperEngine()}
          />
          {libraryLifecycle.firstRun.error ? (
            <div className="single-page-first-run__suggestion-error" role="status">
              <span>Optional source suggestions are unavailable.</span>
              <button
                className="btn"
                type="button"
                onClick={libraryLifecycle.firstRun.retrySuggestions}
              >
                Retry suggestions
              </button>
            </div>
          ) : null}
        </LibraryState>
      );
    }
    if (browser.entries.length > 0) {
      return (
        <>
          {browser.loadError ? (
            <div className="single-page-stale-results" role="alert">
              <span>
                Results could not be refreshed. Showing the previous library view.
                {browser.loadErrorDetail ? ` ${browser.loadErrorDetail}` : ''}
              </span>
              <button className="btn" type="button" onClick={() => void browser.reload()}>
                Retry
              </button>
            </div>
          ) : null}
          <LibraryViewport {...viewport} />
          {!browser.refreshing
            && browser.canAppend
            && (viewport.mode === 'grid' || !browser.canAutoAppend) ? (
            <div className="single-page-load-more">
              <button
                className="btn"
                disabled={browser.appending}
                type="button"
                onClick={() => void browser.appendMore()}
                title={!browser.canAutoAppend && browser.loadErrorDetail
                  ? browser.loadErrorDetail
                  : undefined}
              >
                {browser.appending
                  ? 'Loading more…'
                  : !browser.canAutoAppend
                    ? 'Retry loading more'
                    : browser.totalKnown
                      ? `Load more · ${Math.max(0, browser.total - browser.entries.length)} remaining`
                      : 'Load more'}
              </button>
            </div>
          ) : null}
        </>
      );
    }
    if (scanRunning) {
      return (
        <LibraryState
          description="New wallpapers will appear as the scan finds them."
          icon={<ScanSearch className="library-state__spinner" size={28} />}
          role="status"
          title="Indexing wallpapers"
        />
      );
    }
    if (browser.loadError) {
      return (
        <LibraryState
          action={(
            <button className="btn" type="button" onClick={() => void browser.reload()}>
              Retry
            </button>
          )}
          description={browser.loadErrorDetail ?? 'The library could not be read.'}
          icon={<TriangleAlert size={28} />}
          role="alert"
          title="Could not load the wallpaper library"
        />
      );
    }
    if (!browser.emptyConfirmed) {
      return (
        <LibraryState
          description="Confirming whether wallpapers match the current view."
          icon={<SearchCheck className="library-state__spinner" size={28} />}
          role="status"
          title="Checking the library"
        />
      );
    }
    return (
      <LibraryState
        action={(
          <button
            className="btn"
            type="button"
            onClick={() => {
              onClearFilters();
            }}
          >
            Clear filters
          </button>
        )}
        description="Try clearing the active filters or changing your search."
        icon={<SearchX size={28} />}
        title="No matching wallpapers"
      />
    );
  }


/** Recovery/empty results must keep an exit from Knot's hidden shell chrome. */
export default function LibraryContent(props: Props) {
  const { browser, viewport, firstRunEligible } = props;
  if (viewport.mode !== 'knot') return <LibraryContentState {...props} />;
  if (!browser.initialLoading && !firstRunEligible && browser.emptyConfirmed && !browser.loadError && !props.scanRunning && browser.entries.length === 0) {
    return <LibraryViewport {...viewport} />;
  }
  if (browser.entries.length > 0 && !browser.initialLoading && !firstRunEligible) return <LibraryContentState {...props} />;
  return <section className="wallpaper-knot wallpaper-knot--recovery" aria-label="Knot wallpaper browser">
    {viewport.onViewModeChange ? <div className="wallpaper-knot__view-switch">
      <LibraryViewSwitch value={viewport.viewMode ?? 'knot'} onChange={viewport.onViewModeChange} />
    </div> : null}
    <LibraryContentState {...props} />
  </section>;
}
