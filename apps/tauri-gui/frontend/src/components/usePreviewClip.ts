import { useLayoutEffect, useRef, type RefObject } from 'react';
import { api } from '../api/bridge.ts';
import { PreviewClipPlayer, PreviewClipUrlCache, watchPreviewClipFocus } from './previewClip.ts';

let sharedCache: PreviewClipUrlCache | null = null;
let owners = 0;

/** Shared across mounted Flow/Book media; the last unmount revokes every cached URL. */
function retainCache() {
  const cache = sharedCache ??= new PreviewClipUrlCache((path) => api.previewClip(path));
  owners += 1;
  return { cache, release: () => {
    owners -= 1;
    if (owners === 0) { cache.dispose(); sharedCache = null; }
  } };
}

export function usePreviewClip(
  entryPath: string,
  videoRef: RefObject<HTMLVideoElement | null>,
  enabled: boolean,
  path: string | null,
  onRunning: () => void,
  onFailure: () => void,
  fadeInMs?: number,
): void {
  const playerRef = useRef<PreviewClipPlayer | null>(null);
  const callbacks = useRef({ onRunning, onFailure });
  callbacks.current = { onRunning, onFailure };
  useLayoutEffect(() => {
    const video = videoRef.current;
    if (!enabled || !video) return;
    const owner = retainCache();
    const player = new PreviewClipPlayer(video, owner.cache, {
      onRunning: () => callbacks.current.onRunning(),
      onFailure: () => callbacks.current.onFailure(),
      fadeInMs,
    });
    playerRef.current = player;
    const stopWatching = watchPreviewClipFocus(player);
    return () => {
      stopWatching();
      player.dispose();
      playerRef.current = null;
      owner.release();
    };
  }, [enabled, entryPath, fadeInMs, videoRef]);
  useLayoutEffect(() => { playerRef.current?.want(path); }, [enabled, entryPath, path]);
}
