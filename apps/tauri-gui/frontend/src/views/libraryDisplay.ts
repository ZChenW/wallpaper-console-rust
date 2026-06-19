export type LibraryDisplay = 'loading' | 'indexing' | 'empty' | 'grid';

export interface LibraryDisplayInput {
  initialLoading: boolean;
  hasLoadedOnce: boolean;
  total: number;
  entryCount: number;
  scanRunning: boolean;
}

export function resolveLibraryDisplay(input: LibraryDisplayInput): LibraryDisplay {
  if (input.entryCount > 0) return 'grid';
  if (input.scanRunning) return 'indexing';
  if (input.initialLoading || !input.hasLoadedOnce) return 'loading';
  if (input.total === 0) return 'empty';
  return 'loading';
}
