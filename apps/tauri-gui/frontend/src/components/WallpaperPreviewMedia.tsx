import {
  useCallback,
  useEffect,
  useMemo,
  useLayoutEffect,
  useRef,
  useState,
} from 'react';
import type { LibraryBrowserItemDTO } from '../api/types.ts';
import { useThumbnail, useThumbnailStore } from '../state/ThumbnailStoreContext.tsx';
import { typeIcon } from './wallpaperCardHelpers.ts';
import {
  attachVideoDecoder,
  ENHANCED_MEDIA_ACTIVATION_DELAY_MS,
  enhancedMediaActivationPlan,
  enhancedMediaCandidates,
  previewImagePath,
  previewVideoHandoff,
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
  /** Optional Book source choice; other views keep their existing preview paths. */
  readonly staticSource?: {
    readonly thumbnailPath: string;
    readonly fallbackPath: string | null;
    /** Store key of a sharper preview to show instead of `thumbnailPath` once it has arrived. */
    readonly largeThumbnailPath?: string | null;
  };
  readonly stabilizeEntranceDuringMotion?: boolean;
  /** Book zoom captures the displayed frame through a canvas. */
  readonly captureFrame?: boolean;
  readonly onReady?: (ready: boolean) => void;
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
  staticSource,
  stabilizeEntranceDuringMotion = false,
  onEnhancedError,
  onReady,
  captureFrame = false,
}: WallpaperPreviewMediaProps) {
  const assetPath = staticSource?.thumbnailPath ?? staticPreviewAssetPath(entry);
  const { thumbnail: standardThumbnail, failure: thumbnailFailure } = useThumbnail(assetPath);
  const { isScrolling } = useThumbnailStore();
  // The grid-size picture shows first; the large one replaces it when ready (decoded before the swap).
  const { thumbnail: largeThumbnail } = useThumbnail(staticSource?.largeThumbnailPath ?? '');
  const thumbnail = largeThumbnail ?? standardThumbnail;
  const [staticFallbackLoadFailed, setStaticFallbackLoadFailed] = useState(false);
  const fallbackAssetPath = staticSource
    ? staticFallback ? staticSource.fallbackPath : null
    : staticFallbackAssetPath(entry, staticFallback);
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
  const bookVideo = staticSource !== undefined && entry.type === 'video';
  const candidates = useMemo(() => {
    if (transientImagePath) return [{ kind: 'image' as const, path: transientImagePath }];
    const media = enhancedMediaCandidates(entry, {
      ...eligibility,
      settled: activationPlan.retain,
    });
    // A playback error must not replace a healthy video-frame still with preview.gif.
    return bookVideo ? media.filter((candidate) => candidate.kind === 'video') : media;
  }, [activationPlan.retain, bookVideo, eligibility, entry, transientImagePath]);
  const candidateKey = candidates.map((candidate) => `${candidate.kind}:${candidate.path}`).join('\0');
  const [candidateIndex, setCandidateIndex] = useState(0);
  const [enhancedError, setEnhancedError] = useState<string | null>(null);
  const [thumbnailLoadFailed, setThumbnailLoadFailed] = useState(false);
  const [loadedImage, setLoadedImage] = useState<{
    entryPath: string;
    path: string;
    stableEntry: boolean;
  } | null>(null);
  const entranceStabilityRef = useRef({
    entryPath: entry.path,
    stabilize: stabilizeEntranceDuringMotion && !eligibility.settled,
  });
  if (entranceStabilityRef.current.entryPath !== entry.path) {
    entranceStabilityRef.current = {
      entryPath: entry.path,
      stabilize: stabilizeEntranceDuringMotion && !eligibility.settled,
    };
  } else if (stabilizeEntranceDuringMotion && !eligibility.settled) {
    entranceStabilityRef.current.stabilize = true;
  }
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
  const [readyVideoSource, setReadyVideoSource] = useState<string | null>(null);
  const [retainedVideo, setRetainedVideo] = useState<{ entryPath: string; source: string } | null>(null);
  const activeVideoSource = activeCandidate?.kind === 'video' && authorizedCandidate.path
    ? safeFileSrc(authorizedCandidate.path)
    : null;
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
    // Decode the Book still under its live video using the existing image handoff.
    candidateKind: staticSource && activeCandidate?.kind === 'video' ? null : activeCandidate?.kind ?? null,
    authorizedCandidatePath: activeCandidate?.kind === 'video' && staticSource ? null : authorizedCandidate.path,
    authorizedStaticFallbackPath: authorizedStaticFallback.path,
    staticFallbackLoadFailed,
    thumbnail,
    thumbnailLoadFailed,
  });
  const videoPosterPath = (staticFallbackLoadFailed ? null : authorizedStaticFallback.path)
    ?? (thumbnailLoadFailed ? undefined : thumbnail);
  const displayedImage = loadedImage?.entryPath === entry.path ? loadedImage : null;
  const imageLoaded = imagePath !== null
    && imagePath !== undefined
    && displayedImage?.path === imagePath;
  const handoff = previewVideoHandoff(
    activeVideoSource,
    retainedVideo?.entryPath === entry.path ? retainedVideo.source : null,
    imageLoaded,
    staticSource !== undefined,
  );
  const retainVideoForStill = staticSource !== undefined;
  const setVideoRef = useCallback((video: HTMLVideoElement | null) => {
    videoRef.current = attachVideoDecoder(videoRef.current, video, handoff.source);
  }, [handoff.source]);
  useLayoutEffect(() => {
    if (retainVideoForStill && activeVideoSource) setRetainedVideo({ entryPath: entry.path, source: activeVideoSource });
  }, [activeVideoSource, entry.path, retainVideoForStill]);
  useEffect(() => {
    if (!handoff.source || activeVideoSource) return undefined;
    // Freeze the last sharp frame while the still is decoding, then fade it away.
    videoRef.current?.pause();
    if (!handoff.fading) return undefined;
    const timer = window.setTimeout(() => setRetainedVideo(null), eligibility.reducedMotion ? 0 : 160);
    return () => window.clearTimeout(timer);
  }, [activeVideoSource, eligibility.reducedMotion, handoff.fading, handoff.source]);
  useEffect(() => {
    if (retainVideoForStill && activeVideoSource) void videoRef.current?.play().catch(() => {});
  }, [activeVideoSource, retainVideoForStill]);
  useLayoutEffect(() => {
    onReady?.(activeVideoSource !== null ? readyVideoSource === activeVideoSource : imageLoaded);
  }, [activeVideoSource, imageLoaded, onReady, readyVideoSource]);
  const pendingImagePath = imagePath && !imageLoaded ? imagePath : null;

  const handleImageError = (failedPath: string) => {
    setLoadedImage((current) => (
      current?.entryPath === entry.path && current.path === failedPath ? null : current
    ));
    if (failedPath !== imagePath) return;
    if (failedPath === authorizedCandidate.path) {
      handleEnhancedError();
    } else if (failedPath === authorizedStaticFallback.path) {
      setStaticFallbackLoadFailed(true);
    } else {
      setThumbnailLoadFailed(true);
    }
  };

  const handleImageLoad = (
    image: HTMLImageElement,
    loadedPath: string,
    replacingDisplayedImage: boolean,
  ) => {
    const commitDecodedImage = () => {
      if (!image.isConnected || image.getAttribute('data-preview-path') !== loadedPath) return;
      setLoadedImage({
        entryPath: entry.path,
        path: loadedPath,
        // A picture that turns up while the view is scrolling appears at once. Fading each one in
        // kept a handful of opacity animations running throughout a scroll (measured in Grid: about
        // 8% more CPU and 7 to 13 late frames in 6 s, against none). Decided per picture when it
        // loads, so nothing replays when the scroll stops.
        stableEntry: replacingDisplayedImage || isScrolling() || (
          entranceStabilityRef.current.entryPath === entry.path
          && entranceStabilityRef.current.stabilize
        ),
      });
    };
    if (typeof image.decode !== 'function') {
      commitDecodedImage();
      return;
    }
    void image.decode().then(commitDecodedImage, () => {
      if (image.complete && image.naturalWidth > 0) commitDecodedImage();
    });
  };

  const video = handoff.source ? (
    <video
      aria-hidden="true"
      autoPlay
      crossOrigin={captureFrame ? 'anonymous' : undefined}
      className={[className, staticSource ? 'wallpaper-preview-video--handoff' : ''].filter(Boolean).join(' ')}
      data-enhanced-preview="video"
      data-preview-fading={handoff.fading || undefined}
      key={`video:${entry.path}`}
      loop
      muted
      onLoadedData={() => setReadyVideoSource(handoff.source)}
      onError={activeVideoSource ? handleEnhancedError : undefined}
      playsInline
      poster={videoPosterPath ? safeFileSrc(videoPosterPath) : undefined}
      preload="metadata"
      ref={setVideoRef}
      src={handoff.source}
    />
  ) : null;
  if (video && !staticSource) {
    return (
      <>
        {video}
        {enhancedError && !thumbnail ? (
          <span aria-label="Preview unavailable" className="wallpaper-thumb-error" title="Preview unavailable">!</span>
        ) : null}
      </>
    );
  }

  if (imagePath || (staticSource && (displayedImage || video))) {
    const imageClassName = ['wallpaper-preview-image', className].filter(Boolean).join(' ');
    return (
      <>
        {video}
        {!displayedImage ? (
          <span
            aria-hidden="true"
            className="wallpaper-thumb-placeholder wallpaper-thumb-placeholder--loading"
          >
            <span className="wallpaper-type-icon">{typeIcon(entry.type)}</span>
          </span>
        ) : null}
        {displayedImage ? (
          <img
            alt={alt}
            className={imageClassName}
            data-enhanced-preview={
              displayedImage.path === authorizedCandidate.path ? 'image' : undefined
            }
            data-preview-entry-stable={displayedImage.stableEntry || undefined}
            data-preview-loaded="true"
            data-preview-path={displayedImage.path}
            decoding="async"
            draggable={false}
            key={displayedImage.path}
            loading={loading}
            onError={() => handleImageError(displayedImage.path)}
            onLoad={(event) => handleImageLoad(
              event.currentTarget,
              displayedImage.path,
              false,
            )}
            src={safeFileSrc(displayedImage.path)}
          />
        ) : null}
        {pendingImagePath ? (
          <img
            alt={displayedImage ? '' : alt}
            aria-hidden={displayedImage ? 'true' : undefined}
            className={[
              imageClassName,
              displayedImage ? 'wallpaper-preview-image--preload' : '',
            ].filter(Boolean).join(' ')}
            data-preview-path={pendingImagePath}
            decoding="async"
            draggable={false}
            key={pendingImagePath}
            loading={loading}
            onError={() => handleImageError(pendingImagePath)}
            onLoad={(event) => handleImageLoad(
              event.currentTarget,
              pendingImagePath,
              displayedImage !== null,
            )}
            src={safeFileSrc(pendingImagePath)}
          />
        ) : null}
        {enhancedError && !thumbnail ? (
          <span aria-label="Preview unavailable" className="wallpaper-thumb-error" title="Preview unavailable">!</span>
        ) : null}
      </>
    );
  }

  return (
    <div
      className={`wallpaper-thumb-placeholder${staticSource ? ' wallpaper-thumb-placeholder--loading' : ''}${className ? ` ${className}` : ''}`}
    >
      <span className="wallpaper-type-icon">{typeIcon(entry.type)}</span>
      {enhancedError || thumbnailFailure || thumbnailLoadFailed ? (
        <span
          aria-label="Preview unavailable"
          className="wallpaper-thumb-error"
          title={thumbnailFailure ? `Preview unavailable: ${thumbnailFailure}` : 'Preview unavailable'}
        >!</span>
      ) : null}
    </div>
  );
}
