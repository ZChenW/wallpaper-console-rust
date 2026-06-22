import type { CommandResult, LibrarySourceStatusDTO, LinuxWallpaperEngineStatusDTO, ThumbnailCacheDTO, WeDebugInfoDTO } from '../api/bridge';
import type { CommandFeedback } from '../api/feedback';
import type { SettingEntry } from './configSchema';

export type DbAction = 'verify' | 'rebuild' | 'backup' | 'export' | 'restore';

export interface GeneralPageProps {
  libraryStatus: LibrarySourceStatusDTO | null;
  libraryStatusError: string | null;
  libraryStatusLoading: boolean;
  weStatus: LinuxWallpaperEngineStatusDTO | null;
  weStatusError: string | null;
  weStatusLoading: boolean;
  thumbCache: ThumbnailCacheDTO | null;
  thumbCacheError: string | null;
  thumbCacheLoading: boolean;
  configs: Record<string, string>;
  saving: string | null;
  onSet: (key: string, value: string) => Promise<boolean>;
}

export interface WallpaperPageProps {
  configs: Record<string, string>;
  saving: string | null;
  onSet: (key: string, value: string) => Promise<boolean>;
  onAdvancedCollapse?: () => void;
}

export interface WallpaperEnginePageProps {
  weStatus: LinuxWallpaperEngineStatusDTO | null;
  weStatusError: string | null;
  weStatusLoading: boolean;
  configs: Record<string, string>;
  saving: string | null;
  onSet: (key: string, value: string) => Promise<boolean>;
  onAdvancedCollapse?: () => void;
}

export interface LibraryPageProps {
  configs: Record<string, string>;
  saving: string | null;
  onSet: (key: string, value: string) => Promise<boolean>;
  thumbCache: ThumbnailCacheDTO | null;
  thumbCacheError: string | null;
  thumbCacheLoading: boolean;
  onFeedback: (fb: CommandFeedback) => void;
  handleCleanupThumbnails: () => Promise<void>;
  refreshSettingsStatus: (reason?: string) => void;
  confirmAndRun: (title: string, msg: string, fn: () => Promise<void>, danger?: boolean, action?: DbAction) => void;
  operationLock: boolean;
  onAdvancedCollapse?: () => void;
}

export interface DatabasePageProps {
  libraryStatus: LibrarySourceStatusDTO | null;
  libraryStatusError: string | null;
  libraryStatusLoading: boolean;
  dbAction: DbAction | null;
  operationLock: boolean;
  runDbAction: (action: DbAction, label: string, fn: () => Promise<CommandResult>) => Promise<void>;
  onFeedback: (fb: CommandFeedback) => void;
  confirmAndRun: (title: string, msg: string, fn: () => Promise<void>, danger?: boolean, action?: DbAction) => void;
  onRestore: () => void;
  restoreInputRef: React.RefObject<HTMLInputElement | null>;
  onRestoreFileSelected: (e: React.ChangeEvent<HTMLInputElement>) => void;
  invalidateLibrary: () => void;
  refreshSettingsStatus: (reason?: string) => void;
  diagnosticsRunning: boolean;
  runDiagnosticsExport: () => Promise<void>;
}

export interface AdvancedPageProps {
  configs: Record<string, string>;
  saving: string | null;
  onSet: (key: string, value: string) => Promise<boolean>;
  weDebugInfo: WeDebugInfoDTO | null;
  weDebugError: string | null;
  showRawConfig: boolean;
  setShowRawConfig: (v: boolean) => void;
}

export interface ConfigRowProps {
  setting: SettingEntry;
  value: string;
  saving: boolean;
  onSet: (v: string) => Promise<boolean>;
}

export interface StatusCardProps {
  label: string;
  value: string;
  detail?: string;
  tone?: 'neutral' | 'success' | 'warning' | 'danger';
}
