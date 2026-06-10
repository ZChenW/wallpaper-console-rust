import { invoke } from '@tauri-apps/api/core';

export interface CommandResult {
  success: boolean;
  stdout: string;
  stderr: string;
  exitCode: number;
}

export interface HistoryDTO {
  path: string;
}

export interface LibraryCountDTO {
  total: number;
  images: number;
  gifs: number;
  videos: number;
}

export interface LibraryPageDTO {
  total: number;
  items: WallpaperDTO[];
}

export interface SourceDTO {
  path: string;
  exists: boolean;
  isWE: boolean;
  label: string;
}

export interface StatusDTO {
  configDir: string;
  current: string;
  lastBackend: string;
  sourceCount: number;
}

export interface ThumbnailCacheDTO {
  dir: string;
  size: string;
  entries: number;
}

export interface ThumbnailDTO {
  path: string;
  thumbnail?: string;
  cacheHit: boolean;
}

export interface WallpaperDTO {
  path: string;
  type: string;
  ext: string;
  backend: string;
  size: number;
  mtime: number;
  resolution: string;
}

export const api = {
  status: (): Promise<StatusDTO> => invoke('status'),

  apply: (path: string): Promise<CommandResult> => invoke('apply', { path }),
  stop: (): Promise<CommandResult> => invoke('stop'),
  restore: (): Promise<CommandResult> => invoke('restore'),

  libraryList: (source: string): Promise<WallpaperDTO[]> => invoke('library_list', { source }),

  libraryCount: (): Promise<LibraryCountDTO> =>
    invoke('library_count').catch(() => ({ total: 0, images: 0, gifs: 0, videos: 0 })),

  libraryPage: (
    source: string,
    filter: string,
    sort: string,
    search: string,
    offset: number,
    limit: number,
  ): Promise<LibraryPageDTO> =>
    invoke('library_page', { source, filter, sort, search, offset, limit })
      .catch((err) => Promise.reject(err)),

  rescan: (): Promise<CommandResult> => invoke('rescan'),

  favoritesList: (): Promise<string[]> => invoke('favorites_list'),
  favoriteAdd: (path: string): Promise<CommandResult> => invoke('favorite_add', { path }),
  favoriteRemove: (path: string): Promise<CommandResult> => invoke('favorite_remove', { path }),

  historyList: (): Promise<HistoryDTO[]> => invoke('history_list'),
  historyClear: (): Promise<CommandResult> => invoke('history_clear'),

  sourcesList: (): Promise<SourceDTO[]> => invoke('sources_list'),
  sourceAdd: (path: string): Promise<CommandResult> => invoke('source_add', { path }),
  sourceRemove: (path: string): Promise<CommandResult> => invoke('source_remove', { path }),
  validateSources: (): Promise<CommandResult> => invoke('validate_sources'),
  removeMissingSources: (): Promise<CommandResult> => invoke('remove_missing_sources'),
  scanSteamWorkshop: (): Promise<CommandResult> => invoke('scan_steam_workshop'),

  configGet: (key: string): Promise<string> => invoke('config_get', { key }),
  configSet: (key: string, value: string): Promise<CommandResult> => invoke('config_set', { key, value }),

  sqliteVerify: (): Promise<CommandResult> => invoke('sqlite_verify'),
  sqliteResync: (): Promise<CommandResult> => invoke('sqlite_resync'),
  sqliteBackup: (): Promise<CommandResult> => invoke('sqlite_backup'),
  sqliteRestore: (path: string): Promise<CommandResult> => invoke('sqlite_restore', { path }),
  sqliteExportFlat: (): Promise<CommandResult> => invoke('sqlite_export_flat'),
  migrateToSqlite: (): Promise<CommandResult> => invoke('migrate_to_sqlite'),

  thumbnailFor: (path: string): Promise<ThumbnailDTO> =>
    invoke<ThumbnailDTO>('thumbnail_for', { path }).catch(() => ({ path, cacheHit: false })),

  thumbnailCacheStatus: (): Promise<ThumbnailCacheDTO> => invoke('thumbnail_cache_status'),
  thumbnailCacheClear: (): Promise<CommandResult> => invoke('thumbnail_cache_clear'),

  openPath: (path: string): Promise<CommandResult> => invoke('open_path', { path }),
  revealInFileManager: (path: string): Promise<CommandResult> => invoke('reveal_in_file_manager', { path }),
  browseDirectory: (): Promise<string> => invoke('browse_directory'),
};
