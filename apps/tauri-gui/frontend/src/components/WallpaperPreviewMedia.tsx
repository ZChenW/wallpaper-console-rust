import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from 'react';
import type { LibraryBrowserItemDTO } from '../api/types.ts';
import { useThumbnailStore } from '../state/ThumbnailStoreContext.tsx';
import { typeIcon } from './wallpaperCardHelpers.ts';
import {
  attachVideoDecoder,
  enhancedMediaCandidates,
  staticPreviewAssetPath,
  type EnhancedMediaEligibility,
} from './wallpaperPreviewMedia.ts';
import { useAuthorizedPreviewAsset } from './useAuthorizedPreviewAsset.ts';
import { safeFileSrc } from './safeFileSrc.ts';

export { safeFileSrc } from './safeFileSrc.ts';

const STATIC_ELIGIBILITY: EnhancedMediaEligibility = Object.freeze({
  active: false,
  centered: false,
  selected: false,
  settled: false,
  reducedMotion: false,
});

export interface WallpaperPreviewMediaProps {
  readonly entry: LibraryBrowserItemDTO;
  readonly alt?: string;
  readonly className?: string;
  readonly eligibility?: EnhancedMediaEligibility;
  /** Grid's existing hover-GIF seam; Flow uses `eligibility` instead. */
  readonly transientImagePath?: string | null;
  readonly loading?: 'eager' | 'lazy';
  readonly onEnhancedError?: (message: string) => void;
}

export default function WallpaperPreviewMedia({
  entry,
  alt = '',
  className,
  eligibility = STATIC_ELIGIBILITY,
  transientImagePath = null,
  loading = 'lazy',
  onEnhancedError,
}: WallpaperPreviewMediaProps) {
  const store = useThumbnailStore();
  const assetPath = staticPreviewAssetPath(entry);
  const subscribeThumbnail = useCallback(
    (callback: () => void) => store.subscribe(assetPath, callback),
    [assetPath, store],
  );
  const getThumbnail = useCallback(() => store.get(assetPath), [assetPath, store]);
  const getThumbnailFailure = useCallback(
    () => store.getFailure(assetPath),
    [assetPath, store],
  );
  const thumbnail = useSyncExternalStore(subscribeThumbnail, getThumbnail, getThumbnail);
  const thumbnailFailure = useSyncExternalStore(
    subscribeThumbnail,
    getThumbnailFailure,
    getThumbnailFailure,
  );
  const candidates = useMemo(() => {
    if (transientImagePath) return [{ kind: 'image' as const, path: transientImagePath }];
    return enhancedMediaCandidates(entry, eligibility);
  }, [eligibility, entry, transientImagePath]);
  const candidateKey = candidates.map((candidate) => `${candidate.kind}:${candidate.path}`).join('\0');
  const [candidateIndex, setCandidateIndex] = useState(0);
  const [enhancedError, setEnhancedError] = useState<string | null>(null);
  const [thumbnailLoadFailed, setThumbnailLoadFailed] = useState(false);
  const videoRef = useRef<HTMLVideoElement | null>(null);

  useEffect(() => {
    setCandidateIndex(0);
    setEnhancedError(null);
  }, [candidateKey]);

  useEffect(() => {
    setThumbnailLoadFailed(false);
  }, [assetPath, thumbnail]);

  const activeCandidate = candidates[candidateIndex] ?? null;
  const authorizedCandidate = useAuthorizedPreviewAsset(
    activeCandidate?.path ?? null,
    entry.path,
  );
  const activeVideoSource = activeCandidate?.kind === 'video' && authorizedCandidate.path
    ? safeFileSrc(authorizedCandidate.path)
    : null;
  const setVideoRef = useCallback((video: HTMLVideoElement | null) => {
    videoRef.current = attachVideoDecoder(videoRef.current, video, activeVideoSource);
  }, [activeVideoSource]);
  const handleEnhancedError = useCallback(() => {
    const nextIndex = candidateIndex + 1;
    const message = `Enhanced preview unavailable for ${entry.title || entry.path}`;
    if (nextIndex < candidates.length) {
      setCandidateIndex(nextIndex);
      return;
    }
    setCandidateIndex(candidates.length);
    setEnhancedError(message);
    onEnhancedError?.(message);
  }, [candidateIndex, candidates.length, entry.path, entry.title, onEnhancedError]);

  useEffect(() => {
    if (!authorizedCandidate.error) return;
    handleEnhancedError();
  }, [authorizedCandidate.error, handleEnhancedError]);

  if (activeCandidate?.kind === 'video' && authorizedCandidate.path) {
    return (
      <>
        <video
          autoPlay
          className={className}
          data-enhanced-preview="video"
          key={`${activeCandidate.kind}:${activeCandidate.path}`}
          loop
          muted
          onError={handleEnhancedError}
          playsInline
          preload="metadata"
          ref={setVideoRef}
          src={activeVideoSource ?? undefined}
        />
        {enhancedError && !thumbnail ? <span className="wallpaper-preview-status" role="status">Preview unavailable</span> : null}
      </>
    );
  }

  const imagePath = authorizedCandidate.path ?? (thumbnailLoadFailed ? undefined : thumbnail);
  if (imagePath) {
    return (
      <>
        <img
          alt={alt}
          className={className}
          data-enhanced-preview={authorizedCandidate.path ? 'image' : undefined}
          decoding="async"
          draggable={false}
          loading={loading}
          onError={authorizedCandidate.path
            ? handleEnhancedError
            : () => setThumbnailLoadFailed(true)}
          src={safeFileSrc(imagePath)}
        />
        {enhancedError && !thumbnail ? <span className="wallpaper-preview-status" role="status">Preview unavailable</span> : null}
      </>
    );
  }

  return (
    <div
      className={`wallpaper-thumb-placeholder${className ? ` ${className}` : ''}`}
      title={thumbnailFailure ? `Preview failed: ${thumbnailFailure}` : undefined}
    >
      <span className="wallpaper-type-icon">{typeIcon(entry.type)}</span>
      {enhancedError ? (
        <span className="wallpaper-preview-status" role="status">Preview unavailable</span>
      ) : thumbnailFailure || thumbnailLoadFailed ? (
        <span className="wallpaper-thumb-error">Preview failed</span>
      ) : null}
    </div>
  );
}
