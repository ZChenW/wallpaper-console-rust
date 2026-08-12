import {
  useEffect,
  useState,
  type KeyboardEvent,
} from 'react';
import { X } from 'lucide-react';

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
    <div className="wallpaper-details__overlay">
      <section
        aria-labelledby="wallpaper-details-title"
        aria-modal="true"
        className="wallpaper-details"
        onKeyDown={handleKeyDown}
        role="dialog"
      >
        <header className="wallpaper-details__header">
          <h2 className="wallpaper-details__title" id="wallpaper-details-title">{title}</h2>
          <button
            aria-label="Close wallpaper details"
            autoFocus={true}
            className="wallpaper-details__close"
            data-icon-button={true}
            onClick={onClose}
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
  const [currentSrc, setCurrentSrc] = useState(previewSrc ?? fallbackPreviewSrc);
  useEffect(() => {
    setCurrentSrc(previewSrc ?? fallbackPreviewSrc);
  }, [fallbackPreviewSrc, previewSrc]);

  return WallpaperDetailsDialogView({
    ...props,
    fallbackPreviewSrc: null,
    previewSrc: currentSrc,
    onPreviewError: () => {
      if (!currentSrc) return;
      setCurrentSrc(nextDetailsPreviewSource(currentSrc, fallbackPreviewSrc));
    },
  });
}
