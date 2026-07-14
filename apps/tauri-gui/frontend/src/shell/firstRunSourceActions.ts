import type { CommandResult } from '../api/types.ts';
import {
  executeSourceMutation,
  executeTrackedSourceScan,
} from './useWallpaperSources.ts';

export interface SuggestedDirectoryApi {
  sourceAdd(path: string): Promise<CommandResult>;
}

/** Add a detected directory only after explicit confirmation by the caller. */
export function addSuggestedDirectory(
  sourceApi: SuggestedDirectoryApi,
  path: string,
  reconcile: () => Promise<void>,
  onStarted?: () => void,
  onFinished?: () => void,
): Promise<CommandResult> {
  return executeTrackedSourceScan(
    () => executeSourceMutation(() => sourceApi.sourceAdd(path), reconcile),
    onStarted,
    onFinished,
  );
}
