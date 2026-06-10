// Wails bridge API — all calls to the Go backend.
// In Wails v3, the backend methods are available on the global `wails` object.

declare global {
  interface Window {
    wails: {
      Call: (method: string, ...args: unknown[]) => Promise<unknown>;
    };
  }
}

function call<T>(method: string, ...args: unknown[]): Promise<T> {
  if (window.wails) {
    return window.wails.Call(method, ...args) as Promise<T>;
  }
  // Fallback for dev mode without Wails runtime
  return Promise.reject(new Error(`Wails not available (method: ${method})`));
}

// ── Types ───────────────────────────────────────────────────────────

export interface WallpaperDTO {
  path: string;
  type: 'image' | 'gif' | 'video';
  ext: string;
  backend: string;
  size: number;
  mtime: number;
  resolution: string;
}

export interface LibraryCountDTO {
  total: number;
  images: number;
  gifs: number;
  videos: number;
}

export interface StatusDTO {
  configDir: string;
  current: string;
  lastBackend: string;
  sourceCount: number;
}

export interface CommandResult {
  success: boolean;
  stdout: string;
  stderr: string;
  exitCode: number;
}

export interface SourceDTO {
  path: string;
  exists: boolean;
  isWE: boolean;
  label: string;
}

export interface HistoryDTO {
  path: string;
}

export interface ThumbnailDTO {
  path: string;
  thumbnail?: string;
  cacheHit: boolean;
}

export interface ThumbnailCacheDTO {
  dir: string;
  size: string;
  entries: number;
}

// ── API functions ───────────────────────────────────────────────────

export const api = {
  status: () => call<StatusDTO>('Status'),
  apply: (path: string) => call<CommandResult>('Apply', path),
  stop: () => call<CommandResult>('Stop'),
  restore: () => call<CommandResult>('Restore'),

  libraryList: (source: string) => call<WallpaperDTO[]>('LibraryList', source),
  libraryCount: () => call<LibraryCountDTO>('LibraryCount'),
  rescan: () => call<CommandResult>('Rescan'),

  favoritesList: () => call<string[]>('FavoritesList'),
  favoriteAdd: (path: string) => call<CommandResult>('FavoriteAdd', path),
  favoriteRemove: (path: string) => call<CommandResult>('FavoriteRemove', path),

  historyList: () => call<HistoryDTO[]>('HistoryList'),
  historyClear: () => call<CommandResult>('HistoryClear'),

  sourcesList: () => call<SourceDTO[]>('SourcesList'),
  sourceAdd: (path: string) => call<CommandResult>('SourceAdd', path),
  sourceRemove: (path: string) => call<CommandResult>('SourceRemove', path),
  validateSources: () => call<CommandResult>('ValidateSources'),
  removeMissingSources: () => call<CommandResult>('RemoveMissingSources'),
  scanSteamWorkshop: () => call<CommandResult>('ScanSteamWorkshop'),

  configGet: (key: string) => call<string>('ConfigGet', key),
  configSet: (key: string, value: string) => call<CommandResult>('ConfigSet', key, value),

  sqliteVerify: () => call<CommandResult>('SqliteVerify'),
  sqliteResync: () => call<CommandResult>('SqliteResync'),
  sqliteBackup: () => call<CommandResult>('SqliteBackup'),
  sqliteRestore: (path: string) => call<CommandResult>('SqliteRestore', path),
  sqliteExportFlat: () => call<CommandResult>('SqliteExportFlat'),
  migrateToSqlite: () => call<CommandResult>('MigrateToSqlite'),

  thumbnailFor: (path: string) => call<ThumbnailDTO>('ThumbnailFor', path),
  thumbnailCacheStatus: () => call<ThumbnailCacheDTO>('ThumbnailCacheStatus'),
  thumbnailCacheClear: () => call<CommandResult>('ThumbnailCacheClear'),

  openPath: (path: string) => call<CommandResult>('OpenPath', path),
  revealInFileManager: (path: string) => call<CommandResult>('RevealInFileManager', path),
  browseDirectory: () => call<string>('BrowseDirectory'),
};
