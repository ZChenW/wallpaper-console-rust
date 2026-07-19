import { useEffect, useState } from 'react';

import { api } from '../api/bridge.ts';
import { PreviewAssetResolver } from './previewAssetResolver.ts';

const resolver = new PreviewAssetResolver((path, wallpaperPath) => (
  api.previewAssetAuthorize(path, wallpaperPath)
));

interface AuthorizedPreviewAssetState {
  readonly requestedPath: string | null;
  readonly path: string | null;
  readonly error: Error | null;
}

export interface AuthorizedPreviewAsset {
  readonly path: string | null;
  readonly pending: boolean;
  readonly error: Error | null;
}

export function useAuthorizedPreviewAsset(
  path: string | null,
  wallpaperPath: string | null,
): AuthorizedPreviewAsset {
  const [state, setState] = useState<AuthorizedPreviewAssetState>({
    requestedPath: null,
    path: null,
    error: null,
  });

  useEffect(() => {
    let current = true;
    if (!path || !wallpaperPath) {
      setState({ requestedPath: null, path: null, error: null });
      return () => { current = false; };
    }

    setState({ requestedPath: path, path: null, error: null });
    void resolver.resolve(path, wallpaperPath).then(
      (authorizedPath) => {
        if (current) {
          setState({ requestedPath: path, path: authorizedPath, error: null });
        }
      },
      (failure: unknown) => {
        if (!current) return;
        setState({
          requestedPath: path,
          path: null,
          error: failure instanceof Error ? failure : new Error(String(failure)),
        });
      },
    );
    return () => { current = false; };
  }, [path, wallpaperPath]);

  if (state.requestedPath !== path) {
    return { path: null, pending: path !== null, error: null };
  }
  return {
    path: state.path,
    pending: path !== null && state.path === null && state.error === null,
    error: state.error,
  };
}
