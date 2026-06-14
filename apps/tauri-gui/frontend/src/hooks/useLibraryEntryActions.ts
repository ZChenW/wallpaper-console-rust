import { useCallback } from 'react';
import { api, WallpaperDTO } from '../api/bridge';
import { normalizeApplyActions } from '../domain/applyActions';
import type { ContextAction } from '../components/WallpaperGrid';

interface UseLibraryEntryActionsCallbacks {
  onApply: (path: string) => void;
  invalidate?: () => void;
  openFolder: (path: string) => void;
  findEntry: (path: string) => WallpaperDTO | undefined;
}

export function useLibraryEntryActions(callbacks: UseLibraryEntryActionsCallbacks) {
  const { onApply, invalidate, openFolder, findEntry } = callbacks;

  const buildContextActions = useCallback((entry: WallpaperDTO): ContextAction[] => {
    const actions: ContextAction[] = [];

    const normalized = normalizeApplyActions(entry);
    for (const a of normalized) {
      if (!a.enabled) continue;

      switch (a.kind) {
        case 'apply':
          actions.push({
            label: a.label,
            action: (path: string) => { onApply(path); },
          });
          break;
        case 'retry_backend_apply':
          actions.push({
            label: a.label,
            action: async (path: string) => {
              let clearOk = true;
              try { await api.weClearBackendError(path); } catch {
                clearOk = false;
                window.dispatchEvent(new CustomEvent('wc-feedback', {
                  detail: { state: 'error', label: 'Clear backend error', detail: 'Failed to clear backend error before retry.' },
                }));
              }
              onApply(path);
              if (clearOk && invalidate) setTimeout(() => invalidate(), 500);
            },
          });
          break;
        case 'apply_preview':
          if (entry.previewPath) {
            actions.push({
              label: a.label,
              action: (path: string) => {
                const e = findEntry(path);
                if (e?.previewPath) onApply(e.previewPath);
              },
            });
          }
          break;
        case 'open_folder':
          actions.push({
            label: a.label,
            action: openFolder,
          });
          break;
        case 'copy_workshop_id':
          if (entry.workshopId) {
            actions.push({
              label: a.label,
              action: async (path: string) => {
                const e = findEntry(path);
                if (e?.workshopId) {
                  try {
                    await navigator.clipboard?.writeText(e.workshopId);
                  } catch {
                    window.dispatchEvent(new CustomEvent('wc-feedback', {
                      detail: { state: 'error', label: 'Copy Workshop ID', detail: 'Clipboard write failed' },
                    }));
                  }
                }
              },
            });
          }
          break;
      }
    }

    return actions;
  }, [onApply, invalidate, openFolder, findEntry]);

  return { buildContextActions };
}
