import { memo, useMemo } from 'react';
import { createPortal } from 'react-dom';
import { Check, Heart, Info, Maximize2, Minimize2, SearchX, ZoomIn, ZoomOut } from 'lucide-react';

import type { LibraryBrowserItemDTO } from '../api/types.ts';
import { largePreviewKey, type EnhancedMediaEligibility } from './wallpaperPreviewMedia.ts';
import { ApplyIndicator } from './ApplyIndicator.tsx';
import ContextMenu from './ContextMenu.tsx';
import LibraryState from './LibraryState.tsx';
import WallpaperPreviewMedia from './WallpaperPreviewMedia.tsx';
import { useThumbnail } from '../state/ThumbnailStoreContext.tsx';
import BookZoomPicture from './BookZoomPicture.tsx';
import { libraryEntryApplyAvailable, libraryEntryApplyDisabledReason } from './libraryViewModel.ts';
import { bookLeafTransform, bookStaticSource, bookWallpaperIndex, type BookFace } from './wallpaperBookModel.ts';
import { flowStateLabels } from './wallpaperFlowModel.ts';
import { displayName } from './wallpaperCardHelpers.ts';
import { useWallpaperBookController, type WallpaperBookProps } from './useWallpaperBookController.ts';

export type { WallpaperBookProps } from './useWallpaperBookController.ts';

/** Stable media props: motion/window renders cannot restart decode or eligibility. */
const BookPageMedia = memo(function BookPageMedia({ entry, open, selected, active, reducedMotion }: {
  entry: LibraryBrowserItemDTO; open: boolean; selected: boolean; active: boolean; reducedMotion: boolean; moving: boolean;
}) {
  const { failure } = useThumbnail(bookStaticSource(entry).thumbnailPath);
  const base = bookStaticSource(entry, Boolean(failure));
  // An open page is shown large: ask for the 1600 px preview. Pile strips keep the grid-size one.
  const source = useMemo(() => ({
    thumbnailPath: base.thumbnailPath,
    fallbackPath: base.fallbackPath,
    largeThumbnailPath: open ? largePreviewKey(base.thumbnailPath) : null,
  }), [base.thumbnailPath, base.fallbackPath, open]);
  const eligibility = useMemo<EnhancedMediaEligibility>(() => ({
    active, centered: open && selected, selected: open && selected, settled: true, reducedMotion,
  }), [active, open, selected, reducedMotion]);
  return <WallpaperPreviewMedia entry={entry} alt="" eligibility={eligibility}
    loading="eager" staticFallback={open} staticSource={source} stabilizeEntranceDuringMotion captureFrame />;
}, (previous, next) => previous.entry === next.entry && (next.moving || (
  previous.open === next.open && previous.selected === next.selected
  && previous.active === next.active && previous.reducedMotion === next.reducedMotion
  && previous.moving === next.moving
)));

function WallpaperBookReady(props: WallpaperBookProps) {
  const { model } = props;
  const {
    elements: { stageRef, spreadElementRef, zoomRef, zoomMediaRef, zoomStillRef, zoomDecorationRef, leavesRef },
    snapshot: {
      spread, leafKeyOffset, selectedIndex, selectedEntry, settled, zoomIndex, zoomMoving, zoomStillSrc,
      contextMenu, reducedMotion, interactionActive, visibleLeaves, pageScale,
    },
    actions: {
      handlePointerDown, handlePointerMove, finishPointer, handleKeyDown, handleEscape,
      handlePageClick, openContextMenu, applySelected, toggleZoom, closeZoom,
      closeContextMenu, focusStage, cancelPendingMenu, revealZoomLive, mayUpdateZoomStill,
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
        data-book-index={index ?? undefined}
        data-open={open || undefined}
        data-selected={selected || undefined}
        data-zoom-source={zoomed && index === zoomIndex || undefined}
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
            <BookPageMedia
              key={entry.path} entry={entry} open={open} selected={selected}
              active={interactionActive} reducedMotion={reducedMotion} moving={!settled} />
          ) : null}
        </div>
        <span aria-hidden="true" className="book-leaf__spine-shadow" />
        <span aria-hidden="true" className="book-leaf__shade" />
        <span aria-hidden="true" className="book-leaf__highlight" />
        {entry && open ? (
          <span className="book-leaf__states">
            {flowStateLabels({
              selected: false,
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

  const actions = (
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
      <button
        aria-label={props.immersive ? 'Exit immersive view' : 'Enter immersive view'}
        aria-pressed={props.immersive ?? false}
        disabled={!model.active || !props.onImmersiveChange}
        onClick={() => {
          props.onImmersiveChange?.(!props.immersive);
          focusStage();
        }}
        title={props.immersive ? 'Exit immersive view' : 'Enter immersive view'}
        type="button"
      >
        {props.immersive ? <Minimize2 aria-hidden="true" size={18} /> : <Maximize2 aria-hidden="true" size={18} />}
      </button>
    </div>
  );
  const disabledReasonLabel = disabledReason ? (
    <p className="wallpaper-book__disabled-reason" id="book-apply-disabled">{disabledReason}</p>
  ) : null;
  const zoomLayer = selectedEntry ? (
    <div className="wallpaper-book__zoom-layer" data-reduced-motion={reducedMotion || undefined}>
      <div aria-hidden="true" className="wallpaper-book__zoom-backdrop" onClick={closeZoom} />
      <div className="wallpaper-book__zoom-content">
        <div className="wallpaper-book__zoom-projection">
          <div
            aria-current={model.currentPath === selectedEntry.path ? 'true' : undefined}
            aria-label={displayName(selectedEntry)}
            aria-posinset={(zoomIndex ?? selectedIndex) + 1}
            aria-selected="true"
            aria-setsize={model.totalKnown && model.total !== null ? model.total : undefined}
            className="wallpaper-book__zoom-page"
            data-moving={zoomMoving || undefined}
            id={activeId}
            onClick={(event) => event.stopPropagation()}
            onContextMenu={(event) => {
              event.preventDefault();
              openContextMenu(zoomIndex ?? selectedIndex, event.clientX, event.clientY);
            }}
            ref={zoomRef}
            role="option"
          >
            <div className="wallpaper-book__zoom-print">
              <div className="wallpaper-book__zoom-picture">
                <BookZoomPicture entry={selectedEntry}
                  active={interactionActive} reducedMotion={reducedMotion} moving={zoomMoving}
                  stillSrc={zoomStillSrc} mediaRef={zoomMediaRef} stillRef={zoomStillRef}
                  onLiveReady={revealZoomLive} mayUpdateStill={mayUpdateZoomStill} />
              </div>
            </div>
            <div aria-hidden="true" className="book-leaf__face wallpaper-book__zoom-decoration"
              data-open data-selected ref={zoomDecorationRef}>
              <div className="book-leaf__print" />
              <span className="book-leaf__spine-shadow" style={{
                background: `linear-gradient(to ${((zoomIndex ?? selectedIndex) % 2 === 0) ? 'right' : 'left'}, color-mix(in srgb, var(--text) 11%, transparent), transparent 9%)`,
              }} />
              <span className="book-leaf__states">
                {flowStateLabels({
                  selected: false,
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
        </div>
        {actions}
        {disabledReasonLabel}
      </div>
    </div>
  ) : null;

  return (
    <section
      aria-label="Book wallpaper browser"
      className={`wallpaper-book${model.refreshing ? ' is-refreshing' : ''}`}
      data-reduced-motion={reducedMotion || undefined}
      data-zoomed={zoomed || undefined}
      onClickCapture={cancelPendingMenu}
      onPointerDownCapture={cancelPendingMenu}
      onKeyDownCapture={cancelPendingMenu}
      onKeyDown={handleEscape}
      onWheelCapture={cancelPendingMenu}
    >
      <header className="wallpaper-book__heading" inert={zoomed} aria-hidden={zoomed || undefined}>
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
        aria-owns={zoomed ? activeId : undefined}
        aria-label="Wallpaper Book"
        aria-multiselectable={false}
        className="wallpaper-book__stage"
        data-moving={!settled || undefined}
        data-zoomed={zoomed || undefined}
        data-selectable={selectedEntry !== null || undefined}
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
          <div
            aria-hidden="true"
            className="wallpaper-book__contact-shadow"
            data-side={spread === 0 ? 'right' : spread * 2 >= model.entries.length ? 'left' : undefined}
          />
          <div className="wallpaper-book__spread" ref={spreadElementRef}>
            {visibleLeaves.map((leaf) => {
              const transform = bookLeafTransform(leaf, spread, reducedMotion, pageScale);
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
        {zoomed && selectedEntry ? createPortal(zoomLayer, document.body) : null}
      </div>
      {zoomed ? <div aria-hidden="true" className="wallpaper-book__actions wallpaper-book__actions-reserve" /> : actions}
      {zoomed && disabledReason ? <p aria-hidden="true" className="wallpaper-book__disabled-reason wallpaper-book__reason-reserve">{disabledReason}</p> : disabledReasonLabel}
      {contextMenu ? createPortal(
        <div className="wallpaper-book__menu-layer" data-reduced-motion={reducedMotion || undefined}>
          <ContextMenu
            actions={model.buildContextActions(contextMenu.entry)}
            onClose={closeContextMenu}
            path={contextMenu.entry.path}
            x={contextMenu.x}
            y={contextMenu.y}
          />
        </div>, document.body,
      ) : null}
    </section>
  );
}

function WallpaperBookImpl(props: WallpaperBookProps) {
  if (props.model.entries.length === 0) {
    return (
      <div aria-label="Wallpaper Book" className="wallpaper-book__stage" tabIndex={0}>
        <LibraryState
          description="Try clearing the active filters or changing your search."
          icon={<SearchX aria-hidden="true" size={28} />}
          role="status"
          title="No wallpapers found"
        />
      </div>
    );
  }
  if (props.initialAnchorWallpaperId == null && !props.model.currentObservationReady) {
    return (
      <section aria-label="Wallpaper Book" className="wallpaper-book wallpaper-book--preparing wallpaper-book__stage" tabIndex={0}>
        <p role="status">Preparing Book preview…</p>
      </section>
    );
  }
  return <WallpaperBookReady {...props} />;
}

export default memo(WallpaperBookImpl);
