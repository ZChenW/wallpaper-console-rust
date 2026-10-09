/** First Knot prototype: navigation and still previews only. */
import { memo, useEffect } from 'react';
import { SearchX } from 'lucide-react';
import LibraryState from '../LibraryState.tsx';
import { displayName } from '../wallpaperCardHelpers.ts';
import { safeFileSrc } from '../safeFileSrc.ts';
import { useAuthorizedPreviewAsset } from '../useAuthorizedPreviewAsset.ts';
import { useWallpaperKnotController, type WallpaperKnotProps } from './useWallpaperKnotController.ts';

function WallpaperKnotReady(props: WallpaperKnotProps) {
  const { model } = props;
  const { stageRef, canvasRef, engineRef, selection, selectedEntry, status } = useWallpaperKnotController(props);
  const original = useAuthorizedPreviewAsset(
    status === 'ready' && selection.settled && model.active && selectedEntry?.type === 'image' ? selectedEntry.path : null,
    selectedEntry?.path ?? null,
  );
  useEffect(() => {
    const engine = engineRef.current;
    const key = original.path && selectedEntry ? `original:${selectedEntry.path}` : null;
    engine?.setOriginal(key, original.path ? safeFileSrc(original.path) : null);
    return () => engine?.setOriginal(null, null);
  }, [engineRef, original.path, selectedEntry]);
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
      <div className="wallpaper-knot__stage" ref={stageRef} tabIndex={0}
        aria-label="Wallpaper Knot. Use Left and Right arrows to browse, Home and End to jump, drag to orbit, R to reset."
        data-settled={selection.settled || undefined} data-ready={status === 'ready' || undefined}>
        <canvas className="wallpaper-knot__canvas" ref={canvasRef} aria-hidden="true" />
        {status === 'loading' ? <p className="wallpaper-knot__message" role="status">Preparing Knot…</p> : null}
        {status === 'failed' ? <p className="wallpaper-knot__message" role="status">Knot needs WebGL. Choose Grid, Flow or Book to browse this Library.</p> : null}
        {status === 'ready' && selectedEntry ? <p className="wallpaper-knot__caption" aria-hidden="true">{name}</p> : null}
      </div>
      <p className="wallpaper-knot__hint">Scroll or ← → to browse · Drag to orbit · Click a picture to select · Double-click empty space or R to reset</p>
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
