interface CommandResult {
  success: boolean;
  stdout: string;
  stderr: string;
  exitCode: number;
}

interface ApplyRequestDTO {
  kind: string;
  path: string;
  requestId?: string;
}

let lastApplyActionRequest: ApplyRequestDTO | null = null;

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

interface ThumbnailCacheDTO {
  dir: string;
  size: string;
  entries: number;
  oldestMtime?: number;
  newestMtime?: number;
  failureEntries: number;
  cleanupDays: number;
}

interface ThumbnailDTO {
  path: string;
  thumbnail?: string;
  cacheHit: boolean;
  failureReason?: string;
}

interface ScanProgressDTO {
  running: boolean;
  stage: string;
  scanned: number;
  totalHint?: number;
  reusedMetadata: number;
  probedMetadata: number;
  insertedSqlite: number;
  staged: number;
  skipped: number;
  metadataErrors: number;
  currentPath?: string;
  cancelRequested: boolean;
  error?: string;
}

type ApplyAvailability = 'available' | 'unsupported' | 'retryable_failure';

type ApplyActionKind =
  | 'apply'
  | 'retry_backend_apply'
  | 'apply_preview'
  | 'open_folder'
  | 'copy_workshop_id';

interface ApplyActionDTO {
  kind: ApplyActionKind;
  label: string;
  enabled: boolean;
  reason?: string;
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
  applyAvailability?: ApplyAvailability;
  applyBackend?: string;
  applyReason?: string;
  applyActions?: ApplyActionDTO[];
  rendererCompatibility?: string;
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
    applyAvailability: 'available',
    applyBackend: 'linux-wallpaperengine',
    applyActions: [
      { kind: 'apply', label: 'Apply', enabled: true },
      { kind: 'apply_preview', label: 'Apply preview GIF', enabled: true },
      { kind: 'open_folder', label: 'Open folder', enabled: true },
      { kind: 'copy_workshop_id', label: 'Copy Workshop ID', enabled: true },
    ],
    rendererCompatibility: 'Rendered by linux-wallpaperengine — may differ from Wallpaper Engine',
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
    backendStatus: 'renderer_limitation',
    backendErrorKind: 'renderer_limitation',
    backendErrorMessage: 'This scene has renderer limitations with linux-wallpaperengine.',
    applyAvailability: 'retryable_failure',
    applyBackend: 'linux-wallpaperengine',
    applyActions: [
      { kind: 'retry_backend_apply', label: 'Retry backend apply', enabled: true },
      { kind: 'apply_preview', label: 'Apply preview GIF', enabled: true },
      { kind: 'open_folder', label: 'Open folder', enabled: true },
      { kind: 'copy_workshop_id', label: 'Copy Workshop ID', enabled: true },
    ],
    rendererCompatibility: 'Rendered by linux-wallpaperengine — may differ from Wallpaper Engine',
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
    unsupportedReason: 'Wallpaper Engine Web projects are indexed for browsing only and cannot be applied by this app.',
    applyAvailability: 'unsupported',
    applyReason: 'Wallpaper Engine Web projects are indexed for browsing only.',
    applyActions: [
      { kind: 'open_folder', label: 'Open folder', enabled: true },
      { kind: 'apply_preview', label: 'Apply preview only', enabled: true, reason: 'Only the preview GIF can be applied as a static wallpaper; the Web scene itself is not supported.' },
      { kind: 'copy_workshop_id', label: 'Copy Workshop ID', enabled: true },
    ],
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
    applyAvailability: 'unsupported',
    applyActions: [
      { kind: 'open_folder', label: 'Open folder', enabled: true },
      { kind: 'copy_workshop_id', label: 'Copy Workshop ID', enabled: true },
    ],
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
  applyAvailability: 'available' as ApplyAvailability,
  applyBackend: i % 5 === 0 ? 'mpvpaper' : 'awww',
  applyActions: [
    { kind: 'apply' as ApplyActionKind, label: 'Apply', enabled: true },
    { kind: 'open_folder' as ApplyActionKind, label: 'Open folder', enabled: true },
  ],
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
  '/mock/Steam/steamapps/workshop/content/431960/3558034522',
];

const MOCK_HISTORY: string[] = [
  '/mock/path/wallpaper-002.jpg',
  '/mock/path/wallpaper-015.gif',
  '/mock/path/wallpaper-020.jpg',
];

const ok: CommandResult = { success: true, stdout: 'ok', stderr: '', exitCode: 0 };
const failResult: CommandResult = { success: false, stdout: '', stderr: 'mock failure', exitCode: 1 };

const defaultScanProgress: ScanProgressDTO = {
  running: false,
  stage: 'idle',
  scanned: 0,
  reusedMetadata: 0,
  probedMetadata: 0,
  insertedSqlite: 0,
  staged: 0,
  skipped: 0,
  metadataErrors: 0,
  cancelRequested: false,
};

const defaultConfig: Record<string, string> = {
  storage_backend: 'sqlite',
  use_symlinks: 'false',
  image_backend: 'awww',
  gif_backend: 'awww',
  video_backend: 'mpvpaper',
  awww_resize: 'crop',
  awww_transition_type: 'fade',
  awww_transition_duration: '1',
  wallpaper_transition_fps: '60',
  mpvpaper_options: '--loop-file=inf --panscan=1.0',
  mpvpaper_output: '*',
  linux_wallpaperengine_enabled: 'on',
  linux_wallpaperengine_path: 'auto',
  linux_wallpaperengine_target_mode: 'auto',
  linux_wallpaperengine_target: '',
  linux_wallpaperengine_scaling: 'default',
  linux_wallpaperengine_fps: '60',
  linux_wallpaperengine_muted: 'off',
  linux_wallpaperengine_volume: '100',
  gui_thumbnail_mode: 'cache',
  gui_thumbnail_cleanup_days: '30',
  gui_thumbnail_failure_ttl_secs: '900',
  preview_metadata: 'compact',
  gui_debug_logs: 'off',
  gui_theme: 'light',
  open_project_location_mode: 'ask',
  gui_file_manager: 'auto',
  gui_file_manager_custom: '',
  gui_terminal_file_manager: 'yazi',
  gui_terminal_file_manager_custom: '',
};

// Mutable, scenario-driven mock state. When no scenario is active, all methods
// return the same values as the original static mock (so existing smoke tests
// remain green). Tests drive this via `api.__mockControl` (also exposed on
// `globalThis` for Playwright access).
let scanProgressState: ScanProgressDTO = { ...defaultScanProgress };
let scanAutoAdvance = false;
let scanStep = 5;
let configStore: Record<string, string> = {};
const commandFailures = new Set<string>();
const thumbnailFailures = new Set<string>();
let libraryFirstPageEmpty = false;
let libraryFirstPageEmptyConsumed = false;

function resetScanProgressState(): void {
  scanProgressState = { ...defaultScanProgress };
  scanAutoAdvance = false;
  scanStep = 5;
}

function resetConfigStore(): void {
  configStore = {};
}

function resetLibraryScenario(): void {
  libraryFirstPageEmpty = false;
  libraryFirstPageEmptyConsumed = false;
}

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

  weDebugInfo: async () => ({
    lastCommandLine: '',
    lastTargetConfig: '',
    lastStderr: '',
    lastExitStatus: '',
    logPath: '/dev/null',
  }),

  apply: async (): Promise<CommandResult> => ok,
  applyAction: async (request: ApplyRequestDTO): Promise<CommandResult> => {
    lastApplyActionRequest = request;
    return {
      ...ok,
      stdout: JSON.stringify({
        requestId: request.requestId,
        appliedPath: request.path,
        statePath: request.path,
        backend: request.kind === 'apply_preview' ? 'awww' : 'awww',
        fileType: request.kind === 'apply_preview' ? 'gif' : 'image',
        preview: request.kind === 'apply_preview',
      }),
    };
  },
  stop: async (): Promise<CommandResult> => ok,
  weClearBackendError: async (): Promise<CommandResult> => ok,
  restore: async (): Promise<CommandResult> => ok,

  libraryCount: async (): Promise<LibraryCountDTO> => ({ total: 150, images: 90, gifs: 30, videos: 30 }),

  libraryPage: async (
    _filter: string,
    _sort: string,
    _search: string,
    offset: number,
    limit: number,
  ): Promise<LibraryPageDTO> => {
    if (libraryFirstPageEmpty && !libraryFirstPageEmptyConsumed && offset === 0) {
      libraryFirstPageEmptyConsumed = true;
      return { total: 0, items: [] };
    }
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
  scanProgress: async (): Promise<ScanProgressDTO> => {
    if (scanProgressState.running && scanAutoAdvance) {
      scanProgressState = { ...scanProgressState, scanned: scanProgressState.scanned + scanStep };
    }
    return { ...scanProgressState };
  },
  scanCancel: async (): Promise<CommandResult> => {
    scanProgressState = { ...scanProgressState, running: false };
    return ok;
  },
  librarySourceStatus: async () => ({
    configured: 'sqlite',
    effective: 'sqlite',
    sqliteReady: true,
    sqliteRows: 150,
    tsvRows: 150,
    sourceCount: 2,
    stale: false,
    message: 'SQLite active (150 entries)',
  }),

  favoritesPage: async (offset: number, limit: number): Promise<LibraryPageDTO> => {
    const items = MOCK_FAVORITES.map((path) =>
      MOCK_WALLPAPERS.find((w) => w.path === path) ?? {
        path,
        type: 'image',
        ext: 'jpg',
        backend: 'awww',
        size: 12345,
        mtime: 1700000000,
        resolution: '1920x1080',
        applyAvailability: 'available' as ApplyAvailability,
        applyBackend: 'awww',
        applyActions: [
          { kind: 'apply' as ApplyActionKind, label: 'Apply', enabled: true },
          { kind: 'open_folder' as ApplyActionKind, label: 'Open folder', enabled: true },
        ],
      },
    );
    return { total: items.length, items: items.slice(offset, offset + limit) };
  },

  favoriteAdd: async (path: string): Promise<CommandResult> => {
    // Simulate failure for the WE Web mock path so smoke tests can verify error feedback
    if (path.includes('3650880224')) return failResult;
    return ok;
  },
  favoriteRemove: async (): Promise<CommandResult> => ok,

  historyPage: async (offset: number, limit: number): Promise<LibraryPageDTO> => {
    const items = MOCK_HISTORY.map((path) =>
      MOCK_WALLPAPERS.find((w) => w.path === path) ?? {
        path,
        type: 'image',
        ext: 'jpg',
        backend: 'awww',
        size: 12345,
        mtime: 1700000000,
        resolution: '1920x1080',
        applyAvailability: 'available' as ApplyAvailability,
        applyBackend: 'awww',
        applyActions: [
          { kind: 'apply' as ApplyActionKind, label: 'Apply', enabled: true },
          { kind: 'open_folder' as ApplyActionKind, label: 'Open folder', enabled: true },
        ],
      },
    );
    return { total: items.length, items: items.slice(offset, offset + limit) };
  },

  historyClear: async (): Promise<CommandResult> => ok,

  sourcesList: async (): Promise<SourceDTO[]> => MOCK_SOURCES,
  sourceAdd: async (): Promise<CommandResult> => ok,
  sourceRemove: async (): Promise<CommandResult> => ok,
  validateSources: async (): Promise<CommandResult> => ok,
  removeMissingSources: async (): Promise<CommandResult> => ok,
  scanSteamWorkshop: async (): Promise<CommandResult> => ok,

  configGet: async (key: string): Promise<string> => configStore[key] ?? defaultConfig[key] ?? '',
  configGetMany: async (keys: string[]): Promise<Record<string, string>> => {
    const out: Record<string, string> = {};
    for (const key of keys) out[key] = await api.configGet(key);
    return out;
  },

  configSet: async (key: string, value: string): Promise<CommandResult> => {
    configStore[key] = value;
    return ok;
  },

  sqliteVerify: async (): Promise<CommandResult> =>
    commandFailures.has('sqliteVerify') ? failResult : ok,
  sqliteRepair: async (): Promise<CommandResult> => ok,
  sqliteResync: async (): Promise<CommandResult> => ok,
  sqliteBackup: async (): Promise<CommandResult> => ok,
  sqliteRestore: async (): Promise<CommandResult> => ok,
  sqliteExportFlat: async (): Promise<CommandResult> => ok,
  migrateToSqlite: async (): Promise<CommandResult> => ok,
  importLegacyFlatFiles: async (): Promise<CommandResult> => ok,

  thumbnailFor: async (path: string): Promise<ThumbnailDTO> => {
    if (thumbnailFailures.has(path)) {
      return { path, cacheHit: false, failureReason: 'mock thumbnail failure' };
    }
    const mode = configStore['gui_thumbnail_mode'] ?? defaultConfig.gui_thumbnail_mode ?? 'cache';
    if (mode === 'icon') {
      return { path, cacheHit: false };
    }
    return { path, cacheHit: false };
  },

  thumbnailCacheStatus: async () => ({
    dir: '/mock/cache/thumbs',
    size: '12.3 MB',
    entries: 30,
    failureEntries: 2,
    cleanupDays: 30,
  }),
  thumbnailCacheClear: async (): Promise<CommandResult> => ok,
  thumbnailCacheCleanupOld: async (): Promise<CommandResult> => ok,

  openProjectLocation: async (): Promise<CommandResult> => ok,
  openPath: async (): Promise<CommandResult> => ok,
  revealInFileManager: async (): Promise<CommandResult> => ok,
  browseDirectory: async (): Promise<string> => '/mock/selected/dir',
  exportDiagnostics: async (): Promise<CommandResult> => {
    await new Promise((resolve) => setTimeout(resolve, 300));
    return commandFailures.has('exportDiagnostics') ? failResult : ok;
  },

  __mockControl: {
    setScanProgress: (partial: Partial<ScanProgressDTO>): void => {
      scanProgressState = { ...scanProgressState, ...partial };
    },
    resetScanProgress: (): void => {
      resetScanProgressState();
    },
    setScanAutoAdvance: (enabled: boolean, step = 5): void => {
      scanAutoAdvance = enabled;
      scanStep = step;
    },
    injectCommandFailure: (cmd: string): void => {
      commandFailures.add(cmd);
    },
    clearCommandFailure: (cmd: string): void => {
      commandFailures.delete(cmd);
    },
    setConfig: (key: string, value: string): void => {
      configStore[key] = value;
    },
    resetConfig: (): void => {
      resetConfigStore();
    },
    setThumbnailFailure: (path: string): void => {
      thumbnailFailures.add(path);
    },
    clearThumbnailFailure: (path: string): void => {
      thumbnailFailures.delete(path);
    },
    setLibraryFirstPageEmpty: (enabled: boolean): void => {
      libraryFirstPageEmpty = enabled;
      libraryFirstPageEmptyConsumed = false;
    },
    resetAll: (): void => {
      resetScanProgressState();
      resetConfigStore();
      resetLibraryScenario();
      commandFailures.clear();
      thumbnailFailures.clear();
    },
  },
};

if (typeof globalThis !== 'undefined') {
  (globalThis as { __mockControl?: typeof api.__mockControl }).__mockControl = api.__mockControl;
}
