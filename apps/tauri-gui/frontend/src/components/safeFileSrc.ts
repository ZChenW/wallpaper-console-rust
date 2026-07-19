import { convertFileSrc } from '@tauri-apps/api/core';

import { BoundedFileSrcCache } from './fileSrcCache.ts';

const fileSrcCache = new BoundedFileSrcCache((path: string): string => {
  try {
    return convertFileSrc(path);
  } catch {
    return path;
  }
});

export function safeFileSrc(path: string): string {
  return fileSrcCache.get(path);
}
