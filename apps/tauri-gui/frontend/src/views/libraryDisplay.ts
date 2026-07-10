import type { LibrarySourceStatusDTO } from '../api/bridge';

export type LibraryDisplay = 'loading' | 'indexing' | 'empty' | 'grid' | 'error';

export interface LibraryDisplayInput {
  initialLoading: boolean;
  hasLoadedOnce: boolean;
  total: number;
  entryCount: number;
  scanRunning: boolean;
  loadError: boolean;
  emptyConfirmed: boolean;
}

export function resolveLibraryDisplay(input: LibraryDisplayInput): LibraryDisplay {
  if (input.entryCount > 0) return 'grid';
  if (input.scanRunning) return 'indexing';
  if (input.loadError) return 'error';
  if (input.initialLoading || !input.hasLoadedOnce) return 'loading';
  if (input.total === 0 && input.emptyConfirmed) return 'empty';
  return 'loading';
}

export function resolveLibraryEmptyMessage(status: LibrarySourceStatusDTO | null): string {
  if (!status) return 'Loading library status...';
  if (status.sourceCount === 0) {
    return 'No sources configured. Add a source or scan Wallpaper Engine.';
  }
  if (status.stale) {
    return 'Library index is out of sync. Rebuild the SQLite index.';
  }
  if (status.sqliteRows === 0) {
    return 'Library index is empty. Rescan or repair the database.';
  }
  return status.message || 'Library is empty.';
}
