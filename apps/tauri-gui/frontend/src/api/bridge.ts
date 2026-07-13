import { invoke } from '@tauri-apps/api/core';
import type {
  ApplyRequestDTO,
  CommandResult,
  DisplayListDTO,
  DisplayStateDTO,
  LibraryCountDTO,
  LibraryPageDTO,
  LibrarySourceStatusDTO,
  LinuxWallpaperEngineStatusDTO,
  ScanProgressDTO,
  SourceDTO,
  StatusDTO,
  TargetedApplyRequestDTO,
  TargetedRestoreRequestDTO,
  ThumbnailCacheDTO,
  ThumbnailDTO,
  WallpaperConsoleApi,
  WeDebugInfoDTO,
} from './types';

export type * from './types';

export const api = {
  status: (): Promise<StatusDTO> => invoke<StatusDTO>('status'),
  linuxWallpaperEngineStatus: (): Promise<LinuxWallpaperEngineStatusDTO> =>
    invoke<LinuxWallpaperEngineStatusDTO>('linux_wallpaperengine_status'),

  apply: (path: string): Promise<CommandResult> => invoke<CommandResult>('apply', { path }),
  applyAction: (request: ApplyRequestDTO): Promise<CommandResult> =>
    invoke<CommandResult>('apply_action', { request }),
  displaysList: (): Promise<DisplayListDTO> => invoke<DisplayListDTO>('displays_list'),
  displayStateList: (): Promise<DisplayStateDTO[]> =>
    invoke<DisplayStateDTO[]>('display_state_list'),
  applyToDisplay: (request: TargetedApplyRequestDTO): Promise<CommandResult> =>
    invoke<CommandResult>('apply_to_display', { request }),
  stop: (): Promise<CommandResult> => invoke<CommandResult>('stop'),
  weClearBackendError: (path: string): Promise<CommandResult> => invoke<CommandResult>('we_clear_backend_error', { path }),
  weDebugInfo: (): Promise<WeDebugInfoDTO> => invoke<WeDebugInfoDTO>('we_debug_info'),
  restore: (): Promise<CommandResult> => invoke<CommandResult>('restore'),
  restoreDisplays: (request?: TargetedRestoreRequestDTO): Promise<CommandResult> =>
    invoke<CommandResult>('restore_displays', { request }),

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

  sourcesList: (): Promise<SourceDTO[]> => invoke<SourceDTO[]>('sources_list'),
  sourceAdd: (path: string): Promise<CommandResult> => invoke<CommandResult>('source_add', { path }),
  sourceRemove: (path: string): Promise<CommandResult> => invoke<CommandResult>('source_remove', { path }),
  validateSources: (): Promise<CommandResult> => invoke<CommandResult>('validate_sources'),
  removeMissingSources: (): Promise<CommandResult> => invoke<CommandResult>('remove_missing_sources'),
  scanSteamWorkshop: (): Promise<CommandResult> => invoke<CommandResult>('scan_steam_workshop'),

  configGet: (key: string): Promise<string> => invoke<string>('config_get', { key }),
  configGetMany: (keys: string[]): Promise<Record<string, string>> =>
    invoke<Record<string, string>>('config_get_many', { keys }),
  configSet: (key: string, value: string): Promise<CommandResult> => invoke<CommandResult>('config_set', { key, value }),

  sqliteVerify: (): Promise<CommandResult> => invoke<CommandResult>('sqlite_verify'),
  sqliteRepair: (): Promise<CommandResult> => invoke<CommandResult>('sqlite_repair'),
  sqliteResync: (): Promise<CommandResult> => invoke<CommandResult>('sqlite_resync'),
  sqliteBackup: (): Promise<CommandResult> => invoke<CommandResult>('sqlite_backup'),
  sqliteRestore: (path: string): Promise<CommandResult> => invoke<CommandResult>('sqlite_restore', { path }),
  sqliteExportFlat: (): Promise<CommandResult> => invoke<CommandResult>('sqlite_export_flat'),
  migrateToSqlite: (): Promise<CommandResult> => invoke<CommandResult>('migrate_to_sqlite'),
  importLegacyFlatFiles: (): Promise<CommandResult> =>
    invoke<CommandResult>('import_legacy_flat_files'),

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
} satisfies WallpaperConsoleApi;
