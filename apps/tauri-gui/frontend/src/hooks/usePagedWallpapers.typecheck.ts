import type { WallpaperDTO } from '../api/bridge';
import {
  usePagedWallpapers,
  type WallpaperPageDTO,
  type WallpaperPageLoader,
} from './usePagedWallpapers';

interface BrowserWallpaper extends WallpaperDTO {
  wallpaperId: number;
  favorite: boolean;
  author?: string;
  sources: Array<{ id: number; displayName: string }>;
}

const item: BrowserWallpaper = {
  wallpaperId: 1,
  path: '/wall/1.jpg',
  type: 'image',
  ext: 'jpg',
  backend: 'awww',
  size: 1024,
  mtime: 1,
  resolution: '1920x1080',
  favorite: true,
  author: 'Ada',
  sources: [{ id: 7, displayName: 'Curated' }],
};

// Compile-time contract: rich items flow through page, loader, hook state,
// lookup map, append, and reload while legacy callers keep default types.
function typecheckGenericPagingContract() {
  const page: WallpaperPageDTO<BrowserWallpaper> = {
    revision: 1,
    nextCursor: null,
    total: 1,
    items: [item],
  };
  const loader: WallpaperPageLoader<BrowserWallpaper> = async () => page;
  const rich = usePagedWallpapers<BrowserWallpaper>({
    pageSize: 120,
    loadPage: loader,
    onPage: (loaded: WallpaperPageDTO<BrowserWallpaper>) => {
      const favorite: boolean | undefined = loaded.items?.[0]?.favorite;
      void favorite;
    },
  });
  const entries: BrowserWallpaper[] = rich.entries;
  const found: BrowserWallpaper | undefined = rich.entryByPath.get('/wall/1.jpg');
  rich.setEntries((previous: BrowserWallpaper[]) => previous);
  void rich.load(true, 'cursor');
  void rich.reload();
  void found;

  const legacyPage: WallpaperPageDTO = {
    revision: 1,
    nextCursor: null,
    total: 0,
    items: [],
  };
  const legacyLoader: WallpaperPageLoader = async () => legacyPage;
  void legacyLoader;
}

void typecheckGenericPagingContract;
