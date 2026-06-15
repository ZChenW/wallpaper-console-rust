import { invoke } from '@tauri-apps/api/core';

export interface CommandResult {
  success: boolean;
  stdout: string;
  stderr: string;
  exitCode: number;
  error?: CommandErrorDTO;
}

export interface CommandErrorDTO {
  kind: string;
  message: string;
  detail?: string;
  recoverable: boolean;
  suggestion?: string;
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

export interface LibrarySourceStatusDTO {
  configured: string;
  effective: string;
  sqliteReady: boolean;
  sqliteRows: number;
  tsvRows: number;
  stale: boolean;
  message: string;
}

export interface ScanProgressDTO {
  running: boolean;
  stage: string;
  scanned: number;
  totalHint?: number;
  reusedMetadata: number;
  probedMetadata: number;
  insertedSqlite: number;
  currentPath?: string;
  cancelRequested: boolean;
  error?: string;
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

export interface LinuxWallpaperEngineStatusDTO {
  available: boolean;
  path?: string;
  message: string;
  detail?: string;
}

export interface WeDebugInfoDTO {
  lastCommandLine: string;
  lastTargetConfig: string;
  lastStderr: string;
  lastExitStatus: string;
  logPath: string;
}

export interface ThumbnailCacheDTO {
  dir: string;
  size: string;
  entries: number;
  oldestMtime?: number;
  newestMtime?: number;
  failureEntries: number;
  cleanupDays: number;
}

export interface ThumbnailDTO {
  path: string;
  thumbnail?: string;
  cacheHit: boolean;
  failureReason?: string;
}

export type ApplyAvailability = 'available' | 'unsupported' | 'retryable_failure';

export type ApplyActionKind =
  | 'apply'
  | 'retry_backend_apply'
  | 'apply_preview'
  | 'open_folder'
  | 'copy_workshop_id';

export type ApplyRequestKind = 'apply' | 'retry_backend_apply' | 'apply_preview';

export interface ApplyRequestDTO {
  kind: ApplyRequestKind;
  path: string;
  requestId?: string;
}

export interface ApplyResultDTO {
  requestId?: string;
  appliedPath: string;
  statePath: string;
  backend: string;
  fileType: string;
  preview: boolean;
}

export interface ApplyActionDTO {
  kind: ApplyActionKind;
  label: string;
  enabled: boolean;
  reason?: string;
}

export interface WallpaperDTO {
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
  unsupportedReason?: string,
  backendStatus?: string;
  backendErrorKind?: string;
  backendErrorMessage?: string;
  backendErrorDetail?: string;
  backendFailedAt?: string;
  applyAvailability?: ApplyAvailability;
  applyBackend?: string;
  applyReason?: string;
  applyActions?: ApplyActionDTO[];
}

export const api = {
  status: (): Promise<StatusDTO> => invoke<StatusDTO>('status'),
  linuxWallpaperEngineStatus: (): Promise<LinuxWallpaperEngineStatusDTO> =>
    invoke<LinuxWallpaperEngineStatusDTO>('linux_wallpaperengine_status'),

  apply: (path: string): Promise<CommandResult> => invoke<CommandResult>('apply', { path }),
  applyAction: (request: ApplyRequestDTO): Promise<CommandResult> =>
    invoke<CommandResult>('apply_action', { request }),
  stop: (): Promise<CommandResult> => invoke<CommandResult>('stop'),
  weClearBackendError: (path: string): Promise<CommandResult> => invoke<CommandResult>('we_clear_backend_error', { path }),
  weDebugInfo: (): Promise<WeDebugInfoDTO> => invoke<WeDebugInfoDTO>('we_debug_info'),
  restore: (): Promise<CommandResult> => invoke<CommandResult>('restore'),

  libraryCount: (): Promise<LibraryCountDTO> =>
    invoke<LibraryCountDTO>('library_count').catch(() => ({ total: 0, images: 0, gifs: 0, videos: 0 })),

  libraryPage: (
    filter: string,
    sort: string,
    search: string,
    offset: number,
    limit: number,
  ): Promise<LibraryPageDTO> =>
    invoke<LibraryPageDTO>('library_page_gui', { filter, sort, search, offset, limit }),

  rescan: (): Promise<CommandResult> => invoke<CommandResult>('rescan'),
  scanProgress: (): Promise<ScanProgressDTO> => invoke<ScanProgressDTO>('scan_progress'),
  scanCancel: (): Promise<CommandResult> => invoke<CommandResult>('scan_cancel'),
  librarySourceStatus: (): Promise<LibrarySourceStatusDTO> =>
    invoke<LibrarySourceStatusDTO>('library_source_status'),

  favoritesPage: (offset: number, limit: number): Promise<LibraryPageDTO> =>
    invoke<LibraryPageDTO>('favorites_page', { offset, limit }),
  favoriteAdd: (path: string): Promise<CommandResult> => invoke<CommandResult>('favorite_add', { path }),
  favoriteRemove: (path: string): Promise<CommandResult> => invoke<CommandResult>('favorite_remove', { path }),

  historyPage: (offset: number, limit: number): Promise<LibraryPageDTO> =>
    invoke<LibraryPageDTO>('history_page', { offset, limit }),
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
  thumbnailCacheCleanupOld: (days: number): Promise<CommandResult> =>
    invoke<CommandResult>('thumbnail_cache_cleanup_old', { days }),

  openProjectLocation: (path: string, mode?: string): Promise<CommandResult> => invoke<CommandResult>('open_project_location', { path, mode }),
  openPath: (path: string): Promise<CommandResult> => invoke<CommandResult>('open_path', { path }),
  revealInFileManager: (path: string): Promise<CommandResult> => invoke<CommandResult>('reveal_in_file_manager', { path }),
  browseDirectory: (): Promise<string> => invoke<string>('browse_directory'),

  exportDiagnostics: (): Promise<CommandResult> => invoke<CommandResult>('export_diagnostics'),
};
