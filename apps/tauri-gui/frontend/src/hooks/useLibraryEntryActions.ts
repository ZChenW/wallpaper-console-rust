import { useCallback } from 'react';
import type { ApplyRequestDTO, WallpaperDTO } from '../api/bridge.ts';
import { normalizeApplyActions } from '../domain/applyActions.ts';
import { buildApplyRequest } from '../domain/applyRequests.ts';
import type { ContextAction } from '../components/WallpaperGrid.tsx';
import { emitFeedback } from '../events/appEvents.ts';

interface UseLibraryEntryActionsCallbacks {
  onApplyAction: (request: ApplyRequestDTO) => void;
  invalidate?: () => void;
  openFolder: (path: string) => void;
  findEntry: (path: string) => WallpaperDTO | undefined;
}

export function useLibraryEntryActions(callbacks: UseLibraryEntryActionsCallbacks) {
  const { onApplyAction, openFolder } = callbacks;

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
              action: () => {
                onApplyAction(
                  buildApplyRequest(entry, 'retry_backend_apply'),
                );
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
                    emitFeedback({
                      state: 'error',
                      label: 'Copy Workshop ID',
                      detail: 'Clipboard write failed',
                    });
                  }
                },
              });
            }
            break;
        }
      }

      return actions;
    },
    [onApplyAction, openFolder],
  );

  return { buildContextActions };
}
