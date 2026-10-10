/** Twelve fragments per wallpaper assemble as the camera reaches their slot. */
import { memo, useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { Check, Heart, SearchX } from 'lucide-react';
import LibraryState from '../LibraryState.tsx';
import LibraryViewSwitch from '../LibraryViewSwitch.tsx';
import WindowHandle from '../WindowHandle.tsx';
import ContextMenu from '../ContextMenu.tsx';
import { ApplyIndicator } from '../ApplyIndicator.tsx';
import { libraryEntryApplyAvailable, libraryEntryApplyDisabledReason } from '../libraryViewModel.ts';
import { displayName } from '../wallpaperCardHelpers.ts';
import { KNOT_CURVES } from './knotCurves.ts';
import { useWallpaperKnotController, type WallpaperKnotProps } from './useWallpaperKnotController.ts';

let hintSeen = false;
function useSessionHint() {
  const [visible, setVisible] = useState(false);
  const expiresAt = useRef<number | null>(null);
  useEffect(() => {
    if (expiresAt.current === null) {
      if (hintSeen) return;
      hintSeen = true;
      try {
        if (sessionStorage.getItem('knot-controls-seen')) return;
        sessionStorage.setItem('knot-controls-seen', '1');
      } catch { /* The module flag also covers restricted storage. */ }
      expiresAt.current = performance.now() + 4000;
    }
    setVisible(true);
    const timer = setTimeout(() => setVisible(false), Math.max(0, expiresAt.current - performance.now()));
    return () => clearTimeout(timer);
  }, []);
  return visible;
}
function ViewSwitch(props: WallpaperKnotProps) {
  return props.onViewModeChange ? <div className="wallpaper-knot__view-switch">
    <LibraryViewSwitch value={props.viewMode ?? 'knot'} onChange={props.onViewModeChange} />
  </div> : null;
}
function WallpaperKnotReady(props: WallpaperKnotProps) {
  const { model } = props;
  const { stageRef, canvasRef, videoRef, clipLoading, selection, selectedEntry, status, knotIndex, reducedMotion,
    switchKnot, applySelected, focusStage, contextMenu, closeContextMenu } = useWallpaperKnotController(props);
  const hintVisible = useSessionHint();
  const name = selectedEntry ? displayName(selectedEntry) : '';
  const total = model.totalKnown && model.total !== null ? model.total : model.entries.length;
  const applyAvailable = Boolean(selectedEntry && libraryEntryApplyAvailable(model.canApplyToDisplay, model.isEntryApplicable, selectedEntry));
  const disabledReason = applyAvailable ? null : selectedEntry
    ? libraryEntryApplyDisabledReason(model.canApplyToDisplay, model.displayApplyDisabledReason, selectedEntry)?.trim() || 'This wallpaper cannot be applied.'
    : 'Select a wallpaper.';
  const controlsDisabled = !selectedEntry || !model.active || !selection.settled || model.queryReplacementPending || status !== 'ready';
  const favoritePending = Boolean(selectedEntry && model.favoritePendingPaths.has(selectedEntry.path));
  const applying = Boolean(selectedEntry && model.applying && model.activePath === selectedEntry.path);
  const pending = Boolean(selectedEntry && model.pendingPath === selectedEntry.path);
  return (
    <section className="wallpaper-knot" aria-label="Knot wallpaper browser" data-reduced-motion={reducedMotion || undefined}>
      <WindowHandle className="wallpaper-knot__window-handle" />
      <ViewSwitch {...props} />
      <div className="wallpaper-knot__stage" ref={stageRef} tabIndex={0}
        aria-label="Wallpaper Knot. Scroll or drag to travel. Hold Shift and drag a piece to pull the rope. Left and Right arrows or Page Up and Page Down step pictures. Home and End jump. Keys 1 to 4 change knots. A toggles automatic travel. Enter applies. Right-click for actions."
        data-settled={selection.settled || undefined} data-ready={status === 'ready' || undefined}>
        <canvas className="wallpaper-knot__canvas" ref={canvasRef} aria-hidden="true" />
        <video className="wallpaper-knot__video" ref={videoRef} aria-hidden="true" loop muted playsInline tabIndex={-1} />
        {status === 'loading' ? <p className="wallpaper-knot__message" role="status">Preparing Knot…</p> : null}
        {status === 'failed' ? <p className="wallpaper-knot__message" role="status">Knot needs WebGL. Choose Grid, Flow or Book to browse this Library.</p> : null}
        {selectedEntry ? <div className="wallpaper-knot__caption">
          {clipLoading ? <span className="wallpaper-knot__clip-loading" role="status" aria-label="Loading video preview" title="Loading video preview" /> : null}
          <span className="wallpaper-knot__name" title={name}>{name}</span>
          {model.currentPath === selectedEntry.path ? <span className="wallpaper-knot__badge">Current</span> : null}
          {selectedEntry.favorite ? <Heart className="wallpaper-knot__badge" aria-label="Favourite wallpaper" size={12} fill="currentColor" /> : null}
          <div className="wallpaper-book__actions wallpaper-knot__actions" aria-label="Selected wallpaper actions">
            <button type="button" aria-label="Apply" title={disabledReason ?? 'Apply'}
              aria-busy={applying || pending || undefined} aria-describedby={disabledReason ? 'knot-apply-disabled' : undefined}
              disabled={controlsDisabled || !applyAvailable} onClick={applySelected}>
              {applying || pending ? <ApplyIndicator state={applying ? 'applying' : 'pending'} /> : <Check aria-hidden="true" size={16} />}
            </button>
            <button type="button" aria-label="Favourite" title={favoritePending ? 'Saving favourite…' : 'Favourite'}
              aria-busy={favoritePending || undefined} aria-pressed={selectedEntry.favorite}
              disabled={controlsDisabled || favoritePending}
              onClick={() => { void model.onToggleFavorite(selectedEntry); focusStage(); }}>
              <Heart aria-hidden="true" size={16} />
            </button>
          </div>
        </div> : null}
        {disabledReason ? <p className="wallpaper-knot__announcement" id="knot-apply-disabled">{disabledReason}</p> : null}
      </div>
      <div className="wallpaper-knot__knots" role="group" aria-label="Knot path">
        {KNOT_CURVES.map((curve, index) => <button key={curve.id} type="button" aria-label={curve.label}
          aria-pressed={knotIndex === index} disabled={status !== 'ready' || !model.active || model.queryReplacementPending}
          title={curve.label} onClick={() => { switchKnot(index); focusStage(); }} />)}
      </div>
      <p className="wallpaper-knot__hint" aria-hidden={!hintVisible} data-visible={hintVisible || undefined}>Scroll or drag · Shift-drag a piece to pull · ← → step · 1–4 knot · A travel · Enter apply</p>
      <p className="wallpaper-knot__announcement" aria-live="polite" aria-atomic="true">
        {selection.settled && selectedEntry ? `${name}, ${selection.index + 1} of ${total}` : ''}
        {model.loadingMore ? ' Loading more…' : ''}
        {model.appendNeedsRetry ? ' Press End to retry loading more.' : ''}
      </p>
      {contextMenu ? createPortal(<div className="wallpaper-book__menu-layer">
        <ContextMenu actions={model.buildContextActions(contextMenu.entry)} onClose={closeContextMenu}
          path={contextMenu.entry.path} x={contextMenu.x} y={contextMenu.y} />
      </div>, document.body) : null}
    </section>
  );
}

function WallpaperKnot(props: WallpaperKnotProps) {
  if (props.model.entries.length === 0 || (props.initialAnchorWallpaperId == null && !props.model.currentObservationReady)) return (
    <section className="wallpaper-knot" aria-label="Knot wallpaper browser">
      <WindowHandle className="wallpaper-knot__window-handle" />
      <ViewSwitch {...props} />
      <div aria-label="Wallpaper Knot" className="wallpaper-knot__stage" tabIndex={0}>
        {props.model.entries.length === 0 ? <LibraryState description="Try clearing the active filters or changing your search."
          icon={<SearchX aria-hidden="true" size={28} />} role="status" title="No wallpapers found" /> : <p role="status">Preparing Knot…</p>}
      </div>
    </section>
  );
  return <WallpaperKnotReady {...props} />;
}
export default memo(WallpaperKnot);
