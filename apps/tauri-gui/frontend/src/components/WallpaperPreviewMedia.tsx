import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import type { LibraryBrowserItemDTO } from '../api/types.ts';
import { useThumbnail } from '../state/ThumbnailStoreContext.tsx';
import { typeIcon } from './wallpaperCardHelpers.ts';
import {
  attachVideoDecoder,
  ENHANCED_MEDIA_ACTIVATION_DELAY_MS,
  enhancedMediaActivationPlan,
  enhancedMediaCandidates,
  previewImagePath,
  staticFallbackAssetPath,
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
  readonly staticFallback?: boolean;
  readonly stabilizeEntranceDuringMotion?: boolean;
  readonly onEnhancedError?: (message: string) => void;
}

export default function WallpaperPreviewMedia({
  entry,
  alt = '',
  className,
  eligibility = STATIC_ELIGIBILITY,
  transientImagePath = null,
  loading = 'lazy',
  staticFallback = false,
  stabilizeEntranceDuringMotion = false,
  onEnhancedError,
}: WallpaperPreviewMediaProps) {
  const assetPath = staticPreviewAssetPath(entry);
  const { thumbnail, failure: thumbnailFailure } = useThumbnail(assetPath);
  const [staticFallbackLoadFailed, setStaticFallbackLoadFailed] = useState(false);
  const fallbackAssetPath = staticFallbackAssetPath(entry, staticFallback);
  const authorizedStaticFallback = useAuthorizedPreviewAsset(
    thumbnail || staticFallbackLoadFailed ? null : fallbackAssetPath,
    entry.path,
  );
  const [enhancedActivatedPath, setEnhancedActivatedPath] = useState<string | null>(null);
  const activationPlan = enhancedMediaActivationPlan(
    entry,
    enhancedActivatedPath === entry.path,
    eligibility,
  );
  useEffect(() => {
    if (activationPlan.retain) return undefined;
    if (!activationPlan.schedule) {
      setEnhancedActivatedPath(null);
      return undefined;
    }
    const timer = window.setTimeout(
      () => setEnhancedActivatedPath(entry.path),
      ENHANCED_MEDIA_ACTIVATION_DELAY_MS,
    );
    return () => window.clearTimeout(timer);
  }, [activationPlan.retain, activationPlan.schedule, entry.path]);
  const candidates = useMemo(() => {
    if (transientImagePath) return [{ kind: 'image' as const, path: transientImagePath }];
    return enhancedMediaCandidates(entry, {
      ...eligibility,
      settled: activationPlan.retain,
    });
  }, [activationPlan.retain, eligibility, entry, transientImagePath]);
  const candidateKey = candidates.map((candidate) => `${candidate.kind}:${candidate.path}`).join('\0');
  const [candidateIndex, setCandidateIndex] = useState(0);
  const [enhancedError, setEnhancedError] = useState<string | null>(null);
  const [thumbnailLoadFailed, setThumbnailLoadFailed] = useState(false);
  const [loadedImage, setLoadedImage] = useState<{
    path: string;
    stableEntry: boolean;
  } | null>(null);
  const videoRef = useRef<HTMLVideoElement | null>(null);

  useEffect(() => {
    setCandidateIndex(0);
    setEnhancedError(null);
  }, [candidateKey]);

  useEffect(() => {
    setThumbnailLoadFailed(false);
  }, [assetPath, thumbnail]);

  useEffect(() => {
    setStaticFallbackLoadFailed(false);
  }, [fallbackAssetPath]);

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

  const imagePath = previewImagePath({
    candidateKind: activeCandidate?.kind ?? null,
    authorizedCandidatePath: authorizedCandidate.path,
    authorizedStaticFallbackPath: authorizedStaticFallback.path,
    staticFallbackLoadFailed,
    thumbnail,
    thumbnailLoadFailed,
  });
  const videoPosterPath = (staticFallbackLoadFailed ? null : authorizedStaticFallback.path)
    ?? (thumbnailLoadFailed ? undefined : thumbnail);
  const imageLoaded = imagePath !== null
    && imagePath !== undefined
    && loadedImage?.path === imagePath;
  const imageEntryStable = imageLoaded && loadedImage.stableEntry;

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
          poster={videoPosterPath ? safeFileSrc(videoPosterPath) : undefined}
          preload="metadata"
          ref={setVideoRef}
          src={activeVideoSource ?? undefined}
        />
        {enhancedError && !thumbnail ? <span className="wallpaper-preview-status" role="status">Preview unavailable</span> : null}
      </>
    );
  }

  if (imagePath) {
    return (
      <>
        {!imageLoaded ? (
          <span
            aria-hidden="true"
            className="wallpaper-thumb-placeholder wallpaper-thumb-placeholder--loading"
          >
            <span className="wallpaper-type-icon">{typeIcon(entry.type)}</span>
          </span>
        ) : null}
        <img
          alt={alt}
          className={['wallpaper-preview-image', className].filter(Boolean).join(' ')}
          data-enhanced-preview={authorizedCandidate.path ? 'image' : undefined}
          data-preview-entry-stable={imageEntryStable || undefined}
          data-preview-loaded={imageLoaded || undefined}
          decoding="async"
          draggable={false}
          loading={loading}
          onError={() => {
            setLoadedImage(null);
            if (authorizedCandidate.path) {
              handleEnhancedError();
            } else if (authorizedStaticFallback.path) {
              setStaticFallbackLoadFailed(true);
            } else {
              setThumbnailLoadFailed(true);
            }
          }}
          onLoad={() => setLoadedImage({
            path: imagePath,
            stableEntry: stabilizeEntranceDuringMotion && !eligibility.settled,
          })}
          src={safeFileSrc(imagePath)}
        />
        {enhancedError && !thumbnail ? <span className="wallpaper-preview-status" role="status">Preview unavailable</span> : null}
      </>
    );
  }

  return (
    <div
      className={`wallpaper-thumb-placeholder${className ? ` ${className}` : ''}`}
    >
      <span className="wallpaper-type-icon">{typeIcon(entry.type)}</span>
      {enhancedError ? (
        <span className="wallpaper-preview-status" role="status">Preview unavailable</span>
      ) : thumbnailFailure || thumbnailLoadFailed ? (
        <span
          aria-label="Preview unavailable"
          className="wallpaper-thumb-error"
          title={thumbnailFailure ? `Preview unavailable: ${thumbnailFailure}` : 'Preview unavailable'}
        >!</span>
      ) : null}
    </div>
  );
}
