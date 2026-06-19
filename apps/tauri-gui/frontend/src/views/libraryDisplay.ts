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
