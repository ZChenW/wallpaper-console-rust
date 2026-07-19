import { useCallback, useSyncExternalStore } from 'react';

import { safeFileSrc } from '../components/safeFileSrc.ts';
import { staticPreviewAssetPath } from '../components/wallpaperPreviewMedia.ts';
import { useAuthorizedPreviewAsset } from '../components/useAuthorizedPreviewAsset.ts';
import { useThumbnailStore } from '../state/ThumbnailStoreContext.tsx';
import WallpaperDetailsDialog, {
  type WallpaperDetailsDialogProps,
} from './WallpaperDetailsDialog.tsx';
import { detailsPreviewAssetPath } from './wallpaperDetailsPreview.ts';

export default function AuthorizedWallpaperDetailsDialog(
  props: WallpaperDetailsDialogProps,
) {
  const store = useThumbnailStore();
  const assetPath = props.wallpaper ? staticPreviewAssetPath(props.wallpaper) : null;
  const preferredPath = props.wallpaper ? detailsPreviewAssetPath(props.wallpaper) : null;
  const subscribeThumbnail = useCallback((callback: () => void) => (
    assetPath ? store.subscribe(assetPath, callback) : () => undefined
  ), [assetPath, store]);
  const getThumbnail = useCallback(
    () => (assetPath ? store.get(assetPath) : undefined),
    [assetPath, store],
  );
  const thumbnail = useSyncExternalStore(subscribeThumbnail, getThumbnail, getThumbnail);
  const authorized = useAuthorizedPreviewAsset(preferredPath, props.wallpaper?.path ?? null);

  return (
    <WallpaperDetailsDialog
      {...props}
      fallbackPreviewSrc={thumbnail ? safeFileSrc(thumbnail) : null}
      previewPending={authorized.pending}
      previewSrc={authorized.path ? safeFileSrc(authorized.path) : null}
    />
  );
}
