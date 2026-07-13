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

export type LibraryBrowserType =
  | 'usable'
  | 'image'
  | 'gif'
  | 'video'
  | 'weScene'
  | 'unsupported';

export type LibraryBrowserSort = 'recentlyAdded' | 'nameAsc' | 'nameDesc';

export interface LibraryBrowserQueryDTO {
  sourceId?: number;
  typeFilter: LibraryBrowserType;
  favoritesOnly: boolean;
  search: string;
  sort: LibraryBrowserSort;
  offset: number;
  limit: number;
}

export interface LibraryBrowserSourceDTO {
  id: number;
  displayName: string;
}

export interface LibraryBrowserItemDTO extends WallpaperDTO {
  wallpaperId: number;
  favorite: boolean;
  author: string | null;
  addedAt: string;
  sources: LibraryBrowserSourceDTO[];
}

export interface LibraryBrowserPageDTO {
  total: number;
  items: LibraryBrowserItemDTO[];
}

export interface LibrarySourceStatusDTO {
  configured: string;
  effective: string;
  sqliteReady: boolean;
  sqliteRows: number;
  tsvRows: number;
  sourceCount: number;
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
  staged: number;
  skipped: number;
  metadataErrors: number;
  currentPath?: string;
  cancelRequested: boolean;
  error?: string;
}

export interface SourceDTO {
  id: number;
  path: string;
  displayName: string;
  kind: 'directory' | 'wallpaper_engine_workshop';
  recursive: boolean;
  availability: 'unknown' | 'available' | 'offline';
  addedAt: string;
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

export interface DisplayDTO {
  name: string;
}

export interface DisplayListDTO {
  outputs: DisplayDTO[];
}

export type DisplayStateKind = 'allDisplays' | 'output';

export interface DisplayStateDTO {
  targetKey: string;
  kind: DisplayStateKind;
  output: string | null;
  wallpaperPath: string;
  backend: string;
  updatedAt: string;
}

export interface TargetedApplyRequestDTO {
  path: string;
  /** Omitted means All Displays for compatibility with legacy apply callers. */
  target?: string;
  requestId?: string;
}

export interface TargetedRestoreRequestDTO {
  /** Omitted means discover currently connected outputs. */
  outputs?: string[];
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

export interface WallpaperConsoleApi {
  status(): Promise<StatusDTO>;
  linuxWallpaperEngineStatus(): Promise<LinuxWallpaperEngineStatusDTO>;

  apply(path: string): Promise<CommandResult>;
  applyAction(request: ApplyRequestDTO): Promise<CommandResult>;
  displaysList(): Promise<DisplayListDTO>;
  displayStateList(): Promise<DisplayStateDTO[]>;
  applyToDisplay(request: TargetedApplyRequestDTO): Promise<CommandResult>;
  stop(): Promise<CommandResult>;
  weClearBackendError(path: string): Promise<CommandResult>;
  weDebugInfo(): Promise<WeDebugInfoDTO>;
  restore(): Promise<CommandResult>;
  restoreDisplays(request?: TargetedRestoreRequestDTO): Promise<CommandResult>;

  libraryCount(): Promise<LibraryCountDTO>;
  libraryPage(
    filter: string,
    sort: string,
    search: string,
    offset: number,
    limit: number,
  ): Promise<LibraryPageDTO>;
  libraryBrowserPage(query: LibraryBrowserQueryDTO): Promise<LibraryBrowserPageDTO>;
  libraryBrowserRandom(query: LibraryBrowserQueryDTO): Promise<LibraryBrowserItemDTO | null>;
  rescan(): Promise<CommandResult>;
  scanProgress(): Promise<ScanProgressDTO>;
  scanCancel(): Promise<CommandResult>;
  librarySourceStatus(): Promise<LibrarySourceStatusDTO>;

  favoritesPage(offset: number, limit: number): Promise<LibraryPageDTO>;
  favoriteAdd(path: string): Promise<CommandResult>;
  favoriteRemove(path: string): Promise<CommandResult>;

  sourcesList(): Promise<SourceDTO[]>;
  sourceAdd(path: string): Promise<CommandResult>;
  sourceRemove(path: string): Promise<CommandResult>;
  sourceRename(id: number, displayName: string): Promise<CommandResult>;
  sourceSetRecursive(id: number, recursive: boolean): Promise<CommandResult>;
  sourceRefresh(id: number): Promise<CommandResult>;
  sourceRemoveById(id: number): Promise<CommandResult>;
  validateSources(): Promise<CommandResult>;
  removeMissingSources(): Promise<CommandResult>;
  scanSteamWorkshop(): Promise<CommandResult>;

  configGet(key: string): Promise<string>;
  configGetMany(keys: string[]): Promise<Record<string, string>>;
  configSet(key: string, value: string): Promise<CommandResult>;

  sqliteVerify(): Promise<CommandResult>;
  sqliteRepair(): Promise<CommandResult>;
  sqliteResync(): Promise<CommandResult>;
  sqliteBackup(): Promise<CommandResult>;
  sqliteRestore(path: string): Promise<CommandResult>;
  sqliteExportFlat(): Promise<CommandResult>;
  migrateToSqlite(): Promise<CommandResult>;
  importLegacyFlatFiles(): Promise<CommandResult>;

  thumbnailFor(path: string): Promise<ThumbnailDTO>;
  thumbnailCacheStatus(): Promise<ThumbnailCacheDTO>;
  thumbnailCacheClear(): Promise<CommandResult>;
  thumbnailCacheCleanupOld(days: number): Promise<CommandResult>;

  openProjectLocation(path: string, mode?: string): Promise<CommandResult>;
  openPath(path: string): Promise<CommandResult>;
  revealInFileManager(path: string): Promise<CommandResult>;
  browseDirectory(): Promise<string>;
  exportDiagnostics(): Promise<CommandResult>;
}
