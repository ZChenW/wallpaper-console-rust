/** Scroll through a mathematical knot of still previews. */
import { memo } from 'react';
import { SearchX } from 'lucide-react';
import LibraryState from '../LibraryState.tsx';
import { displayName } from '../wallpaperCardHelpers.ts';
import { KNOT_CURVES } from './knotCurves.ts';
import { useWallpaperKnotController, type WallpaperKnotProps } from './useWallpaperKnotController.ts';

function WallpaperKnotReady(props: WallpaperKnotProps) {
  const { model } = props;
  const { stageRef, canvasRef, selection, selectedEntry, status,
    knotIndex, autoTravel, reducedMotion, switchKnot, toggleAuto } = useWallpaperKnotController(props);
  const name = selectedEntry ? displayName(selectedEntry) : '';
  const total = model.totalKnown && model.total !== null ? model.total : model.entries.length;
  return (
    <section className="wallpaper-knot" aria-label="Knot wallpaper browser">
      <header className="wallpaper-knot__heading">
        <h3>Library</h3><p>{total} wallpapers</p>
        {model.loadingMore ? <p role="status">Loading more…</p> : null}
        {model.appendNeedsRetry ? <button type="button" disabled={model.loadingMore}
          title={model.loadErrorDetail ?? 'Load more wallpapers'}
          onClick={() => { void model.onAppendMore(); stageRef.current?.focus({ preventScroll: true }); }}>Load more</button> : null}
      </header>
      <div className="wallpaper-knot__controls" role="group" aria-label="Knot path">
        {KNOT_CURVES.map((curve, index) => <button key={curve.id} type="button"
          aria-pressed={knotIndex === index} disabled={status !== 'ready'}
          title={`${index + 1}: ${curve.label}`}
          onClick={() => { switchKnot(index); stageRef.current?.focus({ preventScroll: true }); }}>{curve.label}</button>)}
        <button type="button" aria-pressed={autoTravel} disabled={reducedMotion || status !== 'ready'}
          title={reducedMotion ? 'Automatic travel is off with reduced motion' : 'A: Toggle automatic travel'}
          onClick={() => { toggleAuto(); stageRef.current?.focus({ preventScroll: true }); }}>Auto travel</button>
      </div>
      <div className="wallpaper-knot__stage" ref={stageRef} tabIndex={0}
        aria-label="Wallpaper Knot. Scroll or drag to travel. Left and Right arrows or Page Up and Page Down step pictures. Home and End jump. Keys 1 to 5 change knots. A toggles automatic travel."
        data-settled={selection.settled || undefined} data-ready={status === 'ready' || undefined}>
        <canvas className="wallpaper-knot__canvas" ref={canvasRef} aria-hidden="true" />
        {status === 'loading' ? <p className="wallpaper-knot__message" role="status">Preparing Knot…</p> : null}
        {status === 'failed' ? <p className="wallpaper-knot__message" role="status">Knot needs WebGL. Choose Grid, Flow or Book to browse this Library.</p> : null}
        {status === 'ready' && selectedEntry ? <p className="wallpaper-knot__caption" aria-hidden="true">{name}</p> : null}
      </div>
      <p className="wallpaper-knot__hint">Scroll or drag to travel · ← → / PgUp PgDn step · Home / End jump · Click a picture to visit · 1–5 change knot · A auto travel</p>
      <p className="wallpaper-knot__announcement" aria-live="polite" aria-atomic="true">
        {selection.settled && selectedEntry ? `${name}, ${selection.index + 1} of ${total}` : ''}
      </p>
    </section>
  );
}

function WallpaperKnot(props: WallpaperKnotProps) {
  if (props.model.entries.length === 0) return (
    <div aria-label="Wallpaper Knot" className="wallpaper-knot__stage" tabIndex={0}>
      <LibraryState description="Try clearing the active filters or changing your search."
        icon={<SearchX aria-hidden="true" size={28} />} role="status" title="No wallpapers found" />
    </div>
  );
  if (props.initialAnchorWallpaperId == null && !props.model.currentObservationReady) return (
    <section className="wallpaper-knot__stage" aria-label="Wallpaper Knot" tabIndex={0}>
      <p role="status">Preparing Knot…</p>
    </section>
  );
  return <WallpaperKnotReady {...props} />;
}
export default memo(WallpaperKnot);
