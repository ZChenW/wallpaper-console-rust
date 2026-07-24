import { invoke } from '@tauri-apps/api/core';
import type {
  ApplyRequestDTO,
  CommandResult,
  DisplayListDTO,
  DisplayStateDTO,
  FirstRunSourceSuggestionDTO,
  LibraryCountDTO,
  LibraryBrowserItemDTO,
  LibraryBrowserPageDTO,
  LibraryBrowserQueryDTO,
  LibraryBrowserTotalDTO,
  LibraryPageDTO,
  LibrarySourceStatusDTO,
  LinuxWallpaperEngineStatusDTO,
  RendererStatusesDTO,
  RuntimeWallpaperObservationDTO,
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

export type InvokeFn = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

export function createSourceMutationApi(invokeFn: InvokeFn = invoke) {
  return {
    sourceRename: (id: number, displayName: string): Promise<CommandResult> =>
      invokeFn<CommandResult>('source_rename', { id, displayName }),
    sourceSetRecursive: (id: number, recursive: boolean): Promise<CommandResult> =>
      invokeFn<CommandResult>('source_set_recursive', { id, recursive }),
    sourceRefresh: (id: number): Promise<CommandResult> =>
      invokeFn<CommandResult>('source_refresh', { id }),
    sourceRemoveById: (id: number): Promise<CommandResult> =>
      invokeFn<CommandResult>('source_remove_by_id', { id }),
  };
}

export function createLibraryBrowserApi(invokeFn: InvokeFn = invoke) {
  return {
    libraryBrowserPage: (query: LibraryBrowserQueryDTO): Promise<LibraryBrowserPageDTO> =>
      invokeFn<LibraryBrowserPageDTO>('library_browser_page', { query }),
    libraryBrowserTotal: (
      query: LibraryBrowserQueryDTO,
      expectedRevision: number,
    ): Promise<LibraryBrowserTotalDTO> => invokeFn<LibraryBrowserTotalDTO>(
      'library_browser_total',
      { query, expectedRevision },
    ),
    libraryBrowserRandom: (
      query: LibraryBrowserQueryDTO,
    ): Promise<LibraryBrowserItemDTO | null> =>
      invokeFn<LibraryBrowserItemDTO | null>('library_browser_random', { query }),
    libraryWallpaperExists: (wallpaperId: number): Promise<boolean> =>
      invokeFn<boolean>('library_wallpaper_exists', { wallpaperId }),
  };
}

export function createRuntimeObservationApi(invokeFn: InvokeFn = invoke) {
  return {
    runtimeWallpaperObservations: (): Promise<RuntimeWallpaperObservationDTO[]> =>
      invokeFn<RuntimeWallpaperObservationDTO[]>('runtime_wallpaper_observations'),
  };
}

export function createRendererStatusApi(invokeFn: InvokeFn = invoke) {
  return {
    rendererStatuses: (): Promise<RendererStatusesDTO> =>
      invokeFn<RendererStatusesDTO>('renderer_statuses'),
  };
}

export function createFirstRunSuggestionApi(invokeFn: InvokeFn = invoke) {
  return {
    firstRunSourceSuggestions: (): Promise<FirstRunSourceSuggestionDTO[]> =>
      invokeFn<FirstRunSourceSuggestionDTO[]>('first_run_source_suggestions'),
  };
}

export const api = {
  status: (): Promise<StatusDTO> => invoke<StatusDTO>('status'),
  linuxWallpaperEngineStatus: (): Promise<LinuxWallpaperEngineStatusDTO> =>
    invoke<LinuxWallpaperEngineStatusDTO>('linux_wallpaperengine_status'),
  ...createRendererStatusApi(),

  apply: (path: string): Promise<CommandResult> => invoke<CommandResult>('apply', { path }),
  applyAction: (request: ApplyRequestDTO): Promise<CommandResult> =>
    invoke<CommandResult>('apply_action', { request }),
  displaysList: (): Promise<DisplayListDTO> => invoke<DisplayListDTO>('displays_list'),
  displayStateList: (): Promise<DisplayStateDTO[]> =>
    invoke<DisplayStateDTO[]>('display_state_list'),
  ...createRuntimeObservationApi(),
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
  ...createLibraryBrowserApi(),

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
  ...createFirstRunSuggestionApi(),
  sourceAdd: (path: string): Promise<CommandResult> => invoke<CommandResult>('source_add', { path }),
  sourceRemove: (path: string): Promise<CommandResult> => invoke<CommandResult>('source_remove', { path }),
  ...createSourceMutationApi(),
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
  previewAssetAuthorize: (path: string, wallpaperPath: string): Promise<string> =>
    invoke<string>('preview_asset_authorize', { path, wallpaperPath }),

  thumbnailCacheStatus: (): Promise<ThumbnailCacheDTO> => invoke<ThumbnailCacheDTO>('thumbnail_cache_status'),
  thumbnailCacheClear: (): Promise<CommandResult> => invoke<CommandResult>('thumbnail_cache_clear'),
  thumbnailCacheCleanupOld: (days: number): Promise<CommandResult> =>
    invoke<CommandResult>('thumbnail_cache_cleanup_old', { days }),

  openProjectLocation: (path: string, mode?: string): Promise<CommandResult> => invoke<CommandResult>('open_project_location', { path, mode }),
  openPath: (path: string): Promise<CommandResult> => invoke<CommandResult>('open_path', { path }),
  revealInFileManager: (path: string): Promise<CommandResult> => invoke<CommandResult>('reveal_in_file_manager', { path }),
  browseDirectory: (): Promise<string> => invoke<string>('browse_directory'),

  libraryReady: (): Promise<void> => invoke<void>('library_ready'),
  revealMainWindow: (): Promise<void> => invoke<void>('reveal_main_window'),
  exportDiagnostics: (): Promise<CommandResult> => invoke<CommandResult>('export_diagnostics'),
} satisfies WallpaperConsoleApi;
