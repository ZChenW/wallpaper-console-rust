import { memo } from 'react';
import { Check, Heart, Info, SearchX, ZoomIn, ZoomOut } from 'lucide-react';

import { ApplyIndicator } from './ApplyIndicator.tsx';
import ContextMenu from './ContextMenu.tsx';
import LibraryState from './LibraryState.tsx';
import WallpaperPreviewMedia from './WallpaperPreviewMedia.tsx';
import { libraryEntryApplyAvailable, libraryEntryApplyDisabledReason } from './libraryViewModel.ts';
import { bookLeafTransform, bookWallpaperIndex, type BookFace } from './wallpaperBookModel.ts';
import { flowStateLabels } from './wallpaperFlowModel.ts';
import { displayName } from './wallpaperCardHelpers.ts';
import { useWallpaperBookController, type WallpaperBookProps } from './useWallpaperBookController.ts';

export type { WallpaperBookProps } from './useWallpaperBookController.ts';

function WallpaperBookReady(props: WallpaperBookProps) {
  const { model } = props;
  const {
    elements: { stageRef, zoomRef, leavesRef },
    snapshot: {
      spread, leafKeyOffset, selectedIndex, selectedEntry, settled, zoomIndex, zoomMoving,
      contextMenu, reducedMotion, interactionActive, visibleLeaves,
    },
    actions: {
      handlePointerDown, handlePointerMove, finishPointer, handleKeyDown,
      handlePageClick, openContextMenu, applySelected, toggleZoom, closeZoom,
      closeContextMenu, focusStage, cancelPendingMenu,
    },
  } = useWallpaperBookController(props);
  const zoomed = zoomIndex !== null;
  const applyAvailable = selectedEntry !== null
    && libraryEntryApplyAvailable(model.canApplyToDisplay, model.isEntryApplicable, selectedEntry);
  const disabledReason = applyAvailable ? null : selectedEntry
    ? libraryEntryApplyDisabledReason(
      model.canApplyToDisplay,
      model.displayApplyDisabledReason,
      selectedEntry,
    )?.trim() || 'This wallpaper cannot be applied.'
    : 'Select an open page.';
  const favoritePending = Boolean(selectedEntry && model.favoritePendingPaths.has(selectedEntry.path));
  const applying = Boolean(selectedEntry && model.applying && model.activePath === selectedEntry.path);
  const pending = Boolean(selectedEntry && model.pendingPath === selectedEntry.path);
  const controlsDisabled = !selectedEntry || !model.active || !settled || zoomMoving;
  const activeId = selectedEntry
    ? `book-${zoomed ? 'zoom' : 'option'}-${selectedEntry.wallpaperId}`
    : undefined;

  const face = (leaf: number, side: BookFace) => {
    const index = bookWallpaperIndex(leaf, side, model.entries.length);
    const entry = index === null ? null : model.entries[index];
    const physicalIndex = leaf * 2 + (side === 'back' ? 1 : 0);
    const open = physicalIndex === spread * 2 - 1 || physicalIndex === spread * 2;
    const selected = index !== null && index === selectedIndex;
    const current = entry !== null && model.currentPath === entry.path;
    const pageApplying = entry !== null && model.applying && model.activePath === entry.path;
    const pagePending = entry !== null && model.pendingPath === entry.path;
    return (
      <div
        aria-current={open && current ? 'true' : undefined}
        aria-hidden={!open || zoomed || !entry ? true : undefined}
        aria-label={entry && open ? `${physicalIndex + 1}. ${displayName(entry)}` : undefined}
        aria-posinset={open && entry ? physicalIndex + 1 : undefined}
        aria-selected={open && entry ? selected : undefined}
        aria-setsize={open && model.totalKnown && model.total !== null ? model.total : undefined}
        className={`book-leaf__face book-leaf__face--${side}`}
        data-open={open || undefined}
        data-selected={selected || undefined}
        data-current={current || undefined}
        data-applying={pageApplying || undefined}
        data-pending={pagePending || undefined}
        data-favorite={entry?.favorite || undefined}
        id={entry ? `book-option-${entry.wallpaperId}` : undefined}
        key={side}
        onClick={index !== null ? (event) => handlePageClick(event, index) : undefined}
        onContextMenu={index !== null ? (event) => {
          event.preventDefault();
          openContextMenu(index, event.clientX, event.clientY);
        } : undefined}
        role={open && entry && !zoomed ? 'option' : undefined}
      >
        <div className="book-leaf__print">
          {entry ? (
            <WallpaperPreviewMedia
              key={entry.path}
              alt=""
              eligibility={{
                active: interactionActive && settled && !zoomed,
                centered: open && selected,
                selected: open && selected,
                settled,
                reducedMotion,
              }}
              entry={entry}
              loading="eager"
              staticFallback
              stabilizeEntranceDuringMotion
            />
          ) : null}
        </div>
        <span aria-hidden="true" className="book-leaf__spine-shadow" />
        <span aria-hidden="true" className="book-leaf__shade" />
        <span aria-hidden="true" className="book-leaf__highlight" />
        {entry && open ? (
          <span className="book-leaf__states">
            {flowStateLabels({
              selected,
              current,
              applying: pageApplying,
              pending: pagePending,
              favorite: entry.favorite,
            }).join(' · ')}
          </span>
        ) : null}
        {open && (pageApplying || pagePending) ? (
          <div aria-hidden="true" className="book-leaf__indicator">
            <ApplyIndicator state={pageApplying ? 'applying' : 'pending'} />
          </div>
        ) : null}
      </div>
    );
  };

  return (
    <section
      aria-label="Book wallpaper browser"
      className={`wallpaper-book${model.refreshing ? ' is-refreshing' : ''}`}
      data-reduced-motion={reducedMotion || undefined}
      onClickCapture={cancelPendingMenu}
      onPointerDownCapture={cancelPendingMenu}
      onKeyDownCapture={cancelPendingMenu}
      onWheelCapture={cancelPendingMenu}
    >
      <header className="wallpaper-book__heading">
        <h3>Library</h3>
        <p>{model.totalKnown && model.total !== null ? model.total : model.entries.length} wallpapers</p>
        {model.loadingMore ? (
          <p className="wallpaper-book__loading" role="status">Loading more…</p>
        ) : null}
        {model.appendNeedsRetry ? (
          <button
            className="wallpaper-book__load-more"
            disabled={model.loadingMore}
            onClick={() => {
              void model.onAppendMore();
              focusStage();
            }}
            title={model.loadErrorDetail ?? 'Load more wallpapers'}
            type="button"
          >
            Load more
          </button>
        ) : null}
      </header>
      <div
        aria-activedescendant={activeId}
        aria-label="Wallpaper Book"
        aria-multiselectable={false}
        className="wallpaper-book__stage"
        data-moving={!settled || undefined}
        data-zoomed={zoomed || undefined}
        onKeyDown={handleKeyDown}
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={finishPointer}
        onPointerCancel={finishPointer}
        onLostPointerCapture={finishPointer}
        ref={stageRef}
        role="listbox"
        tabIndex={0}
      >
        <div className="wallpaper-book__scene">
          <div aria-hidden="true" className="wallpaper-book__contact-shadow" />
          <div className="wallpaper-book__spread">
            {visibleLeaves.map((leaf) => {
              const transform = bookLeafTransform(leaf, spread, reducedMotion);
              return (
                <div
                  className="book-leaf"
                  data-turned={leaf < spread || undefined}
                  key={leaf - leafKeyOffset}
                  ref={(element) => {
                    if (element) leavesRef.current.set(leaf, element);
                    else leavesRef.current.delete(leaf);
                  }}
                  style={{ transform: transform.transform, opacity: transform.opacity }}
                >
                  {face(leaf, 'front')}
                  {face(leaf, 'back')}
                </div>
              );
            })}
          </div>
        </div>
        {zoomed && selectedEntry ? (
          <div className="wallpaper-book__zoom-layer">
            <div aria-hidden="true" className="wallpaper-book__zoom-backdrop" onClick={closeZoom} />
            <div
              aria-current={model.currentPath === selectedEntry.path ? 'true' : undefined}
              aria-label={displayName(selectedEntry)}
              aria-posinset={(zoomIndex ?? selectedIndex) + 1}
              aria-selected="true"
              aria-setsize={model.totalKnown && model.total !== null ? model.total : undefined}
              className="wallpaper-book__zoom-page"
              id={activeId}
              onClick={(event) => event.stopPropagation()}
              onContextMenu={(event) => {
                event.preventDefault();
                openContextMenu(zoomIndex, event.clientX, event.clientY);
              }}
              ref={zoomRef}
              role="option"
            >
              <div className="wallpaper-book__zoom-print">
                <WallpaperPreviewMedia
                  alt=""
                  eligibility={{
                    active: interactionActive && settled && !zoomMoving,
                    centered: true,
                    selected: true,
                    settled: settled && !zoomMoving,
                    reducedMotion,
                  }}
                  entry={selectedEntry}
                  loading="eager"
                  staticFallback
                />
              </div>
              <span className="book-leaf__states">
                {flowStateLabels({
                  selected: true,
                  current: model.currentPath === selectedEntry.path,
                  applying,
                  pending,
                  favorite: selectedEntry.favorite,
                }).join(' · ')}
              </span>
              {applying || pending ? (
                <div aria-hidden="true" className="book-leaf__indicator">
                  <ApplyIndicator state={applying ? 'applying' : 'pending'} />
                </div>
              ) : null}
            </div>
          </div>
        ) : null}
      </div>
      <div aria-label="Selected page actions" className="wallpaper-book__actions">
        <button
          aria-label="Apply"
          aria-busy={applying || pending || undefined}
          aria-describedby={disabledReason ? 'book-apply-disabled' : undefined}
          disabled={controlsDisabled || !applyAvailable}
          onClick={applySelected}
          title={disabledReason ?? 'Apply'}
          type="button"
        >
          <Check aria-hidden="true" size={18} />
        </button>
        <button
          aria-label="Favorite"
          aria-busy={favoritePending || undefined}
          aria-pressed={selectedEntry?.favorite ?? false}
          disabled={controlsDisabled || favoritePending}
          onClick={() => {
            if (selectedEntry) void model.onToggleFavorite(selectedEntry);
            focusStage();
          }}
          title={favoritePending ? 'Saving favorite…' : 'Favorite'}
          type="button"
        >
          <Heart aria-hidden="true" size={18} />
        </button>
        <button
          aria-label={zoomed ? 'Leave zoom' : 'Zoom'}
          aria-pressed={zoomed}
          disabled={!selectedEntry || !model.active || !settled}
          onClick={toggleZoom}
          title={zoomed ? 'Leave zoom' : 'Zoom'}
          type="button"
        >
          {zoomed ? <ZoomOut aria-hidden="true" size={18} /> : <ZoomIn aria-hidden="true" size={18} />}
        </button>
        <button
          aria-label="Details"
          disabled={controlsDisabled}
          onClick={() => {
            focusStage();
            if (selectedEntry) model.onDetails(selectedEntry, stageRef.current);
          }}
          title="Details"
          type="button"
        >
          <Info aria-hidden="true" size={18} />
        </button>
      </div>
      {disabledReason ? (
        <p className="wallpaper-book__disabled-reason" id="book-apply-disabled">{disabledReason}</p>
      ) : null}
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

function WallpaperBookImpl(props: WallpaperBookProps) {
  if (props.model.entries.length === 0) {
    return (
      <LibraryState
        description="Try clearing the active filters or changing your search."
        icon={<SearchX aria-hidden="true" size={28} />}
        role="status"
        title="No wallpapers found"
      />
    );
  }
  if (props.initialAnchorWallpaperId == null && !props.model.currentObservationReady) {
    return (
      <section className="wallpaper-book wallpaper-book--preparing">
        <p role="status">Preparing Book preview…</p>
      </section>
    );
  }
  return <WallpaperBookReady {...props} />;
}

export default memo(WallpaperBookImpl);
