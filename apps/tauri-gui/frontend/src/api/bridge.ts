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
  status: (): Promise<StatusDTO> => invoke<StatusDTO>('status'),

  apply: (path: string): Promise<CommandResult> => invoke<CommandResult>('apply', { path }),
  stop: (): Promise<CommandResult> => invoke<CommandResult>('stop'),
  restore: (): Promise<CommandResult> => invoke<CommandResult>('restore'),

  libraryList: (source: string): Promise<WallpaperDTO[]> => invoke<WallpaperDTO[]>('library_list', { source }),

  libraryCount: (): Promise<LibraryCountDTO> =>
    invoke<LibraryCountDTO>('library_count').catch(() => ({ total: 0, images: 0, gifs: 0, videos: 0 })),

  libraryPage: (
    source: string,
    filter: string,
    sort: string,
    search: string,
    offset: number,
    limit: number,
  ): Promise<LibraryPageDTO> =>
    invoke<LibraryPageDTO>('library_page', { source, filter, sort, search, offset, limit }),

  rescan: (): Promise<CommandResult> => invoke<CommandResult>('rescan'),

  favoritesList: (): Promise<string[]> => invoke<string[]>('favorites_list'),
  favoriteAdd: (path: string): Promise<CommandResult> => invoke<CommandResult>('favorite_add', { path }),
  favoriteRemove: (path: string): Promise<CommandResult> => invoke<CommandResult>('favorite_remove', { path }),

  historyList: (): Promise<HistoryDTO[]> => invoke<HistoryDTO[]>('history_list'),
  historyClear: (): Promise<CommandResult> => invoke<CommandResult>('history_clear'),

  sourcesList: (): Promise<SourceDTO[]> => invoke<SourceDTO[]>('sources_list'),
  sourceAdd: (path: string): Promise<CommandResult> => invoke<CommandResult>('source_add', { path }),
  sourceRemove: (path: string): Promise<CommandResult> => invoke<CommandResult>('source_remove', { path }),
  validateSources: (): Promise<CommandResult> => invoke<CommandResult>('validate_sources'),
  removeMissingSources: (): Promise<CommandResult> => invoke<CommandResult>('remove_missing_sources'),
  scanSteamWorkshop: (): Promise<CommandResult> => invoke<CommandResult>('scan_steam_workshop'),

  configGet: (key: string): Promise<string> => invoke<string>('config_get', { key }),
  configSet: (key: string, value: string): Promise<CommandResult> => invoke<CommandResult>('config_set', { key, value }),

  sqliteVerify: (): Promise<CommandResult> => invoke<CommandResult>('sqlite_verify'),
  sqliteResync: (): Promise<CommandResult> => invoke<CommandResult>('sqlite_resync'),
  sqliteBackup: (): Promise<CommandResult> => invoke<CommandResult>('sqlite_backup'),
  sqliteRestore: (path: string): Promise<CommandResult> => invoke<CommandResult>('sqlite_restore', { path }),
  sqliteExportFlat: (): Promise<CommandResult> => invoke<CommandResult>('sqlite_export_flat'),
  migrateToSqlite: (): Promise<CommandResult> => invoke<CommandResult>('migrate_to_sqlite'),

  thumbnailFor: (path: string): Promise<ThumbnailDTO> =>
    invoke<ThumbnailDTO>('thumbnail_for', { path }).catch(() => ({ path, cacheHit: false })),

  thumbnailCacheStatus: (): Promise<ThumbnailCacheDTO> => invoke<ThumbnailCacheDTO>('thumbnail_cache_status'),
  thumbnailCacheClear: (): Promise<CommandResult> => invoke<CommandResult>('thumbnail_cache_clear'),

  openPath: (path: string): Promise<CommandResult> => invoke<CommandResult>('open_path', { path }),
  revealInFileManager: (path: string): Promise<CommandResult> => invoke<CommandResult>('reveal_in_file_manager', { path }),
  browseDirectory: (): Promise<string> => invoke<string>('browse_directory'),
};
