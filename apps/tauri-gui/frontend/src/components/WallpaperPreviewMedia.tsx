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
  staticFallbackAssetPath,
  staticPreviewAssetPath,
  type EnhancedMediaEligibility,
} from './wallpaperPreviewMedia.ts';
import { useAuthorizedPreviewAsset } from './useAuthorizedPreviewAsset.ts';
import { safeFileSrc } from './safeFileSrc.ts';
import { PREVIEW_CLIP_FADE_OUT_MS } from './previewClip.ts';
import { usePreviewClip } from './usePreviewClip.ts';

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
  /** Video eligibility without the window-focus gate; blur freezes an already shown clip. */
  readonly clipActive?: boolean;
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
  clipActive,
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
  const mediaEligibility = useMemo(() => entry.type === 'video' && clipActive !== undefined
    ? { ...eligibility, active: clipActive }
    : eligibility, [clipActive, eligibility, entry.type]);
  const activationPlan = enhancedMediaActivationPlan(
    entry,
    enhancedActivatedPath === entry.path,
    mediaEligibility,
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
      ...mediaEligibility,
      settled: activationPlan.retain,
    });
    // A playback error must not replace a healthy video-frame still with preview.gif.
    return bookVideo ? media.filter((candidate) => candidate.kind === 'video') : media;
  }, [activationPlan.retain, bookVideo, mediaEligibility, entry, transientImagePath]);
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
    activeCandidate?.kind === 'image' ? activeCandidate.path : null,
    entry.path,
  );
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
  const displayedImage = loadedImage?.entryPath === entry.path ? loadedImage : null;
  const imageLoaded = imagePath !== null
    && imagePath !== undefined
    && displayedImage?.path === imagePath;
  const setVideoRef = useCallback((video: HTMLVideoElement | null) => {
    videoRef.current = attachVideoDecoder(videoRef.current, video, null);
  }, []);
  const [runningVideoPath, setRunningVideoPath] = useState<string | null>(null);
  const clipPath = activeCandidate?.kind === 'video' ? activeCandidate.path : null;
  useLayoutEffect(() => { if (!clipPath) setRunningVideoPath(null); }, [clipPath]);
  const hasStill = Boolean(displayedImage || imagePath);
  // A <video> exists only for the one item that may play, and for the moment its clip fades out.
  // Every <video> element makes the engine set up a media player that holds several connections to
  // the user's session bus; one per video card (49 in a Grid of this library) exhausted the bus's
  // file descriptors within a few view switches and took the whole desktop session down.
  const [clipMounted, setClipMounted] = useState(false);
  useLayoutEffect(() => {
    if (clipPath) { setClipMounted(true); return undefined; }
    if (!clipMounted) return undefined;
    const timer = window.setTimeout(() => setClipMounted(false), eligibility.reducedMotion ? 0 : PREVIEW_CLIP_FADE_OUT_MS + 60);
    return () => window.clearTimeout(timer);
  }, [clipMounted, clipPath, eligibility.reducedMotion]);
  usePreviewClip(entry.path, videoRef, entry.type === 'video' && clipMounted && !eligibility.reducedMotion, clipPath,
    () => setRunningVideoPath(entry.path),
    () => {
      // A missing/broken clip ends on the existing still, rather than trying an animated preview.
      setRunningVideoPath(null);
      setCandidateIndex(candidates.length);
      const message = `Enhanced preview unavailable for ${entry.title || entry.path}`;
      setEnhancedError(message);
      if (!hasStill) onEnhancedError?.(message);
    });
  useLayoutEffect(() => {
    onReady?.(imageLoaded || (clipPath !== null && runningVideoPath === entry.path));
  }, [clipPath, entry.path, imageLoaded, onReady, runningVideoPath]);
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

  // Keep the decoder laid out at its real size while it is transparent. WebKit will not
  // advance hidden/zero-sized video. The player alone assigns a small previewClip blob URL.
  const video = entry.type === 'video' && clipMounted ? (
    <video
      aria-hidden="true"
      className={[className, 'wallpaper-preview-clip'].filter(Boolean).join(' ')}
      data-enhanced-preview="video"
      data-preview-clip="true"
      key={`video:${entry.path}`}
      loop
      muted
      playsInline
      preload="auto"
      ref={setVideoRef}
      style={{ opacity: 0 }}
    />
  ) : null;

  if (imagePath || (staticSource && displayedImage) || video) {
    const imageClassName = ['wallpaper-preview-image', className].filter(Boolean).join(' ');
    return (
      <>
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
        {video}
        {enhancedError && (entry.type === 'video' ? !hasStill : !thumbnail) ? (
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
