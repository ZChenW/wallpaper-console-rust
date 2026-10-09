import {
  useEffect,
  useRef,
  useState,
  type KeyboardEvent,
} from 'react';
import { X } from 'lucide-react';
import { useReducedMotion } from '../hooks/useReducedMotion.ts';
import { useAbortExitWhenReducedMotion } from '../hooks/useAbortExitWhenReducedMotion.ts';

import type { LibraryBrowserItemDTO } from '../api/types.ts';
import { displayName } from '../components/wallpaperCardHelpers.ts';
import { presentWallpaper } from '../components/wallpaperPresentation.ts';
import { trapDialogFocus } from './dialogFocus.ts';
import { nextDetailsPreviewSource } from './wallpaperDetailsPreview.ts';

export interface WallpaperDetailsDialogProps {
  readonly open: boolean;
  readonly wallpaper: LibraryBrowserItemDTO | null;
  /** Browser-ready original image, Workshop preview, or cached thumbnail URL. */
  readonly previewSrc?: string | null;
  readonly fallbackPreviewSrc?: string | null;
  readonly previewPending?: boolean;
  readonly onClose: () => void;
}

interface WallpaperDetailsDialogViewProps extends WallpaperDetailsDialogProps {
  readonly onPreviewError?: () => void;
  readonly presentationPhase?: 'open' | 'exiting';
  readonly reducedMotion?: boolean;
  readonly presentationKey?: number;
}

function wallpaperTypeLabel(type: string): string {
  switch (type) {
    case 'image': return 'Image';
    case 'gif': return 'GIF';
    case 'video': return 'Video';
    case 'we_scene': return 'Wallpaper Engine Scene';
    case 'we_web': return 'Wallpaper Engine Web';
    case 'unsupported': return 'Unsupported';
    default: return type || 'Unknown';
  }
}

export function WallpaperDetailsDialogView({
  open,
  wallpaper,
  previewSrc = null,
  fallbackPreviewSrc = null,
  previewPending = false,
  onPreviewError,
  onClose,
  presentationPhase = 'open',
  reducedMotion = false,
  presentationKey = 0,
}: WallpaperDetailsDialogViewProps) {
  if (!open || wallpaper === null) return null;

  const title = displayName(wallpaper);
  const author = wallpaper.author?.trim();
  const sources = wallpaper.sources
    .map((source) => source.displayName.trim())
    .filter(Boolean)
    .join(', ');
  const compatibility = presentWallpaper(wallpaper).compatibility;
  const handleKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    if (presentationPhase === 'exiting') return;
    if (event.key === 'Tab') {
      trapDialogFocus(event, event.currentTarget);
      return;
    }
    if (event.key !== 'Escape') return;
    event.preventDefault?.();
    event.stopPropagation?.();
    onClose();
  };

  return (
    <div
      key={presentationKey}
      className="wallpaper-details__overlay"
      data-presentation-phase={presentationPhase}
      data-reduced-motion={reducedMotion || undefined}
      aria-hidden={presentationPhase === 'exiting' || undefined}
      inert={presentationPhase === 'exiting'}
      onMouseDown={(event) => {
        if (presentationPhase === 'open' && event.target === event.currentTarget) onClose();
      }}
    >
      <section
        aria-labelledby="wallpaper-details-title"
        aria-modal="true"
        className="wallpaper-details"
        onKeyDown={handleKeyDown}
        role="dialog"
      >
        <header className="wallpaper-details__header">
          <h2
            autoFocus={true}
            className="wallpaper-details__title"
            id="wallpaper-details-title"
            tabIndex={-1}
          >
            {title}
          </h2>
          <button
            aria-label="Close wallpaper details"
            className="wallpaper-details__close"
            data-icon-button={true}
            onClick={() => { if (presentationPhase === 'open') onClose(); }}
            title="Close"
            type="button"
          >
            <X aria-hidden="true" size={18} />
          </button>
        </header>

        <div
          aria-label={`Full-ratio preview of ${title}`}
          className="wallpaper-details__preview"
        >
          {previewSrc || fallbackPreviewSrc ? (
            <img
              alt={`${title} preview`}
              className="wallpaper-details__preview-media"
              draggable={false}
              onError={onPreviewError}
              src={previewSrc ?? fallbackPreviewSrc ?? undefined}
            />
          ) : (
            <span className="wallpaper-details__placeholder">
              {previewPending ? 'Loading preview…' : 'Preview unavailable'}
            </span>
          )}
        </div>

        <dl className="wallpaper-details__metadata">
          <dt className="wallpaper-details__term">Type</dt>
          <dd className="wallpaper-details__value">{wallpaperTypeLabel(wallpaper.type)}</dd>

          {compatibility ? (
            <>
              <dt className="wallpaper-details__term">Compatibility</dt>
              <dd
                className="wallpaper-details__value"
                data-wallpaper-details-field="compatibility"
              >
                {compatibility}
              </dd>
            </>
          ) : null}

          <dt className="wallpaper-details__term">Sources</dt>
          <dd className="wallpaper-details__value">{sources || 'Source information unavailable'}</dd>

          {author ? (
            <>
              <dt className="wallpaper-details__term">Author</dt>
              <dd className="wallpaper-details__value">{author}</dd>
            </>
          ) : null}

          <dt className="wallpaper-details__term">Path</dt>
          <dd className="wallpaper-details__value wallpaper-details__path">
            <code>{wallpaper.path}</code>
          </dd>
        </dl>
      </section>
    </div>
  );
}

export default function WallpaperDetailsDialog(props: WallpaperDetailsDialogProps) {
  const { fallbackPreviewSrc = null, previewSrc = null } = props;
  const reducedMotion = useReducedMotion();
  const [prevOpen, setPrevOpen] = useState(props.open);
  const [shouldRender, setShouldRender] = useState(props.open);
  const [presentationPhase, setPresentationPhase] = useState<'open' | 'exiting'>('open');
  const [presentationKey, setPresentationKey] = useState(0);
  const [retainedWallpaper, setRetainedWallpaper] = useState(props.wallpaper);
  const [currentSrc, setCurrentSrc] = useState(previewSrc ?? fallbackPreviewSrc);
  const exitTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Match the other overlays: close the owning state immediately, retain only
  // inert presentation for exit, and cancel that exit on reopen/reduced motion.
  if (props.open && props.wallpaper !== retainedWallpaper) {
    setRetainedWallpaper(props.wallpaper);
    setCurrentSrc(previewSrc ?? fallbackPreviewSrc);
  }
  if (props.open !== prevOpen) {
    setPrevOpen(props.open);
    if (exitTimerRef.current !== null) clearTimeout(exitTimerRef.current);
    exitTimerRef.current = null;
    if (props.open) {
      setShouldRender(true);
      setPresentationPhase('open');
      // Remount on a quick reopen too, preserving the title's native autofocus.
      setPresentationKey((key) => key + 1);
      setCurrentSrc(previewSrc ?? fallbackPreviewSrc);
    } else {
      setPresentationPhase('exiting');
      if (reducedMotion) setShouldRender(false);
      else exitTimerRef.current = setTimeout(() => {
        exitTimerRef.current = null;
        setShouldRender(false);
      }, 100);
    }
  }
  useAbortExitWhenReducedMotion(props.open, reducedMotion, exitTimerRef, setShouldRender);
  useEffect(() => () => {
    if (exitTimerRef.current !== null) clearTimeout(exitTimerRef.current);
  }, []);
  useEffect(() => {
    if (!props.open) return;
    setCurrentSrc(previewSrc ?? fallbackPreviewSrc);
  }, [fallbackPreviewSrc, previewSrc, props.open]);

  return WallpaperDetailsDialogView({
    ...props,
    open: shouldRender,
    wallpaper: retainedWallpaper,
    presentationPhase,
    presentationKey,
    reducedMotion,
    fallbackPreviewSrc: null,
    previewSrc: currentSrc,
    onPreviewError: () => {
      if (!currentSrc) return;
      setCurrentSrc(nextDetailsPreviewSource(currentSrc, fallbackPreviewSrc));
    },
  });
}
