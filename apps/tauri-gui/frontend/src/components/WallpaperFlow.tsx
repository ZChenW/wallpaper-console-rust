import { memo, type CSSProperties, type MouseEvent } from 'react';
import { SearchX } from 'lucide-react';
import { ApplyIndicator } from './ApplyIndicator.tsx';
import ContextMenu from './ContextMenu.tsx';
import FlowIndexDialog from './FlowIndexDialog.tsx';
import FlowIndexRail from './FlowIndexRail.tsx';
import FlowMetadataRail from './FlowMetadataRail.tsx';
import LibraryState from './LibraryState.tsx';
import WallpaperPreviewMedia from './WallpaperPreviewMedia.tsx';
import { libraryEntryApplyDisabledReason } from './libraryViewModel.ts';
import { displayName } from './wallpaperCardHelpers.ts';
import { flowStateLabels, flowStatePresentation } from './wallpaperFlowModel.ts';
import { aspectClass, resolutionAspectRatio } from './flowGeometry.ts';
import { useWallpaperFlowController, type WallpaperFlowProps } from './useWallpaperFlowController.ts';
export type { WallpaperFlowProps } from './useWallpaperFlowController.ts';

function WallpaperFlowReady(props: WallpaperFlowProps) {
  const { model } = props;
  const {
    elements: { flowRef, streamRef, virtualizer },
    snapshot: {
      centeredEntry, centeredIndex, indexRailEntry, localEntries, interactionActive,
      settled, contextMenu, reducedMotion, showReturnToTop, activeQueueName,
      pendingQueueName, centeredApplicable, indexOpen,
    },
    actions: {
      selectEntry, handleFlowHover, handleKeyDown, handlePointerDown,
      finishPointerInteraction, handleScroll, handleWheel, handleEntryClick,
      openContextMenu, returnToTop, applyEntry, activateIndexEntry, closeIndex,
      openIndex, closeContextMenu, resizeIndexRail,
    },
  } = useWallpaperFlowController(props);

  if (model.entries.length === 0) {
    return (
      <LibraryState
        description="Try clearing the active filters or changing your search."
        icon={<SearchX aria-hidden="true" size={28} />}
        role="status"
        title="No wallpapers found"
      />
    );
  }

  return (
    <section
      aria-label="Flow wallpaper browser"
      className={`wallpaper-flow${model.refreshing ? ' is-refreshing' : ''}`}
      data-active={interactionActive || undefined}
      data-scrolling={!settled || undefined}
      ref={flowRef}
    >
      <FlowIndexRail
        centeredWallpaperId={indexRailEntry?.wallpaperId ?? null}
        entries={localEntries}
        loadedCount={model.entries.length}
        onActivate={selectEntry}
        onHover={handleFlowHover}
        onOpenIndex={openIndex}
        onViewportHeightChange={resizeIndexRail}
        total={model.total}
        totalKnown={model.totalKnown}
      />

      <div
        aria-activedescendant={centeredEntry ? `flow-option-${centeredEntry.wallpaperId}` : undefined}
        aria-label="Wallpaper Flow"
        className="flow-preview-stream"
        onKeyDown={handleKeyDown}
        onPointerDown={handlePointerDown}
        onPointerCancel={finishPointerInteraction}
        onPointerUp={finishPointerInteraction}
        onScroll={handleScroll}
        onWheel={handleWheel}
        ref={streamRef}
        role="listbox"
        aria-multiselectable={false}
        tabIndex={0}
      >
        <div
          className="flow-preview-stream__virtual"
          ref={virtualizer.containerRef}
          style={{ position: 'relative' }}
        >
          {virtualizer.getVirtualItems().map((row) => {
            const entry = model.entries[row.index];
            if (!entry) return null;
            const centered = centeredEntry?.wallpaperId === entry.wallpaperId;
            const selected = model.selectedPath === entry.path;
            const current = model.currentPath === entry.path;
            const applying = model.applying && model.activePath === entry.path;
            const pending = model.pendingPath === entry.path;
            const preloadStaticFallback = Math.abs(row.index - centeredIndex) <= 1;
            const presentation = flowStatePresentation('flow-preview-item', {
              active: interactionActive,
              settled,
              centered,
              hovered: false,
              selected,
              current,
              applying,
              pending,
              favorite: entry.favorite,
            });
            const aspect = resolutionAspectRatio(entry.resolution);
            const style = {
              position: 'absolute',
              insetInline: 0,
              top: 0,
              height: row.size,
              '--flow-media-aspect': String(aspect),
            } as CSSProperties;
            return (
              <div
                {...presentation.attributes}
                aria-current={current ? 'true' : undefined}
                aria-label={`${row.index + 1}. ${displayName(entry)}`}
                aria-posinset={row.index + 1}
                aria-selected={selected}
                aria-setsize={model.totalKnown && model.total !== null ? model.total : undefined}
                className={presentation.className}
                data-aspect={aspectClass(entry)}
                data-index={row.index}
                data-wallpaper-id={entry.wallpaperId}
                data-wallpaper-path={entry.path}
                id={`flow-option-${entry.wallpaperId}`}
                key={row.key}
                onClick={(event) => handleEntryClick(event, entry)}
                onContextMenu={(event: MouseEvent<HTMLDivElement>) => {
                  event.preventDefault();
                  openContextMenu(entry, event.clientX, event.clientY);
                }}
                onPointerEnter={() => handleFlowHover(entry.wallpaperId)}
                onPointerLeave={() => handleFlowHover(null)}
                ref={virtualizer.measureElement}
                role="option"
                style={style}
              >
                <div className="flow-preview-item__media">
                  <WallpaperPreviewMedia
                    alt=""
                    clipActive={model.active && !contextMenu && !indexOpen}
                    eligibility={{
                      active: interactionActive && !contextMenu,
                      centered,
                      selected,
                      settled,
                      reducedMotion,
                    }}
                    entry={entry}
                    loading={preloadStaticFallback ? 'eager' : 'lazy'}
                    staticFallback={preloadStaticFallback}
                    stabilizeEntranceDuringMotion
                  />
                  <span aria-hidden="true" className="flow-preview-item__ordinal">
                    {String(row.index + 1).padStart(2, '0')}
                  </span>
                  <span className="flow-preview-item__states">
                    {flowStateLabels({
                      selected,
                      current,
                      applying,
                      pending,
                      favorite: entry.favorite,
                    }).join(' · ')}
                  </span>
                </div>
                {applying || pending ? (
                  <div aria-hidden="true" className="flow-preview-item__indicator-layer">
                    <ApplyIndicator state={applying ? 'applying' : 'pending'} />
                  </div>
                ) : null}
              </div>
            );
          })}
        </div>
      </div>

      {showReturnToTop ? (
        <button
          aria-label="Return to first wallpaper"
          className="flow-return-to-top"
          data-flow-action="return"
          onClick={(event) => {
            event.stopPropagation();
            returnToTop();
          }}
          title="Return to first wallpaper"
          type="button"
        >
          <span aria-hidden="true">↑</span>
        </button>
      ) : null}

      <FlowMetadataRail
        activeQueueName={activeQueueName}
        allViewed={!model.canAppend && !model.loadingMore}
        applyAvailable={centeredApplicable}
        applyDisabledReason={centeredEntry
          ? libraryEntryApplyDisabledReason(
            model.canApplyToDisplay,
            model.displayApplyDisabledReason,
            centeredEntry,
          )
          : model.displayApplyDisabledReason}
        applying={Boolean(centeredEntry && model.applying && model.activePath === centeredEntry.path)}
        centeredEntry={centeredEntry}
        centeredIndex={centeredIndex}
        current={Boolean(centeredEntry && model.currentPath === centeredEntry.path)}
        favorite={centeredEntry?.favorite ?? false}
        favoritePending={Boolean(centeredEntry && model.favoritePendingPaths.has(centeredEntry.path))}
        loadedCount={model.entries.length}
        loadingMore={model.loadingMore}
        onApply={applyEntry}
        onDetails={(entry) => model.onDetails(
          entry,
          document.activeElement instanceof HTMLElement ? document.activeElement : null,
        )}
        onFavorite={(entry) => { void model.onToggleFavorite(entry); }}
        pending={Boolean(centeredEntry && model.pendingPath === centeredEntry.path)}
        pendingQueueName={pendingQueueName}
        selected={Boolean(centeredEntry && model.selectedPath === centeredEntry.path)}
        total={model.totalKnown ? model.total : null}
        totalKnown={model.totalKnown}
      />

      <FlowIndexDialog
        centeredWallpaperId={centeredEntry?.wallpaperId ?? null}
        currentPath={model.currentPath}
        entries={model.entries}
        onActivate={activateIndexEntry}
        onClose={closeIndex}
        open={indexOpen}
        selectedPath={model.selectedPath}
        total={model.total}
        totalKnown={model.totalKnown}
      />

      {contextMenu ? (
        <ContextMenu
          actions={model.buildContextActions(contextMenu.entry)}
          onClose={closeContextMenu}
          path={contextMenu.entry.path}
          x={contextMenu.x}
          y={contextMenu.y}
        />
      ) : null}

    </section>
  );
}

function WallpaperFlowImpl(props: WallpaperFlowProps) {
  if (
    props.initialAnchorWallpaperId == null
    && !props.model.currentObservationReady
  ) {
    return (
      <section className="wallpaper-flow wallpaper-flow--preparing">
        <div className="flow-preview-preparing" role="status">
          Preparing Flow preview…
        </div>
      </section>
    );
  }

  return <WallpaperFlowReady {...props} />;
}

export default memo(WallpaperFlowImpl);
