import { useCallback } from 'react';
import { api, WallpaperDTO } from '../api/bridge';
import type { ApplyRequestDTO } from '../api/bridge';
import { normalizeApplyActions } from '../domain/applyActions';
import { buildApplyRequest } from '../domain/applyRequests';
import type { ContextAction } from '../components/WallpaperGrid';

interface UseLibraryEntryActionsCallbacks {
  onApplyAction: (request: ApplyRequestDTO) => void;
  invalidate?: () => void;
  openFolder: (path: string) => void;
  findEntry: (path: string) => WallpaperDTO | undefined;
}

export function useLibraryEntryActions(callbacks: UseLibraryEntryActionsCallbacks) {
  const { onApplyAction, invalidate, openFolder } = callbacks;

  const buildContextActions = useCallback(
    (entry: WallpaperDTO): ContextAction[] => {
      const actions: ContextAction[] = [];

      const normalized = normalizeApplyActions(entry);
      for (const a of normalized) {
        if (!a.enabled) continue;

        switch (a.kind) {
          case 'apply':
            actions.push({
              label: a.label,
              action: () => {
                onApplyAction(buildApplyRequest(entry, 'apply'));
              },
            });
            break;
          case 'retry_backend_apply':
            actions.push({
              label: a.label,
              action: async () => {
                let clearOk = true;
                try {
                  const result = await api.weClearBackendError(entry.path);
                  if (!result.success) {
                    clearOk = false;
                    window.dispatchEvent(
                      new CustomEvent('wc-feedback', {
                        detail: {
                          state: 'error',
                          label: 'Clear backend error',
                          detail:
                            result.error?.message ||
                            result.stderr ||
                            'Failed to clear backend error before retry.',
                        },
                      }),
                    );
                  }
                } catch {
                  clearOk = false;
                  window.dispatchEvent(
                    new CustomEvent('wc-feedback', {
                      detail: {
                        state: 'error',
                        label: 'Clear backend error',
                        detail: 'Failed to clear backend error before retry.',
                      },
                    }),
                  );
                }
                onApplyAction(
                  buildApplyRequest(entry, 'retry_backend_apply'),
                );
                if (clearOk && invalidate)
                  setTimeout(() => invalidate(), 500);
              },
            });
            break;
          case 'apply_preview':
            actions.push({
              label: a.label,
              action: () => {
                onApplyAction(
                  buildApplyRequest(entry, 'apply_preview'),
                );
              },
            });
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
                action: async () => {
                  try {
                    await navigator.clipboard?.writeText(entry.workshopId!);
                  } catch {
                    window.dispatchEvent(
                      new CustomEvent('wc-feedback', {
                        detail: {
                          state: 'error',
                          label: 'Copy Workshop ID',
                          detail: 'Clipboard write failed',
                        },
                      }),
                    );
                  }
                },
              });
            }
            break;
        }
      }

      return actions;
    },
    [onApplyAction, invalidate, openFolder],
  );

  return { buildContextActions };
}
