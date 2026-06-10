// Wails bridge API — delegates to generated bindings from Wails v3.
// DO NOT use window.wails.Call directly; always go through this module.

import * as Bridge from '../../bindings/wallpaper-console-gui/bridge';
import type {
  CommandResult,
  HistoryDTO,
  LibraryCountDTO,
  SourceDTO,
  StatusDTO,
  ThumbnailCacheDTO,
  ThumbnailDTO,
  WallpaperDTO,
} from '../../bindings/wallpaper-console-gui/models';

// Re-export model types so views don't need to import from bindings directly.
export type {
  CommandResult,
  HistoryDTO,
  LibraryCountDTO,
  SourceDTO,
  StatusDTO,
  ThumbnailCacheDTO,
  ThumbnailDTO,
  WallpaperDTO,
};

// ── API functions (camelCase wrappers around generated PascalCase) ────────

export const api = {
  status: (): Promise<StatusDTO> =>
    Bridge.Status().then((s: StatusDTO | null) => { if (!s) throw new Error('Status returned null'); return s; }),

  apply: (path: string) => Bridge.Apply(path),
  stop: () => Bridge.Stop(),
  restore: () => Bridge.Restore(),

  libraryList: (source: string): Promise<WallpaperDTO[]> =>
    Bridge.LibraryList(source).then((v: WallpaperDTO[] | null) => v ?? []),

  libraryCount: (): Promise<LibraryCountDTO> =>
    Bridge.LibraryCount().then((v: LibraryCountDTO | null) => v ?? { total: 0, images: 0, gifs: 0, videos: 0 }),

  rescan: () => Bridge.Rescan(),

  favoritesList: (): Promise<string[]> =>
    Bridge.FavoritesList().then((v: string[] | null) => v ?? []),

  favoriteAdd: (path: string) => Bridge.FavoriteAdd(path),
  favoriteRemove: (path: string) => Bridge.FavoriteRemove(path),

  historyList: (): Promise<HistoryDTO[]> =>
    Bridge.HistoryList().then((v: HistoryDTO[] | null) => v ?? []),

  historyClear: () => Bridge.HistoryClear(),

  sourcesList: (): Promise<SourceDTO[]> =>
    Bridge.SourcesList().then((v: SourceDTO[] | null) => v ?? []),

  sourceAdd: (path: string) => Bridge.SourceAdd(path),
  sourceRemove: (path: string) => Bridge.SourceRemove(path),
  validateSources: () => Bridge.ValidateSources(),
  removeMissingSources: () => Bridge.RemoveMissingSources(),
  scanSteamWorkshop: () => Bridge.ScanSteamWorkshop(),

  configGet: (key: string) => Bridge.ConfigGet(key),
  configSet: (key: string, value: string) => Bridge.ConfigSet(key, value),

  sqliteVerify: () => Bridge.SqliteVerify(),
  sqliteResync: () => Bridge.SqliteResync(),
  sqliteBackup: () => Bridge.SqliteBackup(),
  sqliteRestore: (path: string) => Bridge.SqliteRestore(path),
  sqliteExportFlat: () => Bridge.SqliteExportFlat(),
  migrateToSqlite: () => Bridge.MigrateToSqlite(),

  thumbnailFor: (path: string) =>
    Bridge.ThumbnailFor(path).then((v: ThumbnailDTO | null) => v ?? { path, cacheHit: false }),

  thumbnailCacheStatus: () => Bridge.ThumbnailCacheStatus(),
  thumbnailCacheClear: () => Bridge.ThumbnailCacheClear(),

  openPath: (path: string) => Bridge.OpenPath(path),
  revealInFileManager: (path: string) => Bridge.RevealInFileManager(path),
  browseDirectory: () => Bridge.BrowseDirectory(),
};
