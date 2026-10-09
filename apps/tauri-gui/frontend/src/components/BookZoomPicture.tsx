import { useCallback, useEffect, useMemo, useRef, useState, type RefObject } from 'react';
import type { LibraryBrowserItemDTO } from '../api/types.ts';
import { useThumbnail } from '../state/ThumbnailStoreContext.tsx';
import { useAuthorizedPreviewAsset } from './useAuthorizedPreviewAsset.ts';
import { safeFileSrc } from './safeFileSrc.ts';
import { attachVideoDecoder } from './wallpaperPreviewMedia.ts';
import { bookStaticSource } from './wallpaperBookModel.ts';
import { decodeBookStill, watchBookVideoReady } from './wallpaperBookZoomMedia.ts';

export function bookZoomLiveAsset(entry: LibraryBrowserItemDTO) {
  if (entry.type === 'video') return { kind: 'video' as const, path: entry.path };
  if (entry.type === 'gif' || entry.ext.toLowerCase() === 'gif') return { kind: 'image' as const, path: entry.path };
  if (entry.previewPath && /\.(gif|apng)$/i.test(entry.previewPath)) return { kind: 'image' as const, path: entry.previewPath };
  return null;
}

function BookZoomLive({ entry, onReady }: { entry: LibraryBrowserItemDTO; onReady: () => void }) {
  const candidate = bookZoomLiveAsset(entry);
  const authorized = useAuthorizedPreviewAsset(candidate?.path ?? null, entry.path);
  const source = authorized.path ? safeFileSrc(authorized.path) : null;
  const videoRef = useRef<HTMLVideoElement>(null);
  const setVideoRef = useCallback((video: HTMLVideoElement | null) => {
    videoRef.current = attachVideoDecoder(videoRef.current, video, source);
  }, [source]);
  useEffect(() => {
    const video = videoRef.current;
    if (!video || !source) return;
    const stopWatching = watchBookVideoReady(video, onReady);
    void video.play().catch(() => { /* Keep the still when playback is unavailable. */ });
    return stopWatching;
  }, [onReady, source]);
  if (!source) return null;
  if (candidate?.kind === 'video') return <video ref={setVideoRef} src={source} autoPlay muted loop playsInline
    preload="auto" data-enhanced-preview="video" />;
  return <img src={source} alt="" draggable={false} data-enhanced-preview="image" onLoad={(event) => {
    const image = event.currentTarget;
    void image.decode().then(() => { if (image.isConnected && image.src === source) onReady(); }, () => {});
  }} />;
}

/** The same img survives opening, live reveal, navigation, and closing. */
export default function BookZoomPicture({ entry, active, reducedMotion, moving, stillSrc,
  stillRef, mediaRef, onLiveReady, mayUpdateStill }: {
  entry: LibraryBrowserItemDTO; active: boolean; reducedMotion: boolean; moving: boolean;
  stillSrc: string | null; stillRef: RefObject<HTMLImageElement | null>; mediaRef: RefObject<HTMLDivElement | null>;
  onLiveReady: () => void; mayUpdateStill: () => boolean;
}) {
  const [liveEntry, setLiveEntry] = useState<string | null>(null);
  const live = useMemo(() => bookZoomLiveAsset(entry), [entry]);
  useEffect(() => {
    // Once mounted, loss of focus or a close cannot remove/re-hide a live frame.
    if (!moving && mayUpdateStill() && active && !reducedMotion && live) setLiveEntry(entry.path);
  }, [active, entry.path, live, mayUpdateStill, moving, reducedMotion]);
  const { failure } = useThumbnail(bookStaticSource(entry).thumbnailPath);
  const { thumbnailPath, fallbackPath: fallback } = bookStaticSource(entry, Boolean(failure));
  const staticPath = fallback && (!live || !stillSrc) && !/\.(gif|apng)$/i.test(fallback) ? fallback : null;
  const authorized = useAuthorizedPreviewAsset(staticPath, entry.path);
  const { thumbnail } = useThumbnail(thumbnailPath);
  const staticSource = authorized.path ?? (!stillSrc ? thumbnail : null);
  useEffect(() => {
    if (moving || !staticSource || !mayUpdateStill()) return;
    const source = safeFileSrc(staticSource);
    const still = stillRef.current;
    if (!still || source === still.src) return;
    let current = true;
    void decodeBookStill(source, (image) => {
      if (!still.src || image.naturalWidth > still.naturalWidth || image.naturalHeight > still.naturalHeight) still.src = image.src;
    },
      () => current && stillRef.current === still && mayUpdateStill());
    return () => { current = false; };
  }, [staticSource, entry.path, mayUpdateStill, moving, stillRef]);
  return <>
    {live && liveEntry === entry.path ? <div className="wallpaper-book__zoom-media" ref={mediaRef}>
      <BookZoomLive key={entry.path} entry={entry} onReady={onLiveReady} />
    </div> : null}
    <img alt="" className="wallpaper-book__zoom-still" src={stillSrc ?? undefined}
      ref={stillRef} draggable={false} loading="eager" />
  </>;
}
