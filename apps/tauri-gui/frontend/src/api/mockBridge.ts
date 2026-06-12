interface CommandResult {
  success: boolean;
  stdout: string;
  stderr: string;
  exitCode: number;
}

interface LibraryCountDTO {
  total: number;
  images: number;
  gifs: number;
  videos: number;
}

interface LibraryPageDTO {
  total: number;
  items: WallpaperDTO[];
}

interface SourceDTO {
  path: string;
  exists: boolean;
  isWE: boolean;
  label: string;
}

interface StatusDTO {
  configDir: string;
  current: string;
  lastBackend: string;
  sourceCount: number;
}

interface LinuxWallpaperEngineStatusDTO {
  available: boolean;
  path?: string;
  message: string;
  detail?: string;
}

interface WebWallpaperStatusDTO {
  available: boolean;
  path?: string;
  message: string;
  detail?: string;
}

interface ThumbnailCacheDTO {
  dir: string;
  size: string;
  entries: number;
}

interface ThumbnailDTO {
  path: string;
  thumbnail?: string;
  cacheHit: boolean;
}

interface WallpaperDTO {
  path: string;
  type: string;
  ext: string;
  backend: string;
  size: number;
  mtime: number;
  resolution: string;
  projectType?: string;
  previewPath?: string;
  workshopId?: string;
  title?: string;
  weFile?: string;
  unsupportedReason?: string;
  backendStatus?: string;
  backendErrorKind?: string;
  backendErrorMessage?: string;
  backendErrorDetail?: string;
  backendFailedAt?: string;
}

const MOCK_WE_WALLPAPERS: WallpaperDTO[] = [
  {
    path: '/mock/Steam/steamapps/workshop/content/431960/3558034522',
    type: 'we_scene',
    ext: 'scene',
    backend: 'linux-wallpaperengine',
    size: 4096,
    mtime: 1700100000,
    resolution: 'WE',
    projectType: 'we_scene',
    previewPath: '/mock/Steam/steamapps/workshop/content/431960/3558034522/preview.gif',
    workshopId: '3558034522',
    title: 'Scene title',
    weFile: 'scene.json',
  },
  {
    path: '/mock/Steam/steamapps/workshop/content/431960/3589454154',
    type: 'we_scene',
    ext: 'scene',
    backend: 'linux-wallpaperengine',
    size: 2048,
    mtime: 1700080000,
    resolution: 'WE',
    projectType: 'we_scene',
    previewPath: '/mock/Steam/steamapps/workshop/content/431960/3589454154/preview.gif',
    workshopId: '3589454154',
    title: 'Incompatible Scene',
    weFile: 'scene.json',
    backendStatus: 'failed',
    backendErrorKind: 'scene_projection_unsupported',
    backendErrorMessage: 'This scene uses projection data that linux-wallpaperengine cannot render.',
  },
  {
    path: '/mock/Steam/steamapps/workshop/content/431960/3650880224',
    type: 'we_web',
    ext: 'web',
    backend: 'unsupported',
    size: 8192,
    mtime: 1700090000,
    resolution: 'WE',
    projectType: 'we_web',
    previewPath: '/mock/Steam/steamapps/workshop/content/431960/3650880224/preview.gif',
    workshopId: '3650880224',
    title: 'Web title',
    weFile: 'index.html',
    unsupportedReason: 'web_renderer_unavailable',
  },
  {
    path: '/mock/Steam/steamapps/workshop/content/431960/4444444444',
    type: 'unsupported',
    ext: 'application',
    backend: 'unsupported',
    size: 1024,
    mtime: 1700080000,
    resolution: 'WE',
    projectType: 'unsupported',
    workshopId: '4444444444',
    title: 'Application project',
    weFile: 'app.exe',
    unsupportedReason: 'Wallpaper Engine application projects are not supported.',
  },
];

const MOCK_REGULAR_WALLPAPERS: WallpaperDTO[] = Array.from({ length: 150 }, (_, i) => ({
  path: `/mock/path/wallpaper-${String(i).padStart(3, '0')}.${i % 5 === 0 ? 'mp4' : i % 3 === 0 ? 'gif' : 'jpg'}`,
  type: i % 5 === 0 ? 'video' : i % 3 === 0 ? 'gif' : 'image',
  ext: i % 5 === 0 ? 'mp4' : i % 3 === 0 ? 'gif' : 'jpg',
  backend: i % 5 === 0 ? 'mpvpaper' : 'awww',
  size: (i + 1) * 1024 * 100,
  mtime: 1700000000 - i * 3600,
  resolution: i % 3 === 0 ? '1920x1080' : i % 2 === 0 ? '3840x2160' : '2560x1440',
}));

const MOCK_WALLPAPERS: WallpaperDTO[] = [...MOCK_WE_WALLPAPERS, ...MOCK_REGULAR_WALLPAPERS];

const MOCK_SOURCES: SourceDTO[] = [
  { path: '/mock/Pictures', exists: true, isWE: false, label: 'Pictures' },
  { path: '/mock/Wallpapers', exists: true, isWE: false, label: 'Wallpapers' },
  { path: '/mock/steamapps/workshop/content/431960/12345', exists: true, isWE: true, label: 'Steam Workshop: 12345' },
];

const MOCK_FAVORITES: string[] = [
  '/mock/path/wallpaper-001.jpg',
  '/mock/path/wallpaper-010.gif',
  '/mock/path/wallpaper-050.mp4',
];

const MOCK_HISTORY: string[] = [
  '/mock/path/wallpaper-002.jpg',
  '/mock/path/wallpaper-015.gif',
  '/mock/path/wallpaper-020.jpg',
];

const ok: CommandResult = { success: true, stdout: 'ok', stderr: '', exitCode: 0 };

export const api = {
  status: async (): Promise<StatusDTO> => ({
    configDir: '/mock/.config/wallpaper-console',
    current: '/mock/path/wallpaper-001.jpg',
    lastBackend: 'awww',
    sourceCount: 3,
  }),

  linuxWallpaperEngineStatus: async (): Promise<LinuxWallpaperEngineStatusDTO> => ({
    available: false,
    message: 'Wallpaper Engine scene wallpapers require linux-wallpaperengine. Install it from AUR: yay -S linux-wallpaperengine-git',
    detail: 'backend not found: linux-wallpaperengine',
  }),

  webWallpaperStatus: async (): Promise<WebWallpaperStatusDTO> => ({
    available: false,
    message: 'Web wallpaper backend requires a Chromium-based browser.',
    detail: 'no supported web browser found',
  }),

  openWebPreview: async (_path: string): Promise<CommandResult> => ({
    ...ok,
    stdout: 'Chromium preview launched.',
  }),

  apply: async (): Promise<CommandResult> => ok,
  stop: async (): Promise<CommandResult> => ok,
  weClearBackendError: async (): Promise<CommandResult> => ok,
  restore: async (): Promise<CommandResult> => ok,

  libraryList: async (): Promise<WallpaperDTO[]> => MOCK_WALLPAPERS,

  libraryCount: async (): Promise<LibraryCountDTO> => ({ total: 150, images: 90, gifs: 30, videos: 30 }),

  libraryPage: async (
    _filter: string,
    _sort: string,
    _search: string,
    offset: number,
    limit: number,
  ): Promise<LibraryPageDTO> => {
    let items = [...MOCK_WALLPAPERS];
    if (_search) {
      const q = _search.toLowerCase();
      items = items.filter((w) => w.path.toLowerCase().includes(q));
    }
    if (_filter && _filter !== 'all') {
      items = items.filter((w) => w.type === _filter);
    }
    const total = items.length;
    const page = items.slice(offset, offset + limit);
    return { total, items: page };
  },

  rescan: async (): Promise<CommandResult> => ok,
  scanProgress: async () => ({
    running: false,
    stage: 'idle',
    scanned: 0,
    reusedMetadata: 0,
    probedMetadata: 0,
    insertedSqlite: 0,
    cancelRequested: false,
  }),
  scanCancel: async (): Promise<CommandResult> => ok,
  librarySourceStatus: async () => ({
    configured: 'sqlite',
    effective: 'sqlite',
    sqliteReady: true,
    sqliteRows: 150,
    tsvRows: 150,
    stale: false,
    message: 'SQLite active (150 entries)',
  }),

  favoritesList: async (): Promise<WallpaperDTO[]> =>
    MOCK_FAVORITES.map((path) =>
      MOCK_WALLPAPERS.find((w) => w.path === path) ?? {
        path,
        type: 'image',
        ext: 'jpg',
        backend: 'awww',
        size: 12345,
        mtime: 1700000000,
        resolution: '1920x1080',
      },
    ),

  favoriteAdd: async (): Promise<CommandResult> => ok,
  favoriteRemove: async (): Promise<CommandResult> => ok,

  historyList: async (): Promise<WallpaperDTO[]> =>
    MOCK_HISTORY.map((path) =>
      MOCK_WALLPAPERS.find((w) => w.path === path) ?? {
        path,
        type: 'image',
        ext: 'jpg',
        backend: 'awww',
        size: 12345,
        mtime: 1700000000,
        resolution: '1920x1080',
      },
    ),

  historyClear: async (): Promise<CommandResult> => ok,

  sourcesList: async (): Promise<SourceDTO[]> => MOCK_SOURCES,
  sourceAdd: async (): Promise<CommandResult> => ok,
  sourceRemove: async (): Promise<CommandResult> => ok,
  validateSources: async (): Promise<CommandResult> => ok,
  removeMissingSources: async (): Promise<CommandResult> => ok,
  scanSteamWorkshop: async (): Promise<CommandResult> => ok,

  configGet: async (key: string): Promise<string> => {
    const defaults: Record<string, string> = {
      storage_backend: 'sqlite',
      use_symlinks: 'false',
      image_backend: 'awww',
      gif_backend: 'awww',
      video_backend: 'mpvpaper',
      linux_wallpaperengine_enabled: 'on',
      linux_wallpaperengine_path: 'auto',
      linux_wallpaperengine_target_mode: 'auto',
      linux_wallpaperengine_target: '',
      linux_wallpaperengine_scaling: 'default',
      linux_wallpaperengine_fps: '60',
      linux_wallpaperengine_muted: 'off',
      linux_wallpaperengine_volume: '100',
      linux_wallpaperengine_assets_dir: 'auto',
    };
    return defaults[key] ?? '';
  },

  configSet: async (): Promise<CommandResult> => ok,

  sqliteVerify: async (): Promise<CommandResult> => ok,
  sqliteResync: async (): Promise<CommandResult> => ok,
  sqliteBackup: async (): Promise<CommandResult> => ok,
  sqliteRestore: async (): Promise<CommandResult> => ok,
  sqliteExportFlat: async (): Promise<CommandResult> => ok,
  migrateToSqlite: async (): Promise<CommandResult> => ok,

  thumbnailFor: async (path: string): Promise<ThumbnailDTO> => ({
    path,
    cacheHit: false,
  }),

  thumbnailCacheStatus: async () => ({
    dir: '/mock/cache/thumbs',
    size: '12.3 MB',
    entries: 30,
    failureEntries: 2,
    cleanupDays: 30,
  }),
  thumbnailCacheClear: async (): Promise<CommandResult> => ok,
  thumbnailCacheCleanupOld: async (): Promise<CommandResult> => ok,

  openPath: async (): Promise<CommandResult> => ok,
  revealInFileManager: async (): Promise<CommandResult> => ok,
  browseDirectory: async (): Promise<string> => '/mock/selected/dir',
  exportDiagnostics: async (): Promise<CommandResult> => ok,
};
