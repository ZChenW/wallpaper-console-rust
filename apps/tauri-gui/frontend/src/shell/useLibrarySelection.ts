import { useCallback, useEffect, useRef, useState } from 'react';
import { api } from '../api/bridge.ts';
import type { LibraryBrowserItemDTO } from '../api/types.ts';
import { reconcileSelectedEntryByStableId } from './singlePageShellModel.ts';
import type { ShellNoticeInput } from './useShellFeedback.ts';

/** Own selection lifetime, stale existence probes, and details return focus. */
export function useLibrarySelection(
  entries: readonly LibraryBrowserItemDTO[],
  replaceCount: number,
  showNotice: (notice: ShellNoticeInput) => void,
) {
  const [selectedEntry, setSelectedEntry] = useState<LibraryBrowserItemDTO | null>(null);
  const [detailsEntry, setDetailsEntry] = useState<LibraryBrowserItemDTO | null>(null);
  const detailsReturnFocusRef = useRef<HTMLElement | null>(null);
  const closeDetails = useCallback(() => {
    setDetailsEntry(null);
    const trigger = detailsReturnFocusRef.current;
    detailsReturnFocusRef.current = null;
    if (!trigger?.isConnected) return;
    window.requestAnimationFrame(() => trigger.focus());
  }, []);
  const openLibraryDetails = useCallback((
    entry: LibraryBrowserItemDTO,
    returnFocus: HTMLElement | null = null,
  ) => {
    detailsReturnFocusRef.current = returnFocus;
    setDetailsEntry(entry);
  }, []);
  const selectLibraryEntry = useCallback((entry: LibraryBrowserItemDTO) => {
    setSelectedEntry(entry);
  }, []);
  useEffect(() => {
    setSelectedEntry((current) => reconcileSelectedEntryByStableId(current, entries));
  }, [entries, replaceCount]);

  const selectedExistenceRequest = useRef(0);
  useEffect(() => {
    const selected = selectedEntry;
    const requestId = ++selectedExistenceRequest.current;
    if (!selected || entries.some((entry) => entry.wallpaperId === selected.wallpaperId)) {
      return undefined;
    }
    void api.libraryWallpaperExists(selected.wallpaperId).then(
      (exists) => {
        if (exists || selectedExistenceRequest.current !== requestId) return;
        setSelectedEntry((current) => (
          current?.wallpaperId === selected.wallpaperId ? null : current
        ));
        setDetailsEntry((current) => (
          current?.wallpaperId === selected.wallpaperId ? null : current
        ));
        showNotice({
          channel: 'system',
          severity: 'info',
          message: 'The selected wallpaper is no longer in Library.',
        });
      },
      () => {
        // An existence probe is advisory. Preserve selection on transport failures.
      },
    );
    return () => {
      if (selectedExistenceRequest.current === requestId) {
        selectedExistenceRequest.current += 1;
      }
    };
  }, [entries, replaceCount, selectedEntry, showNotice]);

  return { selectedEntry, detailsEntry, selectLibraryEntry, openLibraryDetails, closeDetails };
}
