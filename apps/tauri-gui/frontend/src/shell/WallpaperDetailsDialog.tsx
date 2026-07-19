import {
  useEffect,
  useState,
  type CSSProperties,
  type KeyboardEvent,
} from 'react';

import type { LibraryBrowserItemDTO } from '../api/types.ts';
import { displayName } from '../components/wallpaperCardHelpers.ts';
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

const overlayStyle: CSSProperties = {
  position: 'fixed',
  inset: 0,
  zIndex: 850,
  display: 'grid',
  placeItems: 'center',
  padding: 'clamp(0.75rem, 3vw, 2rem)',
  background: 'rgb(0 0 0 / 58%)',
};

const dialogStyle: CSSProperties = {
  display: 'grid',
  width: 'min(58rem, 100%)',
  maxHeight: 'calc(100vh - 1.5rem)',
  gridTemplateRows: 'auto minmax(0, 1fr) auto',
  gap: '1rem',
  overflow: 'auto',
  padding: 'clamp(0.85rem, 2.5vw, 1.25rem)',
  borderRadius: '0.9rem',
};

const headerStyle: CSSProperties = {
  display: 'flex',
  alignItems: 'start',
  justifyContent: 'space-between',
  gap: '1rem',
};

const titleStyle: CSSProperties = {
  minWidth: 0,
  margin: 0,
  overflowWrap: 'anywhere',
  fontSize: 'clamp(1.05rem, 2vw, 1.35rem)',
};

const closeStyle: CSSProperties = {
  minWidth: '2.25rem',
  minHeight: '2.25rem',
  padding: '0.25rem 0.65rem',
  border: '1px solid color-mix(in srgb, currentColor 18%, transparent)',
  borderRadius: '0.5rem',
  background: 'transparent',
  color: 'inherit',
  cursor: 'pointer',
  font: 'inherit',
};

const previewFrameStyle: CSSProperties = {
  display: 'grid',
  minHeight: '12rem',
  maxHeight: 'min(62vh, 46rem)',
  placeItems: 'center',
  overflow: 'hidden',
  borderRadius: '0.7rem',
  background: 'color-mix(in srgb, CanvasText 8%, Canvas)',
};

const previewStyle: CSSProperties = {
  display: 'block',
  width: '100%',
  height: '100%',
  maxHeight: 'min(62vh, 46rem)',
  objectFit: 'contain',
};

const placeholderStyle: CSSProperties = {
  padding: '2rem',
  textAlign: 'center',
  opacity: 0.68,
};

const metadataStyle: CSSProperties = {
  display: 'grid',
  gridTemplateColumns: 'max-content minmax(0, 1fr)',
  gap: '0.45rem 0.9rem',
  margin: 0,
  fontSize: '0.84rem',
};

const termStyle: CSSProperties = {
  fontWeight: 650,
  opacity: 0.74,
};

const valueStyle: CSSProperties = {
  minWidth: 0,
  margin: 0,
  overflowWrap: 'anywhere',
};

const pathStyle: CSSProperties = {
  font: '0.78rem/1.45 ui-monospace, SFMono-Regular, Consolas, monospace',
};

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
    <div className="wallpaper-details__overlay" style={overlayStyle}>
      <section
        aria-labelledby="wallpaper-details-title"
        aria-modal="true"
        className="wallpaper-details"
        onKeyDown={handleKeyDown}
        role="dialog"
        style={dialogStyle}
      >
        <header style={headerStyle}>
          <h2 id="wallpaper-details-title" style={titleStyle}>{title}</h2>
          <button
            aria-label="Close wallpaper details"
            autoFocus={true}
            onClick={onClose}
            style={closeStyle}
            type="button"
          >
            Close
          </button>
        </header>

        <div
          aria-label={`Full-ratio preview of ${title}`}
          className="wallpaper-details__preview"
          style={previewFrameStyle}
        >
          {previewSrc || fallbackPreviewSrc ? (
            <img
              alt={`${title} preview`}
              draggable={false}
              onError={onPreviewError}
              src={previewSrc ?? fallbackPreviewSrc ?? undefined}
              style={previewStyle}
            />
          ) : (
            <span style={placeholderStyle}>
              {previewPending ? 'Loading preview…' : 'Preview unavailable'}
            </span>
          )}
        </div>

        <dl style={metadataStyle}>
          <dt style={termStyle}>Type</dt>
          <dd style={valueStyle}>{wallpaperTypeLabel(wallpaper.type)}</dd>

          <dt style={termStyle}>Sources</dt>
          <dd style={valueStyle}>{sources || 'Source information unavailable'}</dd>

          {author ? (
            <>
              <dt style={termStyle}>Author</dt>
              <dd style={valueStyle}>{author}</dd>
            </>
          ) : null}

          <dt style={termStyle}>Path</dt>
          <dd style={{ ...valueStyle, ...pathStyle }}><code>{wallpaper.path}</code></dd>
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
